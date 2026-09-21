//! Project-owned, deterministic local Knowledge foundation.
//!
//! This crate deliberately contains no provider, OpenCode, Tauri, network, or
//! embedding runtime dependency. Callers provide authorized immutable Material
//! bytes from `inputs/`; derived records live in `knowledge/knowledge.sqlite`.

#![forbid(unsafe_code)]

mod context;
mod exhaustive;
mod model;
mod ort_provider;
mod summary;
mod thematic;
pub use context::{
    BudgetEstimator, ConservativeCharBudgetEstimator, ContextAssembler, ContextAssemblyOptions,
    EvidenceEntry, EvidencePackage, EvidenceQueryMetadata, EvidenceTotals, ExcerptKind,
};
pub use exhaustive::{ExhaustiveCoverage, ExhaustiveSearchReport, RetrievalMode};
pub use model::{
    ModelArtifact, ModelGeneration, ModelInstallState, ModelManager, ModelManifest,
    semantic_safe_subdivide, token_count, verify_artifact,
};
pub use ort_provider::{
    MAX_INTRA_THREADS, OrtEmbeddingProvider, OrtProviderLoadError, bounded_intra_threads,
    runtime_library_from_executable,
};
pub use summary::{
    BatchOptions, RemoteSummarizer, SUMMARY_CONTRACT_VERSION, SummaryAccounting, SummaryContent,
    SummaryEvidenceRef, SummaryExecutionControl, SummaryFailure, SummaryItem, SummaryLevel,
    SummaryNode, SummaryOutput, SummaryPlan, SummaryRequest, SummaryState, SummaryUsage,
    build_document_summary_request, build_synthesis_request, deserialize_content,
    fingerprint_output, fingerprint_summary_inputs, plan_project_summaries,
    select_document_evidence, serialize_content, validate_summary_output,
};
pub use thematic::ThematicSearchReport;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use project_core::{MaterialId, ProjectId};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

pub const SCHEMA_VERSION: i64 = 9;
pub const NORMALIZATION_VERSION: &str = "nfc-lf-v1";
pub const CHUNKER_ID: &str = "structural-v1";
pub const CHUNKER_VERSION: &str = "structural-v1";
const MAX_CHUNK_CHARS: usize = 1_600;

/// Bounded cadence for incremental embedding progress. Progress is emitted at
/// most once per interval (plus a final report), never per embedding, so the
/// UI stays smooth without thousands of IPC/SQLite updates.
const EMBEDDING_PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug)]
pub enum KnowledgeError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    InvalidProjectRoot,
    UnsupportedFormat(String),
    InvalidUtf8,
    IncompatibleSchema(i64),
    ModelManifest(String),
    ArtifactVerification(String),
    ModelUnavailable,
    Tokenizer(String),
    InputTooLong,
    InvalidEmbedding(String),
    Inference(String),
    InvalidSearchOptions(String),
    InvalidContextAssemblyOptions(String),
    /// A nonzero number of valid vectors failed to commit to the local index;
    /// the vector contract itself is fine. This separates a persistence-only
    /// failure from an inference/serialization failure at the same boundary.
    EmbeddingPersistFailed,
    /// A durable summary lifecycle compare-and-set lost to another actor.
    SummaryTransitionLost,
}

impl std::fmt::Display for KnowledgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "knowledge filesystem error: {error}"),
            Self::Sql(error) => write!(f, "knowledge database error: {error}"),
            Self::InvalidProjectRoot => f.write_str("invalid project root"),
            Self::UnsupportedFormat(format) => write!(f, "unsupported Knowledge format: {format}"),
            Self::InvalidUtf8 => f.write_str("Knowledge text must be valid UTF-8"),
            Self::IncompatibleSchema(version) => {
                write!(f, "unsupported Knowledge schema version: {version}")
            }
            Self::ModelManifest(message) => {
                write!(f, "invalid Knowledge model manifest: {message}")
            }
            Self::ArtifactVerification(message) => {
                write!(f, "Knowledge model artifact rejected: {message}")
            }
            Self::ModelUnavailable => {
                f.write_str("Knowledge model is not installed or unavailable")
            }
            Self::Tokenizer(message) => write!(f, "Knowledge tokenizer error: {message}"),
            Self::InputTooLong => f.write_str("Knowledge input cannot fit the model hard limit"),
            Self::InvalidEmbedding(message) => write!(f, "invalid Knowledge embedding: {message}"),
            Self::Inference(message) => write!(f, "Knowledge local inference error: {message}"),
            Self::InvalidSearchOptions(message) => {
                write!(f, "invalid Knowledge hybrid search options: {message}")
            }
            Self::InvalidContextAssemblyOptions(message) => {
                write!(f, "invalid Knowledge context assembly options: {message}")
            }
            Self::EmbeddingPersistFailed => {
                write!(f, "Knowledge embedding vectors could not be persisted")
            }
            Self::SummaryTransitionLost => f.write_str("summary lifecycle transition lost"),
        }
    }
}

impl std::error::Error for KnowledgeError {}
impl From<std::io::Error> for KnowledgeError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<rusqlite::Error> for KnowledgeError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sql(value)
    }
}

pub type Result<T> = std::result::Result<T, KnowledgeError>;

/// Sanitized structural classification of why embedding persistence failed.
/// The `EmbeddingPersistFailed` (and broader `Sql`/`Io`) failure surfaces are
/// lossy — they can collapse a SQLite extended error, a transaction begin/
/// commit failure, a statement prepare failure, a constraint violation, a
/// vector serialization failure, or an out-of-band IO error into one name.
/// This preserves the runtime-observable cause without leaking SQL text, a
/// path, a vector, or a document body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingPersistClass {
    SqliteBusy,
    SqliteLocked,
    TransactionBeginFailed,
    TransactionCommitFailed,
    StatementPrepareFailed,
    SqlBindingFailed,
    SqlTypeConversionFailed,
    ConstraintFailed,
    DiskFull,
    ReadOnlyStorage,
    VectorSerializationFailed,
    VectorDimensionInvalid,
    IoFailed,
    DbCorrupt,
    OtherStorageFailure,
}

impl EmbeddingPersistClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SqliteBusy => "sqlite_busy",
            Self::SqliteLocked => "sqlite_locked",
            Self::TransactionBeginFailed => "transaction_begin_failed",
            Self::TransactionCommitFailed => "transaction_commit_failed",
            Self::StatementPrepareFailed => "statement_prepare_failed",
            Self::SqlBindingFailed => "sql_binding_failed",
            Self::SqlTypeConversionFailed => "sql_type_conversion_failed",
            Self::ConstraintFailed => "constraint_failed",
            Self::DiskFull => "disk_full",
            Self::ReadOnlyStorage => "read_only_storage",
            Self::VectorSerializationFailed => "vector_serialization_failed",
            Self::VectorDimensionInvalid => "vector_dimension_invalid",
            Self::IoFailed => "io_failed",
            Self::DbCorrupt => "db_corrupt",
            Self::OtherStorageFailure => "other_storage_failure",
        }
    }
}

/// A sanitized, stage-preserving persistence failure: which persistence stage
/// failed, classified to a stable structural class, plus the SQLite primary
/// result code when the failure originated as a SQLite result/error. Never
/// carries SQL text, a path, a vector, or a document body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddingPersistFailure {
    pub stage: &'static str,
    pub class: EmbeddingPersistClass,
    pub sqlite_code: Option<i32>,
    /// Sanitized name of the underlying error variant (for example
    /// `Sql::InvalidParameterCount` or `Sql::SqlInputError`). This names the
    /// *kind* of failure without carrying any error message, SQL text, path,
    /// vector, or document body. It exists because a lossy collapse to
    /// `other_storage_failure` previously hid the real cause (a prepare-time or
    /// bind-time error is not a storage failure).
    pub kind: &'static str,
}

/// The SQLite primary result code, or `None` when the failure did not originate
/// as a SQLite result/error. Extended (sub)codes are intentionally not exposed.
fn sqlite_primary_code(error: &rusqlite::Error) -> Option<i32> {
    let rusqlite::Error::SqliteFailure(ffi_error, _) = error else {
        return None;
    };
    // SQLite encodes the primary result code in the low 8 bits of the extended
    // code. Exposing only the primary code keeps `SQLITE_BUSY` vs `SQLITE_LOCKED`
    // distinguishable without surfacing any sub-code that could encode detail.
    Some(ffi_error.extended_code & 0xFF)
}

/// Classifies a rusqlite error into a structural persistence class. Identity of
/// the caller's stage is applied by the embedding persistence site.
///
/// A non-`SqliteFailure` error is a *bind/prepare/convert* failure, not a
/// storage failure. These are now distinguished explicitly so a human operator
/// sees `statement_prepare_failed` / `sql_binding_failed` /
/// `sql_type_conversion_failed` instead of a misleading `other_storage_failure`.
fn classify_sqlite_embedding_error(error: &rusqlite::Error) -> EmbeddingPersistClass {
    use rusqlite::Error as RError;
    match error {
        RError::SqliteFailure(ffi_error, _) => match ffi_error.code {
            rusqlite::ErrorCode::ConstraintViolation => EmbeddingPersistClass::ConstraintFailed,
            rusqlite::ErrorCode::DatabaseBusy => EmbeddingPersistClass::SqliteBusy,
            rusqlite::ErrorCode::DatabaseLocked => EmbeddingPersistClass::SqliteLocked,
            rusqlite::ErrorCode::DatabaseCorrupt => EmbeddingPersistClass::DbCorrupt,
            rusqlite::ErrorCode::DiskFull => EmbeddingPersistClass::DiskFull,
            rusqlite::ErrorCode::ReadOnly => EmbeddingPersistClass::ReadOnlyStorage,
            _ => EmbeddingPersistClass::OtherStorageFailure,
        },
        // `SqlInputError` is a prepare-time syntax/reference error (SQLite
        // reports an error offset), surfaced only on modern SQLite.
        RError::SqlInputError { .. } => EmbeddingPersistClass::StatementPrepareFailed,
        RError::InvalidParameterCount(..) | RError::InvalidParameterName(_) => {
            EmbeddingPersistClass::SqlBindingFailed
        }
        RError::FromSqlConversionFailure(..)
        | RError::IntegralValueOutOfRange(..)
        | RError::InvalidColumnType(..) => EmbeddingPersistClass::SqlTypeConversionFailed,
        _ => EmbeddingPersistClass::OtherStorageFailure,
    }
}

/// Sanitized name of the underlying [`rusqlite::Error`] variant. Never carries
/// an error message, SQL text, a path, a vector, or a document body.
fn rusqlite_error_kind(error: &rusqlite::Error) -> &'static str {
    use rusqlite::Error as RError;
    match error {
        RError::SqliteFailure(..) => "SqliteFailure",
        RError::SqliteSingleThreadedMode => "SqliteSingleThreadedMode",
        RError::FromSqlConversionFailure(..) => "FromSqlConversionFailure",
        RError::IntegralValueOutOfRange(..) => "IntegralValueOutOfRange",
        RError::Utf8Error(_) => "Utf8Error",
        RError::NulError(_) => "NulError",
        RError::InvalidParameterName(_) => "InvalidParameterName",
        RError::InvalidPath(_) => "InvalidPath",
        RError::ExecuteReturnedResults => "ExecuteReturnedResults",
        RError::QueryReturnedNoRows => "QueryReturnedNoRows",
        RError::QueryReturnedMoreThanOneRow => "QueryReturnedMoreThanOneRow",
        RError::InvalidColumnIndex(_) => "InvalidColumnIndex",
        RError::InvalidColumnName(_) => "InvalidColumnName",
        RError::InvalidColumnType(..) => "InvalidColumnType",
        RError::StatementChangedRows(_) => "StatementChangedRows",
        RError::ToSqlConversionFailure(_) => "ToSqlConversionFailure",
        RError::InvalidQuery => "InvalidQuery",
        RError::UnwindingPanic => "UnwindingPanic",
        RError::MultipleStatement => "MultipleStatement",
        RError::InvalidParameterCount(..) => "InvalidParameterCount",
        RError::SqlInputError { .. } => "SqlInputError",
        _ => "Other",
    }
}

impl KnowledgeError {
    /// Maps a failure to a sanitized structural class plus the SQLite primary
    /// result code when one exists. `stage` is supplied by the caller because
    /// the same `KnowledgeError` variant can arise at different persistence
    /// stages; it is a static label, never a path or SQL fragment.
    pub fn embedding_persist_class(&self, stage: &'static str) -> EmbeddingPersistFailure {
        let (class, sqlite_code, kind) = match self {
            Self::Sql(error) => (
                classify_sqlite_embedding_error(error),
                sqlite_primary_code(error),
                rusqlite_error_kind(error),
            ),
            Self::InvalidEmbedding(message) => {
                if message.contains("384") || message.contains("dimension ") {
                    (
                        EmbeddingPersistClass::VectorDimensionInvalid,
                        None,
                        "InvalidEmbedding",
                    )
                } else {
                    (
                        EmbeddingPersistClass::VectorSerializationFailed,
                        None,
                        "InvalidEmbedding",
                    )
                }
            }
            Self::Io(_) => (EmbeddingPersistClass::IoFailed, None, "Io"),
            _ => (
                EmbeddingPersistClass::OtherStorageFailure,
                None,
                "Knowledge",
            ),
        };
        EmbeddingPersistFailure {
            stage,
            class,
            sqlite_code,
            kind,
        }
    }
}

fn to_sql_error(_error: KnowledgeError) -> rusqlite::Error {
    // A checked enum value in an application-owned table is a schema/data
    // integrity failure. `rusqlite` closures require their own error type;
    // the public boundary maps it back to `KnowledgeError::Sql`.
    rusqlite::Error::InvalidQuery
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialSource {
    pub material_id: MaterialId,
    pub source_name: String,
    /// The project-relative, metadata-validated `inputs/<material-id>/<name>` path.
    pub relative_path: String,
    pub media_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub start_offset: usize,
    pub end_offset: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub heading_path: Vec<String>,
    pub structural_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub id: String,
    pub ordinal: i64,
    pub text: String,
    pub content_sha256: String,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedDocument {
    pub extractor_id: String,
    pub extractor_version: String,
    pub normalized_text: String,
    pub normalized_sha256: String,
    pub chunks: Vec<Chunk>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexOutcome {
    pub document_id: String,
    pub reused: bool,
    pub chunk_count: usize,
}

/// Truthful, durable progress for one explicitly accepted material-import
/// operation. This is deliberately project-local application state, not a
/// general job queue: only an accepted turn may create one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedImportOperation {
    pub operation_id: String,
    pub turn_id: Option<String>,
    pub state: AcceptedImportState,
    /// The remote agent boundary is separate from local Material/Knowledge
    /// progress. A restart must never infer that an outbound request did not
    /// happen merely because operation completion was not persisted.
    pub agent_state: AcceptedImportAgentState,
    pub total: usize,
    pub copied: usize,
    pub lexical_completed: usize,
    pub embedding_completed: usize,
    pub failed: usize,
    pub embeddings_created: usize,
    pub embeddings_reused: usize,
    /// Corpus chunks reachable from this operation's materials (0 before the
    /// embedding phase resolves them).
    pub chunks_total: usize,
    /// Chunks that still require an embedding for the active generation (0
    /// before the embedding phase resolves them).
    pub embeddings_total: usize,
    /// Unix seconds when the embedding phase started; 0 if not started yet.
    /// Used to derive a stable phase throughput, never exposed raw.
    pub embedding_started_at: i64,
    /// The embedding generation the operation's embedding phase resolved, when
    /// one ran. `None` when the phase never resolved chunks (no provider / no
    /// embeddings required), which is the degraded case the frontend treats as
    /// lexically-usable. This is a durable, ONNX-free hint: the user-facing
    /// readiness query is always scoped to the currently active generation, so
    /// stale-generation rows never count.
    pub embedding_generation_id: Option<String>,
    /// Unix seconds the operation was created; used to derive elapsed time.
    pub created_at: i64,
    /// True while the turn's post-embedding synthesis (compact/generic per-item
    /// summary) is running. This is a truthful UI phase, not a lifecycle state:
    /// the operation stays `indexing_embeddings`/`pending_retry` until the
    /// remote boundary completes, and the frontend uses this flag to replace the
    /// misleading "99% · N de N" import line with a synthesis phase. Never set
    /// for the K6 deep route.
    pub synthesizing: bool,
}

/// A durable K6 execution ledger. Summary artifacts themselves remain in the
/// `summaries` tables; this record owns only lifecycle/checkpoint state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryOperation {
    pub operation_id: String,
    pub turn_id: Option<String>,
    pub scope: String,
    pub selected_ids: Vec<String>,
    pub compatibility_fingerprint: String,
    pub model_identity: Option<String>,
    pub status: SummaryOperationStatus,
    pub active_node_id: Option<String>,
    pub active_session_id: Option<String>,
    pub failure_class: Option<String>,
    pub final_summary_id: Option<String>,
    pub nodes_generated: usize,
    pub nodes_reused: usize,
    pub retries: usize,
    pub remote_calls: usize,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryOperationStatus {
    Pending,
    Running,
    Cancelled,
    Failed,
    RetryRequired,
    Completed,
    Stale,
}

impl SummaryOperationStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::RetryRequired => "retry_required",
            Self::Completed => "completed",
            Self::Stale => "stale",
        }
    }
    fn from_db(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            "retry_required" => Ok(Self::RetryRequired),
            "completed" => Ok(Self::Completed),
            "stale" => Ok(Self::Stale),
            _ => Err(KnowledgeError::IncompatibleSchema(-1)),
        }
    }
}

/// Result of committing the final summary artifact together with the terminal
/// `completed` transition. The caller must never treat a `LostToTerminal`
/// outcome as a successful completion and must never re-run provider synthesis
/// while a persisted final artifact exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryCompletionOutcome {
    /// `pending`/`running` -> `completed` succeeded; the row owns the artifact.
    Completed,
    /// The row was already `completed` (idempotent re-completion).
    AlreadyCompleted,
    /// Another legal terminal transition owns the row. The artifact was
    /// persisted but is not referenced by `final_summary_id`.
    LostToTerminal(SummaryOperationStatus),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptedImportState {
    Accepted,
    Copying,
    IndexingLexical,
    IndexingEmbeddings,
    PendingRetry,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptedImportAgentState {
    NotStarted,
    StartedOutcomeUnknown,
    Completed,
    FailedRetryable,
    FailedTerminal,
}

impl AcceptedImportState {
    fn as_db(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Copying => "copying",
            Self::IndexingLexical => "indexing_lexical",
            Self::IndexingEmbeddings => "indexing_embeddings",
            Self::PendingRetry => "pending_retry",
            Self::Completed => "completed",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "copying" => Ok(Self::Copying),
            "indexing_lexical" => Ok(Self::IndexingLexical),
            "indexing_embeddings" => Ok(Self::IndexingEmbeddings),
            "pending_retry" => Ok(Self::PendingRetry),
            "completed" => Ok(Self::Completed),
            _ => Err(KnowledgeError::IncompatibleSchema(-1)),
        }
    }
}

impl AcceptedImportAgentState {
    fn as_db(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::StartedOutcomeUnknown => "started_outcome_unknown",
            Self::Completed => "completed",
            Self::FailedRetryable => "failed_retryable",
            Self::FailedTerminal => "failed_terminal",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "not_started" => Ok(Self::NotStarted),
            "started_outcome_unknown" => Ok(Self::StartedOutcomeUnknown),
            "completed" => Ok(Self::Completed),
            "failed_retryable" => Ok(Self::FailedRetryable),
            "failed_terminal" => Ok(Self::FailedTerminal),
            _ => Err(KnowledgeError::IncompatibleSchema(-1)),
        }
    }
}

/// Durable operational state for one project Material's Knowledge derivation.
/// This is deliberately separate from the Material's acceptance/storage state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterialIndexState {
    Pending,
    Ready,
    Failed,
    Unsupported,
}

impl MaterialIndexState {
    fn as_db(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Unsupported => "unsupported",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            "unsupported" => Ok(Self::Unsupported),
            _ => Err(KnowledgeError::IncompatibleSchema(-1)),
        }
    }
}

/// Sanitized failure category persisted for material indexing. Arbitrary error
/// strings, source text, paths, and provider details are intentionally absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterialIndexFailure {
    UnsupportedFormat,
    InvalidTextEncoding,
    ReadFailed,
    ExtractionFailed,
    ModelUnavailable,
    EmbeddingFailed,
    StorageFailed,
    IntegrityFailed,
}

impl MaterialIndexFailure {
    fn as_db(&self) -> &'static str {
        match self {
            Self::UnsupportedFormat => "unsupported_format",
            Self::InvalidTextEncoding => "invalid_text_encoding",
            Self::ReadFailed => "read_failed",
            Self::ExtractionFailed => "extraction_failed",
            Self::ModelUnavailable => "model_unavailable",
            Self::EmbeddingFailed => "embedding_failed",
            Self::StorageFailed => "storage_failed",
            Self::IntegrityFailed => "integrity_failed",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "unsupported_format" => Ok(Self::UnsupportedFormat),
            "invalid_text_encoding" => Ok(Self::InvalidTextEncoding),
            "read_failed" => Ok(Self::ReadFailed),
            "extraction_failed" => Ok(Self::ExtractionFailed),
            "model_unavailable" => Ok(Self::ModelUnavailable),
            "embedding_failed" => Ok(Self::EmbeddingFailed),
            "storage_failed" => Ok(Self::StorageFailed),
            "integrity_failed" => Ok(Self::IntegrityFailed),
            _ => Err(KnowledgeError::IncompatibleSchema(-1)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialIndexStatus {
    pub material_id: MaterialId,
    pub state: MaterialIndexState,
    pub failure: Option<MaterialIndexFailure>,
    pub retryable: bool,
    pub last_attempt_at: Option<i64>,
    pub updated_at: i64,
}

/// One persisted Knowledge material as surfaced by the local inventory command.
/// Metadata only: never a path, content, or embedding. `material_id` is opaque,
/// `source_name` is the persisted canonical display name, and `indexed_at` is
/// the canonical document import/index timestamp (Unix seconds).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnowledgeMaterialRecord {
    pub material_id: String,
    pub source_name: String,
    pub media_type: Option<String>,
    pub state: MaterialIndexState,
    pub indexed_at: i64,
}

/// Deterministic local ordering for an inventory list. `Unsorted` still yields
/// a stable alphabetical order so every answer is reproducible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventorySort {
    Unsorted,
    ChronologicalAsc,
    ChronologicalDesc,
    Alpha,
}

/// Outcome of a deterministic membership lookup. No semantic matching is ever
/// performed: only normalized-basename / case-insensitive source-name equality.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryMembership {
    /// A unique persisted material matched the needle.
    Exact { material: KnowledgeMaterialRecord },
    /// No persisted material matched the needle.
    NotFound,
    /// Several persisted materials share the same normalized basename; a
    /// deterministic yes/no answer is not possible.
    Ambiguous { count: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchResult {
    pub document_id: String,
    pub source_id: String,
    pub source_name: String,
    pub source_relative_path: String,
    pub chunk_id: String,
    pub chunk_text: String,
    pub score: f64,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddingGeneration {
    pub generation_id: String,
    pub model_id: String,
    pub model_revision: String,
    pub tokenizer_metadata: String,
    pub runtime_backend: String,
    pub runtime_version: String,
    pub dimensions: usize,
    pub max_input_tokens: usize,
    pub query_prefix: String,
    pub passage_prefix: String,
    pub normalization: String,
    pub artifact_variant: String,
}

impl From<&ModelGeneration> for EmbeddingGeneration {
    fn from(model: &ModelGeneration) -> Self {
        Self {
            generation_id: model.generation_id.clone(),
            model_id: model.model_id.clone(),
            model_revision: model.revision.clone(),
            tokenizer_metadata: "tokenizer.json+sentencepiece.bpe.model".to_owned(),
            runtime_backend: model.runtime_backend.clone(),
            runtime_version: model.runtime_version.clone(),
            dimensions: model.dimensions,
            max_input_tokens: model.max_input_tokens,
            query_prefix: model.query_prefix.clone(),
            passage_prefix: model.passage_prefix.clone(),
            normalization: model.normalization.clone(),
            artifact_variant: model.default_artifact_variant.clone(),
        }
    }
}

/// Local-only boundary: implementations may use ONNX Runtime, while the
/// Knowledge store knows only the stable generation and normalized vectors.
pub trait EmbeddingProvider {
    fn generation(&self) -> &EmbeddingGeneration;
    fn embed_query(&mut self, query: &str) -> Result<Vec<f32>>;
    fn embed_passages(&mut self, passages: &[String]) -> Result<Vec<Vec<f32>>>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticSearchResult {
    pub document_id: String,
    pub source_id: String,
    pub source_name: String,
    pub source_relative_path: String,
    pub chunk_id: String,
    pub chunk_text: String,
    pub similarity: f32,
    pub generation_id: String,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddingIndexOutcome {
    pub embedded: usize,
    pub reused: usize,
}

/// Sanitized incremental embedding progress. Counters only: never chunk text,
/// paths, vectors, or prompts. `completed` counts chunks whose vectors were
/// produced so far; `created` mirrors newly produced vectors, and `reused`
/// counts chunks that already had a valid vector before this pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddingIndexProgress {
    pub completed: usize,
    pub total: usize,
    pub created: usize,
    pub reused: usize,
    /// Milliseconds since this embedding pass started.
    pub elapsed_ms: u64,
}

/// Durable accepted-import context. When supplied, the store advances the
/// accepted-import ledger at a bounded cadence as embeddings complete, so the
/// existing polling path observes incremental progress without per-embedding
/// IPC/SQLite churn. The caller has already written the operation's Material
/// counters (`copied`/`lexical_completed`/`failed`); this pass only advances the
/// embedding counters and the phase start.
#[derive(Clone, Copy, Debug)]
pub struct AcceptedImportEmbeddingLedger<'a> {
    pub operation_id: &'a str,
}

/// Bounded controls for project-local hybrid retrieval.  Candidate lists are
/// intentionally overfetched before fusion; no query-specific state is stored.
#[derive(Clone, Debug, PartialEq)]
pub struct HybridSearchOptions {
    pub final_limit: usize,
    pub lexical_candidate_limit: usize,
    pub semantic_candidate_limit: usize,
    pub rrf_k: usize,
    pub max_per_document: usize,
    pub max_per_source: usize,
    /// A bounded additive preference for a literal identifier match. It never
    /// replaces RRF and is only used for identifier-like query tokens.
    pub exact_identifier_boost: f64,
}

impl Default for HybridSearchOptions {
    fn default() -> Self {
        Self {
            final_limit: 10,
            lexical_candidate_limit: 40,
            semantic_candidate_limit: 40,
            rrf_k: 60,
            max_per_document: 3,
            max_per_source: 3,
            exact_identifier_boost: 0.02,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SemanticAvailability {
    Available,
    #[default]
    Unavailable,
}

/// Sanitized, structural reason a local semantic provider is (un)available.
///
/// This is deliberately a closed set of codes, never an exception body: no
/// model path, runtime path, or provider error string is exposed. The local E5
/// provider is the only semantic provider; there is no remote embedding
/// fallback, so an unavailable local provider degrades to lexical retrieval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticProviderState {
    /// Local E5 loaded and a semantic query completed successfully.
    Available,
    /// The semantic provider was deliberately not consulted for this turn
    /// (for example, a corpus-wide thematic synthesis runs on local lexical
    /// aggregation and does not query embeddings).
    NotRequested,
    /// The first-use model directory is not installed.
    ModelNotInstalled,
    /// The model directory exists but is missing one or more artifacts.
    ModelIncomplete,
    /// The model directory exists but an artifact fails checksum/size verification.
    ModelCorrupt,
    /// The bundled ONNX Runtime library cannot be resolved from the executable.
    RuntimeNotFound,
    /// `ort` failed to initialize from the resolved runtime library.
    RuntimeLoadFailed,
    /// The tokenizer could not be loaded.
    TokenizerLoadFailed,
    /// The ONNX session could not be built or its input contract was unexpected.
    ProviderInitializationFailed,
    /// The local provider generated invalid output or could not run inference.
    InferenceFailed,
    /// Derived embedding bytes could not be committed to the local index.
    EmbeddingPersistFailed,
    /// The provider loaded, but a semantic query/embedding failed at runtime.
    SemanticQueryFailed,
    /// Any other local failure not represented by a more specific safe code.
    OtherTypedLocalFailure,
}

impl SemanticProviderState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::NotRequested => "not_requested",
            Self::ModelNotInstalled => "model_not_installed",
            Self::ModelIncomplete => "model_incomplete",
            Self::ModelCorrupt => "model_corrupt",
            Self::RuntimeNotFound => "runtime_not_found",
            Self::RuntimeLoadFailed => "runtime_load_failed",
            Self::TokenizerLoadFailed => "tokenizer_load_failed",
            Self::ProviderInitializationFailed => "provider_initialization_failed",
            Self::InferenceFailed => "inference_failed",
            Self::EmbeddingPersistFailed => "embedding_persist_failed",
            Self::SemanticQueryFailed => "semantic_query_failed",
            Self::OtherTypedLocalFailure => "other_typed_local_failure",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HybridMatchSignals {
    pub lexical_match: bool,
    pub semantic_match: bool,
    pub exact_identifier_match: bool,
    /// K3 does not expand neighbors. Reserved for future context assembly
    /// candidates without changing the evidence contract.
    pub neighbor_of: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HybridSearchResult {
    pub document_id: String,
    pub source_id: String,
    pub source_name: String,
    pub source_relative_path: String,
    pub chunk_id: String,
    pub chunk_text: String,
    pub provenance: Provenance,
    pub lexical_rank: Option<usize>,
    pub semantic_rank: Option<usize>,
    pub lexical_score: Option<f64>,
    pub semantic_score: Option<f32>,
    pub fusion_score: f64,
    pub embedding_generation_id: Option<String>,
    pub signals: HybridMatchSignals,
}

/// Sanitized per-signal candidate counts for one hybrid retrieval. Counts only,
/// never chunk text, queries, paths, or vectors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HybridSearchMetrics {
    pub lexical_candidates: usize,
    pub semantic_candidates: usize,
    pub fused_candidates: usize,
    pub semantic_availability: SemanticAvailability,
}

/// Sanitized, structural corpus/index state for one project. Counts and byte/
/// character/token estimates only; no document bodies, paths, or vector data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KnowledgeCorpusStats {
    /// Number of materials with a durable Knowledge index-state record (the sum
    /// of `pending` + `ready` + `failed` + `unsupported`).
    pub material_count: usize,
    pub pending: usize,
    pub ready: usize,
    pub failed: usize,
    pub unsupported: usize,
    pub chunks_total: usize,
    pub embeddings_ready: usize,
    pub corpus_bytes: u64,
    pub corpus_utf8_chars: usize,
    /// Conservative estimate: `ceil(corpus_bytes / 3)` (same shape as the K4
    /// budget estimator), never a provider tokenizer claim.
    pub naive_corpus_est_tokens: usize,
}

/// SQLite-backed derived Knowledge data for exactly one existing project.
pub struct KnowledgeStore {
    project_id: String,
    database_path: PathBuf,
    connection: Connection,
}

impl KnowledgeStore {
    /// Opens `<canonical-project-root>/knowledge/knowledge.sqlite`.
    ///
    /// The project root must already exist and its final component must be the
    /// supplied project id. Sources are accepted as bytes, never host paths.
    pub fn open(project_root: impl AsRef<Path>, project_id: &ProjectId) -> Result<Self> {
        let root = fs::canonicalize(project_root)?;
        if root.file_name().and_then(|name| name.to_str()) != Some(project_id.as_str())
            || !root.join("project.json").is_file()
        {
            return Err(KnowledgeError::InvalidProjectRoot);
        }
        let knowledge_dir = root.join("knowledge");
        if let Ok(metadata) = fs::symlink_metadata(&knowledge_dir)
            && metadata.file_type().is_symlink()
        {
            return Err(KnowledgeError::InvalidProjectRoot);
        }
        fs::create_dir_all(&knowledge_dir)?;
        let database_path = knowledge_dir.join("knowledge.sqlite");
        let connection = Connection::open(&database_path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        // A concurrent reader/writer (e.g. the reopen-recovery seam racing the
        // live accepted-turn pipeline before its remote-boundary claim) must not
        // fail the whole local index with an immediate SQLITE_BUSY. Wait briefly
        // instead of surfacing `embedding_persist_failed` on a transient lock.
        connection.pragma_update(None, "busy_timeout", 5000_i64)?;
        migrate(&connection)?;
        Ok(Self {
            project_id: project_id.as_str().to_owned(),
            database_path,
            connection,
        })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    /// Creates the durable ledger at the accepted-turn boundary, before any
    /// project-owned Material copy. `operation_id` is supplied by the
    /// application so it can be correlated with a user turn without exposing
    /// a storage implementation detail to the UI.
    pub fn create_accepted_import_operation(
        &mut self,
        operation_id: &str,
        total: usize,
    ) -> Result<()> {
        let now = unix_seconds();
        self.connection.execute(
            "INSERT INTO accepted_import_operations(
                operation_id, turn_id, state, total, copied, lexical_completed,
                embedding_completed, failed, embeddings_created, embeddings_reused, agent_state,
                created_at, updated_at
             ) VALUES(?1, NULL, 'accepted', ?2, 0, 0, 0, 0, 0, 0, 'not_started', ?3, ?3)",
            params![operation_id, total as i64, now],
        )?;
        Ok(())
    }

    /// Records only the final remote-agent boundary. This intentionally does
    /// not alter local Material/Knowledge progress counters.
    pub fn update_accepted_import_agent_state(
        &mut self,
        operation_id: &str,
        agent_state: AcceptedImportAgentState,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE accepted_import_operations SET agent_state=?2, updated_at=?3 WHERE operation_id=?1",
            params![operation_id, agent_state.as_db(), unix_seconds()],
        )?;
        Ok(())
    }

    /// Truthful UI synthesis phase: marks whether the turn's post-embedding
    /// synthesis (compact/generic per-item summary) is currently running. This
    /// is a presentation flag, never a lifecycle state; it is set before the
    /// remote boundary runs and cleared when the operation reaches a terminal
    /// state.
    pub fn set_accepted_import_synthesizing(
        &mut self,
        operation_id: &str,
        synthesizing: bool,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE accepted_import_operations SET synthesizing=?2, updated_at=?3 WHERE operation_id=?1",
            params![operation_id, i64::from(synthesizing), unix_seconds()],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // mirrors the durable ledger columns exactly
    pub fn update_accepted_import_operation(
        &mut self,
        operation_id: &str,
        turn_id: Option<&str>,
        state: AcceptedImportState,
        copied: usize,
        lexical_completed: usize,
        embedding_completed: usize,
        failed: usize,
        embeddings_created: usize,
        embeddings_reused: usize,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE accepted_import_operations SET turn_id=COALESCE(?2, turn_id),
                state=?3, copied=?4, lexical_completed=?5, embedding_completed=?6,
                failed=?7, embeddings_created=?8, embeddings_reused=?9, updated_at=?10
             WHERE operation_id=?1",
            params![
                operation_id,
                turn_id,
                state.as_db(),
                copied as i64,
                lexical_completed as i64,
                embedding_completed as i64,
                failed as i64,
                embeddings_created as i64,
                embeddings_reused as i64,
                unix_seconds()
            ],
        )?;
        Ok(())
    }

    /// Advances the durable accepted-import ledger while a bounded embedding
    /// phase runs. `embedding_started_at` is captured once (`COALESCE`) so
    /// throughput can be derived from a stable phase start. `generation_id` is
    /// captured once (`COALESCE`) from the embedding-progress path, where the
    /// generation is already known without touching ONNX. This is a bounded
    /// cadence update, never a per-embedding write.
    #[allow(clippy::too_many_arguments)] // mirrors the durable ledger columns exactly
    pub fn update_accepted_import_embedding_progress(
        &mut self,
        operation_id: &str,
        chunks_total: usize,
        embeddings_total: usize,
        embedding_completed: usize,
        embeddings_created: usize,
        embeddings_reused: usize,
        embedding_started_at: Option<i64>,
        generation_id: Option<&str>,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE accepted_import_operations
             SET state='indexing_embeddings',
                 chunks_total=?2,
                 embeddings_total=?3,
                 embedding_completed=?4,
                 embeddings_created=?5,
                 embeddings_reused=?6,
                 embedding_started_at=COALESCE(?7, embedding_started_at),
                 embedding_generation_id=COALESCE(?8, embedding_generation_id),
                 updated_at=?9
             WHERE operation_id=?1",
            params![
                operation_id,
                chunks_total as i64,
                embeddings_total as i64,
                embedding_completed as i64,
                embeddings_created as i64,
                embeddings_reused as i64,
                embedding_started_at,
                generation_id,
                unix_seconds()
            ],
        )?;
        Ok(())
    }

    /// Records that the embedding phase has resolved an active generation,
    /// before the failure-prone chunk-selection/persistence work runs. This
    /// makes `embedding_generation_id` durable even when a later step fails, so
    /// the user-facing readiness query stays generation-scoped (strict) instead
    /// of silently falling back to the lexical-only degraded rule. A failed
    /// embedding must never masquerade as a healthy lexical-only import.
    pub fn mark_accepted_import_embedding_generation(
        &mut self,
        operation_id: &str,
        generation_id: &str,
    ) -> Result<()> {
        let now = unix_seconds();
        self.connection.execute(
            "UPDATE accepted_import_operations
             SET state='indexing_embeddings',
                 embedding_started_at=COALESCE(embedding_started_at, ?2),
                 embedding_generation_id=COALESCE(embedding_generation_id, ?3),
                 updated_at=?4
             WHERE operation_id=?1",
            params![operation_id, now, generation_id, now],
        )?;
        Ok(())
    }

    /// Corrects the operation's `total` to the number of distinct accepted
    /// Materials. A selection can contain duplicate-in-batch sources (same
    /// content hash) that collapse to a single Material; the durable counter
    /// must reflect distinct Materials so the progress denominators never count
    /// the same file twice and can reach completion.
    pub fn set_accepted_import_operation_total(
        &mut self,
        operation_id: &str,
        total: usize,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE accepted_import_operations SET total=?2, updated_at=?3 WHERE operation_id=?1",
            params![operation_id, total as i64, unix_seconds()],
        )?;
        Ok(())
    }

    /// Binds an already persisted project Material to the accepted operation.
    /// The material remains the source of truth for its own Pending/Ready/
    /// Failed/Unsupported state; this table only makes recovery ownership
    /// explicit.
    pub fn bind_accepted_import_material(
        &mut self,
        operation_id: &str,
        material_id: &MaterialId,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT OR IGNORE INTO accepted_import_materials(operation_id, material_id)
             VALUES(?1, ?2)",
            params![operation_id, material_id.as_str()],
        )?;
        Ok(())
    }

    pub fn accepted_import_material_ids(&self, operation_id: &str) -> Result<Vec<MaterialId>> {
        let mut statement = self.connection.prepare(
            "SELECT material_id FROM accepted_import_materials WHERE operation_id=?1 ORDER BY material_id",
        )?;
        statement
            .query_map([operation_id], |row| {
                MaterialId::parse(row.get::<_, String>(0)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Counts the accepted-import operation's materials that are fully usable
    /// for the given embedding generation.
    ///
    /// A material counts ready when:
    ///   1. its lexical/indexing stage is complete (`material_index_state` is
    ///      `ready`); AND
    ///   2. every chunk reachable from its document has a `ready` embedding for
    ///      the active generation.
    ///
    /// A zero-chunk material counts ready once its lexical stage is complete
    /// (the inner `NOT EXISTS` is vacuously satisfied).
    ///
    /// This is deliberately distinct from the internal lexical
    /// [`MaterialIndexState::Ready`]: that state alone must never increment the
    /// user-facing `materialsReady` count.
    ///
    /// `generation_id` is `Some` for the strict semantic rule. `None` selects
    /// the explicit degraded rule consistent with the current frontend
    /// `semanticReady` fallback: when embeddings are genuinely not available for
    /// this operation, a lexically-ready material is treated as usable. The
    /// caller must not mix both units; this method is all-or-nothing per call.
    ///
    /// Read-only and bounded: it never mutates Knowledge, regenerates
    /// embeddings, or issues provider calls. The plan stays on the
    /// `accepted_import_materials` primary key, the `material_sources` primary
    /// key, the `chunks_document_ordinal` index, and the `chunk_embeddings`
    /// primary key — never a full-corpus scan.
    pub fn accepted_import_materials_ready(
        &self,
        operation_id: &str,
        generation_id: Option<&str>,
    ) -> Result<usize> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*)
                 FROM accepted_import_materials aim
                 JOIN material_index_state mis ON mis.material_id = aim.material_id
                 WHERE aim.operation_id = ?1
                   AND mis.state = 'ready'
                   AND (
                     ?2 IS NULL
                     OR NOT EXISTS (
                       SELECT 1
                       FROM material_sources ms
                       JOIN chunks c ON c.document_id = ms.document_id
                       WHERE ms.material_id = aim.material_id
                         AND NOT EXISTS (
                           SELECT 1 FROM chunk_embeddings e
                           WHERE e.chunk_id = c.chunk_id
                             AND e.generation_id = ?2
                             AND e.state = 'ready'
                             AND e.dimensions = 384
                         )
                     )
                   )",
            params![operation_id, generation_id],
            |row| row.get::<_, i64>(0),
        )? as usize)
    }

    pub fn accepted_import_operation(
        &self,
        operation_id: &str,
    ) -> Result<Option<AcceptedImportOperation>> {
        self.connection
            .query_row(
                "SELECT operation_id, turn_id, state, agent_state, total, copied, lexical_completed,
                    embedding_completed, failed, embeddings_created, embeddings_reused,
                    chunks_total, embeddings_total, embedding_started_at, embedding_generation_id,
                    created_at, synthesizing
             FROM accepted_import_operations WHERE operation_id=?1",
                [operation_id],
                |row| {
                    Ok(AcceptedImportOperation {
                        operation_id: row.get(0)?,
                        turn_id: row.get(1)?,
                        state: AcceptedImportState::from_db(&row.get::<_, String>(2)?)
                            .map_err(to_sql_error)?,
                        agent_state: AcceptedImportAgentState::from_db(
                            &row.get::<_, String>(3)?,
                        )
                        .map_err(to_sql_error)?,
                        total: row.get::<_, i64>(4)? as usize,
                        copied: row.get::<_, i64>(5)? as usize,
                        lexical_completed: row.get::<_, i64>(6)? as usize,
                        embedding_completed: row.get::<_, i64>(7)? as usize,
                        failed: row.get::<_, i64>(8)? as usize,
                        embeddings_created: row.get::<_, i64>(9)? as usize,
                        embeddings_reused: row.get::<_, i64>(10)? as usize,
                        chunks_total: row.get::<_, i64>(11)? as usize,
                        embeddings_total: row.get::<_, i64>(12)? as usize,
                        embedding_started_at: row.get::<_, i64>(13)?,
                        embedding_generation_id: row.get(14)?,
                        created_at: row.get::<_, i64>(15)?,
                        synthesizing: row.get::<_, i64>(16)? != 0,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Incomplete operations are intentionally surfaced to recovery; opening
    /// a store never advances them, so the application cannot silently run
    /// arbitrary background work after restart.
    pub fn incomplete_accepted_import_operations(&self) -> Result<Vec<AcceptedImportOperation>> {
        let mut statement = self.connection.prepare(
            "SELECT operation_id, turn_id, state, agent_state, total, copied, lexical_completed,
                    embedding_completed, failed, embeddings_created, embeddings_reused,
                    chunks_total, embeddings_total, embedding_started_at, embedding_generation_id,
                    created_at, synthesizing
             FROM accepted_import_operations WHERE state != 'completed' ORDER BY created_at",
        )?;
        statement
            .query_map([], |row| {
                Ok(AcceptedImportOperation {
                    operation_id: row.get(0)?,
                    turn_id: row.get(1)?,
                    state: AcceptedImportState::from_db(&row.get::<_, String>(2)?)
                        .map_err(to_sql_error)?,
                    agent_state: AcceptedImportAgentState::from_db(&row.get::<_, String>(3)?)
                        .map_err(to_sql_error)?,
                    total: row.get::<_, i64>(4)? as usize,
                    copied: row.get::<_, i64>(5)? as usize,
                    lexical_completed: row.get::<_, i64>(6)? as usize,
                    embedding_completed: row.get::<_, i64>(7)? as usize,
                    failed: row.get::<_, i64>(8)? as usize,
                    embeddings_created: row.get::<_, i64>(9)? as usize,
                    embeddings_reused: row.get::<_, i64>(10)? as usize,
                    chunks_total: row.get::<_, i64>(11)? as usize,
                    embeddings_total: row.get::<_, i64>(12)? as usize,
                    embedding_started_at: row.get::<_, i64>(13)?,
                    embedding_generation_id: row.get(14)?,
                    created_at: row.get::<_, i64>(15)?,
                    synthesizing: row.get::<_, i64>(16)? != 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Most recent accepted operation for the project.  This is a compact UI
    /// read model; the ledger remains the durable recovery source of truth.
    pub fn latest_accepted_import_operation(&self) -> Result<Option<AcceptedImportOperation>> {
        self.connection
            .query_row(
                "SELECT operation_id, turn_id, state, agent_state, total, copied, lexical_completed,
                    embedding_completed, failed, embeddings_created, embeddings_reused,
                    chunks_total, embeddings_total, embedding_started_at, embedding_generation_id,
                    created_at, synthesizing
                 FROM accepted_import_operations ORDER BY created_at DESC LIMIT 1",
                [],
                |row| {
                    Ok(AcceptedImportOperation {
                        operation_id: row.get(0)?,
                        turn_id: row.get(1)?,
                        state: AcceptedImportState::from_db(&row.get::<_, String>(2)?)
                            .map_err(to_sql_error)?,
                        agent_state: AcceptedImportAgentState::from_db(
                            &row.get::<_, String>(3)?,
                        )
                        .map_err(to_sql_error)?,
                        total: row.get::<_, i64>(4)? as usize,
                        copied: row.get::<_, i64>(5)? as usize,
                        lexical_completed: row.get::<_, i64>(6)? as usize,
                        embedding_completed: row.get::<_, i64>(7)? as usize,
                        failed: row.get::<_, i64>(8)? as usize,
                        embeddings_created: row.get::<_, i64>(9)? as usize,
                        embeddings_reused: row.get::<_, i64>(10)? as usize,
                        chunks_total: row.get::<_, i64>(11)? as usize,
                        embeddings_total: row.get::<_, i64>(12)? as usize,
                        embedding_started_at: row.get::<_, i64>(13)?,
                        embedding_generation_id: row.get(14)?,
                        created_at: row.get::<_, i64>(15)?,
                        synthesizing: row.get::<_, i64>(16)? != 0,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Whether this project-local index owns a material source. This exposes
    /// only activation/deduplication state, never source text.
    pub fn has_material_source(&self, material_id: &str) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM material_sources WHERE material_id=?1 LIMIT 1",
                [material_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )?
            .parse()
            .expect("schema version is controlled"))
    }

    /// Replaces the conversation-active material set with the given ordered
    /// ids. This is the exact material set the user last explicitly attached /
    /// accepted in this conversation; it is used as the cross-turn fallback for
    /// a no-attachment per-source summary. An empty input is a no-op: only
    /// non-empty sets are ever written, so a failed/empty import can never
    /// silently erase a valid active set.
    pub fn set_conversation_active_material_set(
        &mut self,
        material_ids: &[MaterialId],
    ) -> Result<()> {
        if material_ids.is_empty() {
            return Ok(());
        }
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM conversation_active_material_set", [])?;
        let now = unix_seconds();
        for (ordinal, material_id) in material_ids.iter().enumerate() {
            tx.execute(
                "INSERT INTO conversation_active_material_set(ordinal, material_id, updated_at)
                 VALUES(?1, ?2, ?3)",
                params![ordinal as i64, material_id.as_str(), now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Reads the conversation-active material set in original attach order.
    /// Empty when no explicit material set has been accepted in this
    /// conversation yet.
    pub fn conversation_active_material_ids(&self) -> Result<Vec<MaterialId>> {
        let mut statement = self
            .connection
            .prepare("SELECT material_id FROM conversation_active_material_set ORDER BY ordinal")?;
        statement
            .query_map([], |row| {
                MaterialId::parse(row.get::<_, String>(0)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Sanitized display names of every READY-indexed source in this project.
    /// Used by the request boundary to suppress raw workspace attachment
    /// forwarding for supported documents the product already serves through
    /// bounded Knowledge retrieval. Only the source display name is returned —
    /// never a path, id, or content.
    pub fn ready_source_names(&self) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT ms.source_name FROM material_sources ms
             JOIN material_index_state mis ON mis.material_id = ms.material_id
             WHERE mis.state = 'ready' ORDER BY ms.source_name",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Local inventory snapshot over persisted Knowledge materials. Metadata
    /// only: reads `material_sources` / `material_index_state` / `documents`,
    /// never the filesystem and never the agent workspace. `state` filters the
    /// returned rows (`Some(Ready)` is the default plain-inventory semantics;
    /// `None` includes pending/failed/unsupported). The ordering is applied
    /// deterministically in-process; the preferred chronological key is
    /// `documents.indexed_at` with a documented fallback to
    /// `material_sources.updated_at` for a row whose canonical document is
    /// absent.
    pub fn inventory_snapshot(
        &self,
        state: Option<MaterialIndexState>,
        sort: InventorySort,
    ) -> Result<Vec<KnowledgeMaterialRecord>> {
        let mut records = self.all_inventory_records(state)?;
        match sort {
            InventorySort::ChronologicalAsc => records.sort_by(|left, right| {
                left.indexed_at
                    .cmp(&right.indexed_at)
                    .then_with(|| left.source_name.cmp(&right.source_name))
            }),
            InventorySort::ChronologicalDesc => records.sort_by(|left, right| {
                right
                    .indexed_at
                    .cmp(&left.indexed_at)
                    .then_with(|| left.source_name.cmp(&right.source_name))
            }),
            InventorySort::Alpha | InventorySort::Unsorted => records.sort_by(|left, right| {
                left.source_name
                    .cmp(&right.source_name)
                    .then_with(|| left.indexed_at.cmp(&right.indexed_at))
            }),
        }
        Ok(records)
    }

    /// Exact local count of persisted materials matching the given state
    /// filter. Plain inventory semantics default to READY.
    pub fn inventory_count(&self, state: Option<MaterialIndexState>) -> Result<usize> {
        let state_db = state.as_ref().map(MaterialIndexState::as_db);
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM material_sources ms
             JOIN material_index_state mis ON mis.material_id = ms.material_id
             WHERE (?1 IS NULL OR mis.state = ?1)",
            [state_db],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count as usize)
    }

    /// Deterministic local metadata lookup for one named material. Matching is
    /// equality only (normalized basename first, then exact case-insensitive
    /// `source_name`): no provider, no semantic search, no fuzzy matching. The
    /// returned record preserves the persisted canonical display name.
    pub fn find_material_by_source_name(
        &self,
        needle: &str,
        state: Option<MaterialIndexState>,
    ) -> Result<InventoryMembership> {
        let needle = needle.trim().to_lowercase();
        let needle_basename = needle.rsplit(['/', '\\']).next().unwrap_or(&needle);
        let mut basename_matches: Vec<KnowledgeMaterialRecord> = Vec::new();
        for record in self.all_inventory_records(state)? {
            let name_lower = record.source_name.to_lowercase();
            let name_basename = name_lower.rsplit(['/', '\\']).next().unwrap_or(&name_lower);
            if name_basename == needle_basename {
                basename_matches.push(record);
            }
        }
        if basename_matches.is_empty() {
            return Ok(InventoryMembership::NotFound);
        }
        let exact_in_set: Vec<KnowledgeMaterialRecord> = basename_matches
            .iter()
            .filter(|record| record.source_name.to_lowercase() == needle)
            .cloned()
            .collect();
        if exact_in_set.len() == 1 {
            return Ok(InventoryMembership::Exact {
                material: exact_in_set.into_iter().next().expect("len == 1"),
            });
        }
        if basename_matches.len() == 1 {
            return Ok(InventoryMembership::Exact {
                material: basename_matches.pop().expect("len == 1"),
            });
        }
        Ok(InventoryMembership::Ambiguous {
            count: basename_matches.len(),
        })
    }

    fn all_inventory_records(
        &self,
        state: Option<MaterialIndexState>,
    ) -> Result<Vec<KnowledgeMaterialRecord>> {
        let state_db = state.as_ref().map(MaterialIndexState::as_db);
        let mut statement = self.connection.prepare(
            "SELECT ms.material_id, ms.source_name, ms.media_type, mis.state,
                    COALESCE(d.indexed_at, ms.updated_at)
             FROM material_sources ms
             JOIN material_index_state mis ON mis.material_id = ms.material_id
             LEFT JOIN documents d ON d.document_id = ms.document_id
             WHERE (?1 IS NULL OR mis.state = ?1)
             ORDER BY ms.source_name, ms.material_id",
        )?;
        let rows = statement.query_map([state_db], |row| {
            Ok(KnowledgeMaterialRecord {
                material_id: row.get(0)?,
                source_name: row.get(1)?,
                media_type: row.get(2)?,
                state: MaterialIndexState::from_db(&row.get::<_, String>(3)?)
                    .map_err(to_sql_error)?,
                indexed_at: row.get(4)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Structural corpus/index state for observability. Counts and byte/
    /// character/token estimates only; never document bodies, paths, or vectors.
    pub fn corpus_stats(&self) -> Result<KnowledgeCorpusStats> {
        let material_count =
            self.connection
                .query_row("SELECT COUNT(*) FROM material_index_state", [], |row| {
                    row.get::<_, i64>(0)
                })? as usize;
        let mut state_counts = std::collections::BTreeMap::new();
        let mut statement = self
            .connection
            .prepare("SELECT state, COUNT(*) FROM material_index_state GROUP BY state")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;
        for row in rows {
            let (state, count) = row?;
            state_counts.insert(state, count);
        }
        let chunks_total = self
            .connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| {
                row.get::<_, i64>(0)
            })? as usize;
        let embeddings_ready = self.connection.query_row(
            "SELECT COUNT(*) FROM chunk_embeddings WHERE state='ready'",
            [],
            |row| row.get::<_, i64>(0),
        )? as usize;
        // Corpus sizing must count each canonical document's bytes once and
        // each chunk's text once. A single documents-chunks JOIN would multiply
        // `SUM(byte_size)` by the chunk multiplicity, so the two aggregates are
        // taken from separate tables.
        let (corpus_bytes, corpus_utf8_chars): (i64, i64) = self.connection.query_row(
            "SELECT
               (SELECT COALESCE(SUM(byte_size), 0) FROM documents),
               (SELECT COALESCE(SUM(LENGTH(text)), 0) FROM chunks)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let corpus_bytes = u64::try_from(corpus_bytes).unwrap_or(u64::MAX);
        let corpus_utf8_chars = usize::try_from(corpus_utf8_chars).unwrap_or(0);
        Ok(KnowledgeCorpusStats {
            material_count,
            pending: state_counts.get("pending").copied().unwrap_or(0),
            ready: state_counts.get("ready").copied().unwrap_or(0),
            failed: state_counts.get("failed").copied().unwrap_or(0),
            unsupported: state_counts.get("unsupported").copied().unwrap_or(0),
            chunks_total,
            embeddings_ready,
            corpus_bytes,
            corpus_utf8_chars,
            naive_corpus_est_tokens: (corpus_bytes / 3) as usize
                + usize::from(corpus_bytes % 3 != 0),
        })
    }

    /// Returns this project's durable operational Knowledge state for a
    /// Material. A missing row means this Material has not been submitted to
    /// the Knowledge indexing boundary yet.
    pub fn material_index_status(
        &self,
        material_id: &MaterialId,
    ) -> Result<Option<MaterialIndexStatus>> {
        let row = self
            .connection
            .query_row(
                "SELECT state, failure_category, retryable, last_attempt_at, updated_at
                 FROM material_index_state WHERE material_id=?1",
                [material_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, i64>(2)? != 0,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(state, failure, retryable, last_attempt_at, updated_at)| {
            Ok(MaterialIndexStatus {
                material_id: material_id.clone(),
                state: MaterialIndexState::from_db(&state)?,
                failure: failure
                    .as_deref()
                    .map(MaterialIndexFailure::from_db)
                    .transpose()?,
                retryable,
                last_attempt_at,
                updated_at,
            })
        })
        .transpose()
    }

    /// Persists `Pending` before an indexing attempt. Unsupported formats are
    /// instead durably marked `Unsupported`; their bytes are never treated as
    /// indexed. This commit is intentionally separate from Material storage.
    pub fn begin_material_indexing(
        &mut self,
        source: &MaterialSource,
    ) -> Result<MaterialIndexState> {
        validate_source(source)?;
        let now = unix_seconds();
        match extractor_contract(source) {
            Ok(_) => {
                self.upsert_material_index_state(
                    &source.material_id,
                    MaterialIndexState::Pending,
                    None,
                    true,
                    Some(now),
                    now,
                )?;
                Ok(MaterialIndexState::Pending)
            }
            Err(KnowledgeError::UnsupportedFormat(_)) => {
                self.upsert_material_index_state(
                    &source.material_id,
                    MaterialIndexState::Unsupported,
                    Some(MaterialIndexFailure::UnsupportedFormat),
                    false,
                    Some(now),
                    now,
                )?;
                Ok(MaterialIndexState::Unsupported)
            }
            Err(error) => Err(error),
        }
    }

    /// Indexes bytes from an already authorized immutable project input.
    /// No provider or process adapter is reachable from this code path. It
    /// commits `Pending` first, then commits `Ready` or a sanitized `Failed`
    /// result, so a crash cannot silently claim readiness.
    pub fn index(&mut self, source: &MaterialSource, bytes: &[u8]) -> Result<IndexOutcome> {
        if self.begin_material_indexing(source)? == MaterialIndexState::Unsupported {
            return Err(KnowledgeError::UnsupportedFormat(
                source
                    .media_type
                    .clone()
                    .unwrap_or_else(|| source.source_name.clone()),
            ));
        }
        let prior_document: Option<String> = self
            .connection
            .query_row(
                "SELECT document_id FROM material_sources WHERE material_id = ?1",
                [source.material_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        match self.index_derived(source, bytes) {
            Ok(outcome) => {
                let changed = prior_document.as_deref() != Some(outcome.document_id.as_str())
                    || !outcome.reused;
                let now = unix_seconds();
                self.upsert_material_index_state(
                    &source.material_id,
                    MaterialIndexState::Ready,
                    None,
                    false,
                    None,
                    now,
                )?;
                // Incremental invalidation: a changed or renewed document
                // invalidates only its own summary lineage (and, when the
                // canonical document identity changed, the previous identity's).
                if changed {
                    self.invalidate_document_summaries(&outcome.document_id)?;
                    if let Some(prior) = prior_document
                        && prior != outcome.document_id
                    {
                        self.invalidate_document_summaries(&prior)?;
                    }
                }
                Ok(outcome)
            }
            Err(error) => {
                let now = unix_seconds();
                self.upsert_material_index_state(
                    &source.material_id,
                    MaterialIndexState::Failed,
                    Some(index_failure_category(&error)),
                    true,
                    Some(now),
                    now,
                )?;
                Err(error)
            }
        }
    }

    /// Explicit retry boundary for a previously failed or crash-interrupted
    /// Material. It remains synchronous; this crate starts no worker.
    pub fn retry_material_index(
        &mut self,
        source: &MaterialSource,
        bytes: &[u8],
    ) -> Result<IndexOutcome> {
        self.index(source, bytes)
    }

    fn index_derived(&mut self, source: &MaterialSource, bytes: &[u8]) -> Result<IndexOutcome> {
        validate_source(source)?;
        let (extractor_id, extractor_version) = extractor_contract(source)?;
        let original_sha256 = sha256_hex(bytes);
        let document_id = original_sha256.clone();
        let byte_size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
        let now = unix_seconds();
        let tx = self.connection.transaction()?;
        let reusable = document_is_current(&tx, &document_id, &extractor_id, &extractor_version)?;
        let existing_document: Option<String> = tx
            .query_row(
                "SELECT document_id FROM material_sources WHERE material_id = ?1",
                [&source.material_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;

        if existing_document.as_deref() != Some(&document_id) && existing_document.is_some() {
            tx.execute(
                "DELETE FROM material_sources WHERE material_id = ?1",
                [&source.material_id.as_str()],
            )?;
            cleanup_orphaned_documents(&tx)?;
        }
        let chunk_count = if reusable {
            tx.query_row(
                "SELECT COUNT(*) FROM chunks WHERE document_id = ?1",
                [&document_id],
                |row| row.get::<_, i64>(0),
            )? as usize
        } else {
            let extracted = extract(source, bytes)?;
            replace_document(
                &tx,
                &document_id,
                &original_sha256,
                byte_size,
                &extracted,
                now,
            )?;
            extracted.chunks.len()
        };
        tx.execute(
            "INSERT INTO material_sources(material_id, document_id, source_name, source_relative_path, media_type, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(material_id) DO UPDATE SET document_id=excluded.document_id, source_name=excluded.source_name,
                 source_relative_path=excluded.source_relative_path, media_type=excluded.media_type, updated_at=excluded.updated_at",
            params![source.material_id.as_str(), document_id, source.source_name, source.relative_path, source.media_type, now],
        )?;
        tx.commit()?;
        Ok(IndexOutcome {
            document_id,
            reused: reusable,
            chunk_count,
        })
    }

    fn upsert_material_index_state(
        &self,
        material_id: &MaterialId,
        state: MaterialIndexState,
        failure: Option<MaterialIndexFailure>,
        retryable: bool,
        last_attempt_at: Option<i64>,
        updated_at: i64,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO material_index_state(material_id, state, failure_category, retryable, last_attempt_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(material_id) DO UPDATE SET state=excluded.state, failure_category=excluded.failure_category,
                 retryable=excluded.retryable, last_attempt_at=excluded.last_attempt_at, updated_at=excluded.updated_at",
            params![material_id.as_str(), state.as_db(), failure.as_ref().map(MaterialIndexFailure::as_db), i64::from(retryable), last_attempt_at, updated_at],
        )?;
        Ok(())
    }

    /// Removes one source link. Shared same-byte canonical documents remain
    /// available until their last source is removed.
    pub fn remove(&mut self, material_id: &MaterialId) -> Result<bool> {
        let prior_document: Option<String> = self
            .connection
            .query_row(
                "SELECT document_id FROM material_sources WHERE material_id = ?1",
                [material_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        let tx = self.connection.transaction()?;
        let removed = tx.execute(
            "DELETE FROM material_sources WHERE material_id = ?1",
            [material_id.as_str()],
        )? > 0;
        tx.execute(
            "DELETE FROM material_index_state WHERE material_id = ?1",
            [material_id.as_str()],
        )?;
        cleanup_orphaned_documents(&tx)?;
        tx.commit()?;
        if let Some(document_id) = prior_document {
            // Invalidate summary lineage that claimed support from this
            // document. Unrelated summaries remain available.
            let _ = self.invalidate_document_summaries(&document_id);
        }
        Ok(removed)
    }

    /// Local lexical retrieval only. Raw FTS syntax is never interpolated.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let query = fts_query(query);
        if query.is_empty() || limit == 0 {
            return Ok(vec![]);
        }
        let limit = i64::try_from(limit.min(100)).unwrap_or(100);
        let mut statement = self.connection.prepare(
            "SELECT c.document_id, ms.material_id, ms.source_name, ms.source_relative_path, c.chunk_id, c.text,
                    -bm25(chunk_fts) AS score, c.start_offset, c.end_offset, c.start_line, c.end_line,
                    c.heading_path, c.structural_type
             FROM chunk_fts
             JOIN chunks c ON c.chunk_id = chunk_fts.chunk_id
             JOIN material_sources ms ON ms.document_id = c.document_id
             WHERE chunk_fts MATCH ?1
             ORDER BY bm25(chunk_fts), c.ordinal
             LIMIT ?2"
        )?;
        let rows = statement.query_map(params![query, limit], |row| {
            let headings: String = row.get(11)?;
            Ok(SearchResult {
                document_id: row.get(0)?,
                source_id: row.get(1)?,
                source_name: row.get(2)?,
                source_relative_path: row.get(3)?,
                chunk_id: row.get(4)?,
                chunk_text: row.get(5)?,
                score: row.get(6)?,
                provenance: Provenance {
                    start_offset: row.get(7)?,
                    end_offset: row.get(8)?,
                    start_line: row.get(9)?,
                    end_line: row.get(10)?,
                    heading_path: headings
                        .split('\u{1f}')
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect(),
                    structural_type: row.get(12)?,
                },
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn record_generation(&mut self, generation: &EmbeddingGeneration) -> Result<()> {
        if generation.dimensions != 384 || generation.normalization != "l2-f32-le-v1" {
            return Err(KnowledgeError::InvalidEmbedding(
                "unsupported generation vector contract".to_owned(),
            ));
        }
        self.connection.execute(
            "INSERT INTO embedding_generations(generation_id, model_id, model_revision, tokenizer_metadata, runtime_backend, runtime_version, dimensions, max_input_tokens, query_prefix, passage_prefix, normalization, artifact_variant, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(generation_id) DO NOTHING",
            params![generation.generation_id, generation.model_id, generation.model_revision, generation.tokenizer_metadata, generation.runtime_backend, generation.runtime_version, generation.dimensions, generation.max_input_tokens, generation.query_prefix, generation.passage_prefix, generation.normalization, generation.artifact_variant, unix_seconds()],
        )?;
        Ok(())
    }

    /// Embeds only chunks that lack a valid vector for the requested immutable
    /// generation. Vectors are inserted only after all bounded batches succeed.
    pub fn index_embeddings(
        &mut self,
        provider: &mut dyn EmbeddingProvider,
        batch_size: usize,
    ) -> Result<EmbeddingIndexOutcome> {
        self.index_embeddings_for_materials(provider, batch_size, &[])
    }

    /// Batch boundary for an accepted import. A non-empty material list limits
    /// selection to chunks reachable from those materials; it never performs a
    /// repeated whole-corpus scan for every newly accepted file. An empty list
    /// preserves the existing explicit full-repair/rebuild behavior.
    pub fn index_embeddings_for_materials(
        &mut self,
        provider: &mut dyn EmbeddingProvider,
        batch_size: usize,
        material_ids: &[MaterialId],
    ) -> Result<EmbeddingIndexOutcome> {
        self.index_embeddings_for_materials_reporting(
            provider,
            batch_size,
            material_ids,
            None,
            None,
        )
    }

    /// [`Self::index_embeddings_for_materials`] with bounded progress
    /// reporting. `ledger` advances the durable accepted-import operation at a
    /// bounded cadence so the existing polling path observes incremental
    /// progress; `progress` (when present) receives sanitized counters at the
    /// same cadence for observability. Neither is invoked per embedding.
    ///
    /// Pending chunks are embedded in length-aware order (short chunks batch
    /// together, long chunks batch together) via [`sort_pending_by_length`], so
    /// fixed-size batches pad to a tight width. Inference order may differ from
    /// chunk identity order, but the selected chunk set, the chunk→vector
    /// association, and the logical stored-chunk order are unchanged.
    pub fn index_embeddings_for_materials_reporting(
        &mut self,
        provider: &mut dyn EmbeddingProvider,
        batch_size: usize,
        material_ids: &[MaterialId],
        ledger: Option<AcceptedImportEmbeddingLedger<'_>>,
        mut progress: Option<&mut dyn FnMut(EmbeddingIndexProgress)>,
    ) -> Result<EmbeddingIndexOutcome> {
        let generation = provider.generation().clone();
        self.record_generation(&generation)?;
        // Persist the resolved generation id durably BEFORE the failure-prone
        // chunk-selection/persistence work, so a failed embedding leaves the
        // operation marked generation-scoped. Otherwise a failure here would
        // leave `embedding_generation_id` unset and the readiness query would
        // silently downgrade to the lexical-only degraded rule, reporting a
        // failed semantic import as if it were fully usable.
        if let Some(ledger) = ledger {
            self.mark_accepted_import_embedding_generation(
                ledger.operation_id,
                &generation.generation_id,
            )?;
        }
        let scoped = !material_ids.is_empty();
        let ids = material_ids
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>();
        // The generation selector must not collide with the positional `?`
        // material placeholders: a bare `?` auto-numbers to index 1 and would
        // alias an explicit `?1`. Give the generation an explicit index that
        // follows every material placeholder (`ids.len() + 1`) and bind the
        // material ids first, generation last, so each declared parameter is
        // bound exactly once.
        let generation_index = ids.len() + 1;
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let query = if scoped {
            format!(
                "SELECT DISTINCT c.chunk_id, c.text FROM chunks c
                JOIN material_sources ms ON ms.document_id=c.document_id
                WHERE ms.material_id IN ({placeholders}) AND NOT EXISTS (
                    SELECT 1 FROM chunk_embeddings e WHERE e.chunk_id=c.chunk_id
                    AND e.generation_id=?{generation_index} AND e.state='ready' AND e.dimensions=384
                ) ORDER BY c.chunk_id"
            )
        } else {
            "SELECT c.chunk_id, c.text FROM chunks c WHERE NOT EXISTS (
                SELECT 1 FROM chunk_embeddings e WHERE e.chunk_id=c.chunk_id
                AND e.generation_id=?1 AND e.state='ready' AND e.dimensions=384
            ) ORDER BY c.document_id, c.ordinal"
                .to_owned()
        };
        let mut values = ids
            .into_iter()
            .map(|id| rusqlite::types::Value::Text(id.to_owned()))
            .collect::<Vec<_>>();
        values.push(rusqlite::types::Value::Text(
            generation.generation_id.clone(),
        ));
        let mut statement = self.connection.prepare(&query)?;
        let mut pending = statement
            .query_map(rusqlite::params_from_iter(values), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        let total = if scoped {
            let placeholders = std::iter::repeat_n("?", material_ids.len())
                .collect::<Vec<_>>()
                .join(",");
            self.connection.query_row(
                &format!("SELECT COUNT(DISTINCT c.chunk_id) FROM chunks c JOIN material_sources ms ON ms.document_id=c.document_id WHERE ms.material_id IN ({placeholders})"),
                rusqlite::params_from_iter(material_ids.iter().map(|id| id.as_str())),
                |row| row.get::<_, i64>(0),
            )? as usize
        } else {
            self.connection
                .query_row("SELECT COUNT(*) FROM chunks", [], |row| {
                    row.get::<_, i64>(0)
                })? as usize
        };
        let embeddings_total = pending.len();
        let reused = total.saturating_sub(embeddings_total);
        if embeddings_total == 0 {
            let report = EmbeddingIndexProgress {
                completed: 0,
                total: 0,
                created: 0,
                reused,
                elapsed_ms: 0,
            };
            if let Some(ledger) = ledger {
                self.update_accepted_import_embedding_progress(
                    ledger.operation_id,
                    total,
                    0,
                    0,
                    0,
                    reused,
                    Some(unix_seconds()),
                    Some(&generation.generation_id),
                )?;
            }
            if let Some(callback) = progress.as_mut() {
                callback(report);
            }
            return Ok(EmbeddingIndexOutcome {
                embedded: 0,
                reused,
            });
        }
        let size = batch_size.clamp(1, 64);
        // Length-aware batch formation: pending chunks are reordered by text
        // length (with the unique chunk_id as deterministic tie-break) so
        // similar-length chunks share a batch. The SQL ORDER BY above only
        // supplies a deterministic source feed; this sort defines the actual
        // inference order and reduces fixed-size batch padding. It never
        // changes the selected chunk set, the (chunk_id, text) association, or
        // the persisted chunk→embedding identity.
        sort_pending_by_length(&mut pending);
        let mut produced = Vec::with_capacity(embeddings_total);
        let started = std::time::Instant::now();
        let mut last_report = started;
        let mut completed = 0usize;
        if let Some(ledger) = ledger {
            self.update_accepted_import_embedding_progress(
                ledger.operation_id,
                total,
                embeddings_total,
                0,
                0,
                reused,
                Some(unix_seconds()),
                Some(&generation.generation_id),
            )?;
        }
        if let Some(callback) = progress.as_mut() {
            callback(EmbeddingIndexProgress {
                completed: 0,
                total: embeddings_total,
                created: 0,
                reused,
                elapsed_ms: 0,
            });
        }
        for batch in pending.chunks(size) {
            let texts = batch
                .iter()
                .map(|(_, text)| text.clone())
                .collect::<Vec<_>>();
            let vectors = provider.embed_passages(&texts)?;
            if vectors.len() != batch.len() {
                return Err(KnowledgeError::InvalidEmbedding(
                    "batch output count mismatch".to_owned(),
                ));
            }
            for ((chunk_id, _), vector) in batch.iter().zip(vectors) {
                validate_vector(&vector)?;
                produced.push((chunk_id.clone(), serialize_vector(&vector)?));
            }
            completed += batch.len();
            if last_report.elapsed() >= EMBEDDING_PROGRESS_INTERVAL {
                last_report = std::time::Instant::now();
                let report = EmbeddingIndexProgress {
                    completed,
                    total: embeddings_total,
                    created: completed,
                    reused,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                };
                if let Some(ledger) = ledger {
                    self.update_accepted_import_embedding_progress(
                        ledger.operation_id,
                        total,
                        embeddings_total,
                        completed,
                        completed,
                        reused,
                        None,
                        Some(&generation.generation_id),
                    )?;
                }
                if let Some(callback) = progress.as_mut() {
                    callback(report);
                }
            }
        }
        // Final report: progress must reach the exact terminal counters before
        // persistence. The authoritative final ledger state is still written by
        // the accepted-import caller after this method returns, so a crash
        // between here and the caller's terminal write stays recoverable.
        let final_report = EmbeddingIndexProgress {
            completed: embeddings_total,
            total: embeddings_total,
            created: embeddings_total,
            reused,
            elapsed_ms: started.elapsed().as_millis() as u64,
        };
        if let Some(ledger) = ledger {
            self.update_accepted_import_embedding_progress(
                ledger.operation_id,
                total,
                embeddings_total,
                embeddings_total,
                embeddings_total,
                reused,
                None,
                Some(&generation.generation_id),
            )?;
        }
        if let Some(callback) = progress.as_mut() {
            callback(final_report);
        }
        let tx = self.connection.transaction().map_err(KnowledgeError::Sql)?;
        for (chunk_id, vector) in &produced {
            let insert_result = tx.execute(
                "INSERT INTO chunk_embeddings(chunk_id, generation_id, vector, dimensions, normalized, state, embedded_at)
                 VALUES(?1, ?2, ?3, 384, 1, 'ready', ?4)
                 ON CONFLICT(chunk_id, generation_id) DO UPDATE SET vector=excluded.vector, dimensions=384, normalized=1, state='ready', embedded_at=excluded.embedded_at",
                params![chunk_id, generation.generation_id, vector, unix_seconds()],
            );
            // Preserve the actual SQLite failure (e.g. SQLITE_BUSY/LOCKED or a
            // constraint violation) so the sanitized classifier can name the
            // exact cause; the `Sql(error)` carrier keeps the result code.
            if let Err(error) = insert_result {
                drop(tx);
                return Err(KnowledgeError::Sql(error));
            }
        }
        tx.commit().map_err(KnowledgeError::Sql)?;
        Ok(EmbeddingIndexOutcome {
            embedded: produced.len(),
            reused: total.saturating_sub(pending.len()),
        })
    }

    pub fn semantic_search(
        &self,
        provider: &mut dyn EmbeddingProvider,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticSearchResult>> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(vec![]);
        }
        let generation = provider.generation().clone();
        let query_vector = provider.embed_query(query)?;
        validate_vector(&query_vector)?;
        let mut statement = self.connection.prepare(
            "SELECT c.document_id, ms.material_id, ms.source_name, ms.source_relative_path, c.chunk_id, c.text,
                    e.vector, e.generation_id, c.start_offset, c.end_offset, c.start_line, c.end_line,
                    c.heading_path, c.structural_type
             FROM chunk_embeddings e JOIN chunks c ON c.chunk_id=e.chunk_id
             JOIN material_sources ms ON ms.document_id=c.document_id
             WHERE e.generation_id=?1 AND e.state='ready' AND e.dimensions=384 AND e.normalized=1",
        )?;
        let rows = statement.query_map([&generation.generation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Vec<u8>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, usize>(8)?,
                row.get::<_, usize>(9)?,
                row.get::<_, usize>(10)?,
                row.get::<_, usize>(11)?,
                row.get::<_, String>(12)?,
                row.get::<_, String>(13)?,
            ))
        })?;
        let mut results = Vec::new();
        for row in rows {
            let (
                document_id,
                source_id,
                source_name,
                source_relative_path,
                chunk_id,
                chunk_text,
                blob,
                generation_id,
                start_offset,
                end_offset,
                start_line,
                end_line,
                headings,
                structural_type,
            ) = row?;
            let vector = deserialize_vector(&blob)?;
            let similarity = dot_product(&query_vector, &vector)?;
            results.push(SemanticSearchResult {
                document_id,
                source_id,
                source_name,
                source_relative_path,
                chunk_id,
                chunk_text,
                similarity,
                generation_id,
                provenance: Provenance {
                    start_offset,
                    end_offset,
                    start_line,
                    end_line,
                    heading_path: headings
                        .split('\u{1f}')
                        .filter(|part| !part.is_empty())
                        .map(str::to_owned)
                        .collect(),
                    structural_type,
                },
            });
        }
        results.sort_by(|left, right| {
            right
                .similarity
                .total_cmp(&left.similarity)
                .then_with(|| left.chunk_id.cmp(&right.chunk_id))
        });
        results.truncate(limit.min(100));
        Ok(results)
    }

    /// Fuses bounded FTS5/BM25 and current-generation exact semantic results
    /// with RRF. `None` means a local semantic runtime/model is unavailable;
    /// lexical results remain usable and `semantic_availability` reports that.
    pub fn hybrid_search(
        &self,
        query: &str,
        provider: Option<&mut dyn EmbeddingProvider>,
        options: HybridSearchOptions,
    ) -> Result<(Vec<HybridSearchResult>, SemanticAvailability)> {
        let (results, metrics) = self.hybrid_search_metrics(query, provider, options)?;
        Ok((results, metrics.semantic_availability))
    }

    /// [`Self::hybrid_search`] plus sanitized per-signal candidate counts, so a
    /// caller can report `retrieval_mode` / candidate counts without logging
    /// content. The `SemanticProviderState` for query-time failure is left to
    /// the caller (it knows the provider lifecycle), while this method reports
    /// the `SemanticAvailability` that governs fusion.
    pub fn hybrid_search_metrics(
        &self,
        query: &str,
        provider: Option<&mut dyn EmbeddingProvider>,
        options: HybridSearchOptions,
    ) -> Result<(Vec<HybridSearchResult>, HybridSearchMetrics)> {
        validate_hybrid_options(&options)?;
        if fts_query(query).is_empty() {
            return Ok((
                vec![],
                HybridSearchMetrics {
                    semantic_availability: SemanticAvailability::Unavailable,
                    ..Default::default()
                },
            ));
        }
        let lexical = self.search(query, options.lexical_candidate_limit)?;
        let lexical_candidates = lexical.len();
        let (semantic, availability) = match provider {
            None => (vec![], SemanticAvailability::Unavailable),
            Some(provider) => {
                match self.semantic_search(provider, query, options.semantic_candidate_limit) {
                    Ok(results) => (results, SemanticAvailability::Available),
                    Err(KnowledgeError::ModelUnavailable) => {
                        (vec![], SemanticAvailability::Unavailable)
                    }
                    Err(error) => return Err(error),
                }
            }
        };
        let semantic_candidates = semantic.len();
        let fused = fuse_hybrid_results(query, &lexical, &semantic, &options);
        let fused_candidates = fused.len();
        Ok((
            fused,
            HybridSearchMetrics {
                lexical_candidates,
                semantic_candidates,
                fused_candidates,
                semantic_availability: availability,
            },
        ))
    }

    /// Converts an already-bounded K3 result list into a provider-independent
    /// evidence package. This method does not rerun FTS or semantic retrieval.
    /// Neighbor rows, when enabled, are immediate same-document ordinal rows.
    pub fn assemble_context(
        &self,
        query: &str,
        candidates: &[HybridSearchResult],
        options: ContextAssemblyOptions,
    ) -> Result<EvidencePackage> {
        context::validate_options(&options)?;
        let direct = self.active_project_candidates(candidates, options.hybrid_candidate_limit)?;
        let neighbors = self.neighbor_candidates(&direct, options.neighbor_radius)?;
        ContextAssembler::assemble(&self.project_id, query, &direct, &neighbors, options)
    }

    /// Verifies that caller-supplied K3 results still resolve to this store's
    /// project-local chunk and source link. This is an identity check, never a
    /// replacement lexical/vector retrieval path.
    fn active_project_candidates(
        &self,
        candidates: &[HybridSearchResult],
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        let mut active = Vec::new();
        for candidate in candidates.iter().take(limit) {
            let exists: Option<()> = self
                .connection
                .query_row(
                    "SELECT 1 FROM chunks c JOIN material_sources ms ON ms.document_id=c.document_id
                     WHERE c.chunk_id=?1 AND c.document_id=?2 AND ms.material_id=?3 AND ms.source_relative_path=?4",
                    params![candidate.chunk_id, candidate.document_id, candidate.source_id, candidate.source_relative_path],
                    |_| Ok(()),
                )
                .optional()?;
            if exists.is_some() {
                active.push(candidate.clone());
            }
        }
        Ok(active)
    }

    fn neighbor_candidates(
        &self,
        direct: &[HybridSearchResult],
        radius: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        if radius == 0 {
            return Ok(vec![]);
        }
        let radius = i64::try_from(radius).unwrap_or(0);
        let mut neighbors = Vec::new();
        for primary in direct {
            let ordinal: Option<i64> = self
                .connection
                .query_row(
                    "SELECT ordinal FROM chunks WHERE chunk_id=?1 AND document_id=?2",
                    params![primary.chunk_id, primary.document_id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(ordinal) = ordinal else { continue };
            let mut statement = self.connection.prepare(
                "SELECT chunk_id, text, start_offset, end_offset, start_line, end_line, heading_path, structural_type, ordinal
                 FROM chunks WHERE document_id=?1 AND ordinal BETWEEN ?2 AND ?3 AND chunk_id != ?4
                 ORDER BY ABS(ordinal - ?5), ordinal",
            )?;
            let rows = statement.query_map(
                params![
                    primary.document_id,
                    ordinal - radius,
                    ordinal + radius,
                    primary.chunk_id,
                    ordinal
                ],
                |row| {
                    let headings: String = row.get(6)?;
                    Ok(HybridSearchResult {
                        document_id: primary.document_id.clone(),
                        source_id: primary.source_id.clone(),
                        source_name: primary.source_name.clone(),
                        source_relative_path: primary.source_relative_path.clone(),
                        chunk_id: row.get(0)?,
                        chunk_text: row.get(1)?,
                        provenance: Provenance {
                            start_offset: row.get(2)?,
                            end_offset: row.get(3)?,
                            start_line: row.get(4)?,
                            end_line: row.get(5)?,
                            heading_path: headings
                                .split('\u{1f}')
                                .filter(|part| !part.is_empty())
                                .map(str::to_owned)
                                .collect(),
                            structural_type: row.get(7)?,
                        },
                        lexical_rank: None,
                        semantic_rank: None,
                        lexical_score: None,
                        semantic_score: None,
                        // Assembly places every direct candidate before every neighbor.
                        fusion_score: primary.fusion_score,
                        embedding_generation_id: primary.embedding_generation_id.clone(),
                        signals: HybridMatchSignals {
                            lexical_match: false,
                            semantic_match: false,
                            exact_identifier_match: false,
                            neighbor_of: Some(primary.chunk_id.clone()),
                        },
                    })
                },
            )?;
            neighbors.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
        }
        Ok(neighbors)
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    // -- K6 hierarchical summarization --------------------------------------

    /// Creates the small durable ledger for one K6 execution. It deliberately
    /// references existing summary nodes rather than copying their content.
    pub fn create_summary_operation(
        &mut self,
        operation_id: &str,
        turn_id: Option<&str>,
        scope: &str,
        selected_ids: &[String],
        compatibility_fingerprint: &str,
        model_identity: Option<&str>,
    ) -> Result<SummaryOperation> {
        if let Some(existing) = self.summary_operation(operation_id)? {
            return Ok(existing);
        }
        let now = unix_seconds();
        let selected_json = serde_json::to_string(selected_ids)
            .map_err(|_| KnowledgeError::IncompatibleSchema(-1))?;
        self.connection.execute(
            "INSERT INTO summary_operations(operation_id, turn_id, scope, selected_ids_json, compatibility_fingerprint, model_identity, status, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?7)",
            params![operation_id, turn_id, scope, selected_json, compatibility_fingerprint, model_identity, now],
        )?;
        self.summary_operation(operation_id)?
            .ok_or(KnowledgeError::IncompatibleSchema(-1))
    }

    pub fn summary_operation(&self, operation_id: &str) -> Result<Option<SummaryOperation>> {
        self.connection.query_row(
            "SELECT operation_id, turn_id, scope, selected_ids_json, compatibility_fingerprint, model_identity, status, active_node_id, active_session_id, failure_class, final_summary_id, nodes_generated, nodes_reused, retries, remote_calls, created_at, started_at, completed_at FROM summary_operations WHERE operation_id=?1",
            [operation_id],
            summary_operation_from_row,
        ).optional().map_err(Into::into)
    }

    pub fn summary_operation_for_turn(&self, turn_id: &str) -> Result<Option<SummaryOperation>> {
        self.connection.query_row(
            "SELECT operation_id, turn_id, scope, selected_ids_json, compatibility_fingerprint, model_identity, status, active_node_id, active_session_id, failure_class, final_summary_id, nodes_generated, nodes_reused, retries, remote_calls, created_at, started_at, completed_at FROM summary_operations WHERE turn_id=?1 ORDER BY created_at DESC LIMIT 1",
            [turn_id], summary_operation_from_row,
        ).optional().map_err(Into::into)
    }

    /// Writes an in-flight fence before an outbound provider request. A crash
    /// while this fence is present is never auto-retried.
    pub fn summary_operation_begin_node(
        &mut self,
        operation_id: &str,
        node_id: &str,
    ) -> Result<()> {
        let now = unix_seconds();
        let changed = self.connection.execute(
            "UPDATE summary_operations SET status='running', active_node_id=?2, active_session_id=NULL, failure_class=NULL, started_at=COALESCE(started_at, ?3), updated_at=?3 WHERE operation_id=?1 AND status IN ('pending','running')",
            params![operation_id, node_id, now],
        )?;
        (changed == 1)
            .then_some(())
            .ok_or(KnowledgeError::SummaryTransitionLost)
    }

    pub fn summary_operation_set_active_session(
        &mut self,
        operation_id: &str,
        session_id: Option<&str>,
    ) -> Result<()> {
        let changed = self.connection.execute("UPDATE summary_operations SET active_session_id=?2, updated_at=?3 WHERE operation_id=?1 AND status='running'", params![operation_id, session_id, unix_seconds()])?;
        (changed == 1)
            .then_some(())
            .ok_or(KnowledgeError::SummaryTransitionLost)
    }

    pub fn summary_operation_checkpoint(
        &mut self,
        operation_id: &str,
        generated: usize,
        reused: usize,
        retries: usize,
        remote_calls: usize,
    ) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE summary_operations SET active_node_id=NULL, active_session_id=NULL, nodes_generated=?2, nodes_reused=?3, retries=MAX(retries, ?4), remote_calls=?5, updated_at=?6 WHERE operation_id=?1 AND status='running'",
            params![operation_id, generated as i64, reused as i64, retries as i64, remote_calls as i64, unix_seconds()],
        )?;
        (changed == 1)
            .then_some(())
            .ok_or(KnowledgeError::SummaryTransitionLost)
    }

    #[allow(clippy::too_many_arguments)] // mirrors durable lifecycle columns
    pub fn finish_summary_operation(
        &mut self,
        operation_id: &str,
        status: SummaryOperationStatus,
        failure: Option<&str>,
        final_summary_id: Option<&str>,
        generated: usize,
        reused: usize,
        retries: usize,
        remote_calls: usize,
    ) -> Result<()> {
        let now = unix_seconds();
        let allowed = match status {
            SummaryOperationStatus::Cancelled => "'pending','running','cancelled'",
            SummaryOperationStatus::Completed => "'pending','running'",
            SummaryOperationStatus::RetryRequired | SummaryOperationStatus::Failed => {
                "'pending','running'"
            }
            SummaryOperationStatus::Stale => {
                "'pending','running','retry_required','failed','stale'"
            }
            SummaryOperationStatus::Pending => "'retry_required','failed'",
            SummaryOperationStatus::Running => "'pending','running'",
        };
        let sql = format!(
            "UPDATE summary_operations SET status=?2, active_node_id=NULL, active_session_id=NULL, failure_class=?3, final_summary_id=COALESCE(?4, final_summary_id), nodes_generated=?5, nodes_reused=?6, retries=MAX(retries, ?7), remote_calls=?8, completed_at=CASE WHEN ?2 IN ('cancelled','failed','retry_required','completed','stale') THEN ?9 ELSE completed_at END, updated_at=?9 WHERE operation_id=?1 AND status IN ({allowed})"
        );
        let changed = self.connection.execute(
            &sql,
            params![
                operation_id,
                status.as_db(),
                failure,
                final_summary_id,
                generated as i64,
                reused as i64,
                retries as i64,
                remote_calls as i64,
                now
            ],
        )?;
        (changed == 1)
            .then_some(())
            .ok_or(KnowledgeError::SummaryTransitionLost)
    }

    /// Persists the final Ready artifact and completes the operation in one
    /// transaction, so a persisted final artifact can never be followed by a
    /// silently-lost `completed` transition. A `LostToTerminal` outcome means
    /// another legal terminal transition won the row before this commit; the
    /// artifact is still stored (never deleted) but is not referenced by
    /// `final_summary_id`.
    pub fn commit_completed_summary_artifact(
        &mut self,
        operation_id: &str,
        node: &SummaryNode,
        generated: usize,
        reused: usize,
        retries: usize,
        remote_calls: usize,
    ) -> Result<SummaryCompletionOutcome> {
        let now = unix_seconds();
        let tx = self.connection.transaction()?;
        Self::store_summary_node_in(&tx, node)?;
        let changed = tx.execute(
            "UPDATE summary_operations SET status='completed', active_node_id=NULL, active_session_id=NULL, failure_class=NULL, final_summary_id=?2, nodes_generated=?3, nodes_reused=?4, retries=MAX(retries, ?5), remote_calls=?6, completed_at=?7, updated_at=?7 WHERE operation_id=?1 AND status IN ('pending','running')",
            params![
                operation_id,
                node.summary_id,
                generated as i64,
                reused as i64,
                retries as i64,
                remote_calls as i64,
                now
            ],
        )?;
        tx.commit()?;
        if changed == 1 {
            return Ok(SummaryCompletionOutcome::Completed);
        }
        let current = self
            .summary_operation(operation_id)?
            .ok_or(KnowledgeError::IncompatibleSchema(-1))?;
        if current.status == SummaryOperationStatus::Completed {
            return Ok(SummaryCompletionOutcome::AlreadyCompleted);
        }
        Ok(SummaryCompletionOutcome::LostToTerminal(current.status))
    }

    /// CAS-claims exclusive retry ownership. Ordinary resume must never call this.
    /// Only `retry_required` and retryable `failed` rows can be claimed, and only
    /// once: a concurrent claim loses.
    pub fn claim_summary_operation_retry(
        &mut self,
        operation_id: &str,
    ) -> Result<SummaryOperation> {
        let now = unix_seconds();
        let changed = self.connection.execute(
            "UPDATE summary_operations SET status='running', active_node_id=NULL, active_session_id=NULL, failure_class=NULL, retries=retries+1, started_at=COALESCE(started_at, ?2), updated_at=?2 WHERE operation_id=?1 AND status IN ('retry_required','failed') AND IFNULL(failure_class, '') NOT IN ('durable_artifact_missing')",
            params![operation_id, now],
        )?;
        if changed != 1 {
            return Err(KnowledgeError::SummaryTransitionLost);
        }
        self.summary_operation(operation_id)?
            .ok_or(KnowledgeError::IncompatibleSchema(-1))
    }

    /// Fail-closed corruption marker for a Completed row whose final artifact is
    /// missing or unreadable. This is not an implicit retry and never starts work.
    pub fn mark_summary_operation_artifact_corrupt(
        &mut self,
        operation_id: &str,
        failure: &str,
    ) -> Result<()> {
        let now = unix_seconds();
        let changed = self.connection.execute(
            "UPDATE summary_operations SET status='failed', active_node_id=NULL, active_session_id=NULL, failure_class=?2, retries=MAX(retries, 0), completed_at=COALESCE(completed_at, ?3), updated_at=?3 WHERE operation_id=?1 AND status='completed'",
            params![operation_id, failure, now],
        )?;
        (changed == 1)
            .then_some(())
            .ok_or(KnowledgeError::SummaryTransitionLost)
    }

    /// A late provider result may become a Ready node only while this execution
    /// still owns a Running fence for the expected node.
    pub fn summary_operation_allows_commit(
        &self,
        operation_id: &str,
        node_id: &str,
    ) -> Result<bool> {
        let Some(operation) = self.summary_operation(operation_id)? else {
            return Ok(false);
        };
        Ok(operation.status == SummaryOperationStatus::Running
            && operation.active_node_id.as_deref() == Some(node_id))
    }

    #[doc(hidden)]
    pub fn execute_sql_for_tests(&self, sql: &str) -> Result<()> {
        self.connection.execute_batch(sql)?;
        Ok(())
    }

    /// Converts a crash-left in-flight fence to explicit retry-required. A
    /// committed node takes precedence, because it proves the outbound work
    /// reached durable local storage.
    pub fn reconcile_summary_operation_after_restart(
        &mut self,
        operation_id: &str,
    ) -> Result<SummaryOperation> {
        let operation = self
            .summary_operation(operation_id)?
            .ok_or(KnowledgeError::IncompatibleSchema(-1))?;
        if operation.status == SummaryOperationStatus::Running
            && let Some(node_id) = operation.active_node_id.as_deref()
        {
            let committed = self
                .get_summary(node_id)?
                .is_some_and(|node| node.state == SummaryState::Ready);
            if !committed {
                self.finish_summary_operation(
                    operation_id,
                    SummaryOperationStatus::RetryRequired,
                    Some("remote_outcome_unknown"),
                    None,
                    operation.nodes_generated,
                    operation.nodes_reused,
                    operation.retries,
                    operation.remote_calls,
                )?;
            } else {
                self.summary_operation_checkpoint(
                    operation_id,
                    operation.nodes_generated,
                    operation.nodes_reused,
                    operation.retries,
                    operation.remote_calls,
                )?;
            }
        }
        self.summary_operation(operation_id)?
            .ok_or(KnowledgeError::IncompatibleSchema(-1))
    }

    /// Lists this project's current indexed document identities (document_id
    /// and chunk count), in stable `(document_id, source_relative_path)` order.
    /// This is the Level-0 source set the hierarchy planner reduces.
    pub fn summary_document_levels(&self) -> Result<Vec<(String, usize)>> {
        let mut statement = self.connection.prepare(
            "SELECT d.document_id, COUNT(c.chunk_id) AS n
             FROM documents d
             LEFT JOIN chunks c ON c.document_id = d.document_id
             WHERE d.state = 'ready'
             GROUP BY d.document_id
             ORDER BY d.document_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Returns one document's chunks `(chunk_id, text)` in stable ordinal order,
    /// capped at `limit`. Used to assemble bounded document-summary evidence.
    pub fn document_chunks(
        &self,
        document_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        let limit = i64::try_from(limit.min(256)).unwrap_or(256);
        let mut statement = self.connection.prepare(
            "SELECT chunk_id, text FROM chunks WHERE document_id=?1 ORDER BY ordinal LIMIT ?2",
        )?;
        let rows = statement.query_map(params![document_id, limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Returns the canonical document id a material source currently resolves to.
    pub fn document_for_material(&self, material_id: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT document_id FROM material_sources WHERE material_id=?1",
                [material_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Returns the source name (first, stable) for a document, for evidence
    /// labelling. Falls back to the document id.
    pub fn document_source_name(&self, document_id: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT source_name FROM material_sources WHERE document_id=?1 ORDER BY source_relative_path LIMIT 1",
                [document_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Returns the compact text of a persisted `Ready` document-level K6 summary
    /// for `document_id` (`summary` plus topic texts), or `None` when no such
    /// node exists. Never synthesizes anything on demand; a per-item follow-up
    /// uses this as the preferred compact representative before falling back to
    /// deterministic chunk selection.
    pub fn ready_document_summary_text(&self, document_id: &str) -> Result<Option<String>> {
        for summary_id in self.summary_ids()? {
            let Some(node) = self.get_summary(&summary_id)? else {
                continue;
            };
            if node.level != SummaryLevel::Document
                || node.state != SummaryState::Ready
                || !node.source_ids.iter().any(|id| id == document_id)
            {
                continue;
            }
            let Some(content) = node.content else {
                continue;
            };
            let mut text = content.summary.clone();
            for topic in &content.topics {
                text.push(' ');
                text.push_str(&topic.text);
            }
            return Ok(Some(text));
        }
        Ok(None)
    }

    /// Returns the durable summary nodes keyed by summary_id: `(state,
    /// input_fingerprint)`. Used by the planner to decide reuse.
    pub fn summary_existing(&self) -> Result<BTreeMap<String, (SummaryState, String)>> {
        let mut statement = self
            .connection
            .prepare("SELECT summary_id, state, input_fingerprint FROM summaries")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (id, state, fingerprint) = row?;
            out.insert(id, (SummaryState::from_db(&state)?, fingerprint));
        }
        Ok(out)
    }

    /// Retrieves one durable summary node, or `None`.
    pub fn get_summary(&self, summary_id: &str) -> Result<Option<SummaryNode>> {
        let row = self
            .connection
            .query_row(
                "SELECT summary_id, level, state, failure_category, content_json,
                        parent_summary_id, input_fingerprint, output_fingerprint,
                        contract_version, generation_id, model_id, provider_id,
                        created_at, updated_at
                 FROM summaries WHERE summary_id = ?1",
                [summary_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, Option<String>>(10)?,
                        row.get::<_, Option<String>>(11)?,
                        row.get::<_, i64>(12)?,
                        row.get::<_, i64>(13)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            summary_id,
            level,
            state,
            failure,
            content,
            parent,
            input_fingerprint,
            output_fingerprint,
            contract_version,
            generation_id,
            model_id,
            provider_id,
            created_at,
            updated_at,
        )) = row
        else {
            return Ok(None);
        };
        let source_ids = self.summary_source_ids(&summary_id)?;
        let source_chunk_ids = self.summary_chunk_ids(&summary_id)?;
        Ok(Some(SummaryNode {
            summary_id,
            level: SummaryLevel::from_db(&level)?,
            state: SummaryState::from_db(&state)?,
            failure: failure
                .as_deref()
                .map(SummaryFailure::from_db)
                .transpose()?,
            content: content.as_deref().map(deserialize_content).transpose()?,
            source_ids,
            source_chunk_ids,
            parent_summary_id: parent,
            input_fingerprint,
            output_fingerprint,
            generation_id,
            model_id,
            provider_id,
            contract_version,
            created_at,
            updated_at,
        }))
    }

    fn summary_source_ids(&self, summary_id: &str) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT source_id FROM summary_sources WHERE summary_id=?1 ORDER BY source_id",
        )?;
        let rows = statement.query_map([summary_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn summary_chunk_ids(&self, summary_id: &str) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT chunk_id FROM summary_chunks WHERE summary_id=?1 ORDER BY chunk_id")?;
        let rows = statement.query_map([summary_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Persists a generated (Ready) summary node and its provenance. Replaces
    /// any prior version of the same summary_id atomically.
    pub fn store_summary(&mut self, node: &SummaryNode) -> Result<()> {
        let tx = self.connection.transaction()?;
        Self::store_summary_node_in(&tx, node)?;
        tx.commit()?;
        Ok(())
    }

    fn store_summary_node_in(tx: &Transaction, node: &SummaryNode) -> Result<()> {
        let content = node.content.as_ref().map(serialize_content);
        let now = unix_seconds();
        tx.execute(
            "INSERT INTO summaries(summary_id, level, state, failure_category, content_json, parent_summary_id, input_fingerprint, output_fingerprint, contract_version, generation_id, model_id, provider_id, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(summary_id) DO UPDATE SET level=excluded.level, state=excluded.state, failure_category=excluded.failure_category, content_json=excluded.content_json, parent_summary_id=excluded.parent_summary_id, input_fingerprint=excluded.input_fingerprint, output_fingerprint=excluded.output_fingerprint, contract_version=excluded.contract_version, generation_id=excluded.generation_id, model_id=excluded.model_id, provider_id=excluded.provider_id, updated_at=excluded.updated_at",
            params![
                node.summary_id,
                node.level.as_db(),
                node.state.as_db(),
                node.failure.as_ref().map(SummaryFailure::as_db),
                content.as_deref(),
                node.parent_summary_id,
                node.input_fingerprint,
                node.output_fingerprint,
                node.contract_version,
                node.generation_id,
                node.model_id,
                node.provider_id,
                node.created_at,
                now,
            ],
        )?;
        tx.execute(
            "DELETE FROM summary_sources WHERE summary_id=?1",
            [&node.summary_id],
        )?;
        tx.execute(
            "DELETE FROM summary_chunks WHERE summary_id=?1",
            [&node.summary_id],
        )?;
        for source_id in &node.source_ids {
            tx.execute(
                "INSERT INTO summary_sources(summary_id, source_id) VALUES(?1, ?2)",
                params![node.summary_id, source_id],
            )?;
        }
        for chunk_id in &node.source_chunk_ids {
            tx.execute(
                "INSERT INTO summary_chunks(summary_id, chunk_id) VALUES(?1, ?2)",
                params![node.summary_id, chunk_id],
            )?;
        }
        Ok(())
    }

    /// Marks a summary node Failed with a sanitized category. Failing a new
    /// synthesis never replaces an existing Ready summary's content.
    pub fn mark_summary_failed(
        &mut self,
        summary_id: &str,
        level: crate::summary::SummaryLevel,
        failure: SummaryFailure,
    ) -> Result<()> {
        let now = unix_seconds();
        self.connection.execute(
            "INSERT INTO summaries(summary_id, level, state, failure_category, content_json, input_fingerprint, output_fingerprint, contract_version, generation_id, created_at, updated_at)
             VALUES(?1, ?2, 'failed', ?3, NULL, '', '', ?4, '', ?5, ?5)
             ON CONFLICT(summary_id) DO UPDATE SET state='failed', failure_category=excluded.failure_category, updated_at=excluded.updated_at WHERE summaries.state != 'ready'",
            params![summary_id, level.as_db(), failure.as_db(), SUMMARY_CONTRACT_VERSION, now],
        )?;
        Ok(())
    }

    /// Invalidates `summary_id` and, transitively, every summary that lists it
    /// as a source (batches -> global). Unrelated summaries stay `Ready`.
    pub fn invalidate_summary(&mut self, summary_id: &str) -> Result<()> {
        let tx = self.connection.transaction()?;
        let mut frontier = vec![summary_id.to_owned()];
        let mut seen = BTreeSet::new();
        while let Some(id) = frontier.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            tx.execute(
                "UPDATE summaries SET state='stale' WHERE summary_id=?1",
                [&id],
            )?;
            let mut statement = tx.prepare(
                "SELECT s.summary_id FROM summaries s
                 JOIN summary_sources src ON src.summary_id = s.summary_id
                 WHERE src.source_id = ?1",
            )?;
            let children = statement
                .query_map([&id], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            drop(statement);
            frontier.extend(children);
        }
        tx.commit()?;
        Ok(())
    }

    /// Invalidates summaries that reference a changed/deleted document, by
    /// using document summaries keyed to that document plus any batsynthesis
    /// node listing those summaries as sources (transitive).
    pub fn invalidate_document_summaries(&mut self, document_id: &str) -> Result<()> {
        let mut statement = self.connection.prepare(
            "SELECT summary_id FROM summaries
             WHERE level='document' AND summary_id IN (
                SELECT summary_id FROM summary_sources WHERE source_id = ?1
             )",
        )?;
        let roots = statement
            .query_map([document_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        for root in roots {
            self.invalidate_summary(&root)?;
        }
        Ok(())
    }

    /// Removes a deleted source's summary associations and, when a document
    /// summary loses its last legitimate reference, marks it stale. Directly
    /// referenced child summaries (batch/global) are transitively invalidated.
    pub fn remove_summary_source(&mut self, document_id: &str) -> Result<()> {
        self.invalidate_document_summaries(document_id)
    }

    /// Lists every durable summary id in stable order (for status reporting).
    pub fn summary_ids(&self) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT summary_id FROM summaries ORDER BY summary_id")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

#[derive(Clone)]
struct FusionCandidate {
    document_id: String,
    source_id: String,
    source_name: String,
    source_relative_path: String,
    chunk_id: String,
    chunk_text: String,
    provenance: Provenance,
    lexical_rank: Option<usize>,
    semantic_rank: Option<usize>,
    lexical_score: Option<f64>,
    semantic_score: Option<f32>,
    embedding_generation_id: Option<String>,
}

fn validate_hybrid_options(options: &HybridSearchOptions) -> Result<()> {
    if options.final_limit == 0
        || options.final_limit > 20
        || options.lexical_candidate_limit == 0
        || options.lexical_candidate_limit > 100
        || options.semantic_candidate_limit == 0
        || options.semantic_candidate_limit > 100
        || options.rrf_k == 0
        || options.rrf_k > 1_000
        || options.max_per_document == 0
        || options.max_per_document > 20
        || options.max_per_source == 0
        || options.max_per_source > 20
        || !options.exact_identifier_boost.is_finite()
        || !(0.0..=0.1).contains(&options.exact_identifier_boost)
    {
        return Err(KnowledgeError::InvalidSearchOptions(
            "limits must be bounded and exact identifier boost must be 0.0..=0.1".to_owned(),
        ));
    }
    Ok(())
}

fn fuse_hybrid_results(
    query: &str,
    lexical: &[SearchResult],
    semantic: &[SemanticSearchResult],
    options: &HybridSearchOptions,
) -> Vec<HybridSearchResult> {
    // BTreeMap makes deduplication and all tie paths independent of hash order.
    let mut candidates = BTreeMap::<String, FusionCandidate>::new();
    for (index, result) in lexical.iter().enumerate() {
        let entry = candidates
            .entry(result.chunk_id.clone())
            .or_insert_with(|| FusionCandidate {
                document_id: result.document_id.clone(),
                source_id: result.source_id.clone(),
                source_name: result.source_name.clone(),
                source_relative_path: result.source_relative_path.clone(),
                chunk_id: result.chunk_id.clone(),
                chunk_text: result.chunk_text.clone(),
                provenance: result.provenance.clone(),
                lexical_rank: None,
                semantic_rank: None,
                lexical_score: None,
                semantic_score: None,
                embedding_generation_id: None,
            });
        if entry.lexical_rank.is_none() {
            entry.lexical_rank = Some(index + 1);
            entry.lexical_score = Some(result.score);
        }
        prefer_source(
            entry,
            &result.source_id,
            &result.source_name,
            &result.source_relative_path,
        );
    }
    for (index, result) in semantic.iter().enumerate() {
        let entry = candidates
            .entry(result.chunk_id.clone())
            .or_insert_with(|| FusionCandidate {
                document_id: result.document_id.clone(),
                source_id: result.source_id.clone(),
                source_name: result.source_name.clone(),
                source_relative_path: result.source_relative_path.clone(),
                chunk_id: result.chunk_id.clone(),
                chunk_text: result.chunk_text.clone(),
                provenance: result.provenance.clone(),
                lexical_rank: None,
                semantic_rank: None,
                lexical_score: None,
                semantic_score: None,
                embedding_generation_id: Some(result.generation_id.clone()),
            });
        if entry.semantic_rank.is_none() {
            entry.semantic_rank = Some(index + 1);
            entry.semantic_score = Some(result.similarity);
        }
        entry.embedding_generation_id = Some(result.generation_id.clone());
        prefer_source(
            entry,
            &result.source_id,
            &result.source_name,
            &result.source_relative_path,
        );
    }
    let identifiers = exact_identifier_tokens(query);
    let mut ranked = candidates
        .into_values()
        .map(|candidate| {
            let rrf = candidate
                .lexical_rank
                .map_or(0.0, |rank| 1.0 / (options.rrf_k + rank) as f64)
                + candidate
                    .semantic_rank
                    .map_or(0.0, |rank| 1.0 / (options.rrf_k + rank) as f64);
            let exact = !identifiers.is_empty()
                && candidate_contains_identifier(&candidate.chunk_text, &identifiers);
            let fusion_score = rrf
                + if exact {
                    options.exact_identifier_boost
                } else {
                    0.0
                };
            HybridSearchResult {
                document_id: candidate.document_id,
                source_id: candidate.source_id,
                source_name: candidate.source_name,
                source_relative_path: candidate.source_relative_path,
                chunk_id: candidate.chunk_id,
                chunk_text: candidate.chunk_text,
                provenance: candidate.provenance,
                lexical_rank: candidate.lexical_rank,
                semantic_rank: candidate.semantic_rank,
                lexical_score: candidate.lexical_score,
                semantic_score: candidate.semantic_score,
                fusion_score,
                embedding_generation_id: candidate.embedding_generation_id,
                signals: HybridMatchSignals {
                    lexical_match: candidate.lexical_rank.is_some(),
                    semantic_match: candidate.semantic_rank.is_some(),
                    exact_identifier_match: exact,
                    neighbor_of: None,
                },
            }
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .fusion_score
            .total_cmp(&left.fusion_score)
            .then_with(|| best_rank(left).cmp(&best_rank(right)))
            .then_with(|| {
                left.lexical_rank
                    .unwrap_or(usize::MAX)
                    .cmp(&right.lexical_rank.unwrap_or(usize::MAX))
            })
            .then_with(|| {
                left.semantic_rank
                    .unwrap_or(usize::MAX)
                    .cmp(&right.semantic_rank.unwrap_or(usize::MAX))
            })
            .then_with(|| left.document_id.cmp(&right.document_id))
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    let mut document_counts = BTreeMap::<String, usize>::new();
    let mut source_counts = BTreeMap::<String, usize>::new();
    ranked
        .into_iter()
        .filter(|result| {
            let document = document_counts
                .get(&result.document_id)
                .copied()
                .unwrap_or(0);
            let source = source_counts
                .get(&result.source_relative_path)
                .copied()
                .unwrap_or(0);
            if document >= options.max_per_document || source >= options.max_per_source {
                return false;
            }
            document_counts.insert(result.document_id.clone(), document + 1);
            source_counts.insert(result.source_relative_path.clone(), source + 1);
            true
        })
        .take(options.final_limit)
        .collect()
}

fn prefer_source(candidate: &mut FusionCandidate, id: &str, name: &str, path: &str) {
    if path < candidate.source_relative_path.as_str()
        || (path == candidate.source_relative_path && name < candidate.source_name.as_str())
    {
        candidate.source_id = id.to_owned();
        candidate.source_name = name.to_owned();
        candidate.source_relative_path = path.to_owned();
    }
}
fn best_rank(result: &HybridSearchResult) -> usize {
    result
        .lexical_rank
        .into_iter()
        .chain(result.semantic_rank)
        .min()
        .unwrap_or(usize::MAX)
}
fn exact_identifier_tokens(value: &str) -> BTreeSet<String> {
    value
        .nfc()
        .collect::<String>()
        .split_whitespace()
        .filter_map(|raw| {
            let token = raw.trim_matches(|c: char| {
                !c.is_alphanumeric() && !matches!(c, '-' | '_' | ':' | '/')
            });
            let has_letter = token.chars().any(char::is_alphabetic);
            let has_digit = token.chars().any(|c| c.is_ascii_digit());
            (has_letter
                && has_digit
                && token
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '/')))
            .then(|| token.to_lowercase())
        })
        .collect()
}
fn candidate_contains_identifier(text: &str, identifiers: &BTreeSet<String>) -> bool {
    text.nfc()
        .collect::<String>()
        .split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    ',' | '.' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\''
                )
        })
        .flat_map(|token| {
            let normalized = token
                .trim_matches(|c: char| !c.is_alphanumeric() && !matches!(c, '-' | '_' | ':' | '/'))
                .to_lowercase();
            let stripped = normalized.trim_end_matches([':', '/', '-', '_']).to_owned();
            [normalized, stripped]
        })
        .any(|token| identifiers.contains(&token))
}

fn migrate(connection: &Connection) -> Result<()> {
    let prior: Option<String> = connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .or_else(|error| match error {
            rusqlite::Error::SqliteFailure(_, Some(message))
                if message.contains("no such table") =>
            {
                Ok(None)
            }
            other => Err(other),
        })?;
    if let Some(value) = prior {
        let version = value
            .parse()
            .map_err(|_| KnowledgeError::IncompatibleSchema(-1))?;
        if version == 1 {
            let tx = connection.unchecked_transaction()?;
            create_embedding_tables(&tx)?;
            create_material_index_state_table(&tx)?;
            create_summary_tables(&tx)?;
            create_summary_operation_tables(&tx)?;
            create_accepted_import_operation_tables(&tx)?;
            ensure_accepted_import_progress_columns(&tx)?;
            // v9 introduced the conversation-active material set. Every branch
            // that lands on the current schema must create it; it is idempotent
            // so historical catch-up paths cannot reach version 9 without it.
            create_conversation_active_material_set_table(&tx)?;
            tx.execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
            return Ok(());
        }
        if version == 2 {
            let tx = connection.unchecked_transaction()?;
            create_material_index_state_table(&tx)?;
            create_summary_tables(&tx)?;
            create_summary_operation_tables(&tx)?;
            create_accepted_import_operation_tables(&tx)?;
            ensure_accepted_import_progress_columns(&tx)?;
            create_conversation_active_material_set_table(&tx)?;
            tx.execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
            return Ok(());
        }
        if version == 3 {
            let tx = connection.unchecked_transaction()?;
            create_summary_tables(&tx)?;
            create_summary_operation_tables(&tx)?;
            create_accepted_import_operation_tables(&tx)?;
            create_summary_operation_tables(&tx)?;
            ensure_accepted_import_progress_columns(&tx)?;
            create_summary_operation_tables(&tx)?;
            create_conversation_active_material_set_table(&tx)?;
            tx.execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
            return Ok(());
        }
        if version == 4 {
            let tx = connection.unchecked_transaction()?;
            create_accepted_import_operation_tables(&tx)?;
            ensure_accepted_import_progress_columns(&tx)?;
            create_summary_operation_tables(&tx)?;
            create_conversation_active_material_set_table(&tx)?;
            tx.execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
            return Ok(());
        }
        if version == 5 {
            let tx = connection.unchecked_transaction()?;
            tx.execute(
                "ALTER TABLE accepted_import_operations ADD COLUMN agent_state TEXT NOT NULL DEFAULT 'not_started'",
                [],
            )?;
            ensure_accepted_import_progress_columns(&tx)?;
            // v7 introduced the K6 operation ledger.  Direct v5 -> v8
            // upgrades must not skip that additive object.
            create_summary_operation_tables(&tx)?;
            create_conversation_active_material_set_table(&tx)?;
            tx.execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
            return Ok(());
        }
        if version == 6 {
            // Adds bounded incremental embedding-progress columns to the
            // accepted-import ledger. Purely additive: existing rows default to
            // zero and the pre-existing counters are untouched.
            let tx = connection.unchecked_transaction()?;
            ensure_accepted_import_progress_columns(&tx)?;
            // v6 predates `summary_operations`; create it here as well as in
            // the historical v7 step so direct upgrades are complete.
            create_summary_operation_tables(&tx)?;
            create_conversation_active_material_set_table(&tx)?;
            tx.execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
            return Ok(());
        }
        if version == 7 {
            let tx = connection.unchecked_transaction()?;
            create_summary_operation_tables(&tx)?;
            create_conversation_active_material_set_table(&tx)?;
            tx.execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
            return Ok(());
        }
        if version == 8 {
            // v9 introduced the conversation-active material set, used for
            // cross-turn per-source summary continuity. Purely additive.
            let tx = connection.unchecked_transaction()?;
            create_conversation_active_material_set_table(&tx)?;
            tx.execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
            return Ok(());
        }
        if version != SCHEMA_VERSION {
            return Err(KnowledgeError::IncompatibleSchema(version));
        }
        // Self-heal: an already-v9 database produced by an earlier rejected
        // development build can report schema_version=9 while still missing
        // conversation_active_material_set. This is schema repair, not a new
        // migration, so the version stays 9 and the existing idempotent helper
        // recreates the table if (and only if) it is absent.
        let tx = connection.unchecked_transaction()?;
        create_conversation_active_material_set_table(&tx)?;
        tx.commit()?;
        return Ok(());
    }
    let tx = connection.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS documents (
            document_id TEXT PRIMARY KEY NOT NULL, original_sha256 TEXT NOT NULL, normalized_sha256 TEXT NOT NULL,
            extractor_id TEXT NOT NULL, extractor_version TEXT NOT NULL, normalization_version TEXT NOT NULL,
            chunker_id TEXT NOT NULL, chunker_version TEXT NOT NULL, byte_size INTEGER NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('ready', 'error')), indexed_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS material_sources (
            material_id TEXT PRIMARY KEY NOT NULL, document_id TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
            source_name TEXT NOT NULL, source_relative_path TEXT NOT NULL, media_type TEXT, updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS chunks (
            chunk_id TEXT PRIMARY KEY NOT NULL, document_id TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
            ordinal INTEGER NOT NULL, text TEXT NOT NULL, content_sha256 TEXT NOT NULL, start_offset INTEGER NOT NULL,
            end_offset INTEGER NOT NULL, start_line INTEGER NOT NULL, end_line INTEGER NOT NULL,
            heading_path TEXT NOT NULL, structural_type TEXT NOT NULL, chunker_version TEXT NOT NULL,
            UNIQUE(document_id, ordinal)
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS chunk_fts USING fts5(chunk_id UNINDEXED, document_id UNINDEXED, text, tokenize='unicode61 remove_diacritics 2');
         CREATE INDEX IF NOT EXISTS chunks_document_ordinal ON chunks(document_id, ordinal);
         CREATE INDEX IF NOT EXISTS material_sources_document ON material_sources(document_id);"
    )?;
    create_embedding_tables(&tx)?;
    create_material_index_state_table(&tx)?;
    create_summary_tables(&tx)?;
    create_summary_operation_tables(&tx)?;
    create_accepted_import_operation_tables(&tx)?;
    create_conversation_active_material_set_table(&tx)?;
    tx.execute("INSERT INTO schema_meta(key, value) VALUES ('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [SCHEMA_VERSION.to_string()])?;
    tx.commit()?;
    Ok(())
}

fn create_accepted_import_operation_tables(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS accepted_import_operations (
            operation_id TEXT PRIMARY KEY NOT NULL,
            turn_id TEXT,
            state TEXT NOT NULL CHECK(state IN (
                'accepted', 'copying', 'indexing_lexical', 'indexing_embeddings',
                'pending_retry', 'completed'
            )),
            total INTEGER NOT NULL CHECK(total >= 0),
            copied INTEGER NOT NULL CHECK(copied >= 0),
            lexical_completed INTEGER NOT NULL CHECK(lexical_completed >= 0),
            embedding_completed INTEGER NOT NULL CHECK(embedding_completed >= 0),
            failed INTEGER NOT NULL CHECK(failed >= 0),
            embeddings_created INTEGER NOT NULL CHECK(embeddings_created >= 0),
            embeddings_reused INTEGER NOT NULL CHECK(embeddings_reused >= 0),
            chunks_total INTEGER NOT NULL DEFAULT 0 CHECK(chunks_total >= 0),
            embeddings_total INTEGER NOT NULL DEFAULT 0 CHECK(embeddings_total >= 0),
            embedding_started_at INTEGER NOT NULL DEFAULT 0 CHECK(embedding_started_at >= 0),
            embedding_generation_id TEXT,
            agent_state TEXT NOT NULL CHECK(agent_state IN (
                'not_started', 'started_outcome_unknown', 'completed',
                'failed_retryable', 'failed_terminal'
            )),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            synthesizing INTEGER NOT NULL DEFAULT 0 CHECK(synthesizing IN (0, 1))
         );
         CREATE INDEX IF NOT EXISTS accepted_import_operations_state
           ON accepted_import_operations(state, created_at);
         CREATE TABLE IF NOT EXISTS accepted_import_materials (
            operation_id TEXT NOT NULL REFERENCES accepted_import_operations(operation_id) ON DELETE CASCADE,
            material_id TEXT NOT NULL,
            PRIMARY KEY(operation_id, material_id)
         );",
    )?;
    Ok(())
}

/// Conversation-active material set: the exact ordered material identities the
/// user last explicitly attached/accepted, kept so a later no-attachment
/// per-source summary can reuse the current conversation focus across turns and
/// restarts. It is conversation-scoped by construction (the store is per
/// project/conversation), holds only opaque material ids (never bodies or
/// content), and is replaced wholesale on each new explicit attachment set.
fn create_conversation_active_material_set_table(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS conversation_active_material_set (
            ordinal INTEGER PRIMARY KEY NOT NULL,
            material_id TEXT NOT NULL,
            updated_at INTEGER NOT NULL
         );",
    )?;
    Ok(())
}

/// Idempotently ensures the incremental embedding-progress columns exist on the
/// accepted-import ledger. Older upgrade branches can reach the current schema
/// version without passing through the explicit v6 step, so each branch calls
/// this instead of assuming the column set. Existing rows default to zero (or
/// NULL for the optional generation id).
fn ensure_accepted_import_progress_columns(tx: &Transaction<'_>) -> Result<()> {
    for (column, ddl) in [
        (
            "chunks_total",
            "ALTER TABLE accepted_import_operations ADD COLUMN chunks_total INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "embeddings_total",
            "ALTER TABLE accepted_import_operations ADD COLUMN embeddings_total INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "embedding_started_at",
            "ALTER TABLE accepted_import_operations ADD COLUMN embedding_started_at INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "embedding_generation_id",
            "ALTER TABLE accepted_import_operations ADD COLUMN embedding_generation_id TEXT",
        ),
        (
            "synthesizing",
            "ALTER TABLE accepted_import_operations ADD COLUMN synthesizing INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('accepted_import_operations') WHERE name=?1",
            [column],
            |row| row.get(0),
        )?;
        if exists == 0 {
            tx.execute(ddl, [])?;
        }
    }
    Ok(())
}

/// K6 summary tables: summary nodes (source coverage, state, fingerprints),
/// summary-source and summary-chunk associations for provenance, and
/// transmission-safe accounting. No document/chunk/prompt/provider text is
/// persisted beyond the validated structured content.
fn create_summary_tables(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS summaries (
            summary_id TEXT PRIMARY KEY NOT NULL,
            level TEXT NOT NULL CHECK(level IN ('document', 'batch', 'global')),
            state TEXT NOT NULL CHECK(state IN ('pending', 'ready', 'failed', 'stale')),
            failure_category TEXT CHECK(failure_category IN (
                'provider_unavailable', 'execution_failed', 'invalid_output', 'empty_corpus'
            )),
            content_json TEXT,
            parent_summary_id TEXT,
            input_fingerprint TEXT NOT NULL,
            output_fingerprint TEXT NOT NULL,
            contract_version TEXT NOT NULL,
            generation_id TEXT NOT NULL,
            model_id TEXT,
            provider_id TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS summary_sources (
            summary_id TEXT NOT NULL REFERENCES summaries(summary_id) ON DELETE CASCADE,
            source_id TEXT NOT NULL,
            PRIMARY KEY(summary_id, source_id)
         );
         CREATE TABLE IF NOT EXISTS summary_chunks (
            summary_id TEXT NOT NULL REFERENCES summaries(summary_id) ON DELETE CASCADE,
            chunk_id TEXT NOT NULL,
            PRIMARY KEY(summary_id, chunk_id)
         );
         CREATE INDEX IF NOT EXISTS summaries_parent ON summaries(parent_summary_id);
         CREATE INDEX IF NOT EXISTS summaries_state ON summaries(state);",
    )?;
    Ok(())
}

fn create_summary_operation_tables(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS summary_operations (
            operation_id TEXT PRIMARY KEY NOT NULL,
            turn_id TEXT,
            scope TEXT NOT NULL,
            selected_ids_json TEXT NOT NULL,
            compatibility_fingerprint TEXT NOT NULL,
            model_identity TEXT,
            status TEXT NOT NULL CHECK(status IN ('pending','running','cancelled','failed','retry_required','completed','stale')),
            active_node_id TEXT,
            active_session_id TEXT,
            failure_class TEXT,
            final_summary_id TEXT,
            nodes_generated INTEGER NOT NULL DEFAULT 0,
            nodes_reused INTEGER NOT NULL DEFAULT 0,
            retries INTEGER NOT NULL DEFAULT 0,
            remote_calls INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            started_at INTEGER,
            completed_at INTEGER,
            updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS summary_operations_turn ON summary_operations(turn_id, created_at);
         CREATE INDEX IF NOT EXISTS summary_operations_status ON summary_operations(status, created_at);",
    )?;
    Ok(())
}

fn summary_operation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SummaryOperation> {
    let selected_ids_json: String = row.get(3)?;
    let selected_ids = serde_json::from_str(&selected_ids_json)
        .map_err(|_| to_sql_error(KnowledgeError::IncompatibleSchema(-1)))?;
    let status: String = row.get(6)?;
    Ok(SummaryOperation {
        operation_id: row.get(0)?,
        turn_id: row.get(1)?,
        scope: row.get(2)?,
        selected_ids,
        compatibility_fingerprint: row.get(4)?,
        model_identity: row.get(5)?,
        status: SummaryOperationStatus::from_db(&status).map_err(to_sql_error)?,
        active_node_id: row.get(7)?,
        active_session_id: row.get(8)?,
        failure_class: row.get(9)?,
        final_summary_id: row.get(10)?,
        nodes_generated: row.get::<_, i64>(11)? as usize,
        nodes_reused: row.get::<_, i64>(12)? as usize,
        retries: row.get::<_, i64>(13)? as usize,
        remote_calls: row.get::<_, i64>(14)? as usize,
        created_at: row.get(15)?,
        started_at: row.get(16)?,
        completed_at: row.get(17)?,
    })
}

fn create_embedding_tables(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS embedding_generations (
            generation_id TEXT PRIMARY KEY NOT NULL,
            model_id TEXT NOT NULL, model_revision TEXT NOT NULL,
            tokenizer_metadata TEXT NOT NULL, runtime_backend TEXT NOT NULL,
            runtime_version TEXT NOT NULL, dimensions INTEGER NOT NULL CHECK(dimensions = 384),
            max_input_tokens INTEGER NOT NULL, query_prefix TEXT NOT NULL,
            passage_prefix TEXT NOT NULL, normalization TEXT NOT NULL,
            artifact_variant TEXT NOT NULL, created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS chunk_embeddings (
            chunk_id TEXT NOT NULL REFERENCES chunks(chunk_id) ON DELETE CASCADE,
            generation_id TEXT NOT NULL REFERENCES embedding_generations(generation_id) ON DELETE CASCADE,
            vector BLOB NOT NULL, dimensions INTEGER NOT NULL CHECK(dimensions = 384),
            normalized INTEGER NOT NULL CHECK(normalized IN (0, 1)),
            state TEXT NOT NULL CHECK(state IN ('ready', 'error')),
            embedded_at INTEGER NOT NULL,
            PRIMARY KEY(chunk_id, generation_id)
         );
         CREATE INDEX IF NOT EXISTS chunk_embeddings_generation ON chunk_embeddings(generation_id, state);"
    )?;
    Ok(())
}

fn create_material_index_state_table(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS material_index_state (
            material_id TEXT PRIMARY KEY NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('pending', 'ready', 'failed', 'unsupported')),
            failure_category TEXT CHECK(failure_category IN (
                'unsupported_format', 'invalid_text_encoding', 'read_failed',
                'extraction_failed', 'model_unavailable', 'embedding_failed',
                'storage_failed', 'integrity_failed'
            )),
            retryable INTEGER NOT NULL CHECK(retryable IN (0, 1)),
            last_attempt_at INTEGER,
            updated_at INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

fn index_failure_category(error: &KnowledgeError) -> MaterialIndexFailure {
    match error {
        KnowledgeError::UnsupportedFormat(_) => MaterialIndexFailure::UnsupportedFormat,
        KnowledgeError::InvalidUtf8 => MaterialIndexFailure::InvalidTextEncoding,
        KnowledgeError::Io(_) => MaterialIndexFailure::ReadFailed,
        KnowledgeError::Sql(_) | KnowledgeError::IncompatibleSchema(_) => {
            MaterialIndexFailure::StorageFailed
        }
        KnowledgeError::ModelUnavailable => MaterialIndexFailure::ModelUnavailable,
        KnowledgeError::Tokenizer(_)
        | KnowledgeError::InputTooLong
        | KnowledgeError::InvalidEmbedding(_)
        | KnowledgeError::Inference(_) => MaterialIndexFailure::EmbeddingFailed,
        KnowledgeError::InvalidProjectRoot => MaterialIndexFailure::IntegrityFailed,
        KnowledgeError::ModelManifest(_)
        | KnowledgeError::ArtifactVerification(_)
        | KnowledgeError::InvalidSearchOptions(_)
        | KnowledgeError::InvalidContextAssemblyOptions(_) => {
            MaterialIndexFailure::ExtractionFailed
        }
        KnowledgeError::EmbeddingPersistFailed | KnowledgeError::SummaryTransitionLost => {
            MaterialIndexFailure::StorageFailed
        }
    }
}

fn document_is_current(
    tx: &Transaction<'_>,
    id: &str,
    extractor_id: &str,
    extractor_version: &str,
) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT 1 FROM documents WHERE document_id=?1 AND original_sha256=?2 AND extractor_id=?3 AND extractor_version=?4 AND normalization_version=?5 AND chunker_id=?6 AND chunker_version=?7 AND state='ready'",
        params![id, id, extractor_id, extractor_version, NORMALIZATION_VERSION, CHUNKER_ID, CHUNKER_VERSION], |_| Ok(())
    ).optional()?.is_some())
}

fn replace_document(
    tx: &Transaction<'_>,
    id: &str,
    original_sha256: &str,
    byte_size: i64,
    extracted: &ExtractedDocument,
    now: i64,
) -> Result<()> {
    tx.execute("DELETE FROM chunk_fts WHERE document_id = ?1", [id])?;
    tx.execute("DELETE FROM chunks WHERE document_id = ?1", [id])?;
    tx.execute(
        "INSERT INTO documents(document_id, original_sha256, normalized_sha256, extractor_id, extractor_version, normalization_version, chunker_id, chunker_version, byte_size, state, indexed_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'ready', ?10)
         ON CONFLICT(document_id) DO UPDATE SET original_sha256=excluded.original_sha256, normalized_sha256=excluded.normalized_sha256, extractor_id=excluded.extractor_id, extractor_version=excluded.extractor_version, normalization_version=excluded.normalization_version, chunker_id=excluded.chunker_id, chunker_version=excluded.chunker_version, byte_size=excluded.byte_size, state='ready', indexed_at=excluded.indexed_at",
        params![id, original_sha256, extracted.normalized_sha256, extracted.extractor_id, extracted.extractor_version, NORMALIZATION_VERSION, CHUNKER_ID, CHUNKER_VERSION, byte_size, now],
    )?;
    for chunk in &extracted.chunks {
        tx.execute("INSERT INTO chunks(chunk_id, document_id, ordinal, text, content_sha256, start_offset, end_offset, start_line, end_line, heading_path, structural_type, chunker_version) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)", params![chunk.id, id, chunk.ordinal, chunk.text, chunk.content_sha256, chunk.provenance.start_offset, chunk.provenance.end_offset, chunk.provenance.start_line, chunk.provenance.end_line, chunk.provenance.heading_path.join("\u{1f}"), chunk.provenance.structural_type, CHUNKER_VERSION])?;
        tx.execute(
            "INSERT INTO chunk_fts(chunk_id, document_id, text) VALUES(?1, ?2, ?3)",
            params![chunk.id, id, chunk.text],
        )?;
    }
    Ok(())
}

fn cleanup_orphaned_documents(tx: &Transaction<'_>) -> Result<()> {
    let mut statement = tx.prepare("SELECT document_id FROM documents WHERE NOT EXISTS (SELECT 1 FROM material_sources WHERE material_sources.document_id = documents.document_id)")?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);
    for id in ids {
        tx.execute("DELETE FROM chunk_fts WHERE document_id=?1", [&id])?;
        tx.execute("DELETE FROM documents WHERE document_id=?1", [&id])?;
    }
    Ok(())
}

fn validate_source(source: &MaterialSource) -> Result<()> {
    let prefix = format!("inputs/{}/", source.material_id.as_str());
    if source.source_name.is_empty()
        || source.source_name.contains(['/', '\\', '\0'])
        || !source.relative_path.starts_with(&prefix)
        || source.relative_path.contains('\\')
        || source
            .relative_path
            .split('/')
            .any(|part| part == ".." || part.is_empty())
    {
        return Err(KnowledgeError::InvalidProjectRoot);
    }
    Ok(())
}

fn extract(source: &MaterialSource, bytes: &[u8]) -> Result<ExtractedDocument> {
    let (extractor_id, extractor_version) = extractor_contract(source)?;
    let text = std::str::from_utf8(bytes).map_err(|_| KnowledgeError::InvalidUtf8)?;
    let normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .nfc()
        .collect::<String>();
    let units = if extractor_id == "markdown" {
        markdown_units(&normalized)
    } else {
        text_units(&normalized)
    };
    let normalized_sha256 = sha256_hex(normalized.as_bytes());
    let chunks = chunk(&normalized_sha256, units);
    Ok(ExtractedDocument {
        extractor_id,
        extractor_version,
        normalized_text: normalized,
        normalized_sha256,
        chunks,
    })
}

fn extractor_contract(source: &MaterialSource) -> Result<(String, String)> {
    let format = source
        .media_type
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| {
            source
                .source_name
                .rsplit('.')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
        });
    let id = match format.as_str() {
        "txt" | "text/plain" => "txt",
        "md" | "markdown" | "text/markdown" => "markdown",
        other => return Err(KnowledgeError::UnsupportedFormat(other.to_owned())),
    };
    Ok((id.to_owned(), format!("{id}-v1")))
}

#[derive(Clone)]
struct Unit {
    text: String,
    start: usize,
    start_line: usize,
    heading_path: Vec<String>,
    structural_type: &'static str,
}
struct UnitContext {
    start_line: usize,
    headings: Vec<String>,
    kind: &'static str,
}
fn text_units(text: &str) -> Vec<Unit> {
    paragraph_units(text, false)
}
fn markdown_units(text: &str) -> Vec<Unit> {
    paragraph_units(text, true)
}

fn paragraph_units(text: &str, markdown: bool) -> Vec<Unit> {
    let mut units = Vec::new();
    let mut start = None;
    let mut start_line = 1;
    let mut heading_path = Vec::<String>::new();
    let mut offset = 0;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    for (index, raw) in lines.iter().enumerate() {
        let line = raw.trim_end_matches('\n');
        let line_start = offset;
        offset += raw.len();
        if markdown && let Some((level, heading)) = markdown_heading(line) {
            push_unit(
                text,
                &mut units,
                start.take(),
                line_start,
                UnitContext {
                    start_line,
                    headings: heading_path.clone(),
                    kind: "paragraph",
                },
            );
            heading_path.truncate(level.saturating_sub(1));
            heading_path.push(heading.to_owned());
            units.push(Unit {
                text: line.to_owned(),
                start: line_start,
                start_line: index + 1,
                heading_path: heading_path.clone(),
                structural_type: "heading",
            });
            continue;
        }
        let list = markdown && is_list_item(line);
        if line.trim().is_empty() {
            push_unit(
                text,
                &mut units,
                start.take(),
                line_start,
                UnitContext {
                    start_line,
                    headings: heading_path.clone(),
                    kind: "paragraph",
                },
            );
        } else if list {
            push_unit(
                text,
                &mut units,
                start.take(),
                line_start,
                UnitContext {
                    start_line,
                    headings: heading_path.clone(),
                    kind: "paragraph",
                },
            );
            units.push(Unit {
                text: line.to_owned(),
                start: line_start,
                start_line: index + 1,
                heading_path: heading_path.clone(),
                structural_type: "list_item",
            });
        } else if start.is_none() {
            start = Some(line_start);
            start_line = index + 1;
        }
    }
    push_unit(
        text,
        &mut units,
        start,
        text.len(),
        UnitContext {
            start_line,
            headings: heading_path,
            kind: "paragraph",
        },
    );
    units
}
fn push_unit(
    text: &str,
    units: &mut Vec<Unit>,
    start: Option<usize>,
    end: usize,
    context: UnitContext,
) {
    if let Some(start) = start {
        let value = text[start..end].trim();
        if !value.is_empty() {
            let leading = text[start..end].find(value).unwrap_or(0);
            let actual_start = start + leading;
            units.push(Unit {
                text: value.to_owned(),
                start: actual_start,
                start_line: context.start_line,
                heading_path: context.headings,
                structural_type: context.kind,
            });
        }
    }
}
fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (hashes > 0 && hashes <= 6 && line.as_bytes().get(hashes) == Some(&b' '))
        .then(|| (hashes, line[hashes + 1..].trim()))
}
fn is_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    matches!(trimmed.as_bytes().first(), Some(b'-' | b'*' | b'+'))
        && trimmed.as_bytes().get(1) == Some(&b' ')
        || trimmed.chars().take_while(|c| c.is_ascii_digit()).count() > 0 && trimmed.contains(". ")
}

fn chunk(document_hash: &str, units: Vec<Unit>) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut ordinal = 0_i64;
    for unit in units {
        for (text, start_delta, end_delta) in split_unit(&unit.text) {
            let start = unit.start + start_delta;
            let end = unit.start + end_delta;
            chunks.push(Chunk {
                id: sha256_hex(
                    format!(
                        "{document_hash}:{CHUNKER_VERSION}:{ordinal}:{}",
                        sha256_hex(text.as_bytes())
                    )
                    .as_bytes(),
                ),
                ordinal,
                content_sha256: sha256_hex(text.as_bytes()),
                text,
                provenance: Provenance {
                    start_offset: start,
                    end_offset: end,
                    start_line: unit.start_line + line_count(&unit.text[..start_delta]),
                    end_line: unit.start_line + line_count(&unit.text[..end_delta]),
                    heading_path: unit.heading_path.clone(),
                    structural_type: unit.structural_type.to_owned(),
                },
            });
            ordinal += 1;
        }
    }
    chunks
}
fn split_unit(value: &str) -> Vec<(String, usize, usize)> {
    if value.chars().count() <= MAX_CHUNK_CHARS {
        return vec![(value.to_owned(), 0, value.len())];
    }
    let mut output = vec![];
    let mut start = 0;
    while start < value.len() {
        let remaining = &value[start..];
        let mut end = remaining
            .char_indices()
            .nth(MAX_CHUNK_CHARS)
            .map(|(i, _)| start + i)
            .unwrap_or(value.len());
        if end < value.len()
            && let Some(space) = remaining[..end - start].rfind(char::is_whitespace)
        {
            end = start + space + 1;
        }
        if end == start {
            end = value[start..]
                .chars()
                .next()
                .map(|c| start + c.len_utf8())
                .unwrap_or(value.len());
        }
        let chunk_start = start;
        output.push((value[chunk_start..end].trim().to_owned(), chunk_start, end));
        start = end;
        while start < value.len() && value[start..].starts_with(char::is_whitespace) {
            start += value[start..].chars().next().unwrap().len_utf8();
        }
    }
    output
}
fn line_count(value: &str) -> usize {
    value.bytes().filter(|b| *b == b'\n').count()
}
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Length-aware ordering for pending embedding work. Groups chunks of similar
/// text length into the same fixed-size batch so the ONNX batch tensor pads to
/// a tight common width instead of the widest member's width (padding
/// amplification is the measured production slowdown). This is an
/// inference-order-only change: every `(chunk_id, text)` pair is preserved and
/// each chunk is still embedded and persisted under its own `chunk_id`.
///
/// The key is a deterministic total order: primary = text character count (a
/// cheap proxy for token count that needs no second tokenization pass),
/// tie-break = the unique `chunk_id`. No nondeterministic collection iteration
/// is involved, so two runs over the same pending set produce the same order.
fn sort_pending_by_length(pending: &mut [(String, String)]) {
    pending.sort_by_cached_key(|(chunk_id, text)| (text.chars().count(), chunk_id.clone()));
}

/// The persistent vector format is exactly 384 IEEE-754 `f32` values in
/// little-endian order. Stored vectors are L2-normalized once by the provider.
pub fn serialize_vector(vector: &[f32]) -> Result<Vec<u8>> {
    validate_vector(vector)?;
    Ok(vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect())
}

pub fn deserialize_vector(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() != 384 * std::mem::size_of::<f32>() {
        return Err(KnowledgeError::InvalidEmbedding(
            "invalid vector byte length".to_owned(),
        ));
    }
    let vector = bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect::<Vec<_>>();
    validate_vector(&vector)?;
    Ok(vector)
}

fn validate_vector(vector: &[f32]) -> Result<()> {
    if vector.len() != 384 || vector.iter().any(|value| !value.is_finite()) {
        return Err(KnowledgeError::InvalidEmbedding(
            "expected 384 finite f32 values".to_owned(),
        ));
    }
    let norm_squared = vector.iter().map(|value| value * value).sum::<f32>();
    if !(0.999..=1.001).contains(&norm_squared) {
        return Err(KnowledgeError::InvalidEmbedding(
            "expected L2-normalized vector".to_owned(),
        ));
    }
    Ok(())
}

fn dot_product(left: &[f32], right: &[f32]) -> Result<f32> {
    validate_vector(left)?;
    validate_vector(right)?;
    Ok(left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum())
}
fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}
fn fts_query(raw: &str) -> String {
    raw.split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    const PID: &str = "0198e4a6-79b2-7b51-9e68-c2eb7af3db14";
    const MID: &str = "0198e4a6-79b2-7b51-9e68-c2eb7af3db15";
    fn root() -> (TempDir, ProjectId) {
        let temp = TempDir::new().unwrap();
        let id = ProjectId::parse(PID).unwrap();
        let root = temp.path().join(PID);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("project.json"), "{}").unwrap();
        (temp, id)
    }
    fn source(name: &str) -> MaterialSource {
        let id = MaterialId::parse(MID).unwrap();
        MaterialSource {
            relative_path: format!("inputs/{MID}/{name}"),
            material_id: id,
            source_name: name.into(),
            media_type: None,
        }
    }

    #[test]
    fn accepted_import_operation_is_durable_and_never_auto_completes() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store
            .create_accepted_import_operation("accepted-51", 51)
            .unwrap();
        store
            .update_accepted_import_operation(
                "accepted-51",
                None,
                AcceptedImportState::IndexingLexical,
                17,
                17,
                0,
                0,
                0,
                0,
            )
            .unwrap();
        drop(store);

        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        let incomplete = reopened.incomplete_accepted_import_operations().unwrap();
        assert_eq!(incomplete.len(), 1);
        assert_eq!(incomplete[0].state, AcceptedImportState::IndexingLexical);
        assert_eq!(incomplete[0].copied, 17);
        assert_eq!(incomplete[0].total, 51);
    }

    #[test]
    fn accepted_import_synthesizing_flag_roundtrips_and_defaults_false() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store
            .create_accepted_import_operation("synth-1", 4)
            .unwrap();
        // New operations default to not synthesizing.
        let operation = store.accepted_import_operation("synth-1").unwrap().unwrap();
        assert!(!operation.synthesizing);
        store
            .set_accepted_import_synthesizing("synth-1", true)
            .unwrap();
        drop(store);

        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert!(
            reopened
                .accepted_import_operation("synth-1")
                .unwrap()
                .unwrap()
                .synthesizing
        );
    }

    #[test]
    fn conversation_active_material_set_roundtrips_in_attach_order_and_replaces() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert!(store.conversation_active_material_ids().unwrap().is_empty());

        let ids: Vec<MaterialId> = (0..4)
            .map(|i| {
                MaterialId::parse(format!(
                    "0198e4a6-79b2-7b51-9e68-c2eb7af3db{index:02x}",
                    index = 0x20 + i
                ))
                .unwrap()
            })
            .collect();
        store.set_conversation_active_material_set(&ids).unwrap();
        drop(store);

        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(reopened.conversation_active_material_ids().unwrap(), ids);

        // Replacement with a different set must replace (not append) and keep
        // the new order, and never inherit the previous set.
        let mut reopened = reopened;
        let new_ids: Vec<MaterialId> = (0..2)
            .map(|i| {
                MaterialId::parse(format!(
                    "0198e4a6-79b2-7b51-9e68-c2eb7af3db{index:02x}",
                    index = 0x40 + i
                ))
                .unwrap()
            })
            .collect();
        reopened
            .set_conversation_active_material_set(&new_ids)
            .unwrap();
        assert_eq!(
            reopened.conversation_active_material_ids().unwrap(),
            new_ids
        );
    }

    #[test]
    fn empty_conversation_active_material_set_write_is_a_no_op() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        let ids: Vec<MaterialId> = (0..4)
            .map(|i| {
                MaterialId::parse(format!(
                    "0198e4a6-79b2-7b51-9e68-c2eb7af3db{index:02x}",
                    index = 0x20 + i
                ))
                .unwrap()
            })
            .collect();
        store.set_conversation_active_material_set(&ids).unwrap();
        // An empty write must never erase a valid active set.
        store.set_conversation_active_material_set(&[]).unwrap();
        assert_eq!(store.conversation_active_material_ids().unwrap(), ids);
        drop(store);

        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(reopened.conversation_active_material_ids().unwrap(), ids);
    }

    #[test]
    fn v8_migration_adds_conversation_active_material_set_table() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let store = KnowledgeStore::open(&project_root, &pid).unwrap();
        // Rewind the store to a v8-shaped DB: the table does not exist yet.
        store
            .connection
            .execute("DROP TABLE IF EXISTS conversation_active_material_set", [])
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE schema_meta SET value='8' WHERE key='schema_version'",
                [],
            )
            .unwrap();
        drop(store);

        let upgraded = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(upgraded.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(
            upgraded
                .conversation_active_material_ids()
                .unwrap()
                .is_empty()
        );
        let mut upgraded = upgraded;
        let id = MaterialId::parse(MID).unwrap();
        upgraded
            .set_conversation_active_material_set(std::slice::from_ref(&id))
            .unwrap();
        assert_eq!(
            upgraded.conversation_active_material_ids().unwrap(),
            vec![id]
        );
    }

    fn material_source_at(index: usize, name: &str) -> MaterialSource {
        let id =
            MaterialId::parse(format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{index:02x}")).unwrap();
        MaterialSource {
            relative_path: format!("inputs/{}/{}", id.as_str(), name),
            material_id: id,
            source_name: name.into(),
            media_type: None,
        }
    }

    fn bound_material(
        store: &mut KnowledgeStore,
        operation_id: &str,
        index: usize,
        bytes: &[u8],
    ) -> IndexOutcome {
        let source = material_source_at(index, &format!("material-{index}.md"));
        store
            .bind_accepted_import_material(operation_id, &source.material_id)
            .unwrap();
        store.index(&source, bytes).unwrap()
    }

    fn fake_provider(generation: EmbeddingGeneration) -> FakeEmbeddingProvider {
        FakeEmbeddingProvider {
            generation,
            calls: 0,
        }
    }

    /// CASE 1, CASE 2, CASE 12: `materialsReady` is the count of materials
    /// fully usable for the active embedding generation. Lexical
    /// `MaterialIndexState::Ready` alone must never increment it, and a huge
    /// pending file must keep the count at 49/50 until it is truly ready.
    #[test]
    fn materials_ready_requires_embeddings_and_stays_at_49_of_50_for_a_huge_pending_file() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op", 50).unwrap();
        let generation = generation();
        // 49 small files, each a single short chunk.
        let mut small_ids = Vec::new();
        for index in 0..49 {
            let outcome = bound_material(
                &mut store,
                "op",
                index,
                format!("# Nota {index}\n\ncontenido breve\n").as_bytes(),
            );
            assert!(outcome.chunk_count >= 1, "small files produce chunks");
            small_ids.push(material_source_at(index, "x").material_id);
        }
        // One huge file with many chunks.
        let huge = (0..200)
            .map(|part| format!("# Parte {part}\n\n{}\n", "texto de relleno ".repeat(80)))
            .collect::<String>();
        let huge_outcome = bound_material(&mut store, "op", 49, huge.as_bytes());
        assert!(
            huge_outcome.chunk_count > 10,
            "huge file must produce many chunks"
        );

        // CASE 12: nothing embedded yet, so even fully lexically-ready
        // materials must not count as user-facing ready.
        assert_eq!(
            store
                .accepted_import_materials_ready("op", Some(&generation.generation_id))
                .unwrap(),
            0
        );

        // CASE 2: embed only the 49 small files; the huge file's embeddings are
        // still pending, so the truthful count is 49/50.
        store
            .index_embeddings_for_materials_reporting(
                &mut fake_provider(generation.clone()),
                8,
                &small_ids,
                None,
                None,
            )
            .unwrap();
        assert_eq!(
            store
                .accepted_import_materials_ready("op", Some(&generation.generation_id))
                .unwrap(),
            49,
            "49 of 50 while the final file's embeddings are still pending"
        );

        // CASE 1: once every accepted material is usable, exactly 50/50.
        let all_ids = (0..50)
            .map(|index| material_source_at(index, "x").material_id)
            .collect::<Vec<_>>();
        store
            .index_embeddings_for_materials_reporting(
                &mut fake_provider(generation.clone()),
                8,
                &all_ids,
                None,
                None,
            )
            .unwrap();
        let final_ready = store
            .accepted_import_materials_ready("op", Some(&generation.generation_id))
            .unwrap();
        assert_eq!(final_ready, 50);
        // The count is material-scoped and can never exceed the operation total.
        assert!(final_ready <= store.accepted_import_material_ids("op").unwrap().len());
    }

    /// CASE A (knowledge level): a 50-material accepted batch embedded through
    /// the production ledger/progress seam must persist every vector.
    #[test]
    fn accepted_batch_with_ledger_persists_embeddings_for_fifty_materials() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op", 50).unwrap();
        let mut ids = Vec::new();
        for index in 0..50 {
            let outcome = bound_material(
                &mut store,
                "op",
                index,
                format!("# Nota {index}\n\ncontenido breve\n").as_bytes(),
            );
            assert!(outcome.chunk_count >= 1);
            ids.push(material_source_at(index, "x").material_id);
        }
        let generation = generation();
        let ledger = AcceptedImportEmbeddingLedger { operation_id: "op" };
        let outcome = store.index_embeddings_for_materials_reporting(
            &mut fake_provider(generation.clone()),
            8,
            &ids,
            Some(ledger),
            None,
        );
        let outcome = outcome.expect("ledger batch persist must succeed");
        assert!(outcome.embedded >= 1);
    }

    /// The failed-embedding readiness invariant: once the embedding phase has
    /// resolved a generation, a material whose embeddings were NOT persisted
    /// must never count as user-facing ready. A failed embedding must not fall
    /// back to the lexical-only degraded rule.
    #[test]
    fn failed_embedding_never_reports_material_as_ready() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op", 1).unwrap();
        bound_material(&mut store, "op", 0, b"# Nota\n\ncontenido breve\n");
        let generation = generation();

        // Simulate the production failure shape: the generation is resolved
        // (provider available) but NO chunk embedding is persisted.
        store.record_generation(&generation).unwrap();
        store
            .mark_accepted_import_embedding_generation("op", &generation.generation_id)
            .unwrap();

        // Strict (generation-scoped) readiness must be 0, never the lexical
        // degraded count.
        assert_eq!(
            store
                .accepted_import_materials_ready("op", Some(&generation.generation_id))
                .unwrap(),
            0,
            "a material with missing embeddings must not be ready"
        );
        // And the operation still records the resolved generation.
        let operation = store.accepted_import_operation("op").unwrap().unwrap();
        assert_eq!(
            operation.embedding_generation_id.as_deref(),
            Some(generation.generation_id.as_str())
        );
    }

    /// CASE 11: embeddings from a previous/other generation must never make a
    /// material count as ready for the active generation.
    #[test]
    fn materials_ready_ignores_stale_generation_embeddings() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op", 1).unwrap();
        bound_material(&mut store, "op", 0, b"# Nota\n\ncontenido\n");
        let generation = generation();
        store
            .index_embeddings_for_materials_reporting(
                &mut fake_provider(generation.clone()),
                8,
                &[material_source_at(0, "x").material_id],
                None,
                None,
            )
            .unwrap();
        // The active generation counts the material ready.
        assert_eq!(
            store
                .accepted_import_materials_ready("op", Some(&generation.generation_id))
                .unwrap(),
            1
        );
        // A stale generation id must not.
        assert_eq!(
            store
                .accepted_import_materials_ready("op", Some("stale-generation-0000"))
                .unwrap(),
            0
        );
    }

    /// CASE 3: a failed material (lexical stage failed) is excluded from
    /// `materialsReady`; a failed lexical stage never fabricates usability.
    #[test]
    fn materials_ready_excludes_failed_materials() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op", 2).unwrap();
        // Material 0 indexes cleanly.
        bound_material(&mut store, "op", 0, b"# Nota 0\n\ncontenido\n");
        // Material 1 fails its lexical stage (invalid UTF-8).
        let bad = material_source_at(1, "material-1.md");
        store
            .bind_accepted_import_material("op", &bad.material_id)
            .unwrap();
        assert!(store.index(&bad, &[0xff]).is_err());
        let generation = generation();
        store
            .index_embeddings_for_materials_reporting(
                &mut fake_provider(generation.clone()),
                8,
                &[material_source_at(0, "x").material_id],
                None,
                None,
            )
            .unwrap();
        assert_eq!(
            store
                .accepted_import_materials_ready("op", Some(&generation.generation_id))
                .unwrap(),
            1,
            "only the cleanly-indexed material counts ready"
        );
    }

    /// An `error`-state embedding row for the active generation must not make a
    /// chunk count as ready (the readiness predicate requires `state='ready'`).
    #[test]
    fn error_state_embeddings_never_make_a_chunk_ready() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op", 1).unwrap();
        let outcome = bound_material(&mut store, "op", 0, b"# Nota\n\ncontenido\n");
        assert!(outcome.chunk_count >= 1);
        let generation = generation();
        store.record_generation(&generation).unwrap();
        // Fabricate an error embedding for every reachable chunk.
        let chunk_id: String = store
            .connection
            .query_row("SELECT chunk_id FROM chunks LIMIT 1", [], |row| row.get(0))
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO chunk_embeddings(chunk_id, generation_id, vector, dimensions, normalized, state, embedded_at)
                 VALUES(?1, ?2, zeroblob(1536), 384, 1, 'error', ?3)",
                params![chunk_id, generation.generation_id, unix_seconds()],
            )
            .unwrap();
        assert_eq!(
            store
                .accepted_import_materials_ready("op", Some(&generation.generation_id))
                .unwrap(),
            0,
            "an error embedding is not a ready embedding"
        );
    }

    /// CASE 5: duplicate content shares one document/chunks; reused embeddings
    /// satisfy readiness but must never inflate the material/file count.
    #[test]
    fn materials_ready_with_reused_embeddings_counts_materials_not_chunks() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op", 2).unwrap();
        let bytes = b"# Compartido\n\nEl mismo contenido duplicado en dos archivos.\n\ncontenido repetido\n";
        let first = bound_material(&mut store, "op", 0, bytes);
        assert!(first.chunk_count >= 1);
        let second = bound_material(&mut store, "op", 1, bytes);
        assert!(second.reused, "identical bytes reuse the same document");
        let generation = generation();
        let ids = vec![
            material_source_at(0, "x").material_id,
            material_source_at(1, "x").material_id,
        ];
        store
            .index_embeddings_for_materials_reporting(
                &mut fake_provider(generation.clone()),
                8,
                &ids,
                None,
                None,
            )
            .unwrap();
        let ready = store
            .accepted_import_materials_ready("op", Some(&generation.generation_id))
            .unwrap();
        assert_eq!(
            ready, 2,
            "reused embeddings satisfy readiness but count materials, not chunks"
        );
        assert!(
            ready <= store.accepted_import_material_ids("op").unwrap().len(),
            "ready never exceeds the accepted file total"
        );
    }

    /// A zero-chunk material counts ready once its lexical stage completes
    /// (the "every reachable chunk" predicate is vacuously satisfied).
    #[test]
    fn zero_chunk_material_counts_ready_once_lexical_is_complete() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op", 1).unwrap();
        let outcome = bound_material(&mut store, "op", 0, b"");
        assert_eq!(outcome.chunk_count, 0, "empty markdown yields no chunks");
        assert_eq!(
            store
                .accepted_import_materials_ready("op", Some(&generation().generation_id))
                .unwrap(),
            1
        );
    }

    /// Section D degraded rule: without an embedding context the caller passes
    /// `None` and lexically-ready materials count, so an offline/lexical-only
    /// import is not stuck at 0/N. Lexical and semantic units are never mixed
    /// in a single call.
    #[test]
    fn degraded_rule_counts_lexically_ready_when_no_embedding_context() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op", 2).unwrap();
        bound_material(&mut store, "op", 0, b"# A\n\ncontenido\n");
        // Material 1 fails lexical: it must not count even in the degraded rule.
        let bad = material_source_at(1, "material-1.md");
        store
            .bind_accepted_import_material("op", &bad.material_id)
            .unwrap();
        assert!(store.index(&bad, &[0xff]).is_err());
        // The strict rule finds the lexically-ready material unusable (its
        // chunks have no active-generation embeddings)...
        assert_eq!(
            store
                .accepted_import_materials_ready("op", Some(&generation().generation_id))
                .unwrap(),
            0
        );
        // ...while the degraded rule counts lexically-ready materials only.
        assert_eq!(
            store.accepted_import_materials_ready("op", None).unwrap(),
            1
        );
    }

    /// CASE 7: reopening the store recomputes `materialsReady` from durable
    /// state, and the embedding-progress path persisted the generation id.
    #[test]
    fn materials_ready_survives_reopen_with_persisted_generation_id() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        {
            let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
            store.create_accepted_import_operation("op", 2).unwrap();
            bound_material(&mut store, "op", 0, b"# A\n\ncontenido\n");
            bound_material(&mut store, "op", 1, b"# B\n\ncontenido\n");
            let generation = generation();
            let ledger = AcceptedImportEmbeddingLedger { operation_id: "op" };
            store
                .index_embeddings_for_materials_reporting(
                    &mut fake_provider(generation.clone()),
                    8,
                    &[material_source_at(0, "x").material_id],
                    Some(ledger),
                    None,
                )
                .unwrap();
        }
        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        let operation = reopened
            .accepted_import_operation("op")
            .unwrap()
            .expect("operation exists");
        assert_eq!(
            operation.embedding_generation_id.as_deref(),
            Some(generation().generation_id.as_str()),
            "the embedding-progress path persists the generation id"
        );
        assert_eq!(
            reopened
                .accepted_import_materials_ready("op", Some(&generation().generation_id))
                .unwrap(),
            1,
            "recomputed from durable state after reopen, no re-embedding needed"
        );
    }

    /// Performance guard: the readiness aggregate must stay bounded and
    /// indexed even on a large corpus. This never scans the whole corpus; it
    /// walks only the operation's materials and their reachable chunks.
    #[test]
    fn materials_ready_query_stays_bounded_on_a_large_corpus() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op", 50).unwrap();
        let generation = generation();
        let ids = (0..50)
            .map(|index| {
                // Each part is a paragraph slightly over the chunk width, so
                // every file produces ~3 chunks and the corpus reaches the
                // tens-of-thousands-of-chunks scale of a large production
                // import, exposing an accidental full-corpus scan.
                let body = (0..120)
                    .map(|part| {
                        format!("# Nota {index} parte {part}\n\n{}\n", "texto ".repeat(280))
                    })
                    .collect::<String>();
                bound_material(&mut store, "op", index, body.as_bytes());
                material_source_at(index, "x").material_id
            })
            .collect::<Vec<_>>();
        let total_chunks: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .unwrap();
        assert!(
            total_chunks > 10_000,
            "corpus must be large: {total_chunks}"
        );
        store
            .index_embeddings_for_materials_reporting(
                &mut fake_provider(generation.clone()),
                8,
                &ids,
                None,
                None,
            )
            .unwrap();
        // Warm cache, then measure a single readiness aggregate. The bound is
        // deliberately generous (this is a regression guard against a
        // full-corpus scan, not a micro-benchmark).
        let _ = store
            .accepted_import_materials_ready("op", Some(&generation.generation_id))
            .unwrap();
        let started = std::time::Instant::now();
        for _ in 0..10 {
            assert_eq!(
                store
                    .accepted_import_materials_ready("op", Some(&generation.generation_id))
                    .unwrap(),
                50
            );
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "10 readiness aggregates on a {total_chunks}-chunk corpus took {elapsed:?}"
        );
    }

    /// Query-shape guard: the readiness aggregate must not plan a full scan of
    /// the big chunk/embedding tables; it must use the primary keys and the
    /// `chunks_document_ordinal` index.
    #[test]
    fn materials_ready_query_plan_uses_indexes_not_full_scans() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let store = KnowledgeStore::open(&project_root, &pid).unwrap();
        let plan: String = {
            let mut statement = store
                .connection
                .prepare(
                    "EXPLAIN QUERY PLAN
                     SELECT COUNT(*)
                     FROM accepted_import_materials aim
                     JOIN material_index_state mis ON mis.material_id = aim.material_id
                     WHERE aim.operation_id = 'op'
                       AND mis.state = 'ready'
                       AND NOT EXISTS (
                         SELECT 1
                         FROM material_sources ms
                         JOIN chunks c ON c.document_id = ms.document_id
                         WHERE ms.material_id = aim.material_id
                           AND NOT EXISTS (
                             SELECT 1 FROM chunk_embeddings e
                             WHERE e.chunk_id = c.chunk_id
                               AND e.generation_id = 'gen'
                               AND e.state = 'ready'
                               AND e.dimensions = 384
                           )
                       )",
                )
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
                .join("\n")
        };
        let lower = plan.to_lowercase();
        // The plan must reference the big tables through indexes/primary keys,
        // never a bare `SCAN` of `chunks` or `chunk_embeddings`.
        assert!(
            !lower.contains("scan chunks"),
            "chunks must not be full-scanned: {plan}"
        );
        assert!(
            !lower.contains("scan chunk_embeddings"),
            "chunk_embeddings must not be full-scanned: {plan}"
        );
    }

    #[test]
    fn txt_and_markdown_are_deterministic_and_keep_provenance() {
        let txt = extract(&source("a.txt"), b"Uno\r\n\r\nDos\n").unwrap();
        assert_eq!(txt.normalized_text, "Uno\n\nDos\n");
        assert_eq!(txt.chunks.len(), 2);
        let md = extract(&source("a.md"), b"# Titulo\n\nTexto\n\n- uno\n- dos\n").unwrap();
        assert_eq!(
            md.chunks
                .iter()
                .map(|c| c.provenance.structural_type.as_str())
                .collect::<Vec<_>>(),
            ["heading", "paragraph", "list_item", "list_item"]
        );
        assert_eq!(md.chunks[1].provenance.heading_path, ["Titulo"]);
    }
    #[test]
    fn invalid_utf8_and_unsupported_format_fail_locally() {
        assert!(matches!(
            extract(&source("bad.txt"), &[0xff]),
            Err(KnowledgeError::InvalidUtf8)
        ));
        assert!(matches!(
            extract(&source("bad.pdf"), b"x"),
            Err(KnowledgeError::UnsupportedFormat(_))
        ));
    }
    #[test]
    fn structural_oversize_is_deterministically_subdivided() {
        let input = format!("# A\n\n{}", "palabra ".repeat(500));
        let first = extract(&source("a.md"), input.as_bytes()).unwrap();
        let second = extract(&source("a.md"), input.as_bytes()).unwrap();
        assert!(first.chunks.len() > 2);
        assert_eq!(first.chunks, second.chunks);
        assert!(
            first
                .chunks
                .iter()
                .all(|c| c.text.chars().count() <= MAX_CHUNK_CHARS)
        );
    }
    #[test]
    fn indexes_reuses_changes_deletes_and_searches() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let source = source("guia.md");
        let add = store
            .index(
                &source,
                b"# Fotosintesis\n\nLas plantas producen energia solar.",
            )
            .unwrap();
        assert!(!add.reused);
        assert_eq!(store.search("plantas energia", 10).unwrap().len(), 1);
        assert!(
            store
                .index(
                    &source,
                    b"# Fotosintesis\n\nLas plantas producen energia solar."
                )
                .unwrap()
                .reused
        );
        let changed = store
            .index(&source, b"# Fotosintesis\n\nLas hojas capturan luz.")
            .unwrap();
        assert!(!changed.reused);
        assert!(store.search("energia", 10).unwrap().is_empty());
        assert_eq!(store.search("hojas", 10).unwrap().len(), 1);
        assert!(store.remove(&source.material_id).unwrap());
        assert!(store.search("hojas", 10).unwrap().is_empty());
    }
    #[test]
    fn same_bytes_under_renamed_material_reuses_document_and_projects_isolate() {
        let (one, pid) = root();
        let (two, pid2) = root();
        let mut first = KnowledgeStore::open(one.path().join(PID), &pid).unwrap();
        let second = KnowledgeStore::open(two.path().join(PID), &pid2).unwrap();
        let one_source = source("uno.txt");
        let mut renamed = source("renombrado.txt");
        renamed.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db16").unwrap();
        renamed.relative_path = format!("inputs/{}/renombrado.txt", renamed.material_id);
        first.index(&one_source, b"secreto local").unwrap();
        assert!(first.index(&renamed, b"secreto local").unwrap().reused);
        assert_eq!(first.search("secreto", 10).unwrap().len(), 2);
        assert!(second.search("secreto", 10).unwrap().is_empty());
    }
    #[test]
    fn fts_query_is_literal_and_source_path_cannot_escape() {
        assert_eq!(fts_query("x OR y:*"), "\"x\" AND \"OR\" AND \"y\"");
        let mut unsafe_source = source("a.txt");
        unsafe_source.relative_path = "../../etc/passwd".into();
        assert!(extract(&unsafe_source, b"x").is_ok());
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        assert!(matches!(
            store.index(&unsafe_source, b"x"),
            Err(KnowledgeError::InvalidProjectRoot)
        ));
    }

    #[test]
    fn one_invalid_document_does_not_affect_an_already_ready_document() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        let ready = source("ready.txt");
        store.index(&ready, b"contenido conservado").unwrap();
        let malformed = source("malformed.txt");
        assert!(matches!(
            store.index(&malformed, &[0xff]),
            Err(KnowledgeError::InvalidUtf8)
        ));
        assert_eq!(store.search("conservado", 10).unwrap().len(), 1);
    }

    #[test]
    fn material_index_status_transitions_pending_ready_and_survives_reopen() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let source = source("guia.txt");
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();

        assert_eq!(
            store.begin_material_indexing(&source).unwrap(),
            MaterialIndexState::Pending
        );
        let pending = store
            .material_index_status(&source.material_id)
            .unwrap()
            .unwrap();
        assert_eq!(pending.state, MaterialIndexState::Pending);
        assert!(pending.retryable);
        assert!(pending.last_attempt_at.is_some());

        store.index(&source, b"contenido indexado").unwrap();
        drop(store);
        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        let ready = reopened
            .material_index_status(&source.material_id)
            .unwrap()
            .unwrap();
        assert_eq!(ready.state, MaterialIndexState::Ready);
        assert_eq!(ready.failure, None);
        assert!(!ready.retryable);
        assert_eq!(ready.last_attempt_at, None);
    }

    #[test]
    fn failed_material_index_is_sanitized_and_explicit_retry_becomes_ready() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        let source = source("corrupto.txt");
        assert!(matches!(
            store.index(&source, &[0xff]),
            Err(KnowledgeError::InvalidUtf8)
        ));
        let failed = store
            .material_index_status(&source.material_id)
            .unwrap()
            .unwrap();
        assert_eq!(failed.state, MaterialIndexState::Failed);
        assert_eq!(
            failed.failure,
            Some(MaterialIndexFailure::InvalidTextEncoding)
        );
        assert!(failed.retryable);
        assert!(failed.last_attempt_at.is_some());

        store
            .retry_material_index(&source, b"contenido corregido")
            .unwrap();
        let ready = store
            .material_index_status(&source.material_id)
            .unwrap()
            .unwrap();
        assert_eq!(ready.state, MaterialIndexState::Ready);
        assert_eq!(ready.failure, None);
        let stored_category: Option<String> = store
            .connection
            .query_row(
                "SELECT failure_category FROM material_index_state WHERE material_id=?1",
                [source.material_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_ne!(stored_category.as_deref(), Some("contenido corregido"));
        let columns: Vec<String> = store
            .connection
            .prepare("SELECT name FROM pragma_table_info('material_index_state')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(!columns.iter().any(|column| column.contains("text")));
    }

    #[test]
    fn unsupported_and_crash_like_pending_states_are_never_silently_ready() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        let unsupported = source("archivo.pdf");
        assert!(matches!(
            store.index(&unsupported, b"not a PDF"),
            Err(KnowledgeError::UnsupportedFormat(_))
        ));
        let status = store
            .material_index_status(&unsupported.material_id)
            .unwrap()
            .unwrap();
        assert_eq!(status.state, MaterialIndexState::Unsupported);
        assert_eq!(
            status.failure,
            Some(MaterialIndexFailure::UnsupportedFormat)
        );
        assert!(!status.retryable);

        let mut pending = source("pendiente.txt");
        pending.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db16").unwrap();
        pending.relative_path = format!("inputs/{}/pendiente.txt", pending.material_id);
        store.begin_material_indexing(&pending).unwrap();
        drop(store);
        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        let status = reopened
            .material_index_status(&pending.material_id)
            .unwrap()
            .unwrap();
        assert_eq!(status.state, MaterialIndexState::Pending);
        assert!(status.retryable);
    }

    #[test]
    fn material_status_is_project_local_and_delete_cleans_only_its_source_state() {
        let (one, pid) = root();
        let (two, pid2) = root();
        let mut first = KnowledgeStore::open(one.path().join(PID), &pid).unwrap();
        let mut second = KnowledgeStore::open(two.path().join(PID), &pid2).unwrap();
        let one_source = source("uno.txt");
        let mut alias = source("dos.txt");
        alias.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db16").unwrap();
        alias.relative_path = format!("inputs/{}/dos.txt", alias.material_id);
        first.index(&one_source, b"contenido compartido").unwrap();
        first.index(&alias, b"contenido compartido").unwrap();
        second.index(&one_source, b"contenido aislado").unwrap();
        assert_eq!(
            first
                .material_index_status(&one_source.material_id)
                .unwrap()
                .unwrap()
                .state,
            MaterialIndexState::Ready
        );
        assert_eq!(first.search("compartido", 10).unwrap().len(), 2);
        first.remove(&one_source.material_id).unwrap();
        assert!(
            first
                .material_index_status(&one_source.material_id)
                .unwrap()
                .is_none()
        );
        assert_eq!(first.search("compartido", 10).unwrap().len(), 1);
        assert_eq!(
            first
                .material_index_status(&alias.material_id)
                .unwrap()
                .unwrap()
                .state,
            MaterialIndexState::Ready
        );
        assert_eq!(
            second
                .material_index_status(&one_source.material_id)
                .unwrap()
                .unwrap()
                .state,
            MaterialIndexState::Ready
        );
    }

    #[test]
    fn v2_migration_preserves_k1_k2_data_and_adds_material_index_state() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let source = source("legacy.txt");
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store
            .index(&source, b"OpenShift conserva datos heredados")
            .unwrap();
        let mut provider = FakeEmbeddingProvider {
            generation: generation(),
            calls: 0,
        };
        store.index_embeddings(&mut provider, 8).unwrap();
        store
            .connection
            .execute("DROP TABLE material_index_state", [])
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE schema_meta SET value='2' WHERE key='schema_version'",
                [],
            )
            .unwrap();
        drop(store);

        let upgraded = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(upgraded.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(upgraded.search("OpenShift", 10).unwrap().len(), 1);
        assert_eq!(
            upgraded
                .connection
                .query_row("SELECT COUNT(*) FROM documents", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            upgraded
                .connection
                .query_row("SELECT COUNT(*) FROM chunks", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            upgraded
                .connection
                .query_row("SELECT COUNT(*) FROM chunk_embeddings", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(
            upgraded
                .material_index_status(&source.material_id)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn v3_migration_preserves_k1_k3_and_adds_summary_tables() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let source = source("migra.txt");
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store
            .index(&source, b"OpenShift conserva el resumen al migrar")
            .unwrap();
        let mut provider = FakeEmbeddingProvider {
            generation: generation(),
            calls: 0,
        };
        store.index_embeddings(&mut provider, 8).unwrap();
        // Summaries table does not exist yet at v3; simulate a v3 DB.
        store
            .connection
            .execute("DROP TABLE IF EXISTS summaries", [])
            .unwrap();
        store
            .connection
            .execute("DROP TABLE IF EXISTS summary_sources", [])
            .unwrap();
        store
            .connection
            .execute("DROP TABLE IF EXISTS summary_chunks", [])
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE schema_meta SET value='3' WHERE key='schema_version'",
                [],
            )
            .unwrap();
        drop(store);

        let upgraded = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(upgraded.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(upgraded.search("OpenShift", 10).unwrap().len(), 1);
        assert_eq!(upgraded.summary_ids().unwrap().len(), 0);
        let columns: Vec<String> = upgraded
            .connection
            .prepare("SELECT name FROM pragma_table_info('summaries')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(columns.contains(&"content_json".to_owned()));
    }

    #[test]
    fn v6_migration_adds_incremental_embedding_progress_columns() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op-v6", 1).unwrap();
        // Simulate a v6 DB that predates the incremental progress columns.
        for column in ["chunks_total", "embeddings_total", "embedding_started_at"] {
            store
                .connection
                .execute(
                    &format!("ALTER TABLE accepted_import_operations DROP COLUMN {column}"),
                    [],
                )
                .unwrap();
        }
        // HEAD/v6 did not yet have the K6 operation ledger.  This is a
        // production-faithful direct-upgrade fixture, rather than merely
        // changing the version marker on a v8-shaped database.
        store
            .connection
            .execute("DROP TABLE IF EXISTS summary_operations", [])
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE schema_meta SET value='6' WHERE key='schema_version'",
                [],
            )
            .unwrap();
        drop(store);

        let upgraded = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(upgraded.schema_version().unwrap(), SCHEMA_VERSION);
        let operation = upgraded
            .accepted_import_operation("op-v6")
            .unwrap()
            .unwrap();
        assert_eq!(operation.chunks_total, 0);
        assert_eq!(operation.embeddings_total, 0);
        assert_eq!(operation.embedding_started_at, 0);
        let columns: Vec<String> = upgraded
            .connection
            .prepare("SELECT name FROM pragma_table_info('accepted_import_operations')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(columns.contains(&"chunks_total".to_owned()));
        assert!(columns.contains(&"embeddings_total".to_owned()));
        assert!(columns.contains(&"embedding_started_at".to_owned()));
        let mut upgraded = upgraded;
        let operation = upgraded
            .create_summary_operation("v6-k6", Some("turn-v6"), "project", &[], "fixture-v6", None)
            .unwrap();
        assert_eq!(operation.status, SummaryOperationStatus::Pending);
    }

    /// Real committed historical schema was v6: existing user databases can hit
    /// this exact upgrade path. The v9 `conversation_active_material_set` table
    /// must be created by catch-up (not merely left from a fresh store), and the
    /// ordered active set must survive a close/reopen.
    #[test]
    fn v6_migration_creates_conversation_active_material_set_table() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.create_accepted_import_operation("op-v6", 1).unwrap();
        // Simulate a genuinely committed v6 database: it predates the
        // incremental embedding-progress columns, the K6 operation ledger, and
        // the v9 conversation-active material set.
        for column in ["chunks_total", "embeddings_total", "embedding_started_at"] {
            store
                .connection
                .execute(
                    &format!("ALTER TABLE accepted_import_operations DROP COLUMN {column}"),
                    [],
                )
                .unwrap();
        }
        store
            .connection
            .execute("DROP TABLE IF EXISTS summary_operations", [])
            .unwrap();
        store
            .connection
            .execute("DROP TABLE IF EXISTS conversation_active_material_set", [])
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE schema_meta SET value='6' WHERE key='schema_version'",
                [],
            )
            .unwrap();
        drop(store);

        let mut upgraded = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(upgraded.schema_version().unwrap(), SCHEMA_VERSION);
        // Catch-up must have created the table; it starts empty.
        assert!(
            upgraded
                .conversation_active_material_ids()
                .unwrap()
                .is_empty()
        );
        let ids: Vec<MaterialId> = (0..4)
            .map(|i| {
                MaterialId::parse(format!(
                    "0198e4a6-79b2-7b51-9e68-c2eb7af3db{index:02x}",
                    index = 0x20 + i
                ))
                .unwrap()
            })
            .collect();
        upgraded.set_conversation_active_material_set(&ids).unwrap();
        drop(upgraded);

        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(reopened.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(reopened.conversation_active_material_ids().unwrap(), ids);
    }

    /// A database that already reports schema_version=9 but was produced by an
    /// earlier rejected development build can be missing
    /// `conversation_active_material_set`. The ordinary open path must self-heal
    /// that table without bumping the schema version or touching Knowledge data.
    #[test]
    fn already_v9_missing_active_material_table_is_self_healed() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let source = source("self-heal.txt");
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.index(&source, b"preserved knowledge").unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        // Simulate the rejected-build defect: drop the table but leave
        // schema_meta at 9.
        store
            .connection
            .execute("DROP TABLE conversation_active_material_set", [])
            .unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        drop(store);

        // Reopen through the normal production open/migrate path. The store must
        // repair the missing table itself; the test never calls the helper.
        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(reopened.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(
            reopened
                .conversation_active_material_ids()
                .unwrap()
                .is_empty()
        );
        // Knowledge data survives the repair untouched.
        assert_eq!(reopened.search("preserved", 10).unwrap().len(), 1);
        drop(reopened);

        // Write an ordered active set and prove it round-trips after reopen.
        let ids: Vec<MaterialId> = (0..4)
            .map(|i| {
                MaterialId::parse(format!(
                    "0198e4a6-79b2-7b51-9e68-c2eb7af3db{index:02x}",
                    index = 0x30 + i
                ))
                .unwrap()
            })
            .collect();
        let mut writer = KnowledgeStore::open(&project_root, &pid).unwrap();
        writer.set_conversation_active_material_set(&ids).unwrap();
        drop(writer);

        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(reopened.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(reopened.conversation_active_material_ids().unwrap(), ids);
    }

    #[test]
    fn v5_migration_preserves_knowledge_and_creates_usable_summary_operations() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let source = source("v5.txt");
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.index(&source, b"preserved v5 knowledge").unwrap();
        store
            .create_accepted_import_operation("v5-import", 1)
            .unwrap();
        store
            .connection
            .execute("DROP TABLE IF EXISTS summary_operations", [])
            .unwrap();
        store
            .connection
            .execute(
                "ALTER TABLE accepted_import_operations DROP COLUMN agent_state",
                [],
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE schema_meta SET value='5' WHERE key='schema_version'",
                [],
            )
            .unwrap();
        drop(store);

        let mut upgraded = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(upgraded.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(upgraded.search("preserved", 10).unwrap().len(), 1);
        assert!(
            upgraded
                .create_summary_operation("v5-k6", None, "project", &[], "v5", None)
                .is_ok()
        );
    }

    #[test]
    fn v7_migration_creates_usable_summary_operations_and_is_idempotent() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let source = source("v7.txt");
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store.index(&source, b"preserved v7 knowledge").unwrap();
        store
            .connection
            .execute("DROP TABLE IF EXISTS summary_operations", [])
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE schema_meta SET value='7' WHERE key='schema_version'",
                [],
            )
            .unwrap();
        drop(store);

        let mut upgraded = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(upgraded.schema_version().unwrap(), SCHEMA_VERSION);
        upgraded
            .create_summary_operation("v7-k6", None, "project", &[], "v7", None)
            .unwrap();
        drop(upgraded);
        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert!(reopened.summary_operation("v7-k6").unwrap().is_some());
        assert_eq!(reopened.search("preserved", 10).unwrap().len(), 1);
    }

    type KnowledgeRows = (
        Vec<(String, String, i64)>,
        Vec<(String, String, String)>,
        Vec<(String, String, Option<String>)>,
    );

    fn knowledge_rows(store: &KnowledgeStore) -> KnowledgeRows {
        let documents = store
            .connection
            .prepare("SELECT document_id, original_sha256, byte_size FROM documents ORDER BY document_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let chunks = store
            .connection
            .prepare("SELECT chunk_id, document_id, text FROM chunks ORDER BY chunk_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let summaries = store
            .connection
            .prepare("SELECT summary_id, state, content_json FROM summaries ORDER BY summary_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        (documents, chunks, summaries)
    }

    fn summary_operation_table_count(store: &KnowledgeStore) -> usize {
        store
            .connection
            .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='summary_operations'")
            .unwrap()
            .query_row([], |row| row.get::<_, i64>(0))
            .unwrap() as usize
    }

    fn seeded_legacy_store(version: i64) -> (tempfile::TempDir, ProjectId, PathBuf, KnowledgeRows) {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let source = source(&format!("legacy-v{version}.txt"));
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store
            .index(
                &source,
                format!("preserved v{version} knowledge UNIQUE-LEGACY-{version}").as_bytes(),
            )
            .unwrap();
        store
            .create_accepted_import_operation(&format!("import-v{version}"), 1)
            .unwrap();
        let content = summary::SummaryContent {
            summary: format!("legacy v{version} summary"),
            topics: vec![],
            decisions: vec![],
            action_items: vec![],
            questions: vec![],
        };
        store
            .store_summary(&SummaryNode {
                summary_id: format!("legacy-summary-v{version}"),
                level: SummaryLevel::Document,
                state: SummaryState::Ready,
                failure: None,
                content: Some(content.clone()),
                source_ids: vec![
                    store
                        .document_for_material(source.material_id.as_str())
                        .unwrap()
                        .unwrap(),
                ],
                source_chunk_ids: vec![],
                parent_summary_id: None,
                input_fingerprint: "legacy-in".to_owned(),
                output_fingerprint: fingerprint_output(&content),
                generation_id: "g".to_owned(),
                model_id: None,
                provider_id: None,
                contract_version: SUMMARY_CONTRACT_VERSION.to_owned(),
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        let expected = knowledge_rows(&store);
        store
            .connection
            .execute("DROP TABLE IF EXISTS summary_operations", [])
            .unwrap();
        // v9 introduced the conversation-active material set; a real pre-v9
        // database does not have it, so catch-up must create it.
        store
            .connection
            .execute("DROP TABLE IF EXISTS conversation_active_material_set", [])
            .unwrap();
        if version <= 6 {
            for column in ["chunks_total", "embeddings_total", "embedding_started_at"] {
                let _ = store.connection.execute(
                    &format!("ALTER TABLE accepted_import_operations DROP COLUMN {column}"),
                    [],
                );
            }
        }
        if version <= 5 {
            store
                .connection
                .execute(
                    "ALTER TABLE accepted_import_operations DROP COLUMN agent_state",
                    [],
                )
                .unwrap();
        }
        store
            .connection
            .execute(
                "UPDATE schema_meta SET value=?1 WHERE key='schema_version'",
                [version.to_string()],
            )
            .unwrap();
        drop(store);
        (temp, pid, project_root, expected)
    }

    fn assert_legacy_migration(version: i64) {
        let (temp, pid, project_root, expected) = seeded_legacy_store(version);
        let mut upgraded = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(upgraded.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(knowledge_rows(&upgraded), expected);
        assert_eq!(summary_operation_table_count(&upgraded), 1);
        // Every historical branch must land on v9 with the
        // conversation-active material set table present and usable.
        assert!(
            upgraded
                .conversation_active_material_ids()
                .unwrap()
                .is_empty()
        );
        upgraded
            .set_conversation_active_material_set(std::slice::from_ref(
                &MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db60").unwrap(),
            ))
            .unwrap();
        assert_eq!(
            upgraded.conversation_active_material_ids().unwrap().len(),
            1
        );
        assert_eq!(upgraded.search("preserved", 10).unwrap().len(), 1);
        assert_eq!(
            upgraded
                .get_summary(&format!("legacy-summary-v{version}"))
                .unwrap()
                .unwrap()
                .content
                .unwrap()
                .summary,
            format!("legacy v{version} summary")
        );
        upgraded
            .create_summary_operation(
                &format!("v{version}-k6"),
                Some("turn"),
                "project",
                &[],
                &format!("fp-v{version}"),
                None,
            )
            .unwrap();
        assert_eq!(summary_operation_table_count(&upgraded), 1);
        drop(upgraded);
        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        assert_eq!(knowledge_rows(&reopened), expected);
        assert!(
            reopened
                .summary_operation(&format!("v{version}-k6"))
                .unwrap()
                .is_some()
        );
        assert_eq!(summary_operation_table_count(&reopened), 1);
        drop(reopened);
        drop(temp);
    }

    #[test]
    fn case_11_v5_migrates_to_summary_operations_without_data_loss() {
        assert_legacy_migration(5);
    }

    #[test]
    fn case_11_v6_migrates_to_summary_operations_without_data_loss() {
        assert_legacy_migration(6);
    }

    #[test]
    fn case_11_v7_migrates_to_summary_operations_without_data_loss() {
        assert_legacy_migration(7);
    }

    #[test]
    fn summary_store_roundtrips_provenance_and_invalidates_transitively() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();

        let doc = SummaryNode {
            summary_id: "summary-doc".to_owned(),
            level: SummaryLevel::Document,
            state: SummaryState::Ready,
            failure: None,
            content: Some(summary::SummaryContent {
                summary: "resumen doc".to_owned(),
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            }),
            source_ids: vec!["document-id".to_owned()],
            source_chunk_ids: vec!["chunk-1".to_owned()],
            parent_summary_id: None,
            input_fingerprint: "f-in".to_owned(),
            output_fingerprint: "f-out".to_owned(),
            generation_id: "g".to_owned(),
            model_id: Some("m".to_owned()),
            provider_id: Some("p".to_owned()),
            contract_version: SUMMARY_CONTRACT_VERSION.to_owned(),
            created_at: 1,
            updated_at: 1,
        };
        let batch = SummaryNode {
            summary_id: "summary-batch".to_owned(),
            level: SummaryLevel::Batch,
            state: SummaryState::Ready,
            failure: None,
            content: Some(summary::SummaryContent {
                summary: "resumen batch".to_owned(),
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            }),
            source_ids: vec!["summary-doc".to_owned()],
            source_chunk_ids: vec![],
            parent_summary_id: None,
            input_fingerprint: "f-in-batch".to_owned(),
            output_fingerprint: "f-out-batch".to_owned(),
            generation_id: "g".to_owned(),
            model_id: Some("m".to_owned()),
            provider_id: Some("p".to_owned()),
            contract_version: SUMMARY_CONTRACT_VERSION.to_owned(),
            created_at: 1,
            updated_at: 1,
        };
        store.store_summary(&doc).unwrap();
        store.store_summary(&batch).unwrap();

        let read = store.get_summary("summary-doc").unwrap().unwrap();
        assert_eq!(read.source_chunk_ids, vec!["chunk-1".to_owned()]);
        assert_eq!(read.content.as_ref().unwrap().summary, "resumen doc");

        // Transitive invalidation: invalidating the document summary also marks
        // the batch (which lists it as a source) stale.
        store.invalidate_summary("summary-doc").unwrap();
        assert_eq!(
            store.get_summary("summary-doc").unwrap().unwrap().state,
            SummaryState::Stale
        );
        assert_eq!(
            store.get_summary("summary-batch").unwrap().unwrap().state,
            SummaryState::Stale
        );
    }

    fn generation() -> EmbeddingGeneration {
        EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active())
    }
    fn basis(index: usize) -> Vec<f32> {
        let mut vector = vec![0.0; 384];
        vector[index] = 1.0;
        vector
    }
    struct FakeEmbeddingProvider {
        generation: EmbeddingGeneration,
        calls: usize,
    }
    impl EmbeddingProvider for FakeEmbeddingProvider {
        fn generation(&self) -> &EmbeddingGeneration {
            &self.generation
        }
        fn embed_query(&mut self, _query: &str) -> Result<Vec<f32>> {
            self.calls += 1;
            Ok(basis(0))
        }
        fn embed_passages(&mut self, passages: &[String]) -> Result<Vec<Vec<f32>>> {
            self.calls += passages.len();
            Ok(passages
                .iter()
                .map(|text| {
                    if text.contains("OpenShift") {
                        basis(0)
                    } else {
                        basis(1)
                    }
                })
                .collect())
        }
    }

    #[test]
    fn model_manifest_and_vector_contract_are_pinned() {
        let model = ModelManifest::embedded().unwrap().active().clone();
        assert_eq!(model.model_id, "intfloat/multilingual-e5-small");
        assert_eq!(model.dimensions, 384);
        assert_eq!(model.query_prefix, "query: ");
        assert_eq!(model.passage_prefix, "passage: ");
        assert_eq!(model.artifacts[0].bytes, 470_268_510);
        let serialized = serialize_vector(&basis(0)).unwrap();
        assert_eq!(serialized.len(), 1536);
        assert_eq!(deserialize_vector(&serialized).unwrap(), basis(0));
        assert!(deserialize_vector(&serialized[..1532]).is_err());
        assert!(serialize_vector(&vec![0.0; 383]).is_err());
    }

    #[test]
    fn semantic_index_reuses_current_generation_and_ranks_locally() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        store.index(&source("ops.txt"), b"OpenShift utiliza operadores para automatizar la administracion y el ciclo de vida de componentes.").unwrap();
        let mut photos = source("plantas.txt");
        photos.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db16").unwrap();
        photos.relative_path = format!("inputs/{}/plantas.txt", photos.material_id);
        store.index(&photos, b"La fotosintesis transforma energia luminica en energia quimica dentro de las plantas.").unwrap();
        let mut provider = FakeEmbeddingProvider {
            generation: generation(),
            calls: 0,
        };
        assert_eq!(
            store.index_embeddings(&mut provider, 8).unwrap().embedded,
            2
        );
        let after_first = provider.calls;
        assert_eq!(
            store.index_embeddings(&mut provider, 8).unwrap().embedded,
            0
        );
        assert_eq!(provider.calls, after_first);
        let results = store
            .semantic_search(
                &mut provider,
                "Como automatiza OpenShift la administracion?",
                2,
            )
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].chunk_text.contains("OpenShift"));
        assert!(results[0].similarity > results[1].similarity);
    }

    #[test]
    fn embedding_persist_waits_for_a_transient_writer_lock_instead_of_sqlite_busy() {
        // Regression for the `embedding_persist_failed`/SQLITE_BUSY collapse: a
        // concurrent writer (e.g. the reopen-recovery seam racing the accepted
        // turn) holding a short WAL write lock must not abort the local index.
        // `KnowledgeStore::open` sets `busy_timeout=5000ms`, so a commit on a
        // briefly-locked DB waits and succeeds instead of surfacing an opaque
        // persistence failure.
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store
            .index(&source("ops.txt"), b"OpenShift automatiza despliegues.")
            .unwrap();

        // Pin the structural contract first: the connection must carry a
        // non-zero busy_timeout so a transient lock is retried, not fatal.
        let busy_timeout: i64 = store
            .connection
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .unwrap();
        assert_eq!(busy_timeout, 5000, "busy_timeout must be pinned at 5000ms");

        // A second, independent connection holds an exclusive write lock while
        // the store attempts to persist a freshly produced embedding. The
        // commit must wait (busy_timeout) and succeed once the lock is released.
        let database_path = store.database_path().to_owned();
        let blocker = Connection::open(&database_path).unwrap();
        blocker.pragma_update(None, "journal_mode", "WAL").unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();

        let handle = {
            let mut store = store;
            let mut provider = FakeEmbeddingProvider {
                generation: generation(),
                calls: 0,
            };
            std::thread::spawn(move || store.index_embeddings(&mut provider, 8))
        };

        // Give the index thread time to reach the blocked commit, then release
        // the writer lock so busy_timeout can succeed within the 5 s window.
        std::thread::sleep(std::time::Duration::from_millis(150));
        blocker.execute_batch("COMMIT").unwrap();
        drop(blocker);

        let outcome = handle.join().unwrap().unwrap();
        assert_eq!(
            outcome.embedded, 1,
            "a transient writer lock must not produce embedding_persist_failed"
        );
    }

    /// The sanitized persistence classification must name the real failure kind
    /// instead of collapsing every non-`SqliteFailure` error (a prepare/bind/
    /// convert failure) into a misleading `other_storage_failure`.
    #[test]
    fn embedding_persist_classification_names_prepare_bind_and_convert_errors() {
        use rusqlite::Error as RE;

        // A bind-count mismatch is a binding error, never a storage failure.
        let bind = KnowledgeError::Sql(RE::InvalidParameterCount(1, 2));
        let f = bind.embedding_persist_class("embedding_index");
        assert_eq!(f.class, EmbeddingPersistClass::SqlBindingFailed);
        assert_eq!(f.sqlite_code, None);
        assert_eq!(f.kind, "InvalidParameterCount");

        // A prepare-time syntax error is a statement-prepare failure.
        let sqlite = Connection::open_in_memory().unwrap();
        sqlite.execute_batch("CREATE TABLE t(a INTEGER);").unwrap();
        let syntax = sqlite
            .execute("SELEC * FROM t", [])
            .expect_err("syntax error must fail");
        let f = KnowledgeError::Sql(syntax).embedding_persist_class("embedding_index");
        assert_eq!(f.class, EmbeddingPersistClass::StatementPrepareFailed);
        assert_eq!(f.sqlite_code, None);
        assert_eq!(f.kind, "SqlInputError");

        // A genuine SQLite constraint failure still carries its primary code.
        sqlite
            .execute_batch("CREATE TABLE c(x INTEGER CHECK(x > 0));")
            .unwrap();
        let violated = sqlite
            .execute("INSERT INTO c(x) VALUES(0)", [])
            .expect_err("CHECK violation");
        let f = KnowledgeError::Sql(violated).embedding_persist_class("embedding_index");
        assert_eq!(f.class, EmbeddingPersistClass::ConstraintFailed);
        assert_eq!(f.sqlite_code, Some(19));
        assert_eq!(f.kind, "SqliteFailure");

        // A non-Sql, non-Io error is a sanitized "Knowledge" kind.
        let other = KnowledgeError::ModelUnavailable;
        let f = other.embedding_persist_class("embedding_index");
        assert_eq!(f.class, EmbeddingPersistClass::OtherStorageFailure);
        assert_eq!(f.sqlite_code, None);
        assert_eq!(f.kind, "Knowledge");
    }

    #[test]
    fn scoped_embedding_batch_persists_with_distinct_parameters() {
        // Regression for the real `embedding_persist_failed` seen in the fresh
        // Fedora AppImage: the accepted-turn embedding path selects chunks through
        // `index_embeddings_for_materials` with a NON-empty material list.  That
        // scoped `SELECT` mixed a bare `?` material placeholder (auto-numbered to
        // index 1) with an explicit `?1` generation selector, so the two collided
        // on index 1 and `query_map` was handed one value more than the statement
        // declared — surfacing as `KnowledgeError::Sql`, which the app collapse
        // reported as `embedding_persist_failed`.  This test pins that a scoped
        // batch (one accepted material) produces a persisted embedding and never
        // the SQLite binding error.
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store
            .index(
                &source("nota.md"),
                b"# Nota\n\nOpenShift automatiza despliegues.",
            )
            .unwrap();

        let material_id = MaterialId::parse(MID).unwrap();
        let mut provider = FakeEmbeddingProvider {
            generation: generation(),
            calls: 0,
        };
        let outcome = store
            .index_embeddings_for_materials(&mut provider, 8, &[material_id])
            .unwrap();
        assert!(
            outcome.embedded >= 1,
            "a scoped accepted-material batch must persist at least one embedding \
             (not fail with an SQLite parameter-count error)"
        );
        assert_eq!(outcome.reused, 0);
    }

    #[test]
    fn corpus_stats_reports_sanitized_material_and_corpus_counts() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();

        let txt = source("reunion.txt");
        store
            .index(
                &txt,
                b"Primera reunion. Se definio el presupuesto para el trimestre.",
            )
            .unwrap();

        let mut md = source("notas.md");
        md.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db17").unwrap();
        md.relative_path = format!("inputs/{}/notas.md", md.material_id);
        store
            .index(&md, b"# Notas\n\nAccion pendiente: preparar el informe.")
            .unwrap();

        let mut image = source("foto.png");
        image.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db18").unwrap();
        image.relative_path = format!("inputs/{}/foto.png", image.material_id);
        image.media_type = Some("image/png".to_owned());
        // Unsupported media type is durably marked Unsupported, never ready.
        assert!(store.index(&image, b"\x89PNG").is_err());

        let stats = store.corpus_stats().unwrap();
        assert_eq!(stats.material_count, 3);
        assert_eq!(stats.ready, 2);
        assert_eq!(stats.unsupported, 1);
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.failed, 0);
        assert!(stats.chunks_total >= 2);
        assert_eq!(stats.embeddings_ready, 0);
        assert!(stats.corpus_bytes > 0);
        assert!(stats.corpus_utf8_chars > 0);
        assert_eq!(
            stats.naive_corpus_est_tokens,
            (stats.corpus_bytes / 3) as usize + usize::from(!stats.corpus_bytes.is_multiple_of(3))
        );
        // Sanitized: the debug form never leaks document text.
        assert!(!format!("{stats:?}").contains("reunion"));
    }

    #[test]
    fn ready_source_names_returns_only_sanitized_ready_names_for_dedup() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        store
            .index(&source("reunion.txt"), b"Se definio el presupuesto.")
            .unwrap();
        let mut md = source("notas.md");
        md.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db19").unwrap();
        md.relative_path = format!("inputs/{}/notas.md", md.material_id);
        store
            .index(&md, b"# Notas\n\nPendiente el informe.")
            .unwrap();
        // An unsupported source is never `ready`, so it is never in the dedup set.
        let mut image = source("foto.png");
        image.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db22").unwrap();
        image.relative_path = format!("inputs/{}/foto.png", image.material_id);
        image.media_type = Some("image/png".to_owned());
        let _ = store.index(&image, b"\x89PNG");

        let names = store.ready_source_names().unwrap();
        assert_eq!(names, vec!["notas.md".to_owned(), "reunion.txt".to_owned()]);
        // Sanitized: names are display names, never paths or ids.
        assert!(!names.iter().any(|name| name.contains("inputs/")));
        assert!(!names.iter().any(|name| name.contains("0198e4a6")));
    }

    #[test]
    fn corpus_bytes_counts_each_document_once_not_per_chunk() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        // A long document that chunking splits into several chunks.
        let body: String = (0..400)
            .map(|i| {
                format!("Parrafo {i}: se revisaron los acuerdos del proyecto de infraestructura.\n")
            })
            .collect();
        let mut txt = source("largo.txt");
        txt.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db20").unwrap();
        txt.relative_path = format!("inputs/{}/largo.txt", txt.material_id);
        store.index(&txt, body.as_bytes()).unwrap();

        let stats = store.corpus_stats().unwrap();
        let docs = store
            .connection
            .query_row("SELECT COUNT(*) FROM documents", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        let chunks = store
            .connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        assert!(chunks > 1, "fixture must split into several chunks");
        // corpus_bytes must equal the single document's byte count, NOT the
        // byte count multiplied by the chunk multiplicity (join-inflation bug).
        assert_eq!(stats.corpus_bytes, body.len() as u64);
        assert_eq!(stats.material_count, 1);
        assert_eq!(docs, 1);
    }

    fn synthetic_lexical(
        chunk_id: &str,
        document_id: &str,
        source: &str,
        text: &str,
    ) -> SearchResult {
        SearchResult {
            document_id: document_id.to_owned(),
            source_id: source.to_owned(),
            source_name: source.to_owned(),
            source_relative_path: format!("inputs/{source}/source.txt"),
            chunk_id: chunk_id.to_owned(),
            chunk_text: text.to_owned(),
            score: 1.0,
            provenance: Provenance {
                start_offset: 0,
                end_offset: text.len(),
                start_line: 1,
                end_line: 1,
                heading_path: vec![],
                structural_type: "paragraph".to_owned(),
            },
        }
    }
    fn synthetic_semantic(
        chunk_id: &str,
        document_id: &str,
        source: &str,
        text: &str,
        similarity: f32,
    ) -> SemanticSearchResult {
        SemanticSearchResult {
            document_id: document_id.to_owned(),
            source_id: source.to_owned(),
            source_name: source.to_owned(),
            source_relative_path: format!("inputs/{source}/source.txt"),
            chunk_id: chunk_id.to_owned(),
            chunk_text: text.to_owned(),
            similarity,
            generation_id: "active-generation".to_owned(),
            provenance: Provenance {
                start_offset: 0,
                end_offset: text.len(),
                start_line: 1,
                end_line: 1,
                heading_path: vec![],
                structural_type: "paragraph".to_owned(),
            },
        }
    }

    #[test]
    fn rrf_fuses_deduplicates_and_orders_deterministically() {
        let lexical = vec![
            synthetic_lexical("a", "doc-a", "a", "uno"),
            synthetic_lexical("b", "doc-b", "b", "dos"),
        ];
        let semantic = vec![
            synthetic_semantic("b", "doc-b", "b", "dos", 0.9),
            synthetic_semantic("a", "doc-a", "a", "uno", 0.8),
            synthetic_semantic("c", "doc-c", "c", "tres", 0.7),
        ];
        let options = HybridSearchOptions {
            final_limit: 3,
            ..Default::default()
        };
        let first = fuse_hybrid_results("consulta", &lexical, &semantic, &options);
        let second = fuse_hybrid_results("consulta", &lexical, &semantic, &options);
        assert_eq!(first, second);
        assert_eq!(
            first
                .iter()
                .map(|r| r.chunk_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert!(first[0].signals.lexical_match && first[0].signals.semantic_match);
        assert_eq!(first[2].lexical_rank, None);
        assert_eq!(
            first[0].embedding_generation_id.as_deref(),
            Some("active-generation")
        );
    }

    #[test]
    fn exact_identifiers_are_case_normalized_without_boosting_normal_language() {
        let lexical = vec![synthetic_lexical(
            "related",
            "related",
            "r",
            "Incidente general relacionado",
        )];
        let semantic = vec![
            synthetic_semantic(
                "related",
                "related",
                "r",
                "Incidente general relacionado",
                0.99,
            ),
            synthetic_semantic("exact", "exact", "e", "Ticket INC-12345 confirmado", 0.10),
        ];
        let results = fuse_hybrid_results(
            "inc-12345",
            &lexical,
            &semantic,
            &HybridSearchOptions::default(),
        );
        assert_eq!(results[0].chunk_id, "exact");
        assert!(results[0].signals.exact_identifier_match);
        assert!(exact_identifier_tokens("problema de memoria").is_empty());
        assert!(exact_identifier_tokens("RFC-999").contains("rfc-999"));
    }

    #[test]
    fn diversity_caps_and_options_are_enforced() {
        let lexical = vec![
            synthetic_lexical("a", "doc-a", "same", "a"),
            synthetic_lexical("b", "doc-a", "same", "b"),
            synthetic_lexical("c", "doc-c", "other", "c"),
        ];
        let options = HybridSearchOptions {
            final_limit: 3,
            max_per_document: 1,
            max_per_source: 1,
            ..Default::default()
        };
        let results = fuse_hybrid_results("consulta", &lexical, &[], &options);
        assert_eq!(
            results
                .iter()
                .map(|r| r.chunk_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
        assert!(matches!(
            validate_hybrid_options(&HybridSearchOptions {
                final_limit: 0,
                ..Default::default()
            }),
            Err(KnowledgeError::InvalidSearchOptions(_))
        ));
    }

    #[test]
    fn hybrid_falls_back_to_lexical_when_semantic_is_unavailable_and_projects_isolate() {
        let (one, pid) = root();
        let (two, pid2) = root();
        let mut first = KnowledgeStore::open(one.path().join(PID), &pid).unwrap();
        let second = KnowledgeStore::open(two.path().join(PID), &pid2).unwrap();
        first
            .index(&source("incidente.txt"), b"INC-12345: incidente de memoria")
            .unwrap();
        let (results, availability) = first
            .hybrid_search("INC-12345", None, HybridSearchOptions::default())
            .unwrap();
        assert_eq!(availability, SemanticAvailability::Unavailable);
        assert_eq!(results.len(), 1);
        assert!(results[0].signals.lexical_match && results[0].signals.exact_identifier_match);
        assert!(
            second
                .hybrid_search("INC-12345", None, HybridSearchOptions::default())
                .unwrap()
                .0
                .is_empty()
        );
    }

    #[test]
    fn spanish_hybrid_sanity_uses_both_signals_with_partial_vectors() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        store.index(&source("openshift.txt"), b"Los operadores administran automaticamente instalacion, actualizacion y reconciliacion de componentes OpenShift.").unwrap();
        let mut incident = source("incidente.txt");
        incident.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db16").unwrap();
        incident.relative_path = format!("inputs/{}/incidente.txt", incident.material_id);
        store
            .index(&incident, b"INC-12345: incidente de memoria.")
            .unwrap();
        let mut provider = FakeEmbeddingProvider {
            generation: generation(),
            calls: 0,
        };
        // Embed only the OpenShift chunk: the incident remains lexically searchable.
        store.record_generation(provider.generation()).unwrap();
        let open = store.search("OpenShift", 1).unwrap().remove(0);
        store.connection.execute("INSERT INTO chunk_embeddings(chunk_id, generation_id, vector, dimensions, normalized, state, embedded_at) VALUES(?1, ?2, ?3, 384, 1, 'ready', 0)", params![open.chunk_id, provider.generation().generation_id, serialize_vector(&basis(0)).unwrap()]).unwrap();
        let (semantic, availability) = store
            .hybrid_search(
                "Como se automatiza el ciclo de vida de componentes?",
                Some(&mut provider),
                HybridSearchOptions::default(),
            )
            .unwrap();
        assert_eq!(availability, SemanticAvailability::Available);
        assert_eq!(semantic[0].source_name, "openshift.txt");
        let (exact, _) = store
            .hybrid_search(
                "INC-12345",
                Some(&mut provider),
                HybridSearchOptions::default(),
            )
            .unwrap();
        assert_eq!(exact[0].source_name, "incidente.txt");
    }

    #[test]
    fn context_assembly_packages_spanish_hybrid_evidence_with_provenance_and_budget() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        store.index(&source("openshift.md"), b"# Operaciones\n\nOpenShift usa operadores para automatizar la administracion del ciclo de vida de componentes.").unwrap();
        let mut photos = source("plantas.txt");
        photos.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db16").unwrap();
        photos.relative_path = format!("inputs/{}/plantas.txt", photos.material_id);
        store
            .index(
                &photos,
                b"La fotosintesis convierte energia luminica en energia quimica.",
            )
            .unwrap();
        let mut provider = FakeEmbeddingProvider {
            generation: generation(),
            calls: 0,
        };
        store.index_embeddings(&mut provider, 8).unwrap();
        let (candidates, availability) = store
            .hybrid_search(
                "Como automatiza OpenShift la administracion del ciclo de vida de componentes?",
                Some(&mut provider),
                HybridSearchOptions::default(),
            )
            .unwrap();
        assert_eq!(availability, SemanticAvailability::Available);
        let package = store
            .assemble_context(
                "Como automatiza OpenShift la administracion del ciclo de vida de componentes?",
                &candidates,
                ContextAssemblyOptions {
                    max_evidence_budget: 120,
                    reserve_margin: 10,
                    max_entries: 1,
                    max_entry_budget: 110,
                    min_entry_budget: 20,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(package.entries[0].source_name, "openshift.md");
        assert!(
            package
                .entries
                .iter()
                .all(|entry| entry.source_name != "plantas.txt")
        );
        assert_eq!(package.entries[0].document_id, candidates[0].document_id);
        assert_eq!(package.entries[0].chunk_id, candidates[0].chunk_id);
        assert_eq!(package.entries[0].provenance, candidates[0].provenance);
        assert!(package.totals.estimated_budget_used <= package.totals.estimated_budget_limit);
    }

    #[test]
    fn context_assembly_preserves_exact_identifier_and_project_isolation() {
        let (one, pid) = root();
        let (two, pid2) = root();
        let mut first = KnowledgeStore::open(one.path().join(PID), &pid).unwrap();
        let second = KnowledgeStore::open(two.path().join(PID), &pid2).unwrap();
        first
            .index(
                &source("incidente.txt"),
                b"INC-12345: se reinicio el servicio afectado.",
            )
            .unwrap();
        let mut provider = FakeEmbeddingProvider {
            generation: generation(),
            calls: 0,
        };
        first.index_embeddings(&mut provider, 8).unwrap();
        let (candidates, _) = first
            .hybrid_search(
                "Que ocurrio con INC-12345?",
                Some(&mut provider),
                HybridSearchOptions::default(),
            )
            .unwrap();
        let package = first
            .assemble_context(
                "Que ocurrio con INC-12345?",
                &candidates,
                ContextAssemblyOptions::default(),
            )
            .unwrap();
        assert!(package.entries[0].signals.exact_identifier_match);
        assert_eq!(package.entries[0].source_name, "incidente.txt");
        assert_eq!(package.query_metadata.project_id, first.project_id());
        let empty = second
            .assemble_context(
                "Que ocurrio con INC-12345?",
                &[],
                ContextAssemblyOptions::default(),
            )
            .unwrap();
        assert!(empty.entries.is_empty());
        assert_eq!(empty.query_metadata.project_id, second.project_id());
        let foreign = second
            .assemble_context(
                "Que ocurrio con INC-12345?",
                &candidates,
                ContextAssemblyOptions::default(),
            )
            .unwrap();
        assert!(foreign.entries.is_empty());
    }

    #[test]
    fn context_assembly_enforces_tight_budget_multisource_caps_and_neighbors() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        store.index(&source("uno.md"), b"# Plataforma\n\nautomatizacion de componentes con operadores\n\nEl vecino explica reconciliacion y actualizacion.").unwrap();
        let mut two = source("dos.txt");
        two.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db16").unwrap();
        two.relative_path = format!("inputs/{}/dos.txt", two.material_id);
        store
            .index(
                &two,
                b"automatizacion de infraestructura mediante politicas.",
            )
            .unwrap();
        let (candidates, _) = store
            .hybrid_search(
                "automatizacion",
                None,
                HybridSearchOptions {
                    final_limit: 10,
                    ..Default::default()
                },
            )
            .unwrap();
        let broad = store
            .assemble_context(
                "automatizacion",
                &candidates,
                ContextAssemblyOptions {
                    max_evidence_budget: 500,
                    reserve_margin: 0,
                    max_entry_budget: 250,
                    min_entry_budget: 10,
                    max_per_document: 2,
                    max_per_source: 2,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(broad.totals.sources_represented >= 2);
        assert!(
            broad
                .entries
                .iter()
                .any(|entry| entry.signals.neighbor_of.is_some())
        );
        let first_neighbor = broad
            .entries
            .iter()
            .position(|entry| entry.signals.neighbor_of.is_some())
            .unwrap();
        assert!(
            broad.entries[..first_neighbor]
                .iter()
                .all(|entry| entry.signals.neighbor_of.is_none())
        );
        let tight = store
            .assemble_context(
                "automatizacion",
                &candidates,
                ContextAssemblyOptions {
                    max_evidence_budget: 45,
                    reserve_margin: 0,
                    max_entry_budget: 45,
                    min_entry_budget: 10,
                    neighbor_radius: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(tight.totals.estimated_budget_used <= 45);
        assert!(tight.entries.len() <= broad.entries.len());
    }

    /// Deterministic text-hashing provider: returns a distinct one-hot basis
    /// per distinct input, and records the batch sizes it receives. Used to
    /// prove the store batches at most 32 and associates every output with its
    /// exact input (no skips, duplicates, or reordering).
    #[derive(Debug)]
    struct TextHashingEmbeddingProvider {
        generation: EmbeddingGeneration,
        batch_sizes: Vec<usize>,
    }
    impl EmbeddingProvider for TextHashingEmbeddingProvider {
        fn generation(&self) -> &EmbeddingGeneration {
            &self.generation
        }
        fn embed_query(&mut self, query: &str) -> Result<Vec<f32>> {
            Ok(basis(query.chars().count() % 384))
        }
        fn embed_passages(&mut self, passages: &[String]) -> Result<Vec<Vec<f32>>> {
            self.batch_sizes.push(passages.len());
            Ok(passages
                .iter()
                .map(|text| basis(text.chars().count() % 384))
                .collect())
        }
    }

    fn stored_chunk_vector(store: &KnowledgeStore, chunk_id: &str) -> Vec<f32> {
        store
            .connection
            .query_row(
                "SELECT vector FROM chunk_embeddings WHERE chunk_id=?1",
                [chunk_id],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .map(|blob| deserialize_vector(&blob).expect("stored vector must deserialize"))
            .unwrap_or_default()
    }

    fn many_chunk_corpus() -> String {
        // ~80 chars per chunk target; ~97 chunks for ~8,200 chars (not a
        // multiple of 32, so a short final batch is exercised).
        (0..97)
            .map(|index| format!("El fragmento número {} describe un aspecto particular del corpus de pruebas y se repite de forma determinista para generar varios fragmentos de tamaño similar.\n\n", index))
            .collect()
    }

    #[test]
    fn embedding_batch_32_splits_and_preserves_ordering_and_association() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        let source = source("large.txt");
        store
            .index(&source, many_chunk_corpus().as_bytes())
            .unwrap();
        let chunk_ids = store
            .connection
            .prepare("SELECT chunk_id FROM chunks ORDER BY chunk_id")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(chunk_ids.len() > 32, "corpus must exceed one batch");

        let mut provider = TextHashingEmbeddingProvider {
            generation: generation(),
            batch_sizes: Vec::new(),
        };
        let outcome = store
            .index_embeddings(&mut provider, 32)
            .expect("batch-32 indexing must succeed");
        assert_eq!(outcome.embedded, chunk_ids.len());

        // Store batches at most 32 (plus the short remainder), never more.
        assert!(
            provider.batch_sizes.iter().all(|size| *size <= 32),
            "no batch may exceed the bounded size: {provider:?}"
        );
        assert_eq!(
            provider.batch_sizes.iter().sum::<usize>(),
            chunk_ids.len(),
            "every chunk must be embedded exactly once"
        );
        let expected_remainder = chunk_ids.len() % 32;
        if expected_remainder != 0 {
            assert_eq!(
                provider.batch_sizes.last().copied(),
                Some(expected_remainder),
                "the short final batch must be preserved"
            );
        }

        // Every chunk carries exactly the vector derived from its own text.
        let mut seen = std::collections::HashSet::new();
        for chunk_id in &chunk_ids {
            let text: String = store
                .connection
                .query_row(
                    "SELECT text FROM chunks WHERE chunk_id=?1",
                    [chunk_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                stored_chunk_vector(&store, chunk_id),
                basis(text.chars().count() % 384),
                "chunk {chunk_id} must be associated with exactly its own embedding"
            );
            assert!(
                seen.insert(chunk_id.clone()),
                "no chunk may be embedded twice"
            );
        }
        // No embedding was skipped: the ledger count matches the distinct set.
        assert_eq!(seen.len(), chunk_ids.len());
    }

    #[test]
    fn embedding_progress_reports_bounded_monotonic_counters_and_advances_ledger() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        let source = source("large.txt");
        store
            .index(&source, many_chunk_corpus().as_bytes())
            .unwrap();
        let total_chunks: usize = store
            .connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap() as usize;
        assert!(total_chunks > 32);
        store
            .create_accepted_import_operation("progress-op", 1)
            .unwrap();
        // Mirror the production boundary: the accepted-import caller records the
        // embedding-phase row (state + lexical counters) before the embedding
        // pass advances the embedding counters.
        store
            .update_accepted_import_operation(
                "progress-op",
                None,
                AcceptedImportState::IndexingEmbeddings,
                1,
                1,
                0,
                0,
                0,
                0,
            )
            .unwrap();

        let mut provider = TextHashingEmbeddingProvider {
            generation: generation(),
            batch_sizes: Vec::new(),
        };
        let mut events = Vec::new();
        let ledger = AcceptedImportEmbeddingLedger {
            operation_id: "progress-op",
        };
        let outcome = store
            .index_embeddings_for_materials_reporting(
                &mut provider,
                32,
                std::slice::from_ref(&source.material_id),
                Some(ledger),
                Some(&mut |progress| events.push(progress)),
            )
            .unwrap();
        assert_eq!(outcome.embedded, total_chunks);
        assert_eq!(outcome.reused, 0);

        // Bounded cadence: start + final, never a per-embedding event storm.
        assert!(!events.is_empty() && events.len() <= 3);
        assert_eq!(events.first().map(|e| e.completed), Some(0));
        assert_eq!(events.first().map(|e| e.total), Some(total_chunks));
        assert_eq!(events.last().map(|e| e.completed), Some(total_chunks));
        assert_eq!(events.last().map(|e| e.created), Some(total_chunks));
        assert_eq!(events.last().map(|e| e.reused), Some(0));
        // Monotonic, bounded, exact-terminal contract.
        assert!(
            events
                .iter()
                .zip(events.iter().skip(1))
                .all(|(before, after)| before.completed <= after.completed)
        );
        assert!(events.iter().all(|e| e.completed <= e.total));
        assert!(events.iter().all(|e| e.created <= e.total));

        // Durable ledger reflects incremental progress.
        let progress = store
            .accepted_import_operation("progress-op")
            .unwrap()
            .unwrap();
        assert_eq!(progress.state, AcceptedImportState::IndexingEmbeddings);
        assert_eq!(progress.lexical_completed, 1);
        assert_eq!(progress.embedding_completed, total_chunks);
        assert_eq!(progress.embeddings_created, total_chunks);
        assert_eq!(progress.embeddings_reused, 0);
        assert_eq!(progress.chunks_total, total_chunks);
        assert_eq!(progress.embeddings_total, total_chunks);
        assert!(progress.embedding_started_at > 0);
        // Never auto-completes: recovery still owns the terminal boundary.
        assert!(
            store
                .incomplete_accepted_import_operations()
                .unwrap()
                .iter()
                .any(|op| op.operation_id == "progress-op")
        );
    }

    #[test]
    fn embedding_progress_reuses_existing_vectors_without_claiming_new_work() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        let source = source("large.txt");
        store
            .index(&source, many_chunk_corpus().as_bytes())
            .unwrap();
        let mut provider = TextHashingEmbeddingProvider {
            generation: generation(),
            batch_sizes: Vec::new(),
        };
        let first = store.index_embeddings(&mut provider, 32).unwrap();
        let total_chunks = first.embedded;

        store
            .create_accepted_import_operation("progress-reuse", 1)
            .unwrap();
        let mut events = Vec::new();
        let ledger = AcceptedImportEmbeddingLedger {
            operation_id: "progress-reuse",
        };
        let second = store
            .index_embeddings_for_materials_reporting(
                &mut provider,
                32,
                std::slice::from_ref(&source.material_id),
                Some(ledger),
                Some(&mut |progress| events.push(progress)),
            )
            .unwrap();
        assert_eq!(second.embedded, 0);
        assert_eq!(second.reused, total_chunks);

        let progress = store
            .accepted_import_operation("progress-reuse")
            .unwrap()
            .unwrap();
        assert_eq!(progress.embedding_completed, 0);
        assert_eq!(progress.embeddings_created, 0);
        assert_eq!(progress.embeddings_reused, total_chunks);
        assert_eq!(progress.embeddings_total, 0);
        assert_eq!(progress.chunks_total, total_chunks);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            EmbeddingIndexProgress {
                completed: 0,
                total: 0,
                created: 0,
                reused: total_chunks,
                elapsed_ms: 0,
            }
        );
    }

    /// Records the exact ordered texts it receives (flattened across calls)
    /// plus per-call batch sizes and max character widths, so tests can verify
    /// deterministic length-aware ordering, batch composition, and per-chunk
    /// association. The returned vector is derived solely from the input text
    /// (its character count), so a positional-assumption bug that stored the
    /// wrong chunk's embedding would fail the per-chunk association check.
    #[derive(Debug)]
    struct RecordingEmbeddingProvider {
        generation: EmbeddingGeneration,
        received: Vec<String>,
        batch_sizes: Vec<usize>,
        batch_max_chars: Vec<usize>,
    }
    impl EmbeddingProvider for RecordingEmbeddingProvider {
        fn generation(&self) -> &EmbeddingGeneration {
            &self.generation
        }
        fn embed_query(&mut self, query: &str) -> Result<Vec<f32>> {
            Ok(basis(query.chars().count() % 384))
        }
        fn embed_passages(&mut self, passages: &[String]) -> Result<Vec<Vec<f32>>> {
            self.batch_sizes.push(passages.len());
            self.batch_max_chars.push(
                passages
                    .iter()
                    .map(|text| text.chars().count())
                    .max()
                    .unwrap_or(0),
            );
            self.received.extend(passages.iter().cloned());
            Ok(passages
                .iter()
                .map(|text| basis(text.chars().count() % 384))
                .collect())
        }
    }

    /// Deterministic padding-cost proxy for fixed-size batching: the sum of
    /// `batch_size × max_length_in_batch` over all batches. Lower is tighter;
    /// it is machine-independent and used only for structural assertions.
    fn batch_padding_cost(pending: &[(String, String)], batch_size: usize) -> usize {
        pending
            .chunks(batch_size)
            .map(|batch| {
                batch_size
                    * batch
                        .iter()
                        .map(|(_, text)| text.chars().count())
                        .max()
                        .unwrap_or(0)
            })
            .sum()
    }

    /// 64 blank-line-separated paragraphs (one chunk each) with a deliberately
    /// heterogeneous length distribution: even paragraphs are short (~20-116
    /// chars) and odd paragraphs are long (~200-355 chars), all with distinct
    /// character counts so a text-derived vector is injective within the set.
    fn varied_length_corpus(chunks: usize) -> String {
        let mut corpus = String::new();
        for index in 0..chunks {
            let target = if index % 2 == 0 {
                20 + (index / 2) * 3
            } else {
                200 + (index / 2) * 5
            };
            let fill = "abcdefghij ".repeat(target / 11 + 1);
            let paragraph: String = fill.chars().take(target).collect();
            corpus.push_str(paragraph.trim());
            corpus.push_str("\n\n");
        }
        corpus
    }

    /// 40 paragraphs in 20 pairs that share the exact same character length but
    /// carry different text, so length ties must be resolved by chunk_id.
    fn tie_length_corpus() -> String {
        let mut corpus = String::new();
        for group in 0..20 {
            for twin in 0..2 {
                let target = 40 + group * 7;
                let seed = if twin == 0 {
                    "abcdefghij "
                } else {
                    "jihgfedcba "
                };
                let fill = seed.repeat(target / 11 + 1);
                let paragraph: String = fill.chars().take(target).collect();
                corpus.push_str(paragraph.trim());
                corpus.push_str("\n\n");
            }
        }
        corpus
    }

    fn store_chunks(store: &KnowledgeStore) -> Vec<(String, String)> {
        store
            .connection
            .prepare("SELECT chunk_id, text FROM chunks")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn length_aware_batching_groups_similar_lengths_and_cuts_fixed_batch_padding() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        let source = source("large.txt");
        store
            .index(&source, varied_length_corpus(64).as_bytes())
            .unwrap();
        let pending = store_chunks(&store);
        assert_eq!(pending.len(), 64, "one chunk per varied-length paragraph");

        // Previous production feed: pending rows arrived in chunk_id order.
        let mut old = pending.clone();
        old.sort_by(|a, b| a.0.cmp(&b.0));
        let old_cost = batch_padding_cost(&old, 32);

        // New length-aware order.
        let mut new = pending.clone();
        sort_pending_by_length(&mut new);
        let new_cost = batch_padding_cost(&new, 32);

        assert_eq!(new.len(), pending.len(), "the chunk set must be unchanged");
        assert!(
            new_cost < old_cost,
            "length batching must tighten fixed-batch padding (old={old_cost}, new={new_cost})"
        );

        // The store must actually produce the tight length-sorted batches.
        let mut provider = RecordingEmbeddingProvider {
            generation: generation(),
            received: Vec::new(),
            batch_sizes: Vec::new(),
            batch_max_chars: Vec::new(),
        };
        let outcome = store.index_embeddings(&mut provider, 32).unwrap();
        assert_eq!(outcome.embedded, pending.len());
        let store_cost: usize = provider
            .batch_sizes
            .iter()
            .zip(&provider.batch_max_chars)
            .map(|(size, max)| size * max)
            .sum();
        assert_eq!(
            store_cost, new_cost,
            "store batches must match the length-aware grouping exactly"
        );
    }

    #[test]
    fn length_aware_ordering_is_deterministic_and_ties_break_by_chunk_id() {
        // Two independent stores over the same tie-heavy corpus must embed the
        // pending chunks in exactly the same length-aware order, with equal-
        // length chunks ordered deterministically by chunk_id.
        let mut sequences = Vec::new();
        for _ in 0..2 {
            let (temp, pid) = root();
            let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
            let source = source("large.txt");
            store
                .index(&source, tie_length_corpus().as_bytes())
                .unwrap();
            let chunks = store_chunks(&store);
            assert_eq!(chunks.len(), 40);

            let mut expected = chunks.clone();
            expected.sort_by_cached_key(|(id, text)| (text.chars().count(), id.clone()));
            let expected_ids = expected
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();

            let id_by_text = chunks
                .iter()
                .map(|(id, text)| (text.as_str(), id.as_str()))
                .collect::<std::collections::HashMap<_, _>>();
            assert_eq!(
                id_by_text.len(),
                chunks.len(),
                "twin texts must differ so the received order maps back uniquely"
            );

            let mut provider = RecordingEmbeddingProvider {
                generation: generation(),
                received: Vec::new(),
                batch_sizes: Vec::new(),
                batch_max_chars: Vec::new(),
            };
            store.index_embeddings(&mut provider, 32).unwrap();
            let received_ids = provider
                .received
                .iter()
                .map(|text| id_by_text[text.as_str()].to_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                received_ids, expected_ids,
                "the store must embed in (length, chunk_id) order"
            );
            sequences.push(received_ids);
        }
        assert_eq!(
            sequences[0], sequences[1],
            "two runs over the same pending set must produce identical order"
        );
    }

    #[test]
    fn length_aware_batching_preserves_per_chunk_association_and_short_final_batch() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        let source = source("large.txt");
        store
            .index(&source, varied_length_corpus(65).as_bytes())
            .unwrap();
        let chunks = store_chunks(&store);
        assert_eq!(chunks.len(), 65, "65 chunks => one short final batch");

        // Distinct character counts make the text-derived identity vector
        // injective, so a positional-assumption bug would corrupt the check.
        let mut lengths = chunks
            .iter()
            .map(|(_, text)| text.chars().count())
            .collect::<Vec<_>>();
        lengths.sort_unstable();
        assert!(
            lengths.windows(2).all(|pair| pair[0] < pair[1]),
            "the corpus must yield distinct per-chunk lengths"
        );

        store
            .create_accepted_import_operation("assoc-op", 1)
            .unwrap();
        let mut provider = RecordingEmbeddingProvider {
            generation: generation(),
            received: Vec::new(),
            batch_sizes: Vec::new(),
            batch_max_chars: Vec::new(),
        };
        let mut events = Vec::new();
        let ledger = AcceptedImportEmbeddingLedger {
            operation_id: "assoc-op",
        };
        let outcome = store
            .index_embeddings_for_materials_reporting(
                &mut provider,
                32,
                std::slice::from_ref(&source.material_id),
                Some(ledger),
                Some(&mut |progress| events.push(progress)),
            )
            .unwrap();
        assert_eq!(outcome.embedded, chunks.len());
        assert_eq!(outcome.reused, 0);

        // Bounded batches plus a short final batch, never more than 32.
        assert!(
            provider.batch_sizes.iter().all(|size| *size <= 32),
            "no batch may exceed the bounded size"
        );
        assert_eq!(
            provider.batch_sizes.iter().sum::<usize>(),
            chunks.len(),
            "every chunk must be embedded exactly once"
        );
        assert_eq!(
            provider.batch_sizes.last().copied(),
            Some(chunks.len() % 32),
            "the short final batch must be preserved after sorting"
        );

        // Every persisted embedding belongs to exactly its own chunk.
        let mut seen = std::collections::HashSet::new();
        for (chunk_id, text) in &chunks {
            assert_eq!(
                stored_chunk_vector(&store, chunk_id),
                basis(text.chars().count() % 384),
                "chunk {chunk_id} must keep exactly its own embedding"
            );
            assert!(
                seen.insert(chunk_id.clone()),
                "no chunk may be embedded twice"
            );
        }
        assert_eq!(seen.len(), chunks.len());

        // Progress contract stays monotonic, bounded, and exactly terminal.
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].completed <= pair[1].completed)
        );
        assert!(events.iter().all(|event| event.completed <= event.total));
        assert!(events.iter().all(|event| event.created <= event.total));
        let last = events.last().expect("start/final reports");
        assert_eq!(last.completed, chunks.len());
        assert_eq!(last.created, chunks.len());
        assert_eq!(last.reused, 0);
    }

    #[test]
    fn length_aware_sorting_never_reembeds_ready_chunks() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        let source = source("large.txt");
        store
            .index(&source, varied_length_corpus(40).as_bytes())
            .unwrap();

        let mut first = RecordingEmbeddingProvider {
            generation: generation(),
            received: Vec::new(),
            batch_sizes: Vec::new(),
            batch_max_chars: Vec::new(),
        };
        let outcome = store.index_embeddings(&mut first, 32).unwrap();
        assert_eq!(outcome.embedded, 40);

        // Second pass: every chunk already has a ready embedding, so nothing is
        // pending and the provider must never be called again. The length sort
        // applies only to pending work, never to reused chunks.
        let mut second = RecordingEmbeddingProvider {
            generation: generation(),
            received: Vec::new(),
            batch_sizes: Vec::new(),
            batch_max_chars: Vec::new(),
        };
        let outcome = store.index_embeddings(&mut second, 32).unwrap();
        assert_eq!(outcome.embedded, 0);
        assert_eq!(outcome.reused, 40);
        assert!(
            second.received.is_empty(),
            "no ready chunk may be re-embedded after reordering"
        );
    }
}

#[cfg(test)]
mod inventory_tests {
    use super::*;
    use tempfile::TempDir;

    const PID: &str = "0198e4a6-79b2-7b51-9e68-c2eb7af30001";
    const MIDS: [&str; 4] = [
        "0198e4a6-79b2-7b51-9e68-c2eb7af30011",
        "0198e4a6-79b2-7b51-9e68-c2eb7af30012",
        "0198e4a6-79b2-7b51-9e68-c2eb7af30013",
        "0198e4a6-79b2-7b51-9e68-c2eb7af30014",
    ];

    fn root() -> (TempDir, ProjectId) {
        let temp = TempDir::new().unwrap();
        let id = ProjectId::parse(PID).unwrap();
        let root = temp.path().join(PID);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("project.json"), "{}").unwrap();
        (temp, id)
    }

    fn material(name: &str, mid: &str) -> MaterialSource {
        MaterialSource {
            relative_path: format!("inputs/{mid}/{name}"),
            material_id: MaterialId::parse(mid).unwrap(),
            source_name: name.into(),
            media_type: None,
        }
    }

    fn set_indexed_at(store: &KnowledgeStore, mid: &str, indexed_at: i64) {
        store
            .connection
            .execute(
                "UPDATE documents SET indexed_at = ?1 WHERE document_id = (SELECT document_id FROM material_sources WHERE material_id = ?2)",
                rusqlite::params![indexed_at, mid],
            )
            .unwrap();
    }

    fn seed_ready(store: &mut KnowledgeStore, names: &[(&str, &str)]) {
        for (index, (name, mid)) in names.iter().enumerate() {
            store
                .index(
                    &material(name, mid),
                    format!("Contenido del material número {index} para {name}.\n").as_bytes(),
                )
                .unwrap();
        }
    }

    #[test]
    fn snapshot_is_ready_only_and_alphabetical() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        seed_ready(
            &mut store,
            &[
                ("zeta.md", MIDS[0]),
                ("alfa.md", MIDS[1]),
                ("medio.md", MIDS[2]),
            ],
        );
        let records = store
            .inventory_snapshot(Some(MaterialIndexState::Ready), InventorySort::Unsorted)
            .unwrap();
        let names: Vec<&str> = records
            .iter()
            .map(|record| record.source_name.as_str())
            .collect();
        assert_eq!(names, ["alfa.md", "medio.md", "zeta.md"]);
        assert!(
            records
                .iter()
                .all(|record| record.state == MaterialIndexState::Ready)
        );
        assert_eq!(
            store
                .inventory_count(Some(MaterialIndexState::Ready))
                .unwrap(),
            3
        );
    }

    #[test]
    fn chronological_ordering_uses_document_indexed_at_not_filename_dates() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        // Insertion order deliberately differs from the indexed_at timeline.
        seed_ready(
            &mut store,
            &[
                ("2026-09-01-a.md", MIDS[0]),
                ("2026-09-03-b.md", MIDS[1]),
                ("2026-09-02-c.md", MIDS[2]),
            ],
        );
        set_indexed_at(&store, MIDS[0], 1_000_000);
        set_indexed_at(&store, MIDS[1], 3_000_000);
        set_indexed_at(&store, MIDS[2], 2_000_000);
        let asc_records = store
            .inventory_snapshot(
                Some(MaterialIndexState::Ready),
                InventorySort::ChronologicalAsc,
            )
            .unwrap();
        let asc: Vec<&str> = asc_records
            .iter()
            .map(|record| record.source_name.as_str())
            .collect();
        assert_eq!(
            asc,
            ["2026-09-01-a.md", "2026-09-02-c.md", "2026-09-03-b.md"]
        );
        let desc_records = store
            .inventory_snapshot(
                Some(MaterialIndexState::Ready),
                InventorySort::ChronologicalDesc,
            )
            .unwrap();
        let desc: Vec<&str> = desc_records
            .iter()
            .map(|record| record.source_name.as_str())
            .collect();
        assert_eq!(
            desc,
            ["2026-09-03-b.md", "2026-09-02-c.md", "2026-09-01-a.md"]
        );
    }

    #[test]
    fn membership_is_exact_case_insensitive_basename_matching() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        seed_ready(
            &mut store,
            &[
                ("Reunion-2026-09-01.md", MIDS[0]),
                ("informe-mensual.txt", MIDS[1]),
            ],
        );
        // Exact persisted display name.
        assert!(matches!(
            store
                .find_material_by_source_name(
                    "Reunion-2026-09-01.md",
                    Some(MaterialIndexState::Ready)
                )
                .unwrap(),
            InventoryMembership::Exact { .. }
        ));
        // Case-insensitive normalized basename.
        assert!(matches!(
            store
                .find_material_by_source_name(
                    "reunion-2026-09-01.md",
                    Some(MaterialIndexState::Ready)
                )
                .unwrap(),
            InventoryMembership::Exact { .. }
        ));
        // Missing name.
        assert_eq!(
            store
                .find_material_by_source_name("no-existe.pdf", Some(MaterialIndexState::Ready))
                .unwrap(),
            InventoryMembership::NotFound
        );
    }

    #[test]
    fn membership_ambiguity_is_reported_for_duplicate_basenames() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        seed_ready(
            &mut store,
            &[("duplicado.md", MIDS[0]), ("duplicado.md", MIDS[1])],
        );
        assert_eq!(
            store
                .find_material_by_source_name("duplicado.md", Some(MaterialIndexState::Ready))
                .unwrap(),
            InventoryMembership::Ambiguous { count: 2 }
        );
    }

    #[test]
    fn inventory_survives_reopen_with_identical_results() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        {
            let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
            seed_ready(
                &mut store,
                &[("b.md", MIDS[0]), ("a.md", MIDS[1]), ("c.md", MIDS[2])],
            );
            set_indexed_at(&store, MIDS[0], 2_000_000);
            set_indexed_at(&store, MIDS[1], 1_000_000);
            set_indexed_at(&store, MIDS[2], 3_000_000);
        }
        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        let snapshot = reopened
            .inventory_snapshot(Some(MaterialIndexState::Ready), InventorySort::Unsorted)
            .unwrap();
        let names: Vec<&str> = snapshot
            .iter()
            .map(|record| record.source_name.as_str())
            .collect();
        assert_eq!(snapshot.len(), 3);
        assert_eq!(names, ["a.md", "b.md", "c.md"]);
        assert_eq!(
            reopened
                .inventory_count(Some(MaterialIndexState::Ready))
                .unwrap(),
            3
        );
        assert!(matches!(
            reopened
                .find_material_by_source_name("c.md", Some(MaterialIndexState::Ready))
                .unwrap(),
            InventoryMembership::Exact { .. }
        ));
    }

    #[test]
    fn state_filtering_excludes_failed_from_ready_but_keeps_all() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        seed_ready(&mut store, &[("bueno.md", MIDS[0])]);
        // Re-index with invalid UTF-8 to flip the same material to Failed; its
        // persisted material_sources row survives the rolled-back re-index.
        assert!(
            store
                .index(&material("bueno.md", MIDS[0]), b"\xff\xfe\xff")
                .is_err()
        );
        assert_eq!(
            store
                .inventory_snapshot(Some(MaterialIndexState::Ready), InventorySort::Unsorted)
                .unwrap()
                .len(),
            0
        );
        let all = store
            .inventory_snapshot(None, InventorySort::Unsorted)
            .unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].state, MaterialIndexState::Failed);
        assert_eq!(
            store
                .inventory_count(Some(MaterialIndexState::Ready))
                .unwrap(),
            0
        );
        assert_eq!(store.inventory_count(None).unwrap(), 1);
    }

    #[test]
    fn empty_inventory_reports_zero_and_not_found() {
        let (temp, pid) = root();
        let store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        assert_eq!(
            store
                .inventory_snapshot(Some(MaterialIndexState::Ready), InventorySort::Unsorted)
                .unwrap(),
            Vec::new()
        );
        assert_eq!(
            store
                .inventory_count(Some(MaterialIndexState::Ready))
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .find_material_by_source_name("nada.md", Some(MaterialIndexState::Ready))
                .unwrap(),
            InventoryMembership::NotFound
        );
    }

    #[test]
    fn content_addressed_materials_are_counted_once_each() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        // Byte-identical bodies share one canonical document but each persisted
        // material keeps its own canonical source name; no alias is fabricated.
        let body = b"# Identico\n\nMismo contenido exacto para ambas entradas.\n";
        store.index(&material("primero.md", MIDS[0]), body).unwrap();
        store.index(&material("segundo.md", MIDS[1]), body).unwrap();
        assert_eq!(
            store
                .inventory_count(Some(MaterialIndexState::Ready))
                .unwrap(),
            2
        );
        let snapshot = store
            .inventory_snapshot(Some(MaterialIndexState::Ready), InventorySort::Alpha)
            .unwrap();
        let names: Vec<&str> = snapshot
            .iter()
            .map(|record| record.source_name.as_str())
            .collect();
        assert_eq!(names, ["primero.md", "segundo.md"]);
    }

    #[test]
    fn summary_operation_inflight_fence_becomes_explicit_retry_required_after_restart() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        let selected = vec![MIDS[0].to_owned()];
        store
            .create_summary_operation(
                "sum-op",
                Some("turn-1"),
                "selected_sources",
                &selected,
                "compat-v1",
                Some("provider/model"),
            )
            .unwrap();
        store
            .summary_operation_begin_node("sum-op", "not-yet-committed-node")
            .unwrap();
        drop(store);

        let mut reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        let operation = reopened
            .reconcile_summary_operation_after_restart("sum-op")
            .unwrap();
        assert_eq!(operation.status, SummaryOperationStatus::RetryRequired);
        assert_eq!(
            operation.failure_class.as_deref(),
            Some("remote_outcome_unknown")
        );
        assert_eq!(operation.selected_ids, selected);
    }

    #[test]
    fn summary_operation_cancel_preserves_checkpoint_counters_across_restart() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store
            .create_summary_operation(
                "sum-cancel",
                Some("turn-2"),
                "selected_sources",
                &[MIDS[0].to_owned()],
                "compat-v1",
                None,
            )
            .unwrap();
        store
            .summary_operation_begin_node("sum-cancel", "node-before-cancel")
            .unwrap();
        store
            .summary_operation_checkpoint("sum-cancel", 17, 3, 1, 21)
            .unwrap();
        store
            .finish_summary_operation(
                "sum-cancel",
                SummaryOperationStatus::Cancelled,
                None,
                None,
                17,
                3,
                1,
                21,
            )
            .unwrap();
        drop(store);

        let reopened = KnowledgeStore::open(&project_root, &pid).unwrap();
        let operation = reopened.summary_operation("sum-cancel").unwrap().unwrap();
        assert_eq!(operation.status, SummaryOperationStatus::Cancelled);
        assert_eq!(
            (
                operation.nodes_generated,
                operation.nodes_reused,
                operation.retries,
                operation.remote_calls
            ),
            (17, 3, 1, 21)
        );
    }

    #[test]
    fn summary_operation_terminal_states_reject_late_begin_checkpoint_and_completion() {
        let (temp, pid) = root();
        let project_root = temp.path().join(PID);
        let mut store = KnowledgeStore::open(&project_root, &pid).unwrap();
        store
            .create_summary_operation(
                "sum-terminal",
                Some("turn-terminal"),
                "selected_sources",
                &[MIDS[0].to_owned()],
                "compat-v1",
                None,
            )
            .unwrap();
        store
            .finish_summary_operation(
                "sum-terminal",
                SummaryOperationStatus::Cancelled,
                None,
                None,
                0,
                0,
                0,
                0,
            )
            .unwrap();
        assert!(matches!(
            store.summary_operation_begin_node("sum-terminal", "late-node"),
            Err(KnowledgeError::SummaryTransitionLost)
        ));
        assert!(matches!(
            store.summary_operation_checkpoint("sum-terminal", 1, 0, 0, 1),
            Err(KnowledgeError::SummaryTransitionLost)
        ));
        assert!(matches!(
            store.finish_summary_operation(
                "sum-terminal",
                SummaryOperationStatus::Completed,
                None,
                Some("late-summary"),
                1,
                0,
                0,
                1,
            ),
            Err(KnowledgeError::SummaryTransitionLost)
        ));
        assert_eq!(
            store
                .summary_operation("sum-terminal")
                .unwrap()
                .unwrap()
                .status,
            SummaryOperationStatus::Cancelled
        );
    }

    #[test]
    fn summary_operation_retry_count_never_decreases_on_finish() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        store
            .create_summary_operation(
                "sum-retries",
                Some("turn-retries"),
                "selected_sources",
                &[MIDS[0].to_owned()],
                "compat-v1",
                Some("opencode/big-pickle"),
            )
            .unwrap();
        store
            .summary_operation_begin_node("sum-retries", "node-a")
            .unwrap();
        store
            .summary_operation_checkpoint("sum-retries", 1, 0, 4, 1)
            .unwrap();
        store
            .finish_summary_operation(
                "sum-retries",
                SummaryOperationStatus::Cancelled,
                None,
                None,
                1,
                0,
                0,
                1,
            )
            .unwrap();
        assert_eq!(
            store
                .summary_operation("sum-retries")
                .unwrap()
                .unwrap()
                .retries,
            4
        );
    }

    #[test]
    fn mark_summary_failed_does_not_poison_ready_node() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        let content = SummaryContent {
            summary: "ready prose".into(),
            topics: vec![],
            decisions: vec![],
            action_items: vec![],
            questions: vec![],
        };
        store
            .store_summary(&SummaryNode {
                summary_id: "ready-node".into(),
                level: crate::summary::SummaryLevel::Document,
                state: SummaryState::Ready,
                failure: None,
                content: Some(content.clone()),
                source_ids: vec![MIDS[0].to_owned()],
                source_chunk_ids: vec![],
                parent_summary_id: None,
                input_fingerprint: "in".into(),
                output_fingerprint: fingerprint_output(&content),
                generation_id: "g".into(),
                model_id: None,
                provider_id: None,
                contract_version: SUMMARY_CONTRACT_VERSION.to_owned(),
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        store
            .mark_summary_failed(
                "ready-node",
                crate::summary::SummaryLevel::Document,
                SummaryFailure::ExecutionFailed,
            )
            .unwrap();
        let node = store.get_summary("ready-node").unwrap().unwrap();
        assert_eq!(node.state, SummaryState::Ready);
        assert_eq!(node.content.unwrap().summary, "ready prose");
    }

    #[test]
    fn claim_summary_operation_retry_is_single_owner() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        store
            .create_summary_operation(
                "sum-claim",
                Some("turn-claim"),
                "selected_sources",
                &[MIDS[0].to_owned()],
                "compat-v1",
                Some("opencode/big-pickle"),
            )
            .unwrap();
        store
            .summary_operation_begin_node("sum-claim", "node")
            .unwrap();
        store
            .finish_summary_operation(
                "sum-claim",
                SummaryOperationStatus::RetryRequired,
                Some("remote_outcome_unknown"),
                None,
                0,
                0,
                2,
                1,
            )
            .unwrap();
        let claimed = store.claim_summary_operation_retry("sum-claim").unwrap();
        assert_eq!(claimed.status, SummaryOperationStatus::Running);
        assert_eq!(claimed.retries, 3);
        assert!(matches!(
            store.claim_summary_operation_retry("sum-claim"),
            Err(KnowledgeError::SummaryTransitionLost)
        ));
    }
}
