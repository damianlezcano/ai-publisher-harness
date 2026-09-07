//! Project-owned, deterministic local Knowledge foundation.
//!
//! This crate deliberately contains no provider, OpenCode, Tauri, network, or
//! embedding runtime dependency. Callers provide authorized immutable Material
//! bytes from `inputs/`; derived records live in `knowledge/knowledge.sqlite`.

#![forbid(unsafe_code)]

mod context;
mod model;
mod ort_provider;
mod summary;
pub use context::{
    BudgetEstimator, ConservativeCharBudgetEstimator, ContextAssembler, ContextAssemblyOptions,
    EvidenceEntry, EvidencePackage, EvidenceQueryMetadata, EvidenceTotals, ExcerptKind,
};
pub use model::{
    ModelArtifact, ModelGeneration, ModelInstallState, ModelManager, ModelManifest,
    semantic_safe_subdivide, token_count, verify_artifact,
};
pub use ort_provider::{
    OrtEmbeddingProvider, OrtProviderLoadError, runtime_library_from_executable,
};
pub use summary::{
    BatchOptions, RemoteSummarizer, SUMMARY_CONTRACT_VERSION, SummaryAccounting, SummaryContent,
    SummaryEvidenceRef, SummaryFailure, SummaryItem, SummaryLevel, SummaryNode, SummaryOutput,
    SummaryPlan, SummaryRequest, SummaryState, SummaryUsage, build_document_summary_request,
    build_synthesis_request, deserialize_content, fingerprint_output, fingerprint_summary_inputs,
    plan_project_summaries, select_document_evidence, serialize_content, validate_summary_output,
};

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use project_core::{MaterialId, ProjectId};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

pub const SCHEMA_VERSION: i64 = 4;
pub const NORMALIZATION_VERSION: &str = "nfc-lf-v1";
pub const CHUNKER_ID: &str = "structural-v1";
pub const CHUNKER_VERSION: &str = "structural-v1";
const MAX_CHUNK_CHARS: usize = 1_600;

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
        let generation = provider.generation().clone();
        self.record_generation(&generation)?;
        let mut statement = self.connection.prepare(
            "SELECT c.chunk_id, c.text FROM chunks c WHERE NOT EXISTS (
                SELECT 1 FROM chunk_embeddings e WHERE e.chunk_id=c.chunk_id
                AND e.generation_id=?1 AND e.state='ready' AND e.dimensions=384
            ) ORDER BY c.document_id, c.ordinal",
        )?;
        let pending = statement
            .query_map([&generation.generation_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        let total = self
            .connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| {
                row.get::<_, i64>(0)
            })? as usize;
        if pending.is_empty() {
            return Ok(EmbeddingIndexOutcome {
                embedded: 0,
                reused: total,
            });
        }
        let size = batch_size.clamp(1, 64);
        let mut produced = Vec::with_capacity(pending.len());
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
        }
        let tx = self.connection.transaction()?;
        for (chunk_id, vector) in &produced {
            tx.execute(
                "INSERT INTO chunk_embeddings(chunk_id, generation_id, vector, dimensions, normalized, state, embedded_at)
                 VALUES(?1, ?2, ?3, 384, 1, 'ready', ?4)
                 ON CONFLICT(chunk_id, generation_id) DO UPDATE SET vector=excluded.vector, dimensions=384, normalized=1, state='ready', embedded_at=excluded.embedded_at",
                params![chunk_id, generation.generation_id, vector, unix_seconds()],
            )?;
        }
        tx.commit()?;
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
        let content = node.content.as_ref().map(serialize_content);
        let now = unix_seconds();
        let tx = self.connection.transaction()?;
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
        tx.commit()?;
        Ok(())
    }

    /// Marks a summary node Failed with a sanitized category. Failing a new
    /// synthesis never replaces an existing Ready summary's content.
    pub fn mark_summary_failed(&mut self, summary_id: &str, failure: SummaryFailure) -> Result<()> {
        let now = unix_seconds();
        self.connection.execute(
            "INSERT INTO summaries(summary_id, level, state, failure_category, content_json, input_fingerprint, output_fingerprint, contract_version, generation_id, created_at, updated_at)
             VALUES(?1, 'document', 'failed', ?2, NULL, '', '', ?3, '', ?4, ?4)
             ON CONFLICT(summary_id) DO UPDATE SET state='failed', failure_category=excluded.failure_category, updated_at=excluded.updated_at",
            params![summary_id, failure.as_db(), SUMMARY_CONTRACT_VERSION, now],
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
    tx.execute("INSERT INTO schema_meta(key, value) VALUES ('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [SCHEMA_VERSION.to_string()])?;
    tx.commit()?;
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
        assert_eq!(upgraded.schema_version().unwrap(), 4);
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
        assert_eq!(upgraded.schema_version().unwrap(), 4);
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
}
