//! OpenCode `serve` adapter: AgentEngine over a shared OpenCodeBackend.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use project_opencode::{BackendError, BackendStatus, OpenCodeBackend};
use serde_json::{Value, json};

use crate::AgentResult;
use crate::error::AgentError;
use crate::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask, Artifact,
    ArtifactKind, TaskStatus,
};
use crate::port::AgentEngine;

const DEFAULT_TASK: Duration = Duration::from_secs(120);

pub struct OpenCodeAgentEngine {
    backend: Arc<OpenCodeBackend>,
    task_timeout: Duration,
    sessions: Mutex<HashMap<String, String>>,
}

impl OpenCodeAgentEngine {
    /// Production: resolve/spawn `opencode serve` on 127.0.0.1:<port> with an
    /// isolated XDG config, lazily. Version range default ">=1.18 <2".
    pub fn new(binary: PathBuf, config_dir: PathBuf, port: u16) -> Self {
        Self::from_backend(Arc::new(OpenCodeBackend::new(binary, config_dir, port)))
    }

    /// Share an externally owned backend (M7: one `opencode serve` for the
    /// agent engine and the provider connector).
    pub fn from_backend(backend: Arc<OpenCodeBackend>) -> Self {
        Self {
            backend,
            task_timeout: DEFAULT_TASK,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_version_range(mut self, min: &str, max_exclusive: &str) -> Self {
        self.backend_mut().set_version_range(min, max_exclusive);
        self
    }

    /// Test seam: use an already-known base URL (no spawn). `ensure_ready` still
    /// probes `/global/health` and checks version.
    pub fn with_base_url(self, base_url: String) -> Self {
        self.backend.set_base_url(base_url);
        self
    }

    pub fn with_timeouts(mut self, startup: Duration, task: Duration) -> Self {
        self.backend_mut().set_startup_timeout(startup);
        self.task_timeout = task;
        self
    }

    /// Extra child environment after the isolated XDG/PATH/HOME set (tests).
    pub fn with_env(mut self, key: String, value: String) -> Self {
        self.backend_mut().push_env(key, value);
        self
    }

    fn backend_mut(&mut self) -> &mut OpenCodeBackend {
        Arc::get_mut(&mut self.backend)
            .expect("OpenCodeBackend must be uniquely owned while configuring")
    }

    fn lock_sessions(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn require_ready(&self) -> AgentResult<String> {
        self.backend.require_ready().map_err(map_backend_error)
    }

    fn poll_session(&self, session_id: &str) -> AgentResult<(String, Option<String>)> {
        let path = format!("/session/{session_id}");
        let deadline = Instant::now() + self.task_timeout;
        loop {
            let (status, body) = self.backend.get(&path).map_err(map_backend_error)?;
            if !(200..300).contains(&status) {
                return Err(AgentError::Http(format!("session status {status}")));
            }
            let value: Value = serde_json::from_str(&body)
                .map_err(|err| AgentError::Http(format!("malformed session JSON: {err}")))?;
            let phase = session_phase(&value);
            match phase.as_str() {
                "idle" | "done" | "complete" | "completed" | "success" => {
                    return Ok((phase, last_assistant_text(&value)));
                }
                "failed" | "error" | "failure" => {
                    return Err(AgentError::TaskFailed(phase));
                }
                "aborted" | "cancelled" | "canceled" => {
                    return Err(AgentError::Cancelled);
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                return Err(AgentError::Timeout);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn fetch_artifacts(&self, session_id: &str) -> AgentResult<Vec<Artifact>> {
        let path = format!("/session/{session_id}/diff");
        let (status, body) = self.backend.get(&path).map_err(map_backend_error)?;
        if !(200..300).contains(&status) {
            return Err(AgentError::Http(format!("diff status {status}")));
        }
        let value: Value = serde_json::from_str(&body)
            .map_err(|err| AgentError::Http(format!("malformed diff JSON: {err}")))?;
        Ok(artifacts_from_diff(&value))
    }
}

impl AgentEngine for OpenCodeAgentEngine {
    fn ensure_ready(&self) -> AgentResult<AgentBackendInfo> {
        let was_ready = self.backend.status() == BackendStatus::Ready;
        let version = self.backend.ensure_ready().map_err(map_backend_error)?;
        if !was_ready {
            // A backend (re)start invalidates every cached session (M7
            // restart-on-mutation): stale ids belong to the previous process.
            self.lock_sessions().clear();
        }
        Ok(AgentBackendInfo { version })
    }

    fn open_session(&self, project: &AgentProject) -> AgentResult<AgentSession> {
        self.require_ready()?;
        {
            let sessions = self.lock_sessions();
            if let Some(id) = sessions.get(&project.project_id) {
                return Ok(AgentSession {
                    id: id.clone(),
                    project_id: project.project_id.clone(),
                });
            }
        }
        let directory = project.directory.to_string_lossy().replace('\\', "/");
        let body = json!({ "directory": directory });
        let (status, text) = self
            .backend
            .post("/session", &body)
            .map_err(map_backend_error)?;
        if !(200..300).contains(&status) {
            return Err(AgentError::SessionCreationFailed(format!(
                "status {status}"
            )));
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|err| AgentError::SessionCreationFailed(err.to_string()))?;
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::SessionCreationFailed("missing id".into()))?
            .to_owned();
        {
            let mut sessions = self.lock_sessions();
            sessions.insert(project.project_id.clone(), id.clone());
        }
        log_event(&format!(
            "session created project_id={}",
            project.project_id
        ));
        Ok(AgentSession {
            id,
            project_id: project.project_id.clone(),
        })
    }

    fn send(&self, session: &AgentSession, req: &AgentPrompt) -> AgentResult<AgentTask> {
        self.require_ready()?;
        log_event("task started");
        let mut body = json!({
            "parts": [{ "type": "text", "text": req.text }],
        });
        if let Some(model) = &req.model {
            body["model"] = json!({
                "providerID": model.provider_id,
                "modelID": model.model_id,
            });
        }
        let path = format!("/session/{}/prompt_async", session.id);
        let (status, _) = self.backend.post(&path, &body).map_err(map_backend_error)?;
        if status != 204 && !(200..300).contains(&status) {
            log_event("task failed");
            return Err(AgentError::Http(format!("prompt_async status {status}")));
        }

        match self.poll_session(&session.id) {
            Ok((_phase, message)) => {
                let artifacts = self.fetch_artifacts(&session.id)?;
                log_event("task completed");
                Ok(AgentTask {
                    id: format!("{}-task", session.id),
                    status: TaskStatus::Completed,
                    artifacts,
                    message,
                })
            }
            Err(err @ AgentError::TaskFailed(_)) => {
                log_event("task failed");
                Err(err)
            }
            Err(err) => Err(err),
        }
    }

    fn cancel(&self, session: &AgentSession) -> AgentResult<()> {
        self.require_ready()?;
        let path = format!("/session/{}/abort", session.id);
        let (status, text) = self
            .backend
            .post(&path, &json!({}))
            .map_err(map_backend_error)?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        if self.backend.status() == BackendStatus::Ready {
            let _ = text;
            return Err(AgentError::Cancelled);
        }
        Err(AgentError::Http(format!("abort status {status}")))
    }

    fn status(&self) -> AgentStatus {
        match self.backend.status() {
            BackendStatus::Stopped => AgentStatus::Stopped,
            BackendStatus::Starting => AgentStatus::Starting,
            BackendStatus::Ready => AgentStatus::Ready,
            BackendStatus::Failed => AgentStatus::Failed,
        }
    }

    fn shutdown(&self) -> AgentResult<()> {
        self.backend.shutdown().map_err(map_backend_error)?;
        self.lock_sessions().clear();
        Ok(())
    }
}

fn map_backend_error(err: BackendError) -> AgentError {
    match err {
        BackendError::NotReady => AgentError::BackendNotReady,
        BackendError::StartFailed(reason) => AgentError::BackendStartFailed(reason),
        BackendError::BinaryNotFound(name) => AgentError::BinaryNotFound(name),
        BackendError::IncompatibleVersion { found, expected } => {
            AgentError::IncompatibleVersion { found, expected }
        }
        BackendError::Timeout => AgentError::Timeout,
        BackendError::Http(reason) => AgentError::Http(reason),
        BackendError::ShutdownFailed(reason) => AgentError::ShutdownFailed(reason),
    }
}

fn log_event(event: &str) {
    eprintln!("[agent] {event}");
}

fn session_phase(value: &Value) -> String {
    if let Some(status) = value.get("status").and_then(Value::as_str) {
        return status.to_ascii_lowercase();
    }
    if let Some(status) = value
        .get("status")
        .and_then(|s| s.get("type").or_else(|| s.get("name")))
        .and_then(Value::as_str)
    {
        return status.to_ascii_lowercase();
    }
    if value
        .get("time")
        .and_then(|t| t.get("completed"))
        .is_some_and(|c| !c.is_null())
    {
        return "idle".into();
    }
    "working".into()
}

fn last_assistant_text(value: &Value) -> Option<String> {
    let messages = value.get("messages")?.as_array()?;
    let mut last = None;
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .or_else(|| {
                message
                    .get("info")
                    .and_then(|info| info.get("role"))
                    .and_then(Value::as_str)
            })
            .unwrap_or("");
        if role != "assistant" && !role.is_empty() {
            continue;
        }
        if let Some(text) = message_text(message) {
            last = Some(text);
        }
    }
    last
}

fn message_text(message: &Value) -> Option<String> {
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        return Some(text.to_owned());
    }
    let parts = message.get("parts")?.as_array()?;
    let mut chunks = Vec::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = part.get("text").and_then(Value::as_str)
        {
            chunks.push(text);
        }
    }
    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join(""))
    }
}

fn artifacts_from_diff(value: &Value) -> Vec<Artifact> {
    let entries = match value {
        Value::Array(items) => items.clone(),
        Value::Object(map) => map
            .get("files")
            .or_else(|| map.get("entries"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut artifacts = Vec::new();
    for entry in entries {
        let raw_path = entry
            .get("path")
            .or_else(|| entry.get("file"))
            .or_else(|| entry.get("filename"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let path = normalize_output_path(raw_path);
        let Some(path) = path else {
            continue;
        };
        let byte_size = entry
            .get("byte_size")
            .or_else(|| entry.get("bytes"))
            .or_else(|| entry.get("size"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let sha256 = entry
            .get("sha256")
            .or_else(|| entry.get("hash"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let kind = artifact_kind(&path);
        artifacts.push(Artifact {
            path,
            kind,
            byte_size,
            sha256,
        });
    }
    artifacts
}

fn normalize_output_path(raw: &str) -> Option<String> {
    let path = raw.replace('\\', "/");
    let path = path.trim_start_matches("./");
    let path = path.trim_start_matches('/');
    if !path.starts_with("workspace/") {
        return None;
    }
    if path.split('/').any(|seg| seg == ".." || seg.is_empty()) {
        return None;
    }
    Some(path.to_owned())
}

fn artifact_kind(path: &str) -> ArtifactKind {
    let file_name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let lower = file_name.to_ascii_lowercase();
    if lower == "index.html" {
        return ArtifactKind::Web;
    }
    let ext = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "docx" => ArtifactKind::Document,
        "xlsx" => ArtifactKind::Spreadsheet,
        "pptx" => ArtifactKind::Presentation,
        "pdf" => ArtifactKind::Pdf,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" => ArtifactKind::Image,
        "md" | "txt" => ArtifactKind::Text,
        _ => ArtifactKind::Other,
    }
}
