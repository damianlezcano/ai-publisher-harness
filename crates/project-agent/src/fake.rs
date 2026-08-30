use std::sync::{Arc, Mutex};

use crate::AgentError;
use crate::AgentResult;
use crate::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask, Artifact,
    TaskStatus,
};
use crate::port::AgentEngine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FakeCall {
    Ready,
    OpenSession,
    Send,
    Cancel,
    Shutdown,
}

#[derive(Clone, Debug)]
pub struct FakeAgentEngine {
    inner: Arc<Mutex<FakeAgentState>>,
}

#[derive(Debug)]
struct FakeAgentState {
    ready: bool,
    calls: Vec<FakeCall>,
    fail_ready: bool,
    fail_session: bool,
    fail_send: bool,
    artifacts: Vec<Artifact>,
    message: Option<String>,
    next_task_id: u64,
}

impl Default for FakeAgentEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeAgentEngine {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakeAgentState {
                ready: false,
                calls: Vec::new(),
                fail_ready: false,
                fail_session: false,
                fail_send: false,
                artifacts: Vec::new(),
                message: None,
                next_task_id: 1,
            })),
        }
    }

    pub fn calls(&self) -> Vec<FakeCall> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .calls
            .clone()
    }

    pub fn fail_ready(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail_ready = true;
    }

    pub fn fail_session(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail_session = true;
    }

    pub fn fail_send(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail_send = true;
    }

    pub fn set_artifacts(&self, artifacts: Vec<Artifact>) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .artifacts = artifacts;
    }

    pub fn set_message(&self, message: String) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).message = Some(message);
    }
}

impl AgentEngine for FakeAgentEngine {
    fn ensure_ready(&self) -> AgentResult<AgentBackendInfo> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(FakeCall::Ready);
        if state.fail_ready {
            state.fail_ready = false;
            return Err(AgentError::BackendStartFailed("injected".into()));
        }
        if state.ready {
            return Err(AgentError::BackendAlreadyReady);
        }
        state.ready = true;
        Ok(AgentBackendInfo {
            version: "fake".into(),
        })
    }

    fn open_session(&self, project: &AgentProject) -> AgentResult<AgentSession> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(FakeCall::OpenSession);
        if state.fail_session {
            state.fail_session = false;
            return Err(AgentError::SessionCreationFailed("injected".into()));
        }
        if !state.ready {
            return Err(AgentError::BackendNotReady);
        }
        Ok(AgentSession {
            id: format!("session-{}", project.project_id),
            project_id: project.project_id.clone(),
        })
    }

    fn send(&self, _session: &AgentSession, _req: &AgentPrompt) -> AgentResult<AgentTask> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(FakeCall::Send);
        if state.fail_send {
            state.fail_send = false;
            return Err(AgentError::TaskFailed("injected".into()));
        }
        if !state.ready {
            return Err(AgentError::BackendNotReady);
        }
        let task = AgentTask {
            id: format!("task-{}", state.next_task_id),
            status: TaskStatus::Completed,
            artifacts: state.artifacts.clone(),
            message: state.message.clone(),
        };
        state.next_task_id += 1;
        Ok(task)
    }

    fn cancel(&self, session: &AgentSession) -> AgentResult<()> {
        let _ = session;
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(FakeCall::Cancel);
        if !state.ready {
            return Err(AgentError::BackendNotReady);
        }
        Ok(())
    }

    fn status(&self) -> AgentStatus {
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if state.ready {
            AgentStatus::Ready
        } else {
            AgentStatus::Stopped
        }
    }

    fn shutdown(&self) -> AgentResult<()> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(FakeCall::Shutdown);
        state.ready = false;
        Ok(())
    }
}
