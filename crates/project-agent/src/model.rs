pub struct AgentBackendInfo {
    pub version: String,
}

pub struct AgentProject {
    pub project_id: String,
    pub directory: std::path::PathBuf,
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentTask {
    pub id: String,
    pub status: TaskStatus,
    pub artifacts: Vec<Artifact>,
    pub message: Option<String>,
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
