use crate::AgentResult;
use crate::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};

pub trait AgentEngine: Send + Sync {
    fn ensure_ready(&self) -> AgentResult<AgentBackendInfo>;
    fn open_session(&self, project: &AgentProject) -> AgentResult<AgentSession>;
    /// Opens an isolated session for ephemeral Knowledge evidence. Must not
    /// replace the project's conversational session cache. Engines that do not
    /// retain session history can use the ordinary path.
    fn open_fresh_session(&self, project: &AgentProject) -> AgentResult<AgentSession> {
        self.open_session(project)
    }
    /// Drops the cached conversational session for this project, if any.
    /// The next `open_session` must allocate a new conversational id.
    /// Engines without a cache may no-op.
    fn invalidate_cached_session(&self, _project_id: &str) {}
    fn send(&self, session: &AgentSession, req: &AgentPrompt) -> AgentResult<AgentTask>;
    fn cancel(&self, session: &AgentSession) -> AgentResult<()>;
    fn status(&self) -> AgentStatus;
    fn shutdown(&self) -> AgentResult<()>;
}
