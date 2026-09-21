pub struct AgentBackendInfo {
    pub version: String,
}

pub struct AgentProject {
    pub project_id: String,
    pub directory: std::path::PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSession {
    pub id: String,
    pub project_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

pub struct AgentPrompt {
    /// The current user turn, kept distinct from untrusted evidence until
    /// provider-independent request assembly.
    pub text: String,
    pub model: Option<ModelRef>,
    pub knowledge: Option<AgentKnowledgeContext>,
    /// Bounded visible EducAI turns restated into an ephemeral Knowledge
    /// session. Never raw chunks, never the full transcript, never OrdinaryChat.
    pub conversation_context: Option<String>,
}

/// Structural, local-only accounting for the prompt that crossed the agent
/// boundary. These estimates are intentionally not provider billing values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptContextTelemetry {
    pub user_prompt_est_tokens: usize,
    pub conversation_history_est_tokens: usize,
    pub knowledge_context_est_tokens: usize,
    pub system_context_est_tokens: usize,
    /// Number of bounded Knowledge evidence entries serialized into this
    /// provider request. This is distinct from raw workspace attachments.
    pub rag_attachment_count: usize,
    pub raw_attachment_count: usize,
    pub serialized_request_est_tokens: usize,
    /// Knowledge synthesis with serialized evidence uses an ephemeral OpenCode
    /// session so RAG markup cannot accumulate on the conversational transcript.
    pub fresh_session: bool,
    /// `conversational` or `ephemeral_knowledge`. Structural only.
    pub session_role: &'static str,
    /// True when this OrdinaryChat/creation turn reused the same OpenCode
    /// session id already in the cancel map.
    pub session_reused: bool,
    /// True when this conversational turn serialized Knowledge evidence and
    /// the OpenCode session was marked non-reusable for later chat.
    pub session_rotated: bool,
    /// Structural reason only, e.g. `creation_knowledge_evidence`.
    pub rotation_reason: Option<&'static str>,
    /// True when the engine conversational cache was dropped for this project.
    pub cache_invalidated: bool,
    /// Visible EducAI turns restated into this request (0 unless ephemeral Knowledge).
    pub conversation_context_messages: usize,
    pub conversation_context_chars: usize,
}

/// Bounded, provider-neutral reference material. No SQLite, embeddings, or
/// retrieval-provider details may cross this boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentKnowledgeContext {
    pub entries: Vec<AgentKnowledgeEntry>,
    pub evidence_budget_used: usize,
    pub evidence_budget_limit: usize,
    /// Local-only names of Knowledge-managed sources. The attachment provisioner
    /// uses these solely to prevent full-file duplication in the same request.
    pub indexed_source_names: Vec<String>,
    /// Local citation-ready mapping; no database lookup is needed for E1.
    pub citation_map: Vec<AgentEvidenceProvenance>,
    /// `normal` or `exhaustive`. Never a prompt body.
    pub retrieval_mode: Option<String>,
    /// `not_requested`, `complete`, or `incomplete`.
    pub exhaustive_coverage: Option<String>,
    /// Compact structural coverage statement. No document text.
    pub structural_note: Option<String>,
    /// Deterministic local answer that must not cross the remote boundary.
    pub local_answer: Option<String>,
    /// When true, the remote may state global absence. False for incomplete
    /// coverage and for inventory questions with no extracted presence term.
    pub authorize_negative: bool,
    /// Display filenames for locally appended Fuentes. Never filesystem paths.
    pub citation_source_names: Vec<String>,
    /// True when this context grounds a creation-from-material turn: the
    /// target material exists and is READY in Knowledge, and the artifact must
    /// be created from it (the empty filesystem does not mean there is no
    /// source material).
    pub creation_from_material: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentKnowledgeEntry {
    pub label: String,
    pub source_label: String,
    pub source_name: String,
    pub chunk_label: String,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub heading_path: Vec<String>,
    /// Exactly the bounded K4 excerpt.
    pub text: String,
    /// Opaque material identity for local traceability. Never a filesystem path.
    pub source_id: Option<String>,
    /// `lexical`, `semantic`, or `both`.
    pub evidence_kind: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentEvidenceProvenance {
    pub label: String,
    pub source_label: String,
    pub chunk_label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentTask {
    pub id: String,
    pub status: TaskStatus,
    pub artifacts: Vec<Artifact>,
    pub message: Option<String>,
    /// Provider telemetry observed from the completed OpenCode session. This
    /// is never inferred from prompt text or Knowledge estimates.
    pub usage: RemoteUsage,
}

/// Provider-independent accounting from the remote execution that already ran.
/// Missing fields mean the backend did not report that value; they are never
/// converted to zero.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub source: UsageSource,
}

impl Default for RemoteUsage {
    fn default() -> Self {
        Self {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: None,
            cost_usd: None,
            source: UsageSource::Unavailable,
        }
    }
}

impl RemoteUsage {
    pub fn available(&self) -> bool {
        self.source != UsageSource::Unavailable
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsageSource {
    ProviderActual,
    Estimated,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    Web,
    Document,
    Spreadsheet,
    Presentation,
    Pdf,
    Image,
    Text,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    /// Project-relative forward-slash path under `workspace/` (e.g.
    /// `workspace/actividad/index.html`).
    pub path: String,
    pub kind: ArtifactKind,
    pub byte_size: u64,
    pub sha256: Option<String>,
}

/// Infer the artifact kind from a workspace-relative path. Interactive web
/// entries are `index.html` or any `.html`/`.htm` file (the registrar stores
/// web artifacts as `index.html`, the generic publication entry).
pub fn artifact_kind_from_path(path: &str) -> ArtifactKind {
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let lower = file_name.to_ascii_lowercase();
    let ext = std::path::Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "html" | "htm" => ArtifactKind::Web,
        "docx" => ArtifactKind::Document,
        "xlsx" => ArtifactKind::Spreadsheet,
        "pptx" => ArtifactKind::Presentation,
        "pdf" => ArtifactKind::Pdf,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" => ArtifactKind::Image,
        "md" | "txt" => ArtifactKind::Text,
        _ => ArtifactKind::Other,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Stopped,
    Starting,
    Ready,
    Failed,
}
