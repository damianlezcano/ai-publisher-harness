use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::AgentResult;
use crate::error::AgentError;
use crate::model::{AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask};
use crate::port::AgentEngine;
use crate::registrar::CreationRegistrar;

pub struct AgentRequest {
    pub project_id: String,
    pub prompt: AgentPrompt,
}

pub struct AgentRunResult {
    pub task: AgentTask,
    pub registered: Vec<String>,
}

pub struct AgentService<E: AgentEngine, R: CreationRegistrar> {
    engine: E,
    registrar: R,
    projects_base: PathBuf,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    sessions: Mutex<HashMap<String, AgentSession>>,
}

impl<E: AgentEngine, R: CreationRegistrar> AgentService<E, R> {
    pub fn new(engine: E, registrar: R, projects_base: PathBuf) -> Self {
        Self {
            engine,
            registrar,
            projects_base,
            locks: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Serialized per project. ensure_ready -> open_session(workspace dir) -> send -> register artifacts.
    pub fn run(&self, request: AgentRequest) -> AgentResult<AgentRunResult> {
        let lock = self.project_lock(&request.project_id);
        let _serialized = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        match self.engine.ensure_ready() {
            Ok(_) | Err(AgentError::BackendAlreadyReady) => {}
            Err(err) => return Err(err),
        }

        let workspace_dir = self
            .projects_base
            .join("projects")
            .join(&request.project_id)
            .join("workspace");
        fs::create_dir_all(&workspace_dir)
            .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;

        let session = self.engine.open_session(&AgentProject {
            project_id: request.project_id.clone(),
            directory: workspace_dir.clone(),
        })?;
        {
            let mut sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            sessions.insert(
                request.project_id.clone(),
                AgentSession {
                    id: session.id.clone(),
                    project_id: session.project_id.clone(),
                },
            );
        }

        let task = self.engine.send(&session, &request.prompt)?;
        if task.status != crate::model::TaskStatus::Completed {
            return Err(AgentError::TaskFailed("task did not complete".into()));
        }

        let mut registered = Vec::new();
        for artifact in &task.artifacts {
            let bytes = read_workspace_artifact(&workspace_dir, &artifact.path)?;
            let id = self
                .registrar
                .register(&request.project_id, artifact, bytes)?;
            registered.push(id);
        }
        Ok(AgentRunResult { task, registered })
    }

    pub fn cancel(&self, project_id: &str) -> AgentResult<()> {
        let session = {
            let sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            sessions.get(project_id).map(|session| AgentSession {
                id: session.id.clone(),
                project_id: session.project_id.clone(),
            })
        };
        let Some(session) = session else {
            return Err(AgentError::SessionNotFound(project_id.to_owned()));
        };
        self.engine.cancel(&session)
    }

    pub fn engine_status(&self) -> AgentStatus {
        self.engine.status()
    }

    pub fn shutdown(&self) -> AgentResult<()> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.engine.shutdown()
    }

    fn project_lock(&self, project_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self
            .locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locks
            .entry(project_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

/// Read `workspace_dir/<path>` after stripping a leading `workspace/` prefix.
///
/// Traversal (`..`), empty segments, absolute paths, and symlink escapes are
/// rejected with `RegistrationFailed` (the artifact is not registered).
fn read_workspace_artifact(workspace_dir: &Path, artifact_path: &str) -> AgentResult<Vec<u8>> {
    let normalized = artifact_path.replace('\\', "/");
    let relative = normalized.trim_start_matches('/');
    let relative = relative.strip_prefix("workspace/").unwrap_or(relative);
    if relative.is_empty()
        || Path::new(relative).is_absolute()
        || relative
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(AgentError::RegistrationFailed(format!(
            "unsafe artifact path: {artifact_path}"
        )));
    }
    let candidate = workspace_dir.join(relative);
    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Err(AgentError::RegistrationFailed(
            "symlink artifact path".into(),
        ));
    }
    let workspace_canon = workspace_dir
        .canonicalize()
        .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
    let file_canon = candidate
        .canonicalize()
        .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
    if !file_canon.starts_with(&workspace_canon) {
        return Err(AgentError::RegistrationFailed(
            "artifact path escapes workspace".into(),
        ));
    }
    fs::read(&candidate).map_err(|err| AgentError::RegistrationFailed(err.to_string()))
}
