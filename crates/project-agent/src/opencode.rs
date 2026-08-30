//! OpenCode `serve` adapter: loopback HTTP + optional ChildGuard process.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use project_process::{ChildGuard, ProcessError};
use serde_json::{Value, json};

use crate::AgentResult;
use crate::error::AgentError;
use crate::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask, Artifact,
    ArtifactKind, TaskStatus,
};
use crate::port::AgentEngine;

const DEFAULT_STARTUP: Duration = Duration::from_secs(30);
const DEFAULT_TASK: Duration = Duration::from_secs(120);
const DEFAULT_SHUTDOWN: Duration = Duration::from_secs(5);
const HTTP_ATTEMPT: Duration = Duration::from_secs(2);

pub struct OpenCodeAgentEngine {
    binary: PathBuf,
    config_dir: PathBuf,
    port: u16,
    min: Semver,
    max_exclusive: Semver,
    version_range: String,
    extra_env: Vec<(String, String)>,
    startup_timeout: Duration,
    task_timeout: Duration,
    shutdown_timeout: Duration,
    client: reqwest::blocking::Client,
    inner: Mutex<Inner>,
}

struct Inner {
    base_url: Option<String>,
    guard: Option<ChildGuard>,
    status: AgentStatus,
    version: Option<String>,
    sessions: HashMap<String, String>,
}

impl OpenCodeAgentEngine {
    /// Production: resolve/spawn `opencode serve` on 127.0.0.1:<port> with an
    /// isolated XDG config, lazily. Version range default ">=1.18 <2".
    pub fn new(binary: PathBuf, config_dir: PathBuf, port: u16) -> Self {
        Self {
            binary,
            config_dir,
            port,
            min: Semver::parse("1.18").unwrap_or(Semver(1, 18, 0)),
            max_exclusive: Semver::parse("2").unwrap_or(Semver(2, 0, 0)),
            version_range: ">=1.18 <2".into(),
            extra_env: Vec::new(),
            startup_timeout: DEFAULT_STARTUP,
            task_timeout: DEFAULT_TASK,
            shutdown_timeout: DEFAULT_SHUTDOWN,
            client: reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(HTTP_ATTEMPT)
                .build()
                .expect("reqwest client"),
            inner: Mutex::new(Inner {
                base_url: None,
                guard: None,
                status: AgentStatus::Stopped,
                version: None,
                sessions: HashMap::new(),
            }),
        }
    }

    pub fn with_version_range(mut self, min: &str, max_exclusive: &str) -> Self {
        self.min = Semver::parse(min).unwrap_or(Semver(0, 0, 0));
        self.max_exclusive = Semver::parse(max_exclusive).unwrap_or(Semver(u64::MAX, 0, 0));
        self.version_range = format!(">={min} <{max_exclusive}");
        self
    }

    /// Test seam: use an already-known base URL (no spawn). `ensure_ready` still
    /// probes `/global/health` and checks version.
    pub fn with_base_url(self, base_url: String) -> Self {
        {
            let mut inner = lock(&self.inner);
            inner.base_url = Some(trim_slash(&base_url));
        }
        self
    }

    pub fn with_timeouts(mut self, startup: Duration, task: Duration) -> Self {
        self.startup_timeout = startup;
        self.task_timeout = task;
        self
    }

    /// Extra child environment after the isolated XDG/PATH/HOME set (tests).
    pub fn with_env(mut self, key: String, value: String) -> Self {
        self.extra_env.push((key, value));
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        lock(&self.inner)
    }

    fn probe_health(&self, base: &str) -> AgentResult<(bool, String)> {
        let url = format!("{base}/global/health");
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|err| AgentError::Http(err.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|err| AgentError::Http(err.to_string()))?;
        if !status.is_success() {
            return Err(AgentError::Http(format!("health status {status}")));
        }
        let value: Value = serde_json::from_str(&body)
            .map_err(|err| AgentError::Http(format!("malformed health JSON: {err}")))?;
        let healthy = value
            .get("healthy")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let version = value
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        Ok((healthy, version))
    }

    fn check_version(&self, found: &str) -> AgentResult<()> {
        let Some(parsed) = Semver::parse(found) else {
            return Err(AgentError::IncompatibleVersion {
                found: found.to_owned(),
                expected: self.version_range.clone(),
            });
        };
        if parsed < self.min || parsed >= self.max_exclusive {
            return Err(AgentError::IncompatibleVersion {
                found: found.to_owned(),
                expected: self.version_range.clone(),
            });
        }
        Ok(())
    }

    fn fail(&self, err: AgentError) -> AgentError {
        let mut inner = self.lock();
        if let Some(mut guard) = inner.guard.take() {
            guard.force_kill();
        }
        inner.status = AgentStatus::Failed;
        err
    }

    fn spawn_backend(&self, inner: &mut Inner) -> AgentResult<()> {
        fs::create_dir_all(&self.config_dir)
            .map_err(|err| AgentError::BackendStartFailed(err.to_string()))?;
        fs::create_dir_all(self.config_dir.join("data"))
            .map_err(|err| AgentError::BackendStartFailed(err.to_string()))?;
        fs::create_dir_all(self.config_dir.join("cache"))
            .map_err(|err| AgentError::BackendStartFailed(err.to_string()))?;
        fs::create_dir_all(self.config_dir.join("state"))
            .map_err(|err| AgentError::BackendStartFailed(err.to_string()))?;

        let argv = vec![
            "serve".into(),
            "--hostname".into(),
            "127.0.0.1".into(),
            "--port".into(),
            self.port.to_string(),
            "--pure".into(),
        ];
        let mut envs = vec![
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
            ("HOME".into(), std::env::var("HOME").unwrap_or_default()),
            (
                "XDG_CONFIG_HOME".into(),
                self.config_dir.display().to_string(),
            ),
            (
                "XDG_DATA_HOME".into(),
                self.config_dir.join("data").display().to_string(),
            ),
            (
                "XDG_CACHE_HOME".into(),
                self.config_dir.join("cache").display().to_string(),
            ),
            (
                "XDG_STATE_HOME".into(),
                self.config_dir.join("state").display().to_string(),
            ),
        ];
        envs.extend(self.extra_env.iter().cloned());

        let guard = ChildGuard::spawn(&self.binary, &argv, &envs).map_err(map_process_error)?;
        inner.guard = Some(guard);
        inner.base_url = Some(format!("http://127.0.0.1:{}", self.port));
        Ok(())
    }

    fn wait_until_healthy(&self) -> AgentResult<String> {
        let deadline = Instant::now() + self.startup_timeout;
        loop {
            let base = self.lock().base_url.clone();
            let Some(base) = base else {
                return Err(AgentError::BackendStartFailed("missing base url".into()));
            };

            {
                let mut inner = self.lock();
                if let Some(guard) = inner.guard.as_mut()
                    && let Some(status) = guard.try_wait()
                {
                    return Err(AgentError::BackendStartFailed(format!(
                        "process exited ({:?})",
                        status.code()
                    )));
                }
            }

            match self.probe_health(&base) {
                Ok((true, version)) => return Ok(version),
                Ok((false, _)) | Err(AgentError::Http(_)) => {}
                Err(err) => return Err(err),
            }

            if Instant::now() >= deadline {
                return Err(AgentError::Timeout);
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn require_ready(&self) -> AgentResult<String> {
        let inner = self.lock();
        if inner.status != AgentStatus::Ready {
            return Err(AgentError::BackendNotReady);
        }
        inner.base_url.clone().ok_or(AgentError::BackendNotReady)
    }

    fn post_json(&self, url: &str, body: &Value) -> AgentResult<(u16, String)> {
        let response = self
            .client
            .post(url)
            .json(body)
            .send()
            .map_err(|err| AgentError::Http(err.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|err| AgentError::Http(err.to_string()))?;
        Ok((status, text))
    }

    fn get_text(&self, url: &str) -> AgentResult<(u16, String)> {
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|err| AgentError::Http(err.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|err| AgentError::Http(err.to_string()))?;
        Ok((status, text))
    }

    fn poll_session(&self, base: &str, session_id: &str) -> AgentResult<(String, Option<String>)> {
        let url = format!("{base}/session/{session_id}");
        let deadline = Instant::now() + self.task_timeout;
        loop {
            let (status, body) = self.get_text(&url)?;
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

    fn fetch_artifacts(&self, base: &str, session_id: &str) -> AgentResult<Vec<Artifact>> {
        let url = format!("{base}/session/{session_id}/diff");
        let (status, body) = self.get_text(&url)?;
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
        let existing = {
            let inner = self.lock();
            inner.base_url.clone()
        };
        if let Some(base) = existing {
            match self.probe_health(&base) {
                Ok((true, version)) => {
                    self.check_version(&version).map_err(|err| self.fail(err))?;
                    let mut inner = self.lock();
                    inner.status = AgentStatus::Ready;
                    inner.version = Some(version.clone());
                    log_event("backend ready");
                    return Ok(AgentBackendInfo { version });
                }
                Ok((false, version)) => {
                    return Err(self.fail(AgentError::BackendStartFailed(format!(
                        "unhealthy ({version})"
                    ))));
                }
                Err(err) => return Err(self.fail(err)),
            }
        }

        {
            let mut inner = self.lock();
            inner.status = AgentStatus::Starting;
        }
        log_event("backend starting");
        {
            let mut inner = self.lock();
            if let Err(err) = self.spawn_backend(&mut inner) {
                inner.status = AgentStatus::Failed;
                return Err(err);
            }
        }

        match self.wait_until_healthy() {
            Ok(version) => {
                if let Err(err) = self.check_version(&version) {
                    return Err(self.fail(err));
                }
                let mut inner = self.lock();
                inner.status = AgentStatus::Ready;
                inner.version = Some(version.clone());
                log_event("backend ready");
                Ok(AgentBackendInfo { version })
            }
            Err(err) => Err(self.fail(err)),
        }
    }

    fn open_session(&self, project: &AgentProject) -> AgentResult<AgentSession> {
        let base = self.require_ready()?;
        {
            let inner = self.lock();
            if let Some(id) = inner.sessions.get(&project.project_id) {
                return Ok(AgentSession {
                    id: id.clone(),
                    project_id: project.project_id.clone(),
                });
            }
        }
        let directory = project.directory.to_string_lossy().replace('\\', "/");
        let url = format!("{base}/session");
        let body = json!({ "directory": directory });
        let (status, text) = self.post_json(&url, &body)?;
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
            let mut inner = self.lock();
            inner
                .sessions
                .insert(project.project_id.clone(), id.clone());
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
        let base = self.require_ready()?;
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
        let url = format!("{base}/session/{}/prompt_async", session.id);
        let (status, _) = self.post_json(&url, &body)?;
        if status != 204 && !(200..300).contains(&status) {
            log_event("task failed");
            return Err(AgentError::Http(format!("prompt_async status {status}")));
        }

        match self.poll_session(&base, &session.id) {
            Ok((_phase, message)) => {
                let artifacts = self.fetch_artifacts(&base, &session.id)?;
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
        let base = self.require_ready()?;
        let url = format!("{base}/session/{}/abort", session.id);
        let (status, text) = self.post_json(&url, &json!({}))?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        if self.lock().status == AgentStatus::Ready {
            let _ = text;
            return Err(AgentError::Cancelled);
        }
        Err(AgentError::Http(format!("abort status {status}")))
    }

    fn status(&self) -> AgentStatus {
        self.lock().status
    }

    fn shutdown(&self) -> AgentResult<()> {
        let mut inner = self.lock();
        if inner.status == AgentStatus::Stopped && inner.guard.is_none() {
            return Ok(());
        }
        let had_process = inner.guard.is_some();
        if let Some(mut guard) = inner.guard.take() {
            guard.request_stop();
            if guard.wait(self.shutdown_timeout).is_err() {
                guard.force_kill();
            }
        }
        if had_process {
            inner.base_url = None;
        }
        inner.status = AgentStatus::Stopped;
        inner.sessions.clear();
        log_event("backend stopped");
        Ok(())
    }
}

impl Drop for OpenCodeAgentEngine {
    fn drop(&mut self) {
        let mut inner = lock(&self.inner);
        if let Some(mut guard) = inner.guard.take() {
            guard.request_stop();
            if guard.wait(self.shutdown_timeout).is_err() {
                guard.force_kill();
            }
        }
        inner.status = AgentStatus::Stopped;
    }
}

fn lock(mutex: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn map_process_error(err: ProcessError) -> AgentError {
    match err {
        ProcessError::BinaryNotFound(name) => AgentError::BinaryNotFound(name),
        ProcessError::StartFailed(reason) => AgentError::BackendStartFailed(reason),
        ProcessError::Timeout => AgentError::Timeout,
        ProcessError::StopFailed(reason) => AgentError::ShutdownFailed(reason),
    }
}

fn log_event(event: &str) {
    eprintln!("[agent] {event}");
}

fn trim_slash(url: &str) -> String {
    url.trim_end_matches('/').to_owned()
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Semver(u64, u64, u64);

impl Semver {
    fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim().trim_start_matches(['v', 'V', '>', '=', '<']);
        let core = trimmed.split(['-', '+', ' ']).next().unwrap_or("");
        if core.is_empty() {
            return None;
        }
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let patch = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Some(Semver(major, minor, patch))
    }
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
