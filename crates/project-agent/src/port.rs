use crate::AgentResult;
use crate::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};

pub trait AgentEngine: Send + Sync {
    fn ensure_ready(&self) -> AgentResult<AgentBackendInfo>;
    fn open_session(&self, project: &AgentProject) -> AgentResult<AgentSession>;
    fn send(&self, session: &AgentSession, req: &AgentPrompt) -> AgentResult<AgentTask>;
    fn cancel(&self, session: &AgentSession) -> AgentResult<()>;
    fn status(&self) -> AgentStatus;
    fn shutdown(&self) -> AgentResult<()>;
}
