//! OpenCode `serve` adapter: AgentEngine over a shared OpenCodeBackend.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use project_opencode::{BackendError, BackendStatus, OpenCodeBackend, with_directory_query};
use serde_json::{Value, json};

use crate::AgentResult;
use crate::error::AgentError;
use crate::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask, Artifact,
    TaskStatus, artifact_kind_from_path,
};
use crate::port::AgentEngine;

const DEFAULT_TASK: Duration = Duration::from_secs(120);
const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(20);
const IDLE_WITHOUT_TEXT_GRACE: Duration = Duration::from_secs(2);
const ACK_WITHOUT_ARTIFACTS_GRACE: Duration = Duration::from_secs(15);
const ARTIFACT_REFRESH: Duration = Duration::from_millis(250);
const MESSAGE_LIMIT: &str = "1000";

pub struct OpenCodeAgentEngine {
    backend: Arc<OpenCodeBackend>,
    task_timeout: Duration,
    idle_without_text_grace: Duration,
    ack_without_artifacts_grace: Duration,
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
            idle_without_text_grace: IDLE_WITHOUT_TEXT_GRACE,
            ack_without_artifacts_grace: ACK_WITHOUT_ARTIFACTS_GRACE,
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

    /// Test seam: shorten idle/ack graces so poller tests stay deterministic.
    pub fn with_idle_grace(mut self, empty: Duration, ack: Duration) -> Self {
        self.idle_without_text_grace = empty;
        self.ack_without_artifacts_grace = ack;
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

    fn poll_session(
        &self,
        session_id: &str,
        before_assistant_count: usize,
    ) -> AgentResult<(String, Option<String>, Vec<Artifact>)> {
        let path = "/session/status";
        let deadline = Instant::now() + self.task_timeout;
        let mut idle_since: Option<Instant> = None;
        let mut last_artifact_fetch: Option<Instant> = None;
        let mut idle_artifacts: Vec<Artifact> = Vec::new();
        loop {
            let (status, body) = self.backend.get(path).map_err(map_backend_error)?;
            if !(200..300).contains(&status) {
                return Err(AgentError::Http(format!("session status {status}")));
            }
            let value: Value = serde_json::from_str(&body)
                .map_err(|err| AgentError::Http(format!("malformed session status JSON: {err}")))?;
            let phase = session_status_phase(&value, session_id).unwrap_or_else(|| "idle".into());
            match phase.as_str() {
                "idle" | "done" | "complete" | "completed" | "success" => {
                    let messages = self.fetch_message_list(session_id)?;
                    let originating_user_id = last_user_message_id(&messages);
                    if assistant_message_count(&messages) > before_assistant_count {
                        let message = authoritative_assistant_text(
                            &messages,
                            before_assistant_count,
                            originating_user_id.as_deref(),
                        );
                        let should_fetch = match last_artifact_fetch {
                            None => true,
                            Some(fetched) => fetched.elapsed() >= ARTIFACT_REFRESH,
                        };
                        if should_fetch {
                            // A transient /diff failure must not abort an
                            // in-progress ack wait; retry until grace expires.
                            if let Ok(artifacts) = self.fetch_artifacts(session_id) {
                                idle_artifacts = artifacts;
                            }
                            last_artifact_fetch = Some(Instant::now());
                        }
                        // A first nonempty reply (especially a brief "Listo.")
                        // is not terminal while no files exist: the sidecar
                        // often marks idle between the first text part and
                        // tool work. Debounce idle and keep polling.
                        // OpenCode 1.18.25 can return an empty status map while
                        // tool work is active. Only the final assistant message
                        // marker, `finish: "stop"`, closes this specific turn.
                        if assistant_message_is_terminal(
                            &messages,
                            before_assistant_count,
                            originating_user_id.as_deref(),
                        ) {
                            // Do not let the artifact refresh interval hide a
                            // Creation that was written just before the final
                            // message became visible.
                            if let Ok(artifacts) = self.fetch_artifacts(session_id) {
                                return Ok((phase, message, artifacts));
                            }
                        }
                        let grace = self.ack_without_artifacts_grace;
                        match idle_since {
                            None => idle_since = Some(Instant::now()),
                            Some(started) if started.elapsed() >= grace => {
                                if let Ok(artifacts) = self.fetch_artifacts(session_id) {
                                    idle_artifacts = artifacts;
                                }
                                return Ok((phase, message, idle_artifacts));
                            }
                            Some(_) => {}
                        }
                    }
                    // The sidecar may have marked idle before the new assistant
                    // message is visible; keep polling until a new one appears.
                }
                "failed" | "error" | "failure" => {
                    let messages = self.fetch_message_list(session_id)?;
                    let originating_user_id = last_user_message_id(&messages);
                    let text = if assistant_message_count(&messages) > before_assistant_count {
                        authoritative_assistant_text(
                            &messages,
                            before_assistant_count,
                            originating_user_id.as_deref(),
                        )
                    } else {
                        None
                    };
                    return Err(AgentError::TaskFailed(
                        text.unwrap_or_else(|| "task failed".into()),
                    ));
                }
                "aborted" | "cancelled" | "canceled" => {
                    return Err(AgentError::Cancelled);
                }
                _ => {
                    idle_since = None;
                    last_artifact_fetch = None;
                }
            }
            if Instant::now() >= deadline {
                return Err(AgentError::Timeout);
            }
            thread::sleep(STATUS_POLL_INTERVAL);
        }
    }

    fn fetch_message_list(&self, session_id: &str) -> AgentResult<Vec<Value>> {
        let path = format!("/session/{session_id}/message?limit={MESSAGE_LIMIT}");
        let (status, body) = self.backend.get(&path).map_err(map_backend_error)?;
        if !(200..300).contains(&status) {
            return Err(AgentError::Http(format!("message list status {status}")));
        }
        let value: Value = serde_json::from_str(&body)
            .map_err(|err| AgentError::Http(format!("malformed message list JSON: {err}")))?;
        Ok(match value {
            Value::Array(items) => items,
            Value::Object(mut map) => map
                .remove("data")
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default(),
            _ => Vec::new(),
        })
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
        // OpenCode 1.18.25 reads the working directory from `?directory=`.
        // Unknown JSON body fields are ignored (not 400); `permission` is a
        // documented create-session field and is accepted.
        let path = with_directory_query("/session", &directory);
        let body = json!({
            "permission": [{
                "permission": "external_directory",
                "pattern": "*",
                "action": "deny",
            }],
        });
        let (status, text) = self.backend.post(&path, &body).map_err(map_backend_error)?;
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
        let before_messages = self.fetch_message_list(&session.id)?;
        let before_assistant_count = assistant_message_count(&before_messages);
        let path = format!("/session/{}/prompt_async", session.id);
        let (status, _) = self.backend.post(&path, &body).map_err(map_backend_error)?;
        if status != 204 && !(200..300).contains(&status) {
            log_event("task failed");
            return Err(AgentError::Http(format!("prompt_async status {status}")));
        }

        match self.poll_session(&session.id, before_assistant_count) {
            Ok((_phase, message, artifacts)) => {
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
            Err(AgentError::Timeout) => {
                // A task that started but never completed is not a backend
                // startup failure; keep Timeout for ensure_ready only.
                log_event("task failed");
                Err(AgentError::TaskFailed("timed out".into()))
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

fn session_status_phase(value: &Value, session_id: &str) -> Option<String> {
    value
        .get(session_id)
        .and_then(|entry| entry.get("type"))
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase())
}

/// Short acknowledgements that are not a completed turn when no files exist yet.
/// Generic (not product-specific): a lone "Listo." must not end a creation run.
fn assistant_message_is_terminal(
    messages: &[Value],
    before_assistant_count: usize,
    originating_user_id: Option<&str>,
) -> bool {
    let mut assistant_index = 0;
    let mut last = None;
    for message in messages {
        if message_role(message) != "assistant" {
            continue;
        }
        if assistant_index >= before_assistant_count
            && message_belongs_to_turn(message, originating_user_id)
        {
            last = Some(message);
        }
        assistant_index += 1;
    }
    last.and_then(assistant_finish) == Some("stop")
}

fn message_role(message: &Value) -> &str {
    message
        .get("role")
        .and_then(Value::as_str)
        .or_else(|| {
            message
                .get("info")
                .and_then(|info| info.get("role"))
                .and_then(Value::as_str)
        })
        .unwrap_or("")
}

fn last_user_message_id(messages: &[Value]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message_role(message) == "user")
        .and_then(message_id)
        .map(str::to_owned)
}

fn authoritative_assistant_text(
    messages: &[Value],
    before_assistant_count: usize,
    originating_user_id: Option<&str>,
) -> Option<String> {
    let mut assistant_index = 0;
    let mut last = None;
    for message in messages {
        if message_role(message) == "assistant" {
            if assistant_index >= before_assistant_count
                && message_belongs_to_turn(message, originating_user_id)
                && assistant_finish(message) == Some("stop")
            {
                last = Some(message);
            }
            assistant_index += 1;
        }
    }
    last.and_then(message_text)
        .filter(|text| !text.trim().is_empty())
}

fn message_id(message: &Value) -> Option<&str> {
    message.get("id").and_then(Value::as_str).or_else(|| {
        message
            .get("info")
            .and_then(|info| info.get("id"))
            .and_then(Value::as_str)
    })
}

fn parent_message_id(message: &Value) -> Option<&str> {
    message
        .get("parentID")
        .and_then(Value::as_str)
        .or_else(|| message.get("parentId").and_then(Value::as_str))
        .or_else(|| {
            message
                .get("info")
                .and_then(|info| info.get("parentID"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            message
                .get("info")
                .and_then(|info| info.get("parentId"))
                .and_then(Value::as_str)
        })
}

fn message_belongs_to_turn(message: &Value, originating_user_id: Option<&str>) -> bool {
    match (originating_user_id, parent_message_id(message)) {
        (Some(expected), Some(actual)) => expected == actual,
        // Some test doubles and older responses omit parentID. The assistant
        // watermark still proves that such a message was created by this turn.
        (_, None) => true,
        (None, Some(_)) => true,
    }
}

fn assistant_finish(message: &Value) -> Option<&str> {
    message
        .get("info")
        .and_then(|info| info.get("finish"))
        .and_then(Value::as_str)
        .or_else(|| message.get("finish").and_then(Value::as_str))
}

fn assistant_message_count(messages: &[Value]) -> usize {
    messages
        .iter()
        .filter(|message| {
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
            role == "assistant"
        })
        .count()
}

fn message_text(message: &Value) -> Option<String> {
    let parts = message.get("parts").and_then(Value::as_array);
    let mut chunks = Vec::new();
    for part in parts.into_iter().flatten() {
        if part.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = part.get("text").and_then(Value::as_str)
            && !text.trim().is_empty()
        {
            chunks.push(text);
        }
    }
    if !chunks.is_empty() {
        return Some(chunks.join(""));
    }
    message
        .get("content")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
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
        let kind = artifact_kind_from_path(&path);
        artifacts.push(Artifact {
            path,
            kind,
            byte_size,
            sha256,
        });
    }
    artifacts
}

/// Normalize a session-diff path into the internal `workspace/...` contract.
///
/// The OpenCode session directory *is* the project `workspace/`, so real diffs
/// return session-relative paths (`index.html`, `juego/app.js`) rather than
/// `workspace/index.html`. Both forms are accepted. Project-root trees
/// (`inputs/`, `outputs/`, `publish/`) and metadata are rejected.
fn normalize_output_path(raw: &str) -> Option<String> {
    let path = raw.replace('\\', "/");
    let path = path.trim_start_matches("file://");
    if path.is_empty() {
        return None;
    }
    // Absolute paths are only accepted when they include the session
    // `workspace/` segment; `/etc/passwd` must never become a creation.
    if path.starts_with('/') {
        let idx = path.find("/workspace/")?;
        return validate_workspace_artifact_path(&path[idx + 1..]);
    }
    let path = path.trim_start_matches("./");
    let relative = if path.starts_with("workspace/") {
        path.to_owned()
    } else if is_project_root_path(path) {
        return None;
    } else {
        format!("workspace/{path}")
    };
    validate_workspace_artifact_path(&relative)
}

fn is_project_root_path(path: &str) -> bool {
    path == "project.json"
        || path.starts_with("inputs/")
        || path.starts_with("outputs/")
        || path.starts_with("publish/")
        || path == "inputs"
        || path == "outputs"
        || path == "publish"
}

fn validate_workspace_artifact_path(path: &str) -> Option<String> {
    if !path.starts_with("workspace/") {
        return None;
    }
    let rest = &path["workspace/".len()..];
    if rest.is_empty()
        || rest
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
        || rest == "materials"
        || rest.starts_with("materials/")
    {
        return None;
    }
    Some(path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{assistant_message_is_terminal, authoritative_assistant_text, message_text};
    use serde_json::json;

    #[test]
    fn empty_content_falls_through_to_parts() {
        let message = json!({
            "content": "",
            "parts": [{"type": "text", "text": "hola desde parts"}]
        });
        assert_eq!(message_text(&message).as_deref(), Some("hola desde parts"));
    }

    #[test]
    fn human_text_parts_win_over_mixed_content() {
        let message = json!({
            "content": "desde content",
            "parts": [{"type": "text", "text": "desde parts"}]
        });
        assert_eq!(message_text(&message).as_deref(), Some("desde parts"));
    }

    #[test]
    fn only_stop_finish_marks_the_latest_assistant_message_terminal() {
        let in_progress = serde_json::json!([
            {"info":{"role":"assistant","finish":"tool-calls"}}
        ]);
        let complete = serde_json::json!([
            {"info":{"role":"assistant","finish":"stop"}}
        ]);
        assert!(!assistant_message_is_terminal(
            in_progress.as_array().unwrap(),
            0,
            None,
        ));
        assert!(assistant_message_is_terminal(
            complete.as_array().unwrap(),
            0,
            None,
        ));
    }

    #[test]
    fn final_text_requires_new_linked_stop_message() {
        let messages = serde_json::json!([
            {"info":{"id":"user-1","role":"user"}},
            {"info":{"id":"old","role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"vieja"}]},
            {"info":{"id":"wrong","role":"assistant","parentID":"other","finish":"stop"},"parts":[{"type":"text","text":"ajena"}]},
            {"info":{"id":"final","role":"assistant","parentID":"user-1","finish":"stop"},"parts":[{"type":"reasoning","text":"oculto"},{"type":"text","text":"final"}]}
        ]);
        assert_eq!(
            authoritative_assistant_text(messages.as_array().unwrap(), 1, Some("user-1"))
                .as_deref(),
            Some("final")
        );
    }
}
