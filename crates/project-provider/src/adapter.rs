//! `OpenCodeProviderConnector`: `ProviderConnector` over OpenCode's integration API.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use project_opencode::{BackendError, OpenCodeBackend};
use serde_json::{Value, json};

use crate::error::{ProviderError, ProviderResult};
use crate::models::{
    AuthMethodKind, AuthMethodView, AuthPrompt, AuthPromptKind, ConnectionState, ConnectionTest,
    ConnectionTestOutcome, ConnectionView, ModelSummary, OAuthAttempt, OAuthMode, OAuthStatus,
    OAuthStatusKind, ProviderDetail, ProviderSummary,
};
use crate::port::ProviderConnector;
use crate::secret::{SecretString, redact_credentials};

const DEFAULT_FEATURED: [&str; 5] = ["openai", "google", "deepseek", "anthropic", "opencode"];
const DEFAULT_TASK_TIMEOUT: Duration = Duration::from_secs(60);
const TEST_PROMPT: &str = "Respondé: ok";

pub struct OpenCodeProviderConnector {
    backend: Arc<OpenCodeBackend>,
    featured: Vec<String>,
    scratch_root: Option<PathBuf>,
    task_timeout: Duration,
}

impl OpenCodeProviderConnector {
    pub fn new(backend: Arc<OpenCodeBackend>) -> Self {
        Self {
            backend,
            featured: DEFAULT_FEATURED.iter().map(|s| (*s).to_owned()).collect(),
            scratch_root: None,
            task_timeout: DEFAULT_TASK_TIMEOUT,
        }
    }

    pub fn with_featured(mut self, featured: Vec<String>) -> Self {
        self.featured = featured;
        self
    }

    pub fn with_scratch_root(mut self, scratch_root: PathBuf) -> Self {
        self.scratch_root = Some(scratch_root);
        self
    }

    pub fn with_task_timeout(mut self, task_timeout: Duration) -> Self {
        self.task_timeout = task_timeout;
        self
    }

    fn get(&self, path: &str) -> ProviderResult<(u16, String)> {
        self.backend.ensure_ready().map_err(map_backend_error)?;
        self.backend.get(path).map_err(map_backend_error)
    }

    fn post(&self, path: &str, body: &Value) -> ProviderResult<(u16, String)> {
        self.backend.ensure_ready().map_err(map_backend_error)?;
        self.backend.post(path, body).map_err(map_backend_error)
    }

    fn delete(&self, path: &str) -> ProviderResult<(u16, String)> {
        self.backend.ensure_ready().map_err(map_backend_error)?;
        self.backend.delete(path).map_err(map_backend_error)
    }

    fn fetch_integrations(&self) -> ProviderResult<Vec<Value>> {
        let (status, body) = self.get("/api/integration")?;
        if !(200..300).contains(&status) {
            return Err(ProviderError::ProviderUnavailable);
        }
        parse_json_array(&body)
    }

    fn find_integration<'a>(entries: &'a [Value], id: &str) -> Option<&'a Value> {
        entries.iter().find(|entry| {
            entry
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|found| found == id)
        })
    }
}

impl ProviderConnector for OpenCodeProviderConnector {
    fn list_providers(&self) -> ProviderResult<Vec<ProviderSummary>> {
        let entries = self.fetch_integrations()?;
        let live: HashSet<&str> = entries
            .iter()
            .filter_map(|e| e.get("id").and_then(Value::as_str))
            .collect();
        let featured: HashSet<&str> = self
            .featured
            .iter()
            .map(String::as_str)
            .filter(|id| live.contains(id))
            .collect();
        Ok(entries
            .iter()
            .filter_map(|entry| map_summary(entry, &featured))
            .collect())
    }

    fn provider_detail(&self, provider_id: &str) -> ProviderResult<ProviderDetail> {
        let entries = self.fetch_integrations()?;
        let entry = Self::find_integration(&entries, provider_id)
            .ok_or_else(|| ProviderError::NotFound(provider_id.to_owned()))?;
        map_detail(entry).ok_or_else(|| ProviderError::NotFound(provider_id.to_owned()))
    }

    fn connect_api_key(
        &self,
        provider_id: &str,
        key: &SecretString,
        label: Option<&str>,
    ) -> ProviderResult<ConnectionState> {
        let body = json!({ "key": key.expose(), "label": label });
        let (status, _) = self.post(
            &format!("/api/integration/{provider_id}/connect/key"),
            &body,
        )?;
        if status == 401 || status == 403 {
            return Err(ProviderError::CredentialInvalid);
        }
        if status == 404 {
            // Unknown provider id: validated against the live list, never echoed
            // in a human message (design §17).
            return Err(ProviderError::NotFound(provider_id.to_owned()));
        }
        if !(200..300).contains(&status) {
            return Err(ProviderError::ConnectFailed(status.to_string()));
        }
        let entries = self.fetch_integrations()?;
        let entry = Self::find_integration(&entries, provider_id)
            .ok_or_else(|| ProviderError::NotFound(provider_id.to_owned()))?;
        let connections = map_connections(entry);
        let connection = connections.last().cloned();
        log_event("connected");
        Ok(ConnectionState {
            connected: connection.is_some(),
            connection,
        })
    }

    fn begin_oauth(&self, provider_id: &str, method_id: &str) -> ProviderResult<OAuthAttempt> {
        let body = json!({
            "methodID": method_id,
            "inputs": {},
            "label": Value::Null,
        });
        let (status, text) = self.post(
            &format!("/api/integration/{provider_id}/connect/oauth"),
            &body,
        )?;
        if status == 404 {
            return Err(ProviderError::NotFound(provider_id.to_owned()));
        }
        if !(200..300).contains(&status) {
            return Err(ProviderError::OAuthFailed(status.to_string()));
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| ProviderError::OAuthFailed("malformed attempt".into()))?;
        let attempt_id = value
            .get("attemptID")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::OAuthFailed("missing attemptID".into()))?
            .to_owned();
        let url = value
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let instructions = value
            .get("instructions")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mode = match value.get("mode").and_then(Value::as_str).unwrap_or("auto") {
            "code" => OAuthMode::Code,
            "auto" => OAuthMode::Auto,
            _ => OAuthMode::Auto,
        };
        Ok(OAuthAttempt {
            attempt_id,
            url,
            instructions,
            mode,
        })
    }

    fn oauth_status(&self, attempt_id: &str) -> ProviderResult<OAuthStatus> {
        let (status, text) = self.get(&format!("/api/integration/attempt/{attempt_id}"))?;
        if !(200..300).contains(&status) {
            return Err(ProviderError::OAuthFailed(status.to_string()));
        }
        let kind = parse_oauth_status_kind(&text)
            .ok_or_else(|| ProviderError::OAuthFailed("unknown status".into()))?;
        Ok(OAuthStatus {
            status: kind,
            message: None,
        })
    }

    fn complete_oauth(
        &self,
        attempt_id: &str,
        code: Option<&str>,
    ) -> ProviderResult<ConnectionState> {
        let before = connection_ids(&self.fetch_integrations()?);
        let body = json!({ "code": code });
        let (status, _) = self.post(
            &format!("/api/integration/attempt/{attempt_id}/complete"),
            &body,
        )?;
        if !(200..300).contains(&status) {
            return Err(ProviderError::OAuthFailed(status.to_string()));
        }
        let entries = self.fetch_integrations()?;
        let connection = entries
            .iter()
            .flat_map(map_connections)
            .find(|c| !before.contains(&c.id));
        log_event("connected");
        Ok(ConnectionState {
            connected: true,
            connection,
        })
    }

    fn cancel_oauth(&self, attempt_id: &str) -> ProviderResult<()> {
        let (status, _) = self.delete(&format!("/api/integration/attempt/{attempt_id}"))?;
        if status == 404 {
            return Err(ProviderError::NotFound(attempt_id.to_owned()));
        }
        if !(200..300).contains(&status) {
            return Err(ProviderError::OAuthFailed(status.to_string()));
        }
        Ok(())
    }

    fn disconnect(&self, credential_id: &str) -> ProviderResult<()> {
        let (status, _) = self.delete(&format!("/api/credential/{credential_id}"))?;
        if status == 404 {
            return Err(ProviderError::NotFound(credential_id.to_owned()));
        }
        if !(200..300).contains(&status) {
            return Err(ProviderError::DisconnectFailed(status.to_string()));
        }
        log_event("disconnected");
        Ok(())
    }

    fn list_models(&self) -> ProviderResult<Vec<ModelSummary>> {
        let (status, body) = self.get("/api/model")?;
        if !(200..300).contains(&status) {
            return Err(ProviderError::ProviderUnavailable);
        }
        let models = parse_json_array(&body)?;
        let defaults = self.fetch_provider_defaults();
        let mut first_enabled: HashMap<String, String> = HashMap::new();
        let mut summaries = Vec::new();
        for model in &models {
            let Some(summary) = map_model(model) else {
                continue;
            };
            first_enabled
                .entry(summary.provider_id.clone())
                .or_insert_with(|| summary.model_id.clone());
            summaries.push(summary);
        }
        for summary in &mut summaries {
            let recommended = defaults
                .get(&summary.provider_id)
                .is_some_and(|id| id == &summary.model_id)
                || (!defaults.contains_key(&summary.provider_id)
                    && first_enabled.get(&summary.provider_id) == Some(&summary.model_id));
            summary.recommended = recommended;
        }
        Ok(summaries)
    }

    fn test_connection(&self, provider_id: &str, model_id: &str) -> ProviderResult<ConnectionTest> {
        log_event("test started");
        let scratch = make_scratch_dir(self.scratch_root.as_ref())?;
        let _guard = ScratchGuard(scratch.clone());
        let directory = scratch.to_string_lossy().replace('\\', "/");
        let (status, text) = self.post("/session", &json!({ "directory": directory }))?;
        if !(200..300).contains(&status) {
            return failed_test(ConnectionTestOutcome::ProviderUnavailable);
        }
        let value: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return failed_test(ConnectionTestOutcome::ProviderUnavailable),
        };
        let Some(session_id) = value.get("id").and_then(Value::as_str) else {
            return failed_test(ConnectionTestOutcome::ProviderUnavailable);
        };
        let prompt_body = json!({
            "parts": [{"type": "text", "text": TEST_PROMPT}],
            "model": {"providerID": provider_id, "modelID": model_id},
        });
        let (status, _) =
            self.post(&format!("/session/{session_id}/prompt_async"), &prompt_body)?;
        if status == 401 || status == 403 {
            // A 401/403 against a provider that already has a stored credential
            // means the credential was revoked -> "volver a conectar" (design
            // §15/§18); an invalid key at connect time stays CredentialInvalid.
            if self.provider_has_connection(provider_id)? {
                log_event("test failed outcome=credential_revoked");
                return Err(ProviderError::CredentialRevoked);
            }
            return failed_test(ConnectionTestOutcome::CredentialInvalid);
        }
        if status == 404 {
            return failed_test(ConnectionTestOutcome::NoCompatibleModel);
        }
        if !(200..300).contains(&status) {
            return failed_test(ConnectionTestOutcome::ProviderUnavailable);
        }
        match self.poll_test_session(session_id) {
            Ok(test) => {
                if test.outcome == ConnectionTestOutcome::Connected {
                    log_event("connected");
                } else {
                    log_test_failed(test.outcome);
                }
                Ok(test)
            }
            Err(err) => {
                if let Some(outcome) = err.test_outcome() {
                    log_test_failed(outcome);
                    return Ok(ConnectionTest {
                        outcome,
                        message: test_message(outcome).into(),
                    });
                }
                log_event("test failed outcome=internal");
                Err(err)
            }
        }
    }
}

impl OpenCodeProviderConnector {
    /// Whether the provider currently has at least one stored connection.
    fn provider_has_connection(&self, provider_id: &str) -> ProviderResult<bool> {
        let entries = self.fetch_integrations()?;
        Ok(Self::find_integration(&entries, provider_id)
            .map(|entry| !map_connections(entry).is_empty())
            .unwrap_or(false))
    }

    fn fetch_provider_defaults(&self) -> HashMap<String, String> {
        let Ok((status, body)) = self.get("/config/providers") else {
            return HashMap::new();
        };
        if !(200..300).contains(&status) {
            return HashMap::new();
        }
        let Ok(value) = serde_json::from_str::<Value>(&body) else {
            return HashMap::new();
        };
        let Some(map) = value.get("default").and_then(Value::as_object) else {
            return HashMap::new();
        };
        map.iter()
            .filter_map(|(provider, model)| {
                model.as_str().map(|id| (provider.clone(), id.to_owned()))
            })
            .collect()
    }

    fn poll_test_session(&self, session_id: &str) -> ProviderResult<ConnectionTest> {
        let path = format!("/session/{session_id}");
        let deadline = Instant::now() + self.task_timeout;
        loop {
            let (status, body) = self.get(&path)?;
            if !(200..300).contains(&status) {
                return failed_test(ConnectionTestOutcome::ProviderUnavailable);
            }
            let value: Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(_) => return failed_test(ConnectionTestOutcome::ProviderUnavailable),
            };
            let phase = session_phase(&value);
            match phase.as_str() {
                "idle" | "done" | "complete" | "completed" | "success" => {
                    if last_assistant_text(&value).is_some() {
                        return Ok(ConnectionTest {
                            outcome: ConnectionTestOutcome::Connected,
                            message: "Conectado.".into(),
                        });
                    }
                    return failed_test(ConnectionTestOutcome::ProviderUnavailable);
                }
                "failed" | "error" | "failure" => {
                    return Ok(map_session_failure(&value, &body));
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                return failed_test(ConnectionTestOutcome::ProviderUnavailable);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

struct ScratchGuard(PathBuf);

impl Drop for ScratchGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn make_scratch_dir(root: Option<&PathBuf>) -> ProviderResult<PathBuf> {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = root.cloned().unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!("provider-test-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|err| ProviderError::Internal(err.to_string()))?;
    Ok(dir)
}

fn failed_test(outcome: ConnectionTestOutcome) -> ProviderResult<ConnectionTest> {
    log_test_failed(outcome);
    Ok(ConnectionTest {
        outcome,
        message: test_message(outcome).into(),
    })
}

fn test_message(outcome: ConnectionTestOutcome) -> &'static str {
    match outcome {
        ConnectionTestOutcome::Connected => "Conectado.",
        ConnectionTestOutcome::CredentialInvalid => "Esta clave no es válida.",
        ConnectionTestOutcome::ProviderUnavailable => "No pudimos conectarnos con el proveedor.",
        ConnectionTestOutcome::NoCompatibleModel => "Este modelo ya no está disponible.",
        ConnectionTestOutcome::NetworkError => {
            "No hay conexión con el proveedor. Revisá tu conexión."
        }
    }
}

fn log_test_failed(outcome: ConnectionTestOutcome) {
    let label = match outcome {
        ConnectionTestOutcome::Connected => "connected",
        ConnectionTestOutcome::CredentialInvalid => "credential_invalid",
        ConnectionTestOutcome::ProviderUnavailable => "provider_unavailable",
        ConnectionTestOutcome::NoCompatibleModel => "no_compatible_model",
        ConnectionTestOutcome::NetworkError => "network_error",
    };
    log_event(&format!("test failed outcome={label}"));
}

fn log_event(event: &str) {
    // Primary defense is never logging a secret; this scrubber is the second
    // belt-and-suspenders layer (design §19).
    eprintln!("[provider] {}", redact_credentials(event));
}

fn map_backend_error(err: BackendError) -> ProviderError {
    match err {
        BackendError::NotReady => ProviderError::BackendNotReady,
        BackendError::Http(reason) => {
            if is_network_error(&reason) {
                ProviderError::NetworkError
            } else {
                ProviderError::ProviderUnavailable
            }
        }
        _ => ProviderError::Internal(err.to_string()),
    }
}

fn is_network_error(reason: &str) -> bool {
    let lower = reason.to_ascii_lowercase();
    lower.contains("connection refused")
        || lower.contains("os error 111")
        || lower.contains("dns")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("failed to lookup")
        || lower.contains("name or service not known")
        || lower.contains("nodename nor servname")
        || lower.contains("error trying to connect")
        || lower.contains("tcp connect error")
        || lower.contains("error sending request")
}

fn parse_json_array(body: &str) -> ProviderResult<Vec<Value>> {
    let value: Value =
        serde_json::from_str(body).map_err(|_| ProviderError::Internal("malformed JSON".into()))?;
    match value {
        Value::Array(items) => Ok(items),
        _ => Err(ProviderError::Internal("malformed JSON".into())),
    }
}

fn map_summary(entry: &Value, featured: &HashSet<&str>) -> Option<ProviderSummary> {
    let id = entry.get("id").and_then(Value::as_str)?.to_owned();
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_owned();
    let connections = map_connections(entry);
    Some(ProviderSummary {
        highlighted: featured.contains(id.as_str()),
        id,
        name,
        auth_methods: map_auth_methods(entry),
        connected: !connections.is_empty(),
        connection_label: connections.first().and_then(|c| c.label.clone()),
    })
}

fn map_detail(entry: &Value) -> Option<ProviderDetail> {
    let id = entry.get("id").and_then(Value::as_str)?.to_owned();
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_owned();
    Some(ProviderDetail {
        id,
        name,
        auth_methods: map_auth_methods(entry),
        connections: map_connections(entry),
    })
}

fn map_auth_methods(entry: &Value) -> Vec<AuthMethodView> {
    let Some(methods) = entry.get("methods").and_then(Value::as_array) else {
        return Vec::new();
    };
    methods.iter().filter_map(map_auth_method).collect()
}

fn map_auth_method(method: &Value) -> Option<AuthMethodView> {
    let kind = method
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| method.get("kind").and_then(Value::as_str))?;
    match kind {
        "key" => Some(AuthMethodView {
            kind: AuthMethodKind::ApiKey,
            method_id: None,
            label: method
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("Clave de acceso")
                .to_owned(),
            prompts: Vec::new(),
        }),
        "oauth" => Some(AuthMethodView {
            kind: AuthMethodKind::Account,
            method_id: method.get("id").and_then(Value::as_str).map(str::to_owned),
            label: method
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("Conectá tu cuenta")
                .to_owned(),
            prompts: method
                .get("prompts")
                .and_then(Value::as_array)
                .map(|prompts| prompts.iter().filter_map(map_prompt).collect())
                .unwrap_or_default(),
        }),
        "env" => None,
        _ => None,
    }
}

fn map_prompt(prompt: &Value) -> Option<AuthPrompt> {
    let key = prompt.get("key").and_then(Value::as_str)?.to_owned();
    let message = prompt
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let kind = match prompt.get("kind").and_then(Value::as_str).unwrap_or("text") {
        "select" => AuthPromptKind::Select,
        _ => AuthPromptKind::Text,
    };
    let options = prompt
        .get("options")
        .and_then(Value::as_array)
        .map(|opts| {
            opts.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some(AuthPrompt {
        key,
        message,
        kind,
        options,
        placeholder: prompt
            .get("placeholder")
            .and_then(Value::as_str)
            .map(str::to_owned),
        optional: prompt
            .get("optional")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn map_connections(entry: &Value) -> Vec<ConnectionView> {
    let Some(items) = entry.get("connections").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.get("id").and_then(Value::as_str)?.to_owned();
            let label = item.get("label").and_then(Value::as_str).map(str::to_owned);
            Some(ConnectionView { id, label })
        })
        .collect()
}

fn connection_ids(entries: &[Value]) -> HashSet<String> {
    entries
        .iter()
        .flat_map(map_connections)
        .map(|c| c.id)
        .collect()
}

fn map_model(model: &Value) -> Option<ModelSummary> {
    let enabled = model
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let status = model
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if !enabled || status == "disabled" {
        return None;
    }
    let model_id = model.get("id").and_then(Value::as_str)?.to_owned();
    let provider_id = model
        .get("providerID")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if provider_id.is_empty() {
        return None;
    }
    let name = model
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&model_id)
        .to_owned();
    Some(ModelSummary {
        provider_id,
        model_id,
        name,
        free: cost_is_zero(model.get("cost")),
        recommended: false,
        deprecated: status == "deprecated",
    })
}

fn cost_is_zero(cost: Option<&Value>) -> bool {
    match cost {
        Some(Value::Number(n)) => n.as_f64() == Some(0.0) || n.as_i64() == Some(0),
        Some(Value::String(s)) => s == "0" || s.parse::<f64>() == Ok(0.0),
        _ => false,
    }
}

fn parse_oauth_status_kind(body: &str) -> Option<OAuthStatusKind> {
    let trimmed = body.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        let raw = match &value {
            Value::String(s) => s.as_str(),
            Value::Object(_) => value
                .get("status")
                .and_then(Value::as_str)
                .or_else(|| value.get("type").and_then(Value::as_str))?,
            _ => return None,
        };
        return oauth_kind(raw);
    }
    oauth_kind(trimmed.trim_matches('"'))
}

fn oauth_kind(raw: &str) -> Option<OAuthStatusKind> {
    match raw.to_ascii_lowercase().as_str() {
        "pending" => Some(OAuthStatusKind::Pending),
        "complete" => Some(OAuthStatusKind::Complete),
        "failed" => Some(OAuthStatusKind::Failed),
        "expired" => Some(OAuthStatusKind::Expired),
        _ => None,
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

fn map_session_failure(value: &Value, raw_body: &str) -> ConnectionTest {
    let haystack = format!(
        "{} {}",
        last_assistant_text(value).unwrap_or_default(),
        value.get("error").and_then(Value::as_str).unwrap_or("")
    );
    let lower = haystack.to_ascii_lowercase();
    let outcome = if lower.contains("401") || lower.contains("403") {
        ConnectionTestOutcome::CredentialInvalid
    } else if lower.contains("not found") || lower.contains("404") {
        ConnectionTestOutcome::NoCompatibleModel
    } else {
        let _ = raw_body;
        ConnectionTestOutcome::ProviderUnavailable
    };
    ConnectionTest {
        outcome,
        message: test_message(outcome).into(),
    }
}
