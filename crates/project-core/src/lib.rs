//! Pure application core for the local-first project model.
//!
//! This crate deliberately contains no filesystem, network, Tauri, or UI code.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};

pub const PROJECT_SCHEMA_VERSION: u32 = 3;
pub const SCHEMA_V2: u32 = 2;
pub const LEGACY_PROJECT_SCHEMA_VERSION: u32 = 1;
pub const MAX_PROJECT_NAME_CHARS: usize = 120;
pub const MAX_FILE_NAME_CHARS: usize = 180;
pub const MAX_PUBLICATION_ROUTE_CHARS: usize = 80;
pub const MAX_MESSAGE_TEXT_CHARS: usize = 40_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectCoreError {
    InvalidId { kind: &'static str, value: String },
    InvalidName(String),
    InvalidPath(String),
    InvalidTimestamp(String),
    InvalidContentType(String),
    InvalidDigest(String),
    InvalidCreation(String),
    InvalidPublicationRoute(String),
    DuplicateMaterial(MaterialId),
    DuplicateCreation(CreationId),
    DuplicateMessage(MessageId),
    MissingMaterial(MaterialId),
    MissingCreation(CreationId),
    InvalidMessage(String),
    NotFound(ProjectId),
    AlreadyExists(ProjectId),
    Conflict { project_id: ProjectId },
    CorruptMetadata(String),
    UnsupportedSchema(u32),
    StorageUnavailable,
    AtomicWriteFailed,
    SourceUnreadable,
    PathEscape,
    SymlinkRejected,
    WriteFailed,
    IntegrityMismatch,
    OperationFailed { operation: &'static str },
}
impl fmt::Display for ProjectCoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ProjectCoreError::*;
        match self {
            InvalidId { kind, .. } => write!(f, "invalid {kind}"),
            InvalidName(_) => f.write_str("invalid project name"),
            InvalidPath(_) => f.write_str("invalid project-relative path"),
            InvalidTimestamp(_) => f.write_str("invalid timestamp"),
            InvalidContentType(_) => f.write_str("invalid content type"),
            InvalidDigest(_) => f.write_str("invalid SHA-256 digest"),
            InvalidCreation(_) => f.write_str("invalid creation"),
            InvalidPublicationRoute(_) => f.write_str("invalid publication route"),
            DuplicateMaterial(_) => f.write_str("duplicate material"),
            DuplicateCreation(_) => f.write_str("duplicate creation"),
            DuplicateMessage(_) => f.write_str("duplicate message"),
            MissingMaterial(_) => f.write_str("material not found"),
            MissingCreation(_) => f.write_str("creation not found"),
            InvalidMessage(_) => f.write_str("invalid message"),
            NotFound(_) => f.write_str("project not found"),
            AlreadyExists(_) => f.write_str("project already exists"),
            Conflict { .. } => f.write_str("project was changed elsewhere"),
            CorruptMetadata(_) => f.write_str("corrupt project metadata"),
            UnsupportedSchema(_) => f.write_str("unsupported project schema"),
            StorageUnavailable => f.write_str("project storage is unavailable"),
            AtomicWriteFailed => f.write_str("could not safely save project metadata"),
            SourceUnreadable => f.write_str("material source could not be read"),
            PathEscape => f.write_str("project path escapes its fixed root"),
            SymlinkRejected => f.write_str("symbolic links are not allowed"),
            WriteFailed => f.write_str("project content could not be saved"),
            IntegrityMismatch => f.write_str("stored project content failed verification"),
            OperationFailed { .. } => f.write_str("project operation failed"),
        }
    }
}
impl std::error::Error for ProjectCoreError {}
pub type CoreResult<T> = Result<T, ProjectCoreError>;

macro_rules! uuid_id {
    ($name:ident, $kind:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: impl Into<String>) -> CoreResult<Self> {
                let value = value.into();
                if is_uuid_v7(&value) {
                    Ok(Self(value))
                } else {
                    Err(ProjectCoreError::InvalidId { kind: $kind, value })
                }
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}
uuid_id!(ProjectId, "project ID");
uuid_id!(MaterialId, "material ID");
uuid_id!(CreationId, "creation ID");
uuid_id!(MessageId, "message ID");
fn is_uuid_v7(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() == 36
        && [8, 13, 18, 23].iter().all(|&i| b[i] == b'-')
        && b[14] == b'7'
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
        && b.iter().enumerate().all(|(i, c)| {
            [8, 13, 18, 23].contains(&i) || c.is_ascii_hexdigit() && !c.is_ascii_uppercase()
        })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(String);
impl Timestamp {
    pub fn parse(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if is_rfc3339_utc_second(&value) {
            Ok(Self(value))
        } else {
            Err(ProjectCoreError::InvalidTimestamp(value))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
fn is_rfc3339_utc_second(v: &str) -> bool {
    let b = v.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
        || ![0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
    {
        return false;
    }
    let n = |a, b| v.get(a..b).and_then(|s| s.parse::<u32>().ok());
    let (Some(y), Some(m), Some(d), Some(h), Some(mi), Some(s)) =
        (n(0, 4), n(5, 7), n(8, 10), n(11, 13), n(14, 16), n(17, 19))
    else {
        return false;
    };
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if y % 400 == 0 || y % 4 == 0 && y % 100 != 0 => 29,
        2 => 28,
        _ => return false,
    };
    d >= 1 && d <= dim && h < 24 && mi < 60 && s < 60
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectName(String);
impl ProjectName {
    pub fn parse(value: impl AsRef<str>) -> CoreResult<Self> {
        let t = value.as_ref().trim();
        if t.is_empty()
            || t.chars().count() > MAX_PROJECT_NAME_CHARS
            || t.contains(['/', '\\', '\0'])
        {
            Err(ProjectCoreError::InvalidName(value.as_ref().into()))
        } else {
            Ok(Self(t.into()))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelativeProjectPath(String);
impl RelativeProjectPath {
    pub fn parse(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.starts_with('/')
            || value.contains('\\')
            || value
                .split('/')
                .any(|s| s.is_empty() || matches!(s, "." | ".."))
        {
            Err(ProjectCoreError::InvalidPath(value))
        } else {
            Ok(Self(value))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn starts_with_root(&self, root: &str) -> bool {
        self.0
            .strip_prefix(root)
            .is_some_and(|r| r.starts_with('/'))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublicationRoute(String);
impl PublicationRoute {
    pub fn parse(value: impl AsRef<str>) -> CoreResult<Self> {
        let s = value.as_ref();
        if is_valid_publication_route(s) {
            Ok(Self(s.to_owned()))
        } else {
            Err(ProjectCoreError::InvalidPublicationRoute(s.into()))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for PublicationRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
fn is_valid_publication_route(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_PUBLICATION_ROUTE_CHARS {
        return false;
    }
    let bytes = s.as_bytes();
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return false;
    }
    if !bytes[bytes.len() - 1].is_ascii_lowercase() && !bytes[bytes.len() - 1].is_ascii_digit() {
        return false;
    }
    let mut prev_hyphen = false;
    for &b in bytes {
        if b.is_ascii_lowercase() || b.is_ascii_digit() {
            prev_hyphen = false;
        } else if b == b'-' {
            if prev_hyphen {
                return false;
            }
            prev_hyphen = true;
        } else {
            return false;
        }
    }
    true
}

/// Validate a workspace-relative bundle root directory. Empty (`""`) is the
/// workspace root; otherwise a forward-slash list of safe, non-hidden, non-dot
/// segments with no trailing slash, traversal, control bytes, or backslashes.
fn is_valid_bundle_root(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if s.starts_with('/')
        || s.ends_with('/')
        || s.contains('\\')
        || s.bytes().any(|b| b == 0 || b.is_ascii_control())
    {
        return false;
    }
    s.split('/')
        .all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.starts_with('.'))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentType(String);
impl ContentType {
    pub fn parse(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 255
            || value
                .bytes()
                .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
            || !value.contains('/')
        {
            Err(ProjectCoreError::InvalidContentType(value))
        } else {
            Ok(Self(value))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);
impl Sha256Digest {
    pub fn parse(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            Ok(Self(value))
        } else {
            Err(ProjectCoreError::InvalidDigest(value))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectState {
    Local,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CreationKind {
    Web,
    Document,
    Image,
    File,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CreationVisibility {
    Public,
    #[default]
    Private,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Ok,
    Failed,
    Cancelled,
}
/// Durable turn metrics attached to a persisted message.
///
/// Contains provider-actual telemetry and structural Knowledge estimates.
/// All fields except conversation_id, material_count, corpus_bytes,
/// corpus_utf8_chars, corpus_est_tokens, and semantic_provider_state are
/// optional: absent provider data stays null (rendered "No disponible").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnMetrics {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub turn_duration_ms: Option<u64>,
    pub source: Option<String>,
    pub remote_calls: Option<usize>,
    /// Structural Knowledge architecture metrics.
    pub material_count: Option<usize>,
    pub corpus_bytes: Option<u64>,
    pub corpus_utf8_chars: Option<usize>,
    pub corpus_est_tokens: Option<usize>,
    pub retrieval_candidate_count: Option<usize>,
    pub selected_evidence_count: Option<usize>,
    pub selected_evidence_bytes: Option<usize>,
    pub selected_evidence_utf8_chars: Option<usize>,
    pub evidence_est_tokens: Option<usize>,
    pub context_reduction_pct: Option<usize>,
    pub semantic_provider_state: Option<String>,
    pub request_preparation_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eligible_materials: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materials_inspected: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunks_inspected: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exhaustive_coverage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lexical_hits: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_hits: Option<usize>,
    /// Truthful local-mode label for turns that never used a RAG retrieval
    /// mode (e.g. `inventory`). `None` for semantic/exhaustive/thematic turns.
    /// Kept `skip_serializing_if` so older persisted records stay unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_mode: Option<String>,
    /// True when this turn resolved a contextual referent from a previous turn.
    /// Structural telemetry only; never content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contextual_followup: Option<bool>,
    /// `material_set` or `theme_set` for a resolved follow-up turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referent_type: Option<String>,
    /// Exact cardinality of the resolved referent set (no top-K shrink).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referent_count: Option<usize>,
    /// Durable id of the turn that produced the resolved referent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_turn_id: Option<String>,
    /// The retrieval intent this follow-up's content behavior is built on:
    /// `normal`, `exhaustive`, or `thematic`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_intent: Option<String>,
    /// The concrete follow-up action: `per_item_summary`, `per_theme_detail`,
    /// or `scoped_exhaustive`. Never a retrieval mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_kind: Option<String>,
    /// Exact grounded source display names that actually contributed to this
    /// turn's answer (sanitized filenames only, never filesystem paths). This
    /// replaces the formerly text-appended `Fuentes:` block and is presentation
    /// provenance: a non-empty list is never inferred from prose and an empty
    /// list means no grounded source contributed. Kept `skip_serializing_if`
    /// so older persisted records stay byte-stable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_names: Vec<String>,
}

impl TurnMetrics {
    /// Provider telemetry is untrusted input. JSON cannot faithfully encode
    /// non-finite values, so reject them at the durable ingestion boundary
    /// rather than normalising them into a legitimate cost or silently writing
    /// a null value.
    pub fn validate(&self) -> CoreResult<()> {
        if self.cost_usd.is_some_and(|cost| !cost.is_finite()) {
            return Err(ProjectCoreError::InvalidMessage(
                "turn metrics contain a non-finite provider cost".into(),
            ));
        }
        Ok(())
    }
}

/// A durable, structured referent produced by one turn and reused by a later
/// contextual follow-up ("resumí cada uno", "de esos archivos, cuáles
/// mencionan...", "para cada tema..."). This is NOT rendered assistant prose:
/// it preserves only stable identities and display provenance, so a later turn
/// can resolve the exact previous set without re-running discovery, re-embedding,
/// or parsing prose. It never stores raw document content, vectors, or paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TurnReferent {
    /// An explicit set of Knowledge materials (e.g. a full inventory list).
    #[serde(rename = "materialSet")]
    MaterialSet(MaterialSetReferent),
    /// An explicit set of recurring themes (e.g. a CorpusThematic result).
    #[serde(rename = "themeSet")]
    ThemeSet(ThemeSetReferent),
}

/// Exact persisted material set produced by a previous turn. `material_ids` is
/// the opaque stable identity; `source_names` is the display provenance; both
/// keep the same relative order the originating answer used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialSetReferent {
    pub material_ids: Vec<String>,
    pub source_names: Vec<String>,
    #[serde(rename = "originTurnId")]
    pub origin_turn_id: String,
    /// What produced the set, e.g. `inventory`. Structural label, never prose.
    #[serde(rename = "producedBy")]
    pub produced_by: String,
}

/// Exact recurring-theme set produced by a previous turn. `theme_keys` is the
/// stable identity a later turn must reuse; `display_labels` is the same text
/// for rendering; `source_names` records the sources that evidenced the themes
/// (display provenance, never paths). A follow-up must never silently re-run
/// theme discovery and replace these keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSetReferent {
    pub theme_keys: Vec<String>,
    pub display_labels: Vec<String>,
    #[serde(rename = "originTurnId")]
    pub origin_turn_id: String,
    pub source_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Message {
    #[serde(rename = "messageId")]
    pub id: MessageId,
    pub role: MessageRole,
    pub text: String,
    pub status: MessageStatus,
    #[serde(rename = "createdAt")]
    pub created_at: Timestamp,
    #[serde(rename = "materialIds")]
    pub material_ids: Vec<MaterialId>,
    #[serde(rename = "creationIds")]
    pub creation_ids: Vec<CreationId>,
    #[serde(rename = "turnMetrics", skip_serializing_if = "Option::is_none")]
    pub turn_metrics: Option<TurnMetrics>,
    /// Optional structured referent a later turn may resolve. Absent in older
    /// projects; `None` keeps legacy project.json files byte-stable.
    #[serde(
        rename = "turnReferent",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub turn_referent: Option<TurnReferent>,
    /// Durable id of the owning user turn. On an assistant message this equals
    /// the id of the user message whose `turn_metrics` describe the response
    /// (the user message id is the turn id). This is the identity a render uses
    /// to bind an assistant answer to its exact per-turn metrics without
    /// positional guessing across fail→recover sequences. `None` on user
    /// messages and on legacy records written before this field existed.
    #[serde(rename = "turnId", default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<MessageId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Material {
    #[serde(rename = "materialId")]
    pub id: MaterialId,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "originalFileName")]
    pub original_file_name: String,
    #[serde(rename = "relativePath")]
    pub relative_path: RelativeProjectPath,
    #[serde(rename = "contentType", skip_serializing_if = "Option::is_none")]
    pub content_type: Option<ContentType>,
    #[serde(rename = "byteSize")]
    pub byte_size: u64,
    pub sha256: Sha256Digest,
    #[serde(rename = "createdAt")]
    pub created_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Creation {
    #[serde(rename = "creationId")]
    pub id: CreationId,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub kind: CreationKind,
    pub visibility: CreationVisibility,
    #[serde(rename = "relativePath")]
    pub relative_path: RelativeProjectPath,
    #[serde(rename = "contentType", skip_serializing_if = "Option::is_none")]
    pub content_type: Option<ContentType>,
    #[serde(rename = "byteSize")]
    pub byte_size: u64,
    pub revision: u32,
    #[serde(rename = "parentCreationId", skip_serializing_if = "Option::is_none")]
    pub parent_creation_id: Option<CreationId>,
    #[serde(rename = "createdAt")]
    pub created_at: Timestamp,
    /// Stable lineage identity shared by every version of one logical
    /// interactive resource. Two independent resources that happen to share a
    /// display name never collide because this is a real identity. `None` on
    /// legacy single-version records, where the creation id itself is the
    /// lineage (see [`Creation::lineage`]).
    #[serde(rename = "lineageId", default, skip_serializing_if = "Option::is_none")]
    pub lineage_id: Option<CreationId>,
    /// 1-based, monotonically increasing version number inside the lineage.
    /// `None` on legacy records, read as version 1.
    #[serde(
        rename = "versionNumber",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub version_number: Option<u32>,
    /// True for the authoritative current version of the lineage. `None` on
    /// legacy records, read as current. Validation guarantees exactly one
    /// current version per lineage.
    #[serde(rename = "isCurrent", default, skip_serializing_if = "Option::is_none")]
    pub is_current: Option<bool>,
    /// Workspace-relative forward-slash directory of the web bundle this
    /// creation belongs to (`""` for a bundle at the workspace root). This is
    /// the stable ownership key the registrar uses to attach CSS/JS/image
    /// sidecars to the correct lineage, independent of display name. `None` on
    /// legacy records, where ownership falls back to display-name matching.
    #[serde(
        rename = "bundleRoot",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub bundle_root: Option<String>,
}

impl Creation {
    /// Stable lineage identity: the explicit `lineage_id` when present, else the
    /// creation's own id (legacy single-version lineage).
    pub fn lineage(&self) -> &CreationId {
        self.lineage_id.as_ref().unwrap_or(&self.id)
    }

    /// 1-based version number, defaulting to 1 for legacy records.
    pub fn version(&self) -> u32 {
        self.version_number.unwrap_or(1)
    }

    /// Whether this is the current version of its lineage, defaulting to true
    /// for legacy records.
    pub fn current(&self) -> bool {
        self.is_current.unwrap_or(true)
    }
}
/// The optional model explicitly chosen for one conversation. It deliberately
/// stores only provider/model identifiers, never credentials or provider data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationModel {
    pub provider_id: String,
    pub model_id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Project {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "projectId")]
    pub id: ProjectId,
    pub name: ProjectName,
    #[serde(rename = "createdAt")]
    pub created_at: Timestamp,
    #[serde(rename = "updatedAt")]
    pub updated_at: Timestamp,
    pub state: ProjectState,
    #[serde(rename = "publicationRoute", skip_serializing_if = "Option::is_none")]
    pub publication_route: Option<PublicationRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ConversationModel>,
    pub materials: Vec<Material>,
    pub creations: Vec<Creation>,
    #[serde(default)]
    pub messages: Vec<Message>,
}
impl Project {
    pub fn new(id: ProjectId, name: ProjectName, now: Timestamp) -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            id,
            name,
            created_at: now.clone(),
            updated_at: now,
            state: ProjectState::Local,
            publication_route: None,
            model: None,
            materials: vec![],
            creations: vec![],
            messages: vec![],
        }
    }
    pub fn from_json(s: &str) -> CoreResult<Self> {
        let value: serde_json::Value = serde_json::from_str(s)
            .map_err(|e| ProjectCoreError::CorruptMetadata(e.to_string()))?;
        let version = schema_version_of(&value)?;
        let project = match version {
            LEGACY_PROJECT_SCHEMA_VERSION => {
                let v1: SchemaV1Project = serde_json::from_value(value)
                    .map_err(|e| ProjectCoreError::CorruptMetadata(e.to_string()))?;
                v1.into_project()
            }
            SCHEMA_V2 => serde_json::from_value(value)
                .map_err(|e| ProjectCoreError::CorruptMetadata(e.to_string()))?,
            PROJECT_SCHEMA_VERSION => serde_json::from_value(value)
                .map_err(|e| ProjectCoreError::CorruptMetadata(e.to_string()))?,
            other => return Err(ProjectCoreError::UnsupportedSchema(other)),
        };
        project.validate()?;
        Ok(project)
    }
    pub fn migrate_to_v3(&mut self) -> CoreResult<()> {
        match self.schema_version {
            PROJECT_SCHEMA_VERSION => Ok(()),
            SCHEMA_V2 => {
                self.schema_version = PROJECT_SCHEMA_VERSION;
                Ok(())
            }
            LEGACY_PROJECT_SCHEMA_VERSION => {
                for creation in &mut self.creations {
                    creation.visibility = CreationVisibility::Private;
                }
                self.publication_route = None;
                self.schema_version = PROJECT_SCHEMA_VERSION;
                Ok(())
            }
            other => Err(ProjectCoreError::UnsupportedSchema(other)),
        }
    }
    pub fn validate_for_persist(&self) -> CoreResult<()> {
        if self.schema_version != PROJECT_SCHEMA_VERSION {
            return Err(ProjectCoreError::UnsupportedSchema(self.schema_version));
        }
        self.validate()
    }
    pub fn validate(&self) -> CoreResult<()> {
        match self.schema_version {
            LEGACY_PROJECT_SCHEMA_VERSION | SCHEMA_V2 | PROJECT_SCHEMA_VERSION => {}
            other => return Err(ProjectCoreError::UnsupportedSchema(other)),
        }
        if self.schema_version == LEGACY_PROJECT_SCHEMA_VERSION && self.publication_route.is_some()
        {
            return Err(ProjectCoreError::CorruptMetadata(
                "schema v1 cannot carry publicationRoute".into(),
            ));
        }
        if let Some(route) = &self.publication_route {
            PublicationRoute::parse(route.as_str())?;
        }
        let mut ms = HashSet::new();
        for m in &self.materials {
            if !ms.insert(m.id.clone()) {
                return Err(ProjectCoreError::DuplicateMaterial(m.id.clone()));
            }
            validate_file_metadata(
                &m.display_name,
                &m.original_file_name,
                &m.relative_path,
                "inputs",
                m.id.as_str(),
            )?
        }
        let mut cs = HashSet::new();
        for c in &self.creations {
            if !cs.insert(c.id.clone()) {
                return Err(ProjectCoreError::DuplicateCreation(c.id.clone()));
            }
            if c.display_name.trim().is_empty() || c.revision != 1 {
                return Err(ProjectCoreError::InvalidCreation(c.display_name.clone()));
            }
            if c.version_number.is_some_and(|v| v < 1) {
                return Err(ProjectCoreError::InvalidCreation(
                    "version number must be >= 1".into(),
                ));
            }
            if let Some(root) = &c.bundle_root
                && !is_valid_bundle_root(root)
            {
                return Err(ProjectCoreError::InvalidCreation(
                    "invalid bundle root".into(),
                ));
            }
            validate_file_metadata(
                &c.display_name,
                &c.display_name,
                &c.relative_path,
                "outputs",
                c.id.as_str(),
            )?
        }
        for c in &self.creations {
            if let Some(parent) = &c.parent_creation_id
                && (parent == &c.id || !cs.contains(parent))
            {
                return Err(ProjectCoreError::InvalidCreation(
                    "parent creation is absent".into(),
                ));
            }
        }
        validate_creation_lineages(&self.creations)?;
        let mut message_ids = HashSet::new();
        for msg in &self.messages {
            MessageId::parse(msg.id.as_str())?;
            Timestamp::parse(msg.created_at.as_str())?;
            if msg.text.chars().count() > MAX_MESSAGE_TEXT_CHARS {
                return Err(ProjectCoreError::InvalidMessage(format!(
                    "message text exceeds {MAX_MESSAGE_TEXT_CHARS} characters"
                )));
            }
            let material_set: HashSet<_> = msg.material_ids.iter().collect();
            if material_set.len() != msg.material_ids.len() {
                return Err(ProjectCoreError::InvalidMessage(
                    "duplicate material id in message".into(),
                ));
            }
            for mid in &msg.material_ids {
                if !ms.contains(mid) {
                    return Err(ProjectCoreError::MissingMaterial(mid.clone()));
                }
            }
            let creation_set: HashSet<_> = msg.creation_ids.iter().collect();
            if creation_set.len() != msg.creation_ids.len() {
                return Err(ProjectCoreError::InvalidMessage(
                    "duplicate creation id in message".into(),
                ));
            }
            for cid in &msg.creation_ids {
                if !cs.contains(cid) {
                    return Err(ProjectCoreError::MissingCreation(cid.clone()));
                }
            }
            match msg.role {
                MessageRole::User => {
                    if !msg.creation_ids.is_empty() {
                        return Err(ProjectCoreError::InvalidMessage(
                            "user message cannot reference creations".into(),
                        ));
                    }
                }
                MessageRole::Assistant => {
                    if !msg.material_ids.is_empty() {
                        return Err(ProjectCoreError::InvalidMessage(
                            "assistant message cannot reference materials".into(),
                        ));
                    }
                }
            }
            if !message_ids.insert(msg.id.clone()) {
                return Err(ProjectCoreError::DuplicateMessage(msg.id.clone()));
            }
        }
        Ok(())
    }
}
fn validate_file_metadata(
    display: &str,
    file: &str,
    path: &RelativeProjectPath,
    root: &str,
    id: &str,
) -> CoreResult<()> {
    if display.trim().is_empty()
        || file.trim().is_empty()
        || file.contains(['/', '\\', '\0'])
        || !path.starts_with_root(root)
    {
        return Err(ProjectCoreError::InvalidPath(path.as_str().into()));
    }
    if !path.as_str().starts_with(&format!("{root}/{id}/")) {
        return Err(ProjectCoreError::PathEscape);
    }
    Ok(())
}

/// Validate the versioned-lineage invariants of a creation set.
///
/// Legacy records (no explicit version fields) form one-version lineages read
/// as version 1 and current. For every lineage: version numbers are unique and
/// `>= 1`, exactly one version is current, a parent belongs to the same
/// lineage, and version numbers strictly increase from parent to child. The
/// strict increase also makes a parent cycle impossible.
fn validate_creation_lineages(creations: &[Creation]) -> CoreResult<()> {
    let by_id: HashMap<&CreationId, &Creation> = creations.iter().map(|c| (&c.id, c)).collect();
    let mut lineage_versions: HashMap<&CreationId, HashSet<u32>> = HashMap::new();
    let mut lineage_current: HashMap<&CreationId, usize> = HashMap::new();
    for c in creations {
        let lineage = c.lineage();
        if !lineage_versions
            .entry(lineage)
            .or_default()
            .insert(c.version())
        {
            return Err(ProjectCoreError::InvalidCreation(
                "duplicate version number inside lineage".into(),
            ));
        }
        let current = lineage_current.entry(lineage).or_insert(0);
        if c.current() {
            *current += 1;
        }
    }
    for count in lineage_current.values() {
        if *count != 1 {
            return Err(ProjectCoreError::InvalidCreation(
                "lineage must have exactly one current version".into(),
            ));
        }
    }
    for c in creations {
        let Some(parent) = &c.parent_creation_id else {
            continue;
        };
        let Some(parent_creation) = by_id.get(parent) else {
            return Err(ProjectCoreError::InvalidCreation(
                "parent creation is absent".into(),
            ));
        };
        // Fully legacy records (no explicit version fields) may carry the
        // pre-versioning `parent_creation_id` chain; only versioned records
        // are required to keep their parent inside the same lineage and
        // monotonic. This keeps legacy project.json files loading unchanged.
        if c.version_number.is_some() {
            if parent_creation.lineage() != c.lineage() {
                return Err(ProjectCoreError::InvalidCreation(
                    "parent version belongs to a different lineage".into(),
                ));
            }
            if parent_creation.version() >= c.version() {
                return Err(ProjectCoreError::InvalidCreation(
                    "version number must increase from parent".into(),
                ));
            }
        }
    }
    Ok(())
}

fn schema_version_of(value: &serde_json::Value) -> CoreResult<u32> {
    match value.get("schemaVersion") {
        Some(serde_json::Value::Number(n)) => n
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| ProjectCoreError::CorruptMetadata("invalid schemaVersion".into())),
        Some(_) => Err(ProjectCoreError::CorruptMetadata(
            "invalid schemaVersion".into(),
        )),
        None => Err(ProjectCoreError::CorruptMetadata(
            "missing schemaVersion".into(),
        )),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct SchemaV1Project {
    schema_version: u32,
    #[serde(rename = "projectId")]
    id: ProjectId,
    name: ProjectName,
    created_at: Timestamp,
    updated_at: Timestamp,
    state: ProjectState,
    materials: Vec<Material>,
    creations: Vec<SchemaV1Creation>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct SchemaV1Creation {
    #[serde(rename = "creationId")]
    id: CreationId,
    display_name: String,
    kind: CreationKind,
    relative_path: RelativeProjectPath,
    content_type: Option<ContentType>,
    byte_size: u64,
    revision: u32,
    parent_creation_id: Option<CreationId>,
    created_at: Timestamp,
}
impl SchemaV1Project {
    fn into_project(self) -> Project {
        Project {
            schema_version: self.schema_version,
            id: self.id,
            name: self.name,
            created_at: self.created_at,
            updated_at: self.updated_at,
            state: self.state,
            publication_route: None,
            model: None,
            materials: self.materials,
            creations: self
                .creations
                .into_iter()
                .map(|c| Creation {
                    id: c.id,
                    display_name: c.display_name,
                    kind: c.kind,
                    visibility: CreationVisibility::Private,
                    relative_path: c.relative_path,
                    content_type: c.content_type,
                    byte_size: c.byte_size,
                    revision: c.revision,
                    parent_creation_id: c.parent_creation_id,
                    created_at: c.created_at,
                    lineage_id: None,
                    version_number: None,
                    is_current: None,
                    bundle_root: None,
                })
                .collect(),
            messages: vec![],
        }
    }
}

pub trait Clock {
    fn now(&self) -> Timestamp;
}
pub trait IdGenerator {
    fn project_id(&self) -> ProjectId;
    fn material_id(&self) -> MaterialId;
    fn creation_id(&self) -> CreationId;
    fn message_id(&self) -> MessageId;
}
pub trait ProjectRepository {
    fn create(&mut self, project: &Project) -> CoreResult<()>;
    fn get(&self, id: &ProjectId) -> CoreResult<Project>;
    fn list(&self) -> CoreResult<Vec<Project>>;
    fn replace(&mut self, project: &Project, expected_updated_at: &Timestamp) -> CoreResult<()>;
    fn delete(&mut self, id: &ProjectId) -> CoreResult<()>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialContent {
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreationContent {
    pub bytes: Vec<u8>,
    pub file_name: String,
}
/// A descriptor returned only after an adapter has copied and SHA-256 hashed a material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredMaterial {
    pub relative_path: RelativeProjectPath,
    pub byte_size: u64,
    pub sha256: Sha256Digest,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredCreation {
    pub relative_path: RelativeProjectPath,
    pub byte_size: u64,
}
/// One changed/new file laid over an immutable version snapshot. `relative_path`
/// is forward-slash and relative to the version directory (`outputs/<id>/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreationOverlay {
    pub relative_path: String,
    pub bytes: Vec<u8>,
}
/// The complete content of a new immutable creation version: a primary entry
/// plus every changed/new overlay file. Unchanged files are inherited by
/// copying the base version, never sent as overlays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreationVersionContent {
    /// Primary entry relative path, e.g. `index.html` or `notes.pdf`.
    pub primary_relative_path: String,
    pub overlays: Vec<CreationOverlay>,
}
pub trait ProjectContentStore {
    fn store_material(
        &mut self,
        p: &ProjectId,
        m: &MaterialId,
        source: &MaterialContent,
        safe_file_name: &str,
    ) -> CoreResult<StoredMaterial>;
    fn read_material(&self, p: &ProjectId, m: &Material) -> CoreResult<Vec<u8>>;
    fn store_creation(
        &mut self,
        p: &ProjectId,
        c: &CreationId,
        content: &CreationContent,
        safe_file_name: &str,
    ) -> CoreResult<StoredCreation>;
    /// Build a new immutable, self-contained version directory
    /// `outputs/<c>/`: recursively copy `base`'s complete tree (when given),
    /// overlay the changed/new files, validate the result, then atomically
    /// promote it. The previous version directory is never mutated.
    fn store_creation_version(
        &mut self,
        p: &ProjectId,
        c: &CreationId,
        base: Option<&Creation>,
        content: &CreationVersionContent,
    ) -> CoreResult<StoredCreation>;
    /// Remove stale `.staging-*` directories left by a crashed version build.
    /// A staging directory is never referenced by metadata, so removing it is
    /// always safe; an already-promoted orphan version directory is left alone
    /// (immutable and never current).
    fn recover_stale_creation_staging(&mut self, p: &ProjectId) -> CoreResult<()>;
    fn read_creation(&self, p: &ProjectId, c: &Creation) -> CoreResult<Vec<u8>>;
    /// Removes the fixed `inputs/<id>` content directory for a material. The
    /// original source file is never affected: only the app-managed copy is
    /// deleted. Implementations must enforce fixed-root containment and reject
    /// symlinks before removing anything.
    fn remove_material(&mut self, p: &ProjectId, m: &MaterialId) -> CoreResult<()>;
    fn remove_project_tree(&mut self, p: &ProjectId) -> CoreResult<()>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddMaterial {
    pub display_name: String,
    pub original_file_name: String,
    pub content_type: Option<ContentType>,
    pub source: MaterialContent,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateCreation {
    pub display_name: String,
    pub kind: CreationKind,
    pub visibility: CreationVisibility,
    pub content_type: Option<ContentType>,
    pub content: CreationContent,
    pub parent_creation_id: Option<CreationId>,
}
/// Request to build a new immutable version of a creation lineage. When
/// `base_creation_id` is set, the new version inherits that version's complete
/// tree and its lineage; otherwise a new lineage is started (version 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateCreationVersion {
    pub display_name: String,
    pub kind: CreationKind,
    pub visibility: CreationVisibility,
    pub content_type: Option<ContentType>,
    pub content: CreationVersionContent,
    pub base_creation_id: Option<CreationId>,
    /// Workspace-relative forward-slash directory of the web bundle. Only used
    /// when starting a new lineage; an existing lineage inherits its base's
    /// bundle root. `None` is fine for standalone file lineages.
    pub bundle_root: Option<String>,
}
pub struct ProjectService<R, S, C, I> {
    repository: R,
    content: S,
    clock: C,
    ids: I,
}
impl<R, S, C, I> ProjectService<R, S, C, I>
where
    R: ProjectRepository,
    S: ProjectContentStore,
    C: Clock,
    I: IdGenerator,
{
    pub fn new(repository: R, content: S, clock: C, ids: I) -> Self {
        Self {
            repository,
            content,
            clock,
            ids,
        }
    }
    #[cfg(test)]
    fn into_parts(self) -> (R, S, C, I) {
        (self.repository, self.content, self.clock, self.ids)
    }
    pub fn create_project(&mut self, name: impl AsRef<str>) -> CoreResult<Project> {
        let p = Project::new(
            self.ids.project_id(),
            ProjectName::parse(name)?,
            self.clock.now(),
        );
        self.repository.create(&p)?;
        Ok(p)
    }
    pub fn open_project(&self, id: &ProjectId) -> CoreResult<Project> {
        self.repository.get(id)
    }
    pub fn list_projects(&self) -> CoreResult<Vec<Project>> {
        self.repository.list()
    }
    pub fn rename_project(&mut self, id: &ProjectId, name: impl AsRef<str>) -> CoreResult<Project> {
        let mut p = self.repository.get(id)?;
        let e = p.updated_at.clone();
        p.migrate_to_v3()?;
        p.name = ProjectName::parse(name)?;
        p.updated_at = self.clock.now();
        self.repository.replace(&p, &e)?;
        Ok(p)
    }
    pub fn set_project_model(
        &mut self,
        id: &ProjectId,
        model: Option<ConversationModel>,
    ) -> CoreResult<Project> {
        let mut p = self.repository.get(id)?;
        let e = p.updated_at.clone();
        p.migrate_to_v3()?;
        p.model = model;
        p.updated_at = self.clock.now();
        self.repository.replace(&p, &e)?;
        Ok(p)
    }
    pub fn delete_project(&mut self, id: &ProjectId) -> CoreResult<()> {
        self.repository.get(id)?;
        self.repository.delete(id)?;
        self.content.remove_project_tree(id)
    }
    pub fn add_material(&mut self, pid: &ProjectId, r: AddMaterial) -> CoreResult<Material> {
        if r.display_name.trim().is_empty() || r.original_file_name.trim().is_empty() {
            return Err(ProjectCoreError::InvalidName(r.display_name));
        }
        let mut p = self.repository.get(pid)?;
        let e = p.updated_at.clone();
        p.migrate_to_v3()?;
        let id = self.ids.material_id();
        if p.materials.iter().any(|m| m.id == id) {
            return Err(ProjectCoreError::DuplicateMaterial(id));
        }
        let s = safe_file_name(&r.original_file_name);
        let stored = self.content.store_material(pid, &id, &r.source, &s)?;
        ensure_stored_path(&stored.relative_path, "inputs", id.as_str())?;
        let m = Material {
            id,
            display_name: r.display_name,
            original_file_name: r.original_file_name,
            relative_path: stored.relative_path,
            content_type: r.content_type,
            byte_size: stored.byte_size,
            sha256: stored.sha256,
            created_at: self.clock.now(),
        };
        p.materials.push(m.clone());
        p.updated_at = m.created_at.clone();
        self.repository.replace(&p, &e)?;
        Ok(m)
    }
    pub fn read_material(&self, pid: &ProjectId, id: &MaterialId) -> CoreResult<Vec<u8>> {
        let p = self.repository.get(pid)?;
        let m = p
            .materials
            .iter()
            .find(|m| &m.id == id)
            .ok_or_else(|| ProjectCoreError::MissingMaterial(id.clone()))?;
        self.content.read_material(pid, m)
    }
    /// Removes a material from the project metadata and deletes its app-managed
    /// `inputs/<id>` content directory. The user's original source file is never
    /// touched. Ordering mirrors `delete_project`: the metadata reference is
    /// removed under optimistic concurrency first, then the content tree, so a
    /// failed content removal can only leave a benign orphan directory and never
    /// a dangling metadata reference.
    pub fn remove_material(&mut self, pid: &ProjectId, mid: &MaterialId) -> CoreResult<()> {
        let mut p = self.repository.get(pid)?;
        let e = p.updated_at.clone();
        p.migrate_to_v3()?;
        let idx = p
            .materials
            .iter()
            .position(|m| &m.id == mid)
            .ok_or_else(|| ProjectCoreError::MissingMaterial(mid.clone()))?;
        p.materials.remove(idx);
        for msg in &mut p.messages {
            msg.material_ids.retain(|m| m != mid);
        }
        p.updated_at = self.clock.now();
        self.repository.replace(&p, &e)?;
        self.content.remove_material(pid, mid)?;
        Ok(())
    }
    pub fn create_creation(&mut self, pid: &ProjectId, r: CreateCreation) -> CoreResult<Creation> {
        if r.display_name.trim().is_empty() || r.content.file_name.trim().is_empty() {
            return Err(ProjectCoreError::InvalidCreation(r.display_name));
        }
        let mut p = self.repository.get(pid)?;
        let e = p.updated_at.clone();
        p.migrate_to_v3()?;
        if let Some(parent) = &r.parent_creation_id
            && !p.creations.iter().any(|c| &c.id == parent)
        {
            return Err(ProjectCoreError::InvalidCreation(
                "parent creation is absent".into(),
            ));
        }
        let id = self.ids.creation_id();
        if p.creations.iter().any(|c| c.id == id) {
            return Err(ProjectCoreError::DuplicateCreation(id));
        }
        let s = safe_file_name(&r.content.file_name);
        let stored = self.content.store_creation(pid, &id, &r.content, &s)?;
        ensure_stored_path(&stored.relative_path, "outputs", id.as_str())?;
        let c = Creation {
            id,
            display_name: r.display_name,
            kind: r.kind,
            visibility: r.visibility,
            relative_path: stored.relative_path,
            content_type: r.content_type,
            byte_size: stored.byte_size,
            revision: 1,
            parent_creation_id: r.parent_creation_id,
            created_at: self.clock.now(),
            lineage_id: None,
            version_number: None,
            is_current: None,
            bundle_root: None,
        };
        p.creations.push(c.clone());
        p.updated_at = c.created_at.clone();
        self.repository.replace(&p, &e)?;
        Ok(c)
    }

    /// Builds a new immutable version of a creation lineage.
    ///
    /// The snapshot is fully written and atomically promoted to
    /// `outputs/<new-id>/` BEFORE the metadata replace: a crash before
    /// promotion leaves only a recoverable `.staging-*` directory and never a
    /// current version, and a crash after promotion but before the metadata
    /// commit can leave an orphan immutable directory that is never referenced
    /// (and therefore never current). The previous version is never mutated;
    /// only its `is_current` flag moves to the new version in the same metadata
    /// write.
    pub fn create_creation_version(
        &mut self,
        pid: &ProjectId,
        r: CreateCreationVersion,
    ) -> CoreResult<Creation> {
        if r.display_name.trim().is_empty() || r.content.primary_relative_path.trim().is_empty() {
            return Err(ProjectCoreError::InvalidCreation(r.display_name));
        }
        let mut p = self.repository.get(pid)?;
        let e = p.updated_at.clone();
        p.migrate_to_v3()?;
        self.content.recover_stale_creation_staging(pid)?;
        let base = match &r.base_creation_id {
            Some(base_id) => Some(
                p.creations
                    .iter()
                    .find(|c| &c.id == base_id)
                    .cloned()
                    .ok_or_else(|| ProjectCoreError::MissingCreation(base_id.clone()))?,
            ),
            None => None,
        };
        let id = self.ids.creation_id();
        let lineage = match &base {
            Some(base) => base.lineage().clone(),
            None => id.clone(),
        };
        if p.creations.iter().any(|c| c.id == id) {
            return Err(ProjectCoreError::DuplicateCreation(id));
        }
        let version_number = match &base {
            Some(base) => {
                let max = p
                    .creations
                    .iter()
                    .filter(|c| c.lineage() == base.lineage())
                    .map(|c| c.version())
                    .max()
                    .unwrap_or(0);
                max + 1
            }
            None => 1,
        };
        let stored = self
            .content
            .store_creation_version(pid, &id, base.as_ref(), &r.content)?;
        ensure_stored_path(&stored.relative_path, "outputs", id.as_str())?;
        let parent_creation_id = base.as_ref().map(|b| b.id.clone());
        // Bundle root is inherited from the base version; only a brand-new
        // lineage adopts the caller-supplied root.
        let bundle_root = match &base {
            Some(base) => base.bundle_root.clone(),
            None => r.bundle_root.clone(),
        };
        let now = self.clock.now();
        let c = Creation {
            id,
            display_name: r.display_name,
            kind: r.kind,
            visibility: r.visibility,
            relative_path: stored.relative_path,
            content_type: r.content_type,
            byte_size: stored.byte_size,
            revision: 1,
            parent_creation_id,
            created_at: now.clone(),
            lineage_id: Some(lineage),
            version_number: Some(version_number),
            is_current: Some(true),
            bundle_root,
        };
        for existing in &mut p.creations {
            if existing.lineage() == c.lineage() && existing.current() {
                existing.is_current = Some(false);
            }
        }
        p.creations.push(c.clone());
        p.updated_at = now;
        self.repository.replace(&p, &e)?;
        Ok(c)
    }
    pub fn replace_creation_content(
        &mut self,
        pid: &ProjectId,
        id: &CreationId,
        content: CreationContent,
    ) -> CoreResult<Creation> {
        if content.file_name.trim().is_empty() {
            return Err(ProjectCoreError::InvalidCreation(id.as_str().to_owned()));
        }
        let mut p = self.repository.get(pid)?;
        let e = p.updated_at.clone();
        p.migrate_to_v3()?;
        let idx = p
            .creations
            .iter()
            .position(|c| &c.id == id)
            .ok_or_else(|| ProjectCoreError::MissingCreation(id.clone()))?;
        let s = safe_file_name(&content.file_name);
        let stored = self.content.store_creation(pid, id, &content, &s)?;
        ensure_stored_path(&stored.relative_path, "outputs", id.as_str())?;
        p.creations[idx].relative_path = stored.relative_path;
        p.creations[idx].byte_size = stored.byte_size;
        p.updated_at = self.clock.now();
        let updated = p.creations[idx].clone();
        self.repository.replace(&p, &e)?;
        Ok(updated)
    }
    pub fn list_creations(&self, pid: &ProjectId) -> CoreResult<Vec<Creation>> {
        Ok(self.repository.get(pid)?.creations)
    }
    pub fn read_creation(&self, pid: &ProjectId, id: &CreationId) -> CoreResult<Vec<u8>> {
        let p = self.repository.get(pid)?;
        let c = p
            .creations
            .iter()
            .find(|c| &c.id == id)
            .ok_or_else(|| ProjectCoreError::MissingCreation(id.clone()))?;
        self.content.read_creation(pid, c)
    }
    pub fn set_creation_visibility(
        &mut self,
        pid: &ProjectId,
        id: &CreationId,
        visibility: CreationVisibility,
    ) -> CoreResult<Creation> {
        let mut p = self.repository.get(pid)?;
        let e = p.updated_at.clone();
        p.migrate_to_v3()?;
        let idx = p
            .creations
            .iter()
            .position(|c| &c.id == id)
            .ok_or_else(|| ProjectCoreError::MissingCreation(id.clone()))?;
        p.creations[idx].visibility = visibility;
        p.updated_at = self.clock.now();
        self.repository.replace(&p, &e)?;
        Ok(p.creations[idx].clone())
    }
    pub fn append_user_message(
        &mut self,
        pid: &ProjectId,
        text: &str,
        material_ids: &[MaterialId],
    ) -> CoreResult<Message> {
        if text.chars().count() > MAX_MESSAGE_TEXT_CHARS {
            return Err(ProjectCoreError::InvalidMessage(format!(
                "message text exceeds {MAX_MESSAGE_TEXT_CHARS} characters"
            )));
        }
        let mut p = self.repository.get(pid)?;
        let e = p.updated_at.clone();
        p.migrate_to_v3()?;
        let material_set: HashSet<_> = material_ids.iter().collect();
        if material_set.len() != material_ids.len() {
            return Err(ProjectCoreError::InvalidMessage(
                "duplicate material id in message".into(),
            ));
        }
        let project_materials: HashSet<_> = p.materials.iter().map(|m| &m.id).collect();
        for mid in material_ids {
            if !project_materials.contains(mid) {
                return Err(ProjectCoreError::MissingMaterial(mid.clone()));
            }
        }
        let id = self.ids.message_id();
        let created_at = self.clock.now();
        let msg = Message {
            id,
            role: MessageRole::User,
            text: text.to_owned(),
            status: MessageStatus::Ok,
            created_at: created_at.clone(),
            material_ids: material_ids.to_vec(),
            creation_ids: vec![],
            turn_metrics: None,
            turn_referent: None,
            turn_id: None,
        };
        p.messages.push(msg.clone());
        p.updated_at = created_at;
        self.repository.replace(&p, &e)?;
        Ok(msg)
    }
    pub fn append_assistant_message(
        &mut self,
        pid: &ProjectId,
        text: &str,
        status: MessageStatus,
        creation_ids: &[CreationId],
        turn_id: Option<MessageId>,
    ) -> CoreResult<Message> {
        if text.chars().count() > MAX_MESSAGE_TEXT_CHARS {
            return Err(ProjectCoreError::InvalidMessage(format!(
                "message text exceeds {MAX_MESSAGE_TEXT_CHARS} characters"
            )));
        }
        let mut p = self.repository.get(pid)?;
        let e = p.updated_at.clone();
        p.migrate_to_v3()?;
        let creation_set: HashSet<_> = creation_ids.iter().collect();
        if creation_set.len() != creation_ids.len() {
            return Err(ProjectCoreError::InvalidMessage(
                "duplicate creation id in message".into(),
            ));
        }
        let project_creations: HashSet<_> = p.creations.iter().map(|c| &c.id).collect();
        for cid in creation_ids {
            if !project_creations.contains(cid) {
                return Err(ProjectCoreError::MissingCreation(cid.clone()));
            }
        }
        let id = self.ids.message_id();
        let created_at = self.clock.now();
        let msg = Message {
            id,
            role: MessageRole::Assistant,
            text: text.to_owned(),
            status,
            created_at: created_at.clone(),
            material_ids: vec![],
            creation_ids: creation_ids.to_vec(),
            turn_metrics: None,
            turn_referent: None,
            turn_id,
        };
        p.messages.push(msg.clone());
        p.updated_at = created_at;
        self.repository.replace(&p, &e)?;
        Ok(msg)
    }
    /// Stores final accounting on the already-persisted user message that owns
    /// a logical turn. This replaces exactly that message and never creates or
    /// reorders messages; repeating the same terminal write is safe.
    pub fn set_turn_metrics(
        &mut self,
        pid: &ProjectId,
        message_id: &MessageId,
        metrics: TurnMetrics,
    ) -> CoreResult<Message> {
        metrics.validate()?;
        let mut project = self.repository.get(pid)?;
        let expected_updated_at = project.updated_at.clone();
        project.migrate_to_v3()?;
        let index = project
            .messages
            .iter()
            .position(|message| &message.id == message_id)
            .ok_or_else(|| ProjectCoreError::InvalidMessage("turn message is absent".into()))?;
        if project.messages[index].role != MessageRole::User {
            return Err(ProjectCoreError::InvalidMessage(
                "turn metrics must belong to a user message".into(),
            ));
        }
        project.messages[index].turn_metrics = Some(metrics);
        project.updated_at = self.clock.now();
        let updated = project.messages[index].clone();
        self.repository.replace(&project, &expected_updated_at)?;
        Ok(updated)
    }
    /// Stores a structured referent on the already-persisted user message that
    /// owns a logical turn, so a later contextual follow-up can resolve the
    /// exact set produced by this turn even after restart. Repeating the same
    /// write is safe; a `None` value leaves the existing referent untouched.
    pub fn set_turn_referent(
        &mut self,
        pid: &ProjectId,
        message_id: &MessageId,
        referent: TurnReferent,
    ) -> CoreResult<Message> {
        let mut project = self.repository.get(pid)?;
        let expected_updated_at = project.updated_at.clone();
        project.migrate_to_v3()?;
        let index = project
            .messages
            .iter()
            .position(|message| &message.id == message_id)
            .ok_or_else(|| ProjectCoreError::InvalidMessage("turn message is absent".into()))?;
        if project.messages[index].role != MessageRole::User {
            return Err(ProjectCoreError::InvalidMessage(
                "turn referent must belong to a user message".into(),
            ));
        }
        project.messages[index].turn_referent = Some(referent);
        project.updated_at = self.clock.now();
        let updated = project.messages[index].clone();
        self.repository.replace(&project, &expected_updated_at)?;
        Ok(updated)
    }
    pub fn messages(&self, pid: &ProjectId) -> CoreResult<Vec<Message>> {
        Ok(self.repository.get(pid)?.messages)
    }
}
fn ensure_stored_path(path: &RelativeProjectPath, root: &str, id: &str) -> CoreResult<()> {
    validate_file_metadata("stored content", "stored-content", path, root, id)
}
pub fn safe_file_name(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().chars().take(MAX_FILE_NAME_CHARS) {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
            dash = false
        } else if !dash {
            out.push('-');
            dash = true
        }
    }
    let out = out.trim_matches(['.', '-']).to_owned();
    if out.is_empty() { "file".into() } else { out }
}

/// Wall-clock UTC timestamps at second resolution (`YYYY-MM-DDTHH:MM:SSZ`).
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Timestamp::parse(unix_secs_to_rfc3339_utc(secs))
            .expect("system time is a valid UTC timestamp")
    }
}

/// UUIDv7 identifiers (48-bit unix-ms timestamp, version 7, RFC variant, getrandom).
pub struct UuidV7IdGenerator;

impl IdGenerator for UuidV7IdGenerator {
    fn project_id(&self) -> ProjectId {
        ProjectId::parse(uuid_v7()).expect("generated id is UUID v7")
    }
    fn material_id(&self) -> MaterialId {
        MaterialId::parse(uuid_v7()).expect("generated id is UUID v7")
    }
    fn creation_id(&self) -> CreationId {
        CreationId::parse(uuid_v7()).expect("generated id is UUID v7")
    }
    fn message_id(&self) -> MessageId {
        MessageId::parse(uuid_v7()).expect("generated id is UUID v7")
    }
}

fn unix_secs_to_rfc3339_utc(secs: u64) -> String {
    let mut days = secs / 86_400;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    let mut year = 1970u32;
    loop {
        let year_days = if is_gregorian_leap(year) { 366 } else { 365 };
        if days >= year_days {
            days -= year_days;
            year += 1;
        } else {
            break;
        }
    }
    let month_days: [u32; 12] = [
        31,
        if is_gregorian_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for md in month_days {
        if days >= u64::from(md) {
            days -= u64::from(md);
            month += 1;
        } else {
            break;
        }
    }
    let day = days + 1;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn is_gregorian_leap(year: u32) -> bool {
    year.is_multiple_of(400) || year.is_multiple_of(4) && !year.is_multiple_of(100)
}

fn uuid_v7() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut bytes = [0u8; 16];
    bytes[0] = (millis >> 40) as u8;
    bytes[1] = (millis >> 32) as u8;
    bytes[2] = (millis >> 24) as u8;
    bytes[3] = (millis >> 16) as u8;
    bytes[4] = (millis >> 8) as u8;
    bytes[5] = millis as u8;
    let _ = getrandom::fill(&mut bytes[6..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    const P: &str = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
    const M: &str = "0198e4a6-79b2-7b51-9e68-c2eb7af3db14";
    const C: &str = "0198e4a6-86d6-7c16-b4c4-3197b355cf10";
    const MSG_IDS: &[&str] = &[
        "0198e4a6-93fa-7c81-9e68-c2eb7af3db15",
        "0198e4a6-93fa-7c82-9e68-c2eb7af3db15",
        "0198e4a6-93fa-7c83-9e68-c2eb7af3db15",
        "0198e4a6-93fa-7c84-9e68-c2eb7af3db15",
        "0198e4a6-93fa-7c85-9e68-c2eb7af3db15",
    ];
    fn pid() -> ProjectId {
        ProjectId::parse(P).unwrap()
    }
    fn time() -> Timestamp {
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap()
    }
    #[derive(Clone)]
    struct TestClock;
    impl Clock for TestClock {
        fn now(&self) -> Timestamp {
            time()
        }
    }
    struct TestIds;
    impl IdGenerator for TestIds {
        fn project_id(&self) -> ProjectId {
            pid()
        }
        fn material_id(&self) -> MaterialId {
            MaterialId::parse(M).unwrap()
        }
        fn creation_id(&self) -> CreationId {
            CreationId::parse(C).unwrap()
        }
        fn message_id(&self) -> MessageId {
            static IDX: AtomicUsize = AtomicUsize::new(0);
            let i = IDX.fetch_add(1, Ordering::Relaxed);
            MessageId::parse(MSG_IDS[i % MSG_IDS.len()]).unwrap()
        }
    }
    #[derive(Default)]
    struct Repo {
        values: BTreeMap<ProjectId, Project>,
        fail: bool,
        conflict: bool,
    }
    impl ProjectRepository for Repo {
        fn create(&mut self, p: &Project) -> CoreResult<()> {
            p.validate_for_persist()?;
            if self.values.contains_key(&p.id) {
                return Err(ProjectCoreError::AlreadyExists(p.id.clone()));
            }
            self.values.insert(p.id.clone(), p.clone());
            Ok(())
        }
        fn get(&self, id: &ProjectId) -> CoreResult<Project> {
            self.values
                .get(id)
                .cloned()
                .ok_or_else(|| ProjectCoreError::NotFound(id.clone()))
        }
        fn list(&self) -> CoreResult<Vec<Project>> {
            let mut v: Vec<_> = self.values.values().cloned().collect();
            v.sort_by(|a, b| {
                b.updated_at
                    .cmp(&a.updated_at)
                    .then_with(|| a.id.cmp(&b.id))
            });
            Ok(v)
        }
        fn replace(&mut self, p: &Project, e: &Timestamp) -> CoreResult<()> {
            if self.fail {
                return Err(ProjectCoreError::AtomicWriteFailed);
            }
            if self.conflict || self.get(&p.id)?.updated_at != *e {
                return Err(ProjectCoreError::Conflict {
                    project_id: p.id.clone(),
                });
            }
            p.validate_for_persist()?;
            self.values.insert(p.id.clone(), p.clone());
            Ok(())
        }
        fn delete(&mut self, id: &ProjectId) -> CoreResult<()> {
            self.values
                .remove(id)
                .map(|_| ())
                .ok_or_else(|| ProjectCoreError::NotFound(id.clone()))
        }
    }
    #[derive(Default)]
    struct Store {
        materials: BTreeMap<(ProjectId, MaterialId), Vec<u8>>,
        creations: BTreeMap<(ProjectId, CreationId), Vec<u8>>,
        invalid_path: bool,
        fail_remove: bool,
        deleted: Vec<ProjectId>,
    }
    fn hash(_: &[u8]) -> Sha256Digest {
        Sha256Digest::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .unwrap()
    }
    impl ProjectContentStore for Store {
        fn store_material(
            &mut self,
            p: &ProjectId,
            m: &MaterialId,
            s: &MaterialContent,
            file: &str,
        ) -> CoreResult<StoredMaterial> {
            self.materials
                .insert((p.clone(), m.clone()), s.bytes.clone());
            let path = if self.invalid_path {
                "publish/x".into()
            } else {
                format!("inputs/{m}/{file}")
            };
            Ok(StoredMaterial {
                relative_path: RelativeProjectPath::parse(path)?,
                byte_size: s.bytes.len() as u64,
                sha256: hash(&s.bytes),
            })
        }
        fn read_material(&self, p: &ProjectId, m: &Material) -> CoreResult<Vec<u8>> {
            self.materials
                .get(&(p.clone(), m.id.clone()))
                .cloned()
                .ok_or_else(|| ProjectCoreError::NotFound(p.clone()))
        }
        fn store_creation(
            &mut self,
            p: &ProjectId,
            c: &CreationId,
            x: &CreationContent,
            file: &str,
        ) -> CoreResult<StoredCreation> {
            self.creations
                .insert((p.clone(), c.clone()), x.bytes.clone());
            let path = if self.invalid_path {
                "inputs/x".into()
            } else {
                format!("outputs/{c}/{file}")
            };
            Ok(StoredCreation {
                relative_path: RelativeProjectPath::parse(path)?,
                byte_size: x.bytes.len() as u64,
            })
        }
        fn store_creation_version(
            &mut self,
            p: &ProjectId,
            c: &CreationId,
            _base: Option<&Creation>,
            content: &CreationVersionContent,
        ) -> CoreResult<StoredCreation> {
            let primary = content
                .overlays
                .iter()
                .find(|o| o.relative_path == content.primary_relative_path)
                .map(|o| o.bytes.clone())
                .unwrap_or_default();
            let byte_size: u64 = content.overlays.iter().map(|o| o.bytes.len() as u64).sum();
            let path = if self.invalid_path {
                "inputs/x".into()
            } else {
                format!("outputs/{c}/{}", content.primary_relative_path)
            };
            self.creations.insert((p.clone(), c.clone()), primary);
            Ok(StoredCreation {
                relative_path: RelativeProjectPath::parse(path)?,
                byte_size,
            })
        }
        fn recover_stale_creation_staging(&mut self, _p: &ProjectId) -> CoreResult<()> {
            Ok(())
        }
        fn read_creation(&self, p: &ProjectId, c: &Creation) -> CoreResult<Vec<u8>> {
            self.creations
                .get(&(p.clone(), c.id.clone()))
                .cloned()
                .ok_or_else(|| ProjectCoreError::NotFound(p.clone()))
        }
        fn remove_material(&mut self, p: &ProjectId, m: &MaterialId) -> CoreResult<()> {
            self.materials.remove(&(p.clone(), m.clone()));
            Ok(())
        }
        fn remove_project_tree(&mut self, p: &ProjectId) -> CoreResult<()> {
            self.deleted.push(p.clone());
            if self.fail_remove {
                Err(ProjectCoreError::WriteFailed)
            } else {
                Ok(())
            }
        }
    }
    fn service() -> ProjectService<Repo, Store, TestClock, TestIds> {
        ProjectService::new(Repo::default(), Store::default(), TestClock, TestIds)
    }
    fn material() -> AddMaterial {
        AddMaterial {
            display_name: "Guide".into(),
            original_file_name: "Guía de clase.pdf".into(),
            content_type: Some(ContentType::parse("application/pdf").unwrap()),
            source: MaterialContent {
                bytes: b"original bytes".to_vec(),
            },
        }
    }
    #[test]
    fn values_reject_noncanonical_ids_dates_names_and_paths() {
        assert!(ProjectId::parse(P).is_ok());
        assert!(ProjectId::parse("0198e4a6-6e70-6c01-8c0e-8b6fd26f1f22").is_err());
        assert!(Timestamp::parse("2025-02-29T15:00:00Z").is_err());
        assert!(ProjectName::parse("../secret").is_err());
        for p in ["/a", "inputs/../x", "inputs\\x", "inputs//x"] {
            assert!(RelativeProjectPath::parse(p).is_err())
        }
    }
    #[test]
    fn sanitized_file_name_cannot_be_used_as_a_path() {
        assert_eq!(
            safe_file_name(" ../Guía de clase.pdf "),
            "Gu-a-de-clase.pdf"
        );
        assert_eq!(safe_file_name("..."), "file")
    }
    #[test]
    fn project_lifecycle_is_pure_and_deterministic() {
        let mut s = service();
        let p = s.create_project(" Fotosíntesis ").unwrap();
        assert_eq!(p.name.as_str(), "Fotosíntesis");
        assert_eq!(s.open_project(&pid()).unwrap(), p);
        let renamed = s.rename_project(&pid(), "Sistema solar").unwrap();
        assert_eq!(s.list_projects().unwrap(), vec![renamed]);
        assert!(matches!(
            s.create_project("two"),
            Err(ProjectCoreError::AlreadyExists(_))
        ))
    }
    #[test]
    fn material_is_committed_after_content_and_keeps_fixed_root() {
        let mut s = service();
        s.create_project("one").unwrap();
        let item = s.add_material(&pid(), material()).unwrap();
        assert_eq!(
            item.relative_path.as_str(),
            "inputs/0198e4a6-79b2-7b51-9e68-c2eb7af3db14/Gu-a-de-clase.pdf"
        );
        assert_eq!(
            s.read_material(&pid(), &item.id).unwrap(),
            b"original bytes"
        );
        assert_eq!(s.open_project(&pid()).unwrap().materials, vec![item])
    }
    #[test]
    fn creation_is_outputs_only_revision_one_and_readable() {
        let mut s = service();
        s.create_project("one").unwrap();
        let c = s
            .create_creation(
                &pid(),
                CreateCreation {
                    display_name: "Activity".into(),
                    kind: CreationKind::Web,
                    visibility: CreationVisibility::Private,
                    content_type: Some(ContentType::parse("text/html").unwrap()),
                    content: CreationContent {
                        bytes: b"<h1>x</h1>".to_vec(),
                        file_name: "index.html".into(),
                    },
                    parent_creation_id: None,
                },
            )
            .unwrap();
        assert_eq!(
            c.relative_path.as_str(),
            "outputs/0198e4a6-86d6-7c16-b4c4-3197b355cf10/index.html"
        );
        assert_eq!(c.revision, 1);
        assert_eq!(c.visibility, CreationVisibility::Private);
        assert_eq!(s.read_creation(&pid(), &c.id).unwrap(), b"<h1>x</h1>")
    }
    #[test]
    fn failed_metadata_replace_leaves_no_content_reference() {
        let mut s = service();
        s.create_project("one").unwrap();
        let (mut r, c, k, i) = s.into_parts();
        r.fail = true;
        let mut s = ProjectService::new(r, c, k, i);
        assert!(matches!(
            s.add_material(&pid(), material()),
            Err(ProjectCoreError::AtomicWriteFailed)
        ));
        assert!(s.open_project(&pid()).unwrap().materials.is_empty())
    }
    #[test]
    fn content_store_cannot_substitute_publish_or_inputs_roots() {
        let mut s = service();
        s.create_project("one").unwrap();
        let (r, mut c, k, i) = s.into_parts();
        c.invalid_path = true;
        let mut s = ProjectService::new(r, c, k, i);
        assert!(matches!(
            s.add_material(&pid(), material()),
            Err(ProjectCoreError::InvalidPath(_))
        ))
    }
    #[test]
    fn aggregate_rejects_duplicate_ids_and_path_escape() {
        let mut p = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        let id = MaterialId::parse(M).unwrap();
        let m = Material {
            id: id.clone(),
            display_name: "x".into(),
            original_file_name: "x.txt".into(),
            relative_path: RelativeProjectPath::parse(format!("inputs/{id}/x.txt")).unwrap(),
            content_type: None,
            byte_size: 0,
            sha256: hash(b""),
            created_at: time(),
        };
        p.materials = vec![m.clone(), m];
        assert!(matches!(
            p.validate(),
            Err(ProjectCoreError::DuplicateMaterial(_))
        ));
        p.materials[1].id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db15").unwrap();
        p.materials[1].relative_path = RelativeProjectPath::parse("publish/x/x.txt").unwrap();
        assert!(p.validate().is_err())
    }
    #[test]
    fn delete_checks_existence_before_targeting_tree() {
        let mut s = service();
        assert!(matches!(
            s.delete_project(&pid()),
            Err(ProjectCoreError::NotFound(_))
        ));
        s.create_project("one").unwrap();
        assert!(s.delete_project(&pid()).is_ok());
        assert!(matches!(
            s.open_project(&pid()),
            Err(ProjectCoreError::NotFound(_))
        ))
    }
    #[test]
    fn delete_removes_metadata_before_tree_and_never_restores_it() {
        let mut s = service();
        s.create_project("one").unwrap();
        let (r, mut c, k, i) = s.into_parts();
        c.fail_remove = true;
        let mut s = ProjectService::new(r, c, k, i);
        assert!(matches!(
            s.delete_project(&pid()),
            Err(ProjectCoreError::WriteFailed)
        ));
        assert!(matches!(
            s.open_project(&pid()),
            Err(ProjectCoreError::NotFound(_))
        ))
    }
    #[test]
    fn missing_material_and_creation_have_distinct_typed_errors() {
        let mut s = service();
        s.create_project("one").unwrap();
        assert!(matches!(
            s.read_material(&pid(), &MaterialId::parse(M).unwrap()),
            Err(ProjectCoreError::MissingMaterial(_))
        ));
        assert!(matches!(
            s.read_creation(&pid(), &CreationId::parse(C).unwrap()),
            Err(ProjectCoreError::MissingCreation(_))
        ))
    }
    #[test]
    fn remove_material_removes_metadata_and_content_only() {
        let mut s = service();
        s.create_project("one").unwrap();
        let item = s.add_material(&pid(), material()).unwrap();
        assert_eq!(s.open_project(&pid()).unwrap().materials.len(), 1);
        s.remove_material(&pid(), &item.id).unwrap();
        assert!(s.open_project(&pid()).unwrap().materials.is_empty());
        // The content is no longer readable: the store entry is gone.
        assert!(matches!(
            s.read_material(&pid(), &item.id),
            Err(ProjectCoreError::MissingMaterial(_))
        ));
    }
    #[test]
    fn remove_missing_material_is_a_typed_error() {
        let mut s = service();
        s.create_project("one").unwrap();
        assert!(matches!(
            s.remove_material(&pid(), &MaterialId::parse(M).unwrap()),
            Err(ProjectCoreError::MissingMaterial(_))
        ));
    }
    #[test]
    fn remove_material_conflict_preserves_existing_project() {
        let mut s = service();
        s.create_project("one").unwrap();
        let item = s.add_material(&pid(), material()).unwrap();
        let (mut r, c, k, i) = s.into_parts();
        r.conflict = true;
        let mut s = ProjectService::new(r, c, k, i);
        assert!(matches!(
            s.remove_material(&pid(), &item.id),
            Err(ProjectCoreError::Conflict { .. })
        ));
        // Metadata and content are both untouched on conflict.
        assert_eq!(s.open_project(&pid()).unwrap().materials.len(), 1);
        assert_eq!(
            s.read_material(&pid(), &item.id).unwrap(),
            b"original bytes"
        );
    }
    #[test]
    fn remove_material_keeps_original_source_bytes_intact() {
        let mut s = service();
        s.create_project("one").unwrap();
        let item = s.add_material(&pid(), material()).unwrap();
        s.remove_material(&pid(), &item.id).unwrap();
        // The user's source bytes were never stored verbatim as the readable
        // content after removal; the remaining project stays consistent.
        assert!(s.open_project(&pid()).unwrap().materials.is_empty());
    }
    #[test]
    fn create_and_rename_reject_blank_overlong_and_path_like_names() {
        let mut s = service();
        assert!(matches!(
            s.create_project(" "),
            Err(ProjectCoreError::InvalidName(_))
        ));
        assert!(matches!(
            s.create_project("x".repeat(MAX_PROJECT_NAME_CHARS + 1)),
            Err(ProjectCoreError::InvalidName(_))
        ));
        s.create_project("one").unwrap();
        assert!(matches!(
            s.rename_project(&pid(), "a/b"),
            Err(ProjectCoreError::InvalidName(_))
        ))
    }
    #[test]
    fn rename_conflict_preserves_existing_project() {
        let mut s = service();
        s.create_project("one").unwrap();
        let (mut r, c, k, i) = s.into_parts();
        r.conflict = true;
        let mut s = ProjectService::new(r, c, k, i);
        assert!(matches!(
            s.rename_project(&pid(), "two"),
            Err(ProjectCoreError::Conflict { .. })
        ));
        assert_eq!(s.open_project(&pid()).unwrap().name.as_str(), "one")
    }
    #[test]
    fn parent_creation_reference_must_exist_in_aggregate() {
        let mut p = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        p.creations.push(Creation {
            id: CreationId::parse(C).unwrap(),
            display_name: "x".into(),
            kind: CreationKind::File,
            visibility: CreationVisibility::Private,
            relative_path: RelativeProjectPath::parse(format!("outputs/{C}/x.txt")).unwrap(),
            content_type: None,
            byte_size: 0,
            revision: 1,
            parent_creation_id: Some(
                CreationId::parse("0198e4a6-86d6-7c16-b4c4-3197b355cf11").unwrap(),
            ),
            created_at: time(),
            lineage_id: None,
            version_number: None,
            is_current: None,
            bundle_root: None,
        });
        assert!(matches!(
            p.validate(),
            Err(ProjectCoreError::InvalidCreation(_))
        ))
    }
    fn v1_json_with_creation(display_name: &str, kind: &str, file_name: &str) -> String {
        format!(
            r#"{{
  "schemaVersion": 1,
  "projectId": "{P}",
  "name": "Fotosintesis",
  "createdAt": "2026-08-28T15:00:00Z",
  "updatedAt": "2026-08-28T15:00:00Z",
  "state": "local",
  "materials": [],
  "creations": [
    {{
      "creationId": "{C}",
      "displayName": "{display_name}",
      "kind": "{kind}",
      "relativePath": "outputs/{C}/{file_name}",
      "contentType": "text/html",
      "byteSize": 8,
      "revision": 1,
      "createdAt": "2026-08-28T15:00:00Z"
    }}
  ]
}}"#
        )
    }
    #[test]
    fn new_project_is_schema_v3_without_publication_route() {
        let p = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        assert_eq!(p.schema_version, PROJECT_SCHEMA_VERSION);
        assert!(p.publication_route.is_none());
        assert!(p.messages.is_empty());
        assert!(p.validate_for_persist().is_ok())
    }
    #[test]
    fn create_creation_persists_explicit_visibility_defaulting_to_private() {
        let mut s = service();
        s.create_project("one").unwrap();
        let private = s
            .create_creation(
                &pid(),
                CreateCreation {
                    display_name: "Activity".into(),
                    kind: CreationKind::Web,
                    visibility: CreationVisibility::default(),
                    content_type: None,
                    content: CreationContent {
                        bytes: b"x".to_vec(),
                        file_name: "index.html".into(),
                    },
                    parent_creation_id: None,
                },
            )
            .unwrap();
        assert_eq!(private.visibility, CreationVisibility::Private);
        assert_eq!(
            s.open_project(&pid()).unwrap().creations[0].visibility,
            CreationVisibility::Private
        )
    }
    #[test]
    fn replace_creation_content_keeps_id_and_updates_bytes() {
        let mut s = service();
        s.create_project("one").unwrap();
        let created = s
            .create_creation(
                &pid(),
                CreateCreation {
                    display_name: "Activity".into(),
                    kind: CreationKind::Web,
                    visibility: CreationVisibility::Private,
                    content_type: None,
                    content: CreationContent {
                        bytes: b"old".to_vec(),
                        file_name: "index.html".into(),
                    },
                    parent_creation_id: None,
                },
            )
            .unwrap();
        let replaced = s
            .replace_creation_content(
                &pid(),
                &created.id,
                CreationContent {
                    bytes: b"new-bytes".to_vec(),
                    file_name: "index.html".into(),
                },
            )
            .unwrap();
        assert_eq!(replaced.id, created.id);
        assert_eq!(replaced.display_name, "Activity");
        assert_eq!(replaced.byte_size, 9);
        assert_eq!(replaced.revision, 1);
        assert_eq!(replaced.visibility, CreationVisibility::Private);
        assert_eq!(s.read_creation(&pid(), &created.id).unwrap(), b"new-bytes");
        assert_eq!(s.open_project(&pid()).unwrap().creations.len(), 1);
    }
    #[test]
    fn reader_accepts_schema_v1_and_marks_legacy_creations_private() {
        let p = Project::from_json(&v1_json_with_creation(
            "public answers key",
            "web",
            "public.html",
        ))
        .unwrap();
        assert_eq!(p.schema_version, LEGACY_PROJECT_SCHEMA_VERSION);
        assert!(p.publication_route.is_none());
        assert_eq!(p.creations.len(), 1);
        assert_eq!(p.creations[0].visibility, CreationVisibility::Private);
        assert_eq!(p.creations[0].display_name, "public answers key");
        assert_eq!(p.creations[0].kind, CreationKind::Web)
    }
    #[test]
    fn v1_migration_is_private_and_never_infers_visibility() {
        let mut p = Project::from_json(&v1_json_with_creation(
            "PUBLIC worksheet",
            "web",
            "index.html",
        ))
        .unwrap();
        p.creations[0].visibility = CreationVisibility::Public;
        p.migrate_to_v3().unwrap();
        assert_eq!(p.schema_version, PROJECT_SCHEMA_VERSION);
        assert!(p.publication_route.is_none());
        assert_eq!(p.creations[0].visibility, CreationVisibility::Private);
        assert_eq!(p.creations[0].kind, CreationKind::Web);
        assert_eq!(p.creations[0].display_name, "PUBLIC worksheet")
    }
    #[test]
    fn v3_migration_is_idempotent_and_preserves_explicit_public() {
        let mut p = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        p.creations.push(Creation {
            id: CreationId::parse(C).unwrap(),
            display_name: "published".into(),
            kind: CreationKind::Web,
            visibility: CreationVisibility::Public,
            relative_path: RelativeProjectPath::parse(format!("outputs/{C}/index.html")).unwrap(),
            content_type: None,
            byte_size: 1,
            revision: 1,
            parent_creation_id: None,
            created_at: time(),
            lineage_id: None,
            version_number: None,
            is_current: None,
            bundle_root: None,
        });
        p.publication_route = Some(PublicationRoute::parse("fotosintesis-a7k2m9").unwrap());
        p.migrate_to_v3().unwrap();
        p.migrate_to_v3().unwrap();
        assert_eq!(p.schema_version, PROJECT_SCHEMA_VERSION);
        assert_eq!(p.creations[0].visibility, CreationVisibility::Public);
        assert_eq!(
            p.publication_route.as_ref().map(PublicationRoute::as_str),
            Some("fotosintesis-a7k2m9")
        )
    }
    #[test]
    fn persist_rejects_unmigrated_v1_and_unknown_schema() {
        let mut p = Project::from_json(&v1_json_with_creation("x", "file", "x.txt")).unwrap();
        assert!(matches!(
            p.validate_for_persist(),
            Err(ProjectCoreError::UnsupportedSchema(
                LEGACY_PROJECT_SCHEMA_VERSION
            ))
        ));
        p.schema_version = 99;
        assert!(matches!(
            p.validate(),
            Err(ProjectCoreError::UnsupportedSchema(99))
        ));
        assert!(matches!(
            Project::from_json("{\"schemaVersion\":99,\"projectId\":\"x\"}"),
            Err(ProjectCoreError::UnsupportedSchema(99))
        ))
    }
    #[test]
    fn v1_reader_rejects_visibility_or_route_fields() {
        let with_visibility = v1_json_with_creation("x", "web", "index.html").replace(
            "\"kind\": \"web\"",
            "\"kind\": \"web\",\n      \"visibility\": \"public\"",
        );
        assert!(matches!(
            Project::from_json(&with_visibility),
            Err(ProjectCoreError::CorruptMetadata(_))
        ));
        let with_route = v1_json_with_creation("x", "file", "x.txt").replace(
            "\"state\": \"local\"",
            "\"state\": \"local\",\n  \"publicationRoute\": \"fotosintesis-a7k2\"",
        );
        assert!(matches!(
            Project::from_json(&with_route),
            Err(ProjectCoreError::CorruptMetadata(_))
        ))
    }
    #[test]
    fn v2_reader_requires_visibility_and_rejects_unknown_fields() {
        let missing_visibility = v1_json_with_creation("x", "web", "index.html")
            .replace("\"schemaVersion\": 1", "\"schemaVersion\": 2");
        assert!(matches!(
            Project::from_json(&missing_visibility),
            Err(ProjectCoreError::CorruptMetadata(_))
        ));
        let v2 = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        let mut raw = serde_json::to_string(&v2).unwrap();
        raw.insert_str(raw.rfind('}').unwrap(), ",\"unexpectedField\":true");
        assert!(matches!(
            Project::from_json(&raw),
            Err(ProjectCoreError::CorruptMetadata(_))
        ))
    }
    #[test]
    fn first_mutation_migrates_v1_atomically_and_failed_replace_leaves_v1() {
        let mut s = service();
        s.create_project("one").unwrap();
        let (mut r, c, k, i) = s.into_parts();
        let mut legacy = r.get(&pid()).unwrap();
        legacy.schema_version = LEGACY_PROJECT_SCHEMA_VERSION;
        r.values.insert(pid(), legacy);
        r.fail = true;
        let mut s = ProjectService::new(r, c, k, i);
        assert!(matches!(
            s.rename_project(&pid(), "two"),
            Err(ProjectCoreError::AtomicWriteFailed)
        ));
        let unchanged = s.open_project(&pid()).unwrap();
        assert_eq!(unchanged.schema_version, LEGACY_PROJECT_SCHEMA_VERSION);
        assert_eq!(unchanged.name.as_str(), "one");
        let (mut r, c, k, i) = s.into_parts();
        r.fail = false;
        let mut s = ProjectService::new(r, c, k, i);
        let renamed = s.rename_project(&pid(), "two").unwrap();
        assert_eq!(renamed.schema_version, PROJECT_SCHEMA_VERSION);
        assert!(renamed.publication_route.is_none());
        assert_eq!(
            s.open_project(&pid()).unwrap().schema_version,
            PROJECT_SCHEMA_VERSION
        )
    }
    #[test]
    fn publication_route_accepts_m2_grammar_and_rejects_invalid() {
        assert!(PublicationRoute::parse("fotosintesis-a7k2m9").is_ok());
        assert!(PublicationRoute::parse("a").is_ok());
        for bad in ["", "A", "-abc", "abc-", "ab--cd", "ab.cd", "ab/cd", "ñ"] {
            assert!(
                PublicationRoute::parse(bad).is_err(),
                "expected {bad:?} to be rejected"
            )
        }
    }
    #[derive(Clone)]
    struct CountingClock(std::cell::Cell<u64>);
    impl CountingClock {
        fn new() -> Self {
            Self(std::cell::Cell::new(0))
        }
    }
    impl Clock for CountingClock {
        fn now(&self) -> Timestamp {
            let i = self.0.get();
            self.0.set(i + 1);
            Timestamp::parse(format!("2026-08-28T15:{:02}:{:02}Z", i / 60, i % 60)).unwrap()
        }
    }
    fn advancing_service() -> ProjectService<Repo, Store, CountingClock, TestIds> {
        ProjectService::new(
            Repo::default(),
            Store::default(),
            CountingClock::new(),
            TestIds,
        )
    }
    #[test]
    fn messages_are_append_only_and_ordered() {
        let mut s = advancing_service();
        s.create_project("one").unwrap();
        let first = s.append_user_message(&pid(), "hello", &[]).unwrap();
        let second = s
            .append_assistant_message(&pid(), "hi there", MessageStatus::Ok, &[], None)
            .unwrap();
        let msgs = s.messages(&pid()).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].id, first.id);
        assert_eq!(msgs[0].role, MessageRole::User);
        assert_eq!(msgs[1].id, second.id);
        assert_eq!(msgs[1].role, MessageRole::Assistant);
        assert_eq!(
            s.open_project(&pid()).unwrap().updated_at,
            second.created_at
        );
    }
    #[test]
    fn message_references_must_be_subset_of_project() {
        let mut s = service();
        s.create_project("one").unwrap();
        let missing_material = MaterialId::parse(M).unwrap();
        assert!(matches!(
            s.append_user_message(&pid(), "x", &[missing_material]),
            Err(ProjectCoreError::MissingMaterial(_))
        ));
        let c = s
            .create_creation(
                &pid(),
                CreateCreation {
                    display_name: "Activity".into(),
                    kind: CreationKind::Web,
                    visibility: CreationVisibility::Private,
                    content_type: None,
                    content: CreationContent {
                        bytes: b"x".to_vec(),
                        file_name: "index.html".into(),
                    },
                    parent_creation_id: None,
                },
            )
            .unwrap();
        let missing_creation = CreationId::parse("0198e4a6-86d6-7c16-b4c4-3197b355cf11").unwrap();
        assert!(matches!(
            s.append_assistant_message(&pid(), "x", MessageStatus::Ok, &[missing_creation], None),
            Err(ProjectCoreError::MissingCreation(_))
        ));
        assert!(
            s.append_assistant_message(&pid(), "y", MessageStatus::Ok, &[c.id], None)
                .is_ok()
        );
    }
    #[test]
    fn user_message_cannot_reference_creations() {
        let mut p = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        let cid = CreationId::parse(C).unwrap();
        p.creations.push(Creation {
            id: cid.clone(),
            display_name: "x".into(),
            kind: CreationKind::Web,
            visibility: CreationVisibility::Private,
            relative_path: RelativeProjectPath::parse(format!("outputs/{C}/x.txt")).unwrap(),
            content_type: None,
            byte_size: 0,
            revision: 1,
            parent_creation_id: None,
            created_at: time(),
            lineage_id: None,
            version_number: None,
            is_current: None,
            bundle_root: None,
        });
        p.messages.push(Message {
            id: MessageId::parse(MSG_IDS[0]).unwrap(),
            role: MessageRole::User,
            text: "x".into(),
            status: MessageStatus::Ok,
            created_at: time(),
            material_ids: vec![],
            creation_ids: vec![cid],
            turn_metrics: None,
            turn_referent: None,
            turn_id: None,
        });
        assert!(matches!(
            p.validate(),
            Err(ProjectCoreError::InvalidMessage(_))
        ));
    }
    #[test]
    fn assistant_message_cannot_reference_materials() {
        let mut p = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        let mid = MaterialId::parse(M).unwrap();
        p.materials.push(Material {
            id: mid.clone(),
            display_name: "x".into(),
            original_file_name: "x.txt".into(),
            relative_path: RelativeProjectPath::parse(format!("inputs/{M}/x.txt")).unwrap(),
            content_type: None,
            byte_size: 0,
            sha256: hash(b""),
            created_at: time(),
        });
        p.messages.push(Message {
            id: MessageId::parse(MSG_IDS[0]).unwrap(),
            role: MessageRole::Assistant,
            text: "x".into(),
            status: MessageStatus::Ok,
            created_at: time(),
            material_ids: vec![mid],
            creation_ids: vec![],
            turn_metrics: None,
            turn_referent: None,
            turn_id: None,
        });
        assert!(matches!(
            p.validate(),
            Err(ProjectCoreError::InvalidMessage(_))
        ));
    }
    #[test]
    fn message_text_capped() {
        let mut s = service();
        s.create_project("one").unwrap();
        let oversized = "x".repeat(MAX_MESSAGE_TEXT_CHARS + 1);
        assert!(matches!(
            s.append_user_message(&pid(), &oversized, &[]),
            Err(ProjectCoreError::InvalidMessage(_))
        ));
    }
    #[test]
    fn message_ids_are_uuid_v7() {
        let mut s = service();
        s.create_project("one").unwrap();
        let msg = s.append_user_message(&pid(), "hello", &[]).unwrap();
        assert!(MessageId::parse(msg.id.as_str()).is_ok());
    }
    #[test]
    fn v2_project_migrates_to_v3_with_empty_messages() {
        let mut p = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        p.schema_version = SCHEMA_V2;
        let raw = serde_json::to_string(&p).unwrap();
        let loaded = Project::from_json(&raw).unwrap();
        assert_eq!(loaded.schema_version, SCHEMA_V2);
        assert!(loaded.messages.is_empty());
        let mut migrated = loaded;
        migrated.migrate_to_v3().unwrap();
        assert_eq!(migrated.schema_version, PROJECT_SCHEMA_VERSION);
        assert!(migrated.messages.is_empty());
    }
    #[test]
    fn v1_project_migrates_to_v3() {
        let mut p = Project::from_json(&v1_json_with_creation("x", "web", "index.html")).unwrap();
        assert_eq!(p.schema_version, LEGACY_PROJECT_SCHEMA_VERSION);
        assert!(p.messages.is_empty());
        p.migrate_to_v3().unwrap();
        assert_eq!(p.schema_version, PROJECT_SCHEMA_VERSION);
        assert_eq!(p.creations[0].visibility, CreationVisibility::Private);
        assert!(p.publication_route.is_none());
        assert!(p.messages.is_empty());
    }
    #[test]
    fn deleting_material_clears_message_reference() {
        let mut s = service();
        s.create_project("one").unwrap();
        let m = s.add_material(&pid(), material()).unwrap();
        s.append_user_message(&pid(), "look at this", std::slice::from_ref(&m.id))
            .unwrap();
        s.remove_material(&pid(), &m.id).unwrap();
        let msgs = s.messages(&pid()).unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].material_ids.is_empty());
    }
    #[test]
    fn turn_referent_round_trips_through_project_json() {
        let mut s = service();
        s.create_project("one").unwrap();
        let m = s.add_material(&pid(), material()).unwrap();
        let msg = s
            .append_user_message(&pid(), "listame los archivos", std::slice::from_ref(&m.id))
            .unwrap();
        let referent = TurnReferent::MaterialSet(MaterialSetReferent {
            material_ids: vec![m.id.as_str().to_owned()],
            source_names: vec!["material.txt".to_owned()],
            origin_turn_id: msg.id.as_str().to_owned(),
            produced_by: "inventory".to_owned(),
        });
        s.set_turn_referent(&pid(), &msg.id, referent.clone())
            .unwrap();
        let project = s.open_project(&pid()).unwrap();
        assert_eq!(project.messages[0].turn_referent, Some(referent.clone()));
        let raw = serde_json::to_string(&project).unwrap();
        let reloaded = Project::from_json(&raw).unwrap();
        assert_eq!(
            reloaded.messages[0].turn_referent,
            Some(referent),
            "referent must survive serialize + from_json"
        );
    }
    #[test]
    fn theme_referent_round_trips_and_serializes_with_kind_tag() {
        let referent = TurnReferent::ThemeSet(ThemeSetReferent {
            theme_keys: vec![
                "presente continuo".to_owned(),
                "google workspace".to_owned(),
            ],
            display_labels: vec![
                "presente continuo".to_owned(),
                "google workspace".to_owned(),
            ],
            origin_turn_id: "turn-1".to_owned(),
            source_names: vec!["reunion-a.md".to_owned()],
        });
        let raw = serde_json::to_string(&referent).unwrap();
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(json["kind"], "themeSet");
        assert_eq!(json["themeKeys"][0], "presente continuo");
        let back: TurnReferent = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, referent);
    }
    #[test]
    fn legacy_messages_without_turn_referent_load_successfully() {
        // A pre-referent project.json message (no turnReferent key) must load
        // with turn_referent == None. Simulated by serializing an old-shaped
        // message and re-parsing it through the current schema.
        let mut p = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        p.messages.push(Message {
            id: MessageId::parse(MSG_IDS[0]).unwrap(),
            role: MessageRole::User,
            text: "listame los archivos".into(),
            status: MessageStatus::Ok,
            created_at: time(),
            material_ids: vec![],
            creation_ids: vec![],
            turn_metrics: None,
            turn_referent: None,
            turn_id: None,
        });
        let value: serde_json::Value = serde_json::to_value(&p).unwrap();
        let message = value["messages"][0].as_object().unwrap().clone();
        assert!(
            !message.contains_key("turnReferent"),
            "None referents must not be serialized"
        );
        let raw = serde_json::to_string(&p).unwrap();
        let reloaded = Project::from_json(&raw).unwrap();
        assert_eq!(reloaded.messages[0].turn_referent, None);
    }
    #[test]
    fn set_turn_referent_requires_a_user_message() {
        let mut s = service();
        s.create_project("one").unwrap();
        let msg = s
            .append_assistant_message(&pid(), "hola", MessageStatus::Ok, &[], None)
            .unwrap();
        let referent = TurnReferent::MaterialSet(MaterialSetReferent {
            material_ids: vec![],
            source_names: vec![],
            origin_turn_id: msg.id.as_str().to_owned(),
            produced_by: "inventory".to_owned(),
        });
        assert!(s.set_turn_referent(&pid(), &msg.id, referent).is_err());
    }
}

#[cfg(test)]
mod production_ids_and_clock {
    use super::*;
    use std::collections::HashSet;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn system_clock_produces_parseable_nondecreasing_timestamps() {
        let clock = SystemClock;
        let first = clock.now();
        Timestamp::parse(first.as_str()).expect("first timestamp parses");
        thread::sleep(Duration::from_millis(1100));
        let second = clock.now();
        Timestamp::parse(second.as_str()).expect("second timestamp parses");
        assert!(
            second.as_str() >= first.as_str(),
            "clock moved backwards: {} then {}",
            first.as_str(),
            second.as_str()
        );
        assert_ne!(first.as_str(), second.as_str());
    }

    #[test]
    fn uuid_v7_ids_parse_and_are_unique() {
        let generator = UuidV7IdGenerator;
        let mut seen = HashSet::new();
        for _ in 0..4_000 {
            let id = generator.project_id();
            ProjectId::parse(id.as_str()).expect("uuid v7");
            assert!(
                seen.insert(id.as_str().to_owned()),
                "duplicate {}",
                id.as_str()
            );
        }
    }
}
