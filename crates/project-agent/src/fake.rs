use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::AgentError;
use crate::AgentResult;
use crate::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask, Artifact,
    RemoteUsage, TaskStatus,
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
    files_at_open_session: Vec<String>,
    last_prompt_text: Option<String>,
    last_prompt_model: Option<crate::model::ModelRef>,
    cancelled_session_ids: Vec<String>,
    usage: RemoteUsage,
    cached_sessions: HashMap<String, String>,
    session_generations: HashMap<String, u64>,
    fresh_session_counter: u64,
    opened_session_ids: Vec<String>,
    sent_session_ids: Vec<String>,
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
                files_at_open_session: Vec::new(),
                last_prompt_text: None,
                last_prompt_model: None,
                cancelled_session_ids: Vec::new(),
                usage: RemoteUsage::default(),
                cached_sessions: HashMap::new(),
                session_generations: HashMap::new(),
                fresh_session_counter: 0,
                opened_session_ids: Vec::new(),
                sent_session_ids: Vec::new(),
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

    pub fn set_usage(&self, usage: RemoteUsage) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).usage = usage;
    }

    /// Workspace-relative file paths observed when `open_session` ran.
    pub fn files_at_open_session(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .files_at_open_session
            .clone()
    }

    pub fn last_prompt_text(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_prompt_text
            .clone()
    }

    pub fn cancelled_session_ids(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancelled_session_ids
            .clone()
    }

    pub fn last_prompt_model(&self) -> Option<crate::model::ModelRef> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_prompt_model
            .clone()
    }

    pub fn opened_session_ids(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .opened_session_ids
            .clone()
    }

    pub fn sent_session_ids(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sent_session_ids
            .clone()
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
        let files = list_files_relative(&project.directory);
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(FakeCall::OpenSession);
        state.files_at_open_session = files;
        if state.fail_session {
            state.fail_session = false;
            return Err(AgentError::SessionCreationFailed("injected".into()));
        }
        if !state.ready {
            return Err(AgentError::BackendNotReady);
        }
        let id = if let Some(cached) = state.cached_sessions.get(&project.project_id) {
            cached.clone()
        } else {
            let generation = state
                .session_generations
                .entry(project.project_id.clone())
                .or_insert(0);
            *generation += 1;
            let id = if *generation == 1 {
                format!("session-{}", project.project_id)
            } else {
                format!("session-{}-{generation}", project.project_id)
            };
            state
                .cached_sessions
                .insert(project.project_id.clone(), id.clone());
            id
        };
        state.opened_session_ids.push(id.clone());
        Ok(AgentSession {
            id,
            project_id: project.project_id.clone(),
        })
    }

    fn open_fresh_session(&self, project: &AgentProject) -> AgentResult<AgentSession> {
        let files = list_files_relative(&project.directory);
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(FakeCall::OpenSession);
        state.files_at_open_session = files;
        if state.fail_session {
            state.fail_session = false;
            return Err(AgentError::SessionCreationFailed("injected".into()));
        }
        if !state.ready {
            return Err(AgentError::BackendNotReady);
        }
        state.fresh_session_counter += 1;
        let id = format!(
            "fresh-session-{}-{}",
            project.project_id, state.fresh_session_counter
        );
        state.opened_session_ids.push(id.clone());
        Ok(AgentSession {
            id,
            project_id: project.project_id.clone(),
        })
    }

    fn invalidate_cached_session(&self, project_id: &str) {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.cached_sessions.remove(project_id);
    }

    fn send(&self, session: &AgentSession, req: &AgentPrompt) -> AgentResult<AgentTask> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(FakeCall::Send);
        state.last_prompt_text = Some(req.text.clone());
        state.last_prompt_model = req.model.clone();
        state.sent_session_ids.push(session.id.clone());
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
            usage: state.usage.clone(),
        };
        state.next_task_id += 1;
        Ok(task)
    }

    fn cancel(&self, session: &AgentSession) -> AgentResult<()> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(FakeCall::Cancel);
        state.cancelled_session_ids.push(session.id.clone());
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

fn list_files_relative(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files);
    files.sort();
    files
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_files(root, &path, out);
        } else if metadata.is_file()
            && let Ok(relative) = path.strip_prefix(root)
        {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}
