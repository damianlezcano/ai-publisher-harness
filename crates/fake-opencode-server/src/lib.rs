//! In-process fake OpenCode HTTP server for offline adapter tests.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

pub struct FakeServer {
    base_url: String,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    script: Arc<Mutex<Script>>,
}

pub struct Script {
    pub health_status: u16,
    pub health_body: String,
    pub health_delay: Duration,
    pub session_status: u16,
    pub session_body: String,
    pub last_directory: Option<String>,
    pub prompt_status: u16,
    pub prompt_delay: Duration,
    pub prompt_called: bool,
    pub status_sequence: Vec<String>,
    pub status_index: usize,
    pub status_delay: Duration,
    pub status_body_override: Option<String>,
    pub diff_status: u16,
    pub diff_body: String,
    pub abort_status: u16,
    pub abort_called: bool,
    pub integrations: Vec<Value>,
    pub integrations_raw: Option<String>,
    pub models: Vec<Value>,
    pub models_raw: Option<String>,
    pub models_empty_requests: usize,
    pub providers_config: Value,
    pub connect_key_status: u16,
    pub last_connect: Option<CapturedConnect>,
    pub oauth_attempts: HashMap<String, Value>,
    pub oauth_begin_status: u16,
    pub oauth_complete_status: u16,
    pub next_attempt: u64,
    pub next_credential: u64,
    pub credentials: HashSet<String>,
}

/// Last `POST /api/integration/{id}/connect/key` body, with a redacted `Debug`.
#[derive(Clone)]
pub struct CapturedConnect {
    pub provider_id: String,
    key: String,
    pub label: Option<String>,
}

impl CapturedConnect {
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl fmt::Debug for CapturedConnect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapturedConnect")
            .field("provider_id", &self.provider_id)
            .field("key", &"[REDACTED]")
            .field("label", &self.label)
            .finish()
    }
}

impl fmt::Debug for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Script")
            .field("health_status", &self.health_status)
            .field("session_status", &self.session_status)
            .field("prompt_status", &self.prompt_status)
            .field("prompt_called", &self.prompt_called)
            .field("abort_called", &self.abort_called)
            .field("last_connect", &self.last_connect)
            .field("connect_key_status", &self.connect_key_status)
            .finish_non_exhaustive()
    }
}

fn default_integrations() -> Vec<Value> {
    vec![
        json!({
            "id": "openai",
            "name": "OpenAI",
            "methods": [
                {"type": "key", "label": "API key"},
                {
                    "type": "oauth",
                    "id": "chatgpt-browser",
                    "label": "ChatGPT Pro/Plus (browser)",
                    "prompts": [{
                        "key": "url",
                        "message": "URL de tu organización",
                        "kind": "text",
                        "placeholder": "https://...",
                        "optional": true
                    }]
                },
                {"type": "env"}
            ],
            "connections": []
        }),
        json!({
            "id": "google",
            "name": "Google",
            "methods": [{"type": "key"}, {"type": "env"}],
            "connections": []
        }),
        json!({
            "id": "opencode",
            "name": "OpenCode Zen",
            "methods": [{"type": "key"}],
            "connections": []
        }),
    ]
}

fn default_models() -> Vec<Value> {
    vec![
        json!({
            "id": "gpt-4o",
            "providerID": "openai",
            "family": "gpt",
            "name": "GPT-4o",
            "cost": 1,
            "status": "enabled",
            "enabled": true
        }),
        json!({
            "id": "gpt-old",
            "providerID": "openai",
            "family": "gpt",
            "name": "GPT Old",
            "cost": 1,
            "status": "deprecated",
            "enabled": true
        }),
        json!({
            "id": "hidden",
            "providerID": "openai",
            "name": "Hidden",
            "cost": 1,
            "status": "disabled",
            "enabled": false
        }),
        json!({
            "id": "big-pickle",
            "providerID": "opencode",
            "family": "opencode",
            "name": "Big Pickle",
            "cost": 0,
            "status": "enabled",
            "enabled": true
        }),
        json!({
            "id": "free-2",
            "providerID": "opencode",
            "name": "Free 2",
            "cost": "0",
            "status": "enabled",
            "enabled": true
        }),
        // Mirrors the pinned opencode 1.18.25 catalog shape: `cost` is an array
        // of per-tier objects (see project-provider `cost_is_zero`).
        json!({
            "id": "array-free",
            "providerID": "opencode",
            "name": "Array Free",
            "cost": [{"input": 0, "output": 0, "cache": {"read": 0, "write": 0}}],
            "status": "enabled",
            "enabled": true
        }),
        json!({
            "id": "array-paid",
            "providerID": "opencode",
            "name": "Array Paid",
            "cost": [{"input": 2, "output": 2, "cache": {"read": 1, "write": 1}}],
            "status": "enabled",
            "enabled": true
        }),
    ]
}

impl Default for Script {
    fn default() -> Self {
        Self {
            health_status: 200,
            health_body: r#"{"healthy":true,"version":"1.18.25"}"#.into(),
            health_delay: Duration::ZERO,
            session_status: 200,
            session_body: r#"{"id":"ses-1"}"#.into(),
            last_directory: None,
            prompt_status: 204,
            prompt_delay: Duration::ZERO,
            prompt_called: false,
            status_sequence: vec!["idle".into()],
            status_index: 0,
            status_delay: Duration::ZERO,
            status_body_override: None,
            diff_status: 200,
            diff_body: "[]".into(),
            abort_status: 204,
            abort_called: false,
            integrations: default_integrations(),
            integrations_raw: None,
            models: default_models(),
            models_raw: None,
            models_empty_requests: 0,
            providers_config: json!({
                "providers": [],
                "default": {"opencode": "big-pickle"}
            }),
            connect_key_status: 204,
            last_connect: None,
            oauth_attempts: HashMap::new(),
            oauth_begin_status: 200,
            oauth_complete_status: 204,
            next_attempt: 1,
            next_credential: 1,
            credentials: HashSet::new(),
        }
    }
}

impl FakeServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake OpenCode server");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://127.0.0.1:{}", addr.port());
        let shutdown = Arc::new(AtomicBool::new(false));
        let script = Arc::new(Mutex::new(Script::default()));
        let flag = Arc::clone(&shutdown);
        let state = Arc::clone(&script);
        let thread = thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => handle_client(stream, &state),
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            base_url,
            shutdown,
            thread: Some(thread),
            script,
        }
    }

    pub fn base_url(&self) -> String {
        self.base_url.clone()
    }

    pub fn script(&self) -> std::sync::MutexGuard<'_, Script> {
        self.script
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_health_version(&self, version: &str) {
        self.script().health_body = format!(r#"{{"healthy":true,"version":"{version}"}}"#);
    }

    pub fn set_session_id(&self, id: &str) {
        self.script().session_body = format!(r#"{{"id":"{id}"}}"#);
    }

    pub fn fail_session(&self) {
        let mut script = self.script();
        script.session_status = 500;
        script.session_body = r#"{"error":"nope"}"#.into();
    }

    pub fn set_status_sequence(&self, phases: &[&str]) {
        let mut script = self.script();
        script.status_sequence = phases.iter().map(|s| (*s).to_owned()).collect();
        script.status_index = 0;
        script.status_body_override = None;
    }

    pub fn set_malformed_session(&self) {
        self.script().status_body_override = Some("not-json".into());
    }

    pub fn set_diff_body(&self, body: &str) {
        self.script().diff_body = body.to_owned();
    }

    pub fn set_status_delay(&self, delay: Duration) {
        self.script().status_delay = delay;
    }

    pub fn last_directory(&self) -> Option<String> {
        self.script().last_directory.clone()
    }

    pub fn abort_called(&self) -> bool {
        self.script().abort_called
    }

    pub fn prompt_called(&self) -> bool {
        self.script().prompt_called
    }

    pub fn set_prompt_status(&self, status: u16) {
        self.script().prompt_status = status;
    }

    pub fn set_session_poll_body(&self, body: &str) {
        self.script().status_body_override = Some(body.to_owned());
    }

    pub fn set_integrations(&self, integrations: Vec<Value>) {
        self.script().integrations = integrations;
    }

    pub fn set_malformed_integrations(&self) {
        self.script().integrations_raw = Some("not-json".into());
    }

    pub fn set_models(&self, models: Vec<Value>) {
        self.script().models = models;
    }

    /// Make the first `n` `/api/model` responses an empty `{"data": []}` to
    /// simulate the pinned sidecar's cold-start catalog-loading window.
    pub fn set_models_empty_first(&self, n: usize) {
        self.script().models_empty_requests = n;
    }

    pub fn set_malformed_models(&self) {
        self.script().models_raw = Some("not-json".into());
    }

    pub fn set_providers_config(&self, config: Value) {
        self.script().providers_config = config;
    }

    pub fn set_connect_key_status(&self, status: u16) {
        self.script().connect_key_status = status;
    }

    pub fn last_connect_key(&self) -> Option<String> {
        self.script()
            .last_connect
            .as_ref()
            .map(|c| c.key().to_owned())
    }

    pub fn last_connect_label(&self) -> Option<String> {
        self.script()
            .last_connect
            .as_ref()
            .and_then(|c| c.label.clone())
    }

    pub fn set_oauth_status(&self, attempt_id: &str, status: &str) {
        if let Some(attempt) = self.script().oauth_attempts.get_mut(attempt_id) {
            attempt["status"] = json!(status);
        }
    }

    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn enveloped(array_bytes: &[u8]) -> Vec<u8> {
    // Mirror the pinned opencode 1.18.25 list-endpoint shape: `{"location":
    // {...}, "data": [...]}` around a bare array.
    let items: Value = serde_json::from_slice(array_bytes).unwrap_or(Value::Array(Vec::new()));
    serde_json::to_vec(&json!({
        "location": { "directory": "/tmp/fake-opencode-server" },
        "data": items,
    }))
    .unwrap_or_else(|_| b"[]".to_vec())
}

fn handle_client(mut stream: TcpStream, script: &Arc<Mutex<Script>>) {
    let Some((method, path, body)) = read_request(&mut stream) else {
        return;
    };
    let mut state = script
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if method == "GET" && path == "/global/health" {
        let delay = state.health_delay;
        let status = state.health_status;
        let body = state.health_body.clone();
        drop(state);
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        write_response(&mut stream, status, body.as_bytes());
        return;
    }

    if method == "GET" && path == "/api/integration" {
        if let Some(raw) = state.integrations_raw.clone() {
            drop(state);
            write_response(&mut stream, 200, raw.as_bytes());
            return;
        }
        let body = serde_json::to_vec(&state.integrations).unwrap_or_else(|_| b"[]".to_vec());
        let body = enveloped(&body);
        drop(state);
        write_response(&mut stream, 200, &body);
        return;
    }

    if method == "GET" && path == "/api/model" {
        if let Some(raw) = state.models_raw.clone() {
            drop(state);
            write_response(&mut stream, 200, raw.as_bytes());
            return;
        }
        // Simulate the pinned sidecar's cold-start window: the health endpoint
        // is up before the model catalog finishes loading, so the first N
        // `/api/model` responses are an empty `{"data": []}`.
        let body = if state.models_empty_requests > 0 {
            state.models_empty_requests -= 1;
            let body = serde_json::to_vec(&Vec::<Value>::new()).unwrap_or_else(|_| b"[]".to_vec());
            enveloped(&body)
        } else {
            let body = serde_json::to_vec(&state.models).unwrap_or_else(|_| b"[]".to_vec());
            enveloped(&body)
        };
        drop(state);
        write_response(&mut stream, 200, &body);
        return;
    }

    if method == "GET" && path == "/config/providers" {
        let body = serde_json::to_vec(&state.providers_config).unwrap_or_else(|_| b"{}".to_vec());
        drop(state);
        write_response(&mut stream, 200, &body);
        return;
    }

    if method == "POST"
        && let Some(provider_id) = path
            .strip_prefix("/api/integration/")
            .and_then(|rest| rest.strip_suffix("/connect/key"))
    {
        let parsed = serde_json::from_slice::<Value>(&body).ok();
        let key = parsed
            .as_ref()
            .and_then(|v| v.get("key"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let label = parsed
            .as_ref()
            .and_then(|v| v.get("label"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        state.last_connect = Some(CapturedConnect {
            provider_id: provider_id.to_owned(),
            key,
            label: label.clone(),
        });
        let status = state.connect_key_status;
        if (200..300).contains(&status) && !append_connection(&mut state, provider_id, label) {
            drop(state);
            write_response(&mut stream, 404, b"{\"error\":\"not found\"}");
            return;
        }
        drop(state);
        write_response(&mut stream, status, b"");
        return;
    }

    if method == "POST"
        && let Some(provider_id) = path
            .strip_prefix("/api/integration/")
            .and_then(|rest| rest.strip_suffix("/connect/oauth"))
    {
        let exists = state.integrations.iter().any(|p| {
            p.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == provider_id)
        });
        if !exists {
            drop(state);
            write_response(&mut stream, 404, b"{\"error\":\"not found\"}");
            return;
        }
        let status = state.oauth_begin_status;
        if !(200..300).contains(&status) {
            drop(state);
            write_response(&mut stream, status, b"");
            return;
        }
        let parsed = serde_json::from_slice::<Value>(&body).ok();
        let method_id = parsed
            .as_ref()
            .and_then(|v| v.get("methodID"))
            .and_then(Value::as_str)
            .unwrap_or("oauth");
        let id = format!("att-{}", state.next_attempt);
        state.next_attempt += 1;
        let attempt = json!({
            "attemptID": id,
            "url": format!("https://example.test/oauth/{method_id}"),
            "instructions": "Abrí el enlace y aprobá el acceso.",
            "mode": "auto",
            "time": 0,
            "status": "pending",
            "providerID": provider_id,
        });
        state.oauth_attempts.insert(id.clone(), attempt.clone());
        let mut response = attempt.clone();
        if let Some(obj) = response.as_object_mut() {
            obj.remove("providerID");
            obj.remove("status");
        }
        let body = serde_json::to_vec(&response).unwrap_or_default();
        drop(state);
        write_response(&mut stream, 200, &body);
        return;
    }

    if method == "POST"
        && let Some(attempt_id) = path
            .strip_prefix("/api/integration/attempt/")
            .and_then(|rest| rest.strip_suffix("/complete"))
    {
        let status = state.oauth_complete_status;
        let Some(attempt) = state.oauth_attempts.get(attempt_id).cloned() else {
            drop(state);
            write_response(&mut stream, 404, b"{\"error\":\"not found\"}");
            return;
        };
        if (200..300).contains(&status) {
            if let Some(entry) = state.oauth_attempts.get_mut(attempt_id) {
                entry["status"] = json!("complete");
            }
            let provider_id = attempt
                .get("providerID")
                .and_then(Value::as_str)
                .unwrap_or("");
            let _ = append_connection(&mut state, provider_id, Some("Cuenta conectada".into()));
        }
        drop(state);
        write_response(&mut stream, status, b"");
        return;
    }

    if method == "GET"
        && let Some(attempt_id) = path.strip_prefix("/api/integration/attempt/")
        && !attempt_id.contains('/')
    {
        let Some(attempt) = state.oauth_attempts.get(attempt_id) else {
            drop(state);
            write_response(&mut stream, 404, b"{\"error\":\"not found\"}");
            return;
        };
        let status = attempt
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("pending");
        let body = json!({ "status": status });
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        drop(state);
        write_response(&mut stream, 200, &bytes);
        return;
    }

    if method == "DELETE"
        && let Some(attempt_id) = path.strip_prefix("/api/integration/attempt/")
        && !attempt_id.contains('/')
    {
        let removed = state.oauth_attempts.remove(attempt_id).is_some();
        drop(state);
        if removed {
            write_response(&mut stream, 204, b"");
        } else {
            write_response(&mut stream, 404, b"{\"error\":\"not found\"}");
        }
        return;
    }

    if method == "DELETE"
        && let Some(credential_id) = path.strip_prefix("/api/credential/")
        && !credential_id.contains('/')
    {
        let removed = state.credentials.remove(credential_id);
        if removed {
            for provider in &mut state.integrations {
                if let Some(connections) = provider
                    .get_mut("connections")
                    .and_then(Value::as_array_mut)
                {
                    connections.retain(|c| {
                        c.get("id")
                            .and_then(Value::as_str)
                            .is_none_or(|id| id != credential_id)
                    });
                }
            }
            drop(state);
            write_response(&mut stream, 204, b"");
        } else {
            drop(state);
            write_response(&mut stream, 404, b"{\"error\":\"not found\"}");
        }
        return;
    }

    if method == "POST" && path == "/session" {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body)
            && let Some(dir) = value.get("directory").and_then(|v| v.as_str())
        {
            state.last_directory = Some(dir.to_owned());
        }
        let status = state.session_status;
        let body = state.session_body.clone();
        drop(state);
        write_response(&mut stream, status, body.as_bytes());
        return;
    }

    if let Some(id) = path
        .strip_prefix("/session/")
        .and_then(|rest| rest.strip_suffix("/prompt_async"))
        && method == "POST"
    {
        let _ = id;
        state.prompt_called = true;
        let delay = state.prompt_delay;
        let status = state.prompt_status;
        drop(state);
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        write_response(&mut stream, status, b"");
        return;
    }

    if let Some(id) = path
        .strip_prefix("/session/")
        .and_then(|rest| rest.strip_suffix("/abort"))
        && method == "POST"
    {
        let _ = id;
        state.abort_called = true;
        let status = state.abort_status;
        drop(state);
        write_response(&mut stream, status, b"");
        return;
    }

    if let Some(id) = path
        .strip_prefix("/session/")
        .and_then(|rest| rest.strip_suffix("/diff"))
        && method == "GET"
    {
        let _ = id;
        let status = state.diff_status;
        let body = state.diff_body.clone();
        drop(state);
        write_response(&mut stream, status, body.as_bytes());
        return;
    }

    if let Some(id) = path.strip_prefix("/session/")
        && method == "GET"
        && !id.contains('/')
    {
        let _ = id;
        let delay = state.status_delay;
        let override_body = state.status_body_override.clone();
        let phase = if state.status_index < state.status_sequence.len() {
            let phase = state.status_sequence[state.status_index].clone();
            if state.status_index + 1 < state.status_sequence.len() {
                state.status_index += 1;
            }
            phase
        } else {
            state
                .status_sequence
                .last()
                .cloned()
                .unwrap_or_else(|| "working".into())
        };
        drop(state);
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        let body = override_body.unwrap_or_else(|| {
            format!(
                r#"{{"id":"{id}","status":"{phase}","messages":[{{"role":"assistant","parts":[{{"type":"text","text":"done"}}]}}]}}"#
            )
        });
        write_response(&mut stream, 200, body.as_bytes());
        return;
    }

    write_response(&mut stream, 404, b"{\"error\":\"not found\"}");
}

fn append_connection(state: &mut Script, provider_id: &str, label: Option<String>) -> bool {
    let cred_id = format!("cred-{}", state.next_credential);
    state.next_credential += 1;
    let Some(provider) = state.integrations.iter_mut().find(|p| {
        p.get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == provider_id)
    }) else {
        return false;
    };
    let connection = json!({
        "type": "credential",
        "id": cred_id,
        "label": label,
    });
    match provider.get_mut("connections") {
        Some(Value::Array(items)) => items.push(connection),
        _ => {
            provider["connections"] = json!([connection]);
        }
    }
    state.credentials.insert(cred_id);
    true
}

fn read_request(stream: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if find_header_end(&buf).is_some() {
                    break;
                }
                if buf.len() > 64 * 1024 {
                    return None;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(_) => return None,
        }
    }
    let header_end = find_header_end(&buf)?;
    let header = std::str::from_utf8(&buf[..header_end]).ok()?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();
    let mut content_length = 0usize;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < content_length {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
    }
    body.truncate(content_length);
    Some((method, path, body))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}
