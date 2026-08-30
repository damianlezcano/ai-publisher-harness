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
    pub text: String,
    pub model: Option<ModelRef>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Stopped,
    Starting,
    Ready,
    Failed,
}
