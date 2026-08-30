//! M7 app facade: provider/model surface over FakeProviderConnector + FakeRestarter.

use std::path::Path;
use std::sync::{Arc, Mutex};

use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};
use project_agent::{AgentEngine, Artifact, ArtifactKind, FakeAgentEngine};
use project_app::{AppState, ErrorCode, SharedBackendRestarter};
use project_provider::{
    BackendRestarter, ConnectionTestOutcome, FakeProviderConnector, FakeRestarter, ModelSummary,
    ProviderDetail, ProviderError, ProviderSummary, SecretString,
};
use project_tunnel::FakeTunnel;

fn provider(id: &str, name: &str) -> ProviderDetail {
    ProviderDetail {
        id: id.to_owned(),
        name: name.to_owned(),
        auth_methods: Vec::new(),
        connections: Vec::new(),
    }
}

fn model(provider_id: &str, model_id: &str, free: bool, recommended: bool) -> ModelSummary {
    ModelSummary {
        provider_id: provider_id.to_owned(),
        model_id: model_id.to_owned(),
        name: model_id.to_owned(),
        free,
        recommended,
        deprecated: false,
    }
}

fn default_connector() -> FakeProviderConnector {
    FakeProviderConnector::new()
        .with_provider(provider("opencode", "Gratis"))
        .with_provider(provider("openai", "ChatGPT"))
        .with_model(model("opencode", "big-pickle", true, true))
        .with_model(model("openai", "gpt-4o", false, true))
}

fn app(
    base: &Path,
    connector: FakeProviderConnector,
    restarter: FakeRestarter,
) -> AppState<FakeAgentEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
    AppState::with_components(
        base.to_path_buf(),
        FakeAgentEngine::new(),
        FakeTunnel::new(),
        connector,
        restarter,
    )
}

#[test]
fn provider_list_and_detail_round_trip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = app(tmp.path(), default_connector(), FakeRestarter::new());
    let list = state.provider_list().expect("list");
    let ids: Vec<_> = list.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.contains(&"opencode"));
    assert!(ids.contains(&"openai"));

    let openai = state.provider_detail("openai").expect("detail");
    assert_eq!(openai.name, "ChatGPT");
    let err = state.provider_detail("nope").expect_err("unknown");
    assert_eq!(err.code, ErrorCode::ProviderNotFound);
    assert_eq!(err.message, "Ese proveedor no está disponible.");
}

#[test]
fn connect_key_flows_once_and_never_returns_secret() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let connector = default_connector();
    let state = app(tmp.path(), connector.clone(), FakeRestarter::new());
    let view = state
        .provider_connect_key(
            "openai",
            &SecretString::new("sk-super-secret".into()),
            Some("clave"),
        )
        .expect("connect");
    assert_eq!(view.label.as_deref(), Some("clave"));
    let json = serde_json::to_string(&view).expect("json");
    assert!(!json.contains("sk-super-secret"));
    assert_eq!(
        connector.last_connect_key().as_deref(),
        Some("sk-super-secret")
    );
}

#[test]
fn credential_mutations_trigger_backend_restart() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let connector = default_connector();
    let restarter = FakeRestarter::new();
    let state = app(tmp.path(), connector.clone(), restarter.clone());

    state
        .provider_connect_key("openai", &SecretString::new("sk-x".into()), None)
        .expect("connect");
    assert_eq!(restarter.restart_count(), 1);

    let attempt = state
        .provider_oauth_begin("openai", "chatgpt-browser")
        .expect("begin");
    assert_eq!(restarter.restart_count(), 1, "begin must not restart");
    let _ = state
        .provider_oauth_status(&attempt.attempt_id)
        .expect("status");
    assert_eq!(restarter.restart_count(), 1, "status must not restart");

    state
        .provider_oauth_complete(&attempt.attempt_id, None)
        .expect("complete");
    assert_eq!(restarter.restart_count(), 2);

    let connections = connector.connections("openai");
    assert!(!connections.is_empty());
    state
        .provider_disconnect(&connections[0].id)
        .expect("disconnect");
    assert_eq!(restarter.restart_count(), 3);

    state
        .provider_oauth_cancel(&attempt.attempt_id)
        .expect("cancel");
    assert_eq!(restarter.restart_count(), 3, "cancel must not restart");
}

#[test]
fn provider_errors_map_to_human_codes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let connector = default_connector();
    connector.set_connect_error(ProviderError::CredentialInvalid);
    let state = app(tmp.path(), connector, FakeRestarter::new());
    let err = state
        .provider_connect_key("openai", &SecretString::new("sk-bad".into()), None)
        .expect_err("invalid");
    assert_eq!(err.code, ErrorCode::CredentialInvalid);
    assert_eq!(err.message, "Esta clave no es válida.");
}

#[test]
fn app_errors_serialize_codes_as_snake_case() {
    use project_app::AppError;
    // The frontend contract is snake_case (M6 mocks: publish_failed, ...).
    let err = AppError::new(
        ErrorCode::ProviderConnectFailed,
        "No pudimos conectar tu cuenta.",
    );
    let json = serde_json::to_value(&err).expect("json");
    assert_eq!(json["code"], "provider_connect_failed");
    assert_eq!(json["message"], "No pudimos conectar tu cuenta.");
    let err = AppError::new(ErrorCode::CredentialRevoked, "x");
    let json = serde_json::to_value(&err).expect("json");
    assert_eq!(json["code"], "credential_revoked");
    let err = AppError::new(ErrorCode::PublishFailed, "x");
    let json = serde_json::to_value(&err).expect("json");
    assert_eq!(json["code"], "publish_failed");
}

#[test]
fn model_list_select_and_get_selected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = app(tmp.path(), default_connector(), FakeRestarter::new());

    let models = state.model_list().expect("list");
    assert!(models.iter().any(|m| m.model_id == "big-pickle"));

    let default = state.model_get_selected().expect("default");
    assert_eq!(default.model.provider_id, "opencode");
    assert_eq!(default.model.model_id, "big-pickle");
    assert!(!default.requires_choice);

    let chosen = state
        .model_select("openai", "gpt-4o")
        .expect("select explicit paid");
    assert_eq!(chosen.model_id, "gpt-4o");

    let selected = state.model_get_selected().expect("selected");
    assert_eq!(selected.model.provider_id, "openai");
    assert_eq!(selected.model.model_id, "gpt-4o");

    let err = state.model_select("openai", "ghost").expect_err("unknown");
    assert_eq!(err.code, ErrorCode::ModelUnavailable);
}

#[test]
fn model_selection_persists_across_app_restart() {
    let tmp = tempfile::tempdir().expect("tempdir");
    {
        let state = app(tmp.path(), default_connector(), FakeRestarter::new());
        state.model_select("openai", "gpt-4o").expect("select");
    }
    let reloaded = app(tmp.path(), default_connector(), FakeRestarter::new());
    let selected = reloaded.model_get_selected().expect("reload");
    assert_eq!(selected.model.provider_id, "openai");
    assert_eq!(selected.model.model_id, "gpt-4o");
}

#[test]
fn test_connection_returns_human_outcome() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let connector = default_connector();
    connector.set_test_outcome(
        ConnectionTestOutcome::CredentialInvalid,
        "Esta clave no es válida.",
    );
    let state = app(tmp.path(), connector, FakeRestarter::new());
    let test = state
        .provider_test_connection("openai", Some("gpt-4o"))
        .expect("test");
    assert_eq!(test.outcome, ConnectionTestOutcome::CredentialInvalid);
    assert_eq!(test.message, "Esta clave no es válida.");
}

#[test]
fn oauth_open_rejects_non_https() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = app(tmp.path(), default_connector(), FakeRestarter::new());
    let err = state
        .provider_oauth_open("http://evil.test")
        .expect_err("http");
    assert_eq!(err.code, ErrorCode::InvalidInput);
    let err = state
        .provider_oauth_open("javascript:alert(1)")
        .expect_err("js");
    assert_eq!(err.code, ErrorCode::InvalidInput);
}

// -- SharedBackendRestarter stops the shared backend ---------------------------

#[test]
fn shared_restarter_stops_the_shared_backend() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let server = fake_opencode_server::FakeServer::start();
    let backend = Arc::new(project_opencode::OpenCodeBackend::new(
        std::path::PathBuf::from("/usr/bin/true"),
        tmp.path().join("oc"),
        0,
    ));
    backend.set_base_url(server.base_url());
    backend.ensure_ready().expect("ready");
    assert_eq!(backend.status(), project_opencode::BackendStatus::Ready);

    let restarter = SharedBackendRestarter::new(Arc::clone(&backend));
    restarter.restart().expect("restart");
    assert_eq!(backend.status(), project_opencode::BackendStatus::Stopped);
}

// -- run_agent passes the selected model to the engine -------------------------

struct RecordingEngine {
    inner: FakeAgentEngine,
    last_model: Arc<Mutex<Option<(String, String)>>>,
}

impl RecordingEngine {
    fn new(inner: FakeAgentEngine, last_model: Arc<Mutex<Option<(String, String)>>>) -> Self {
        Self { inner, last_model }
    }
}

impl AgentEngine for RecordingEngine {
    fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
        self.inner.ensure_ready()
    }
    fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
        self.inner.open_session(project)
    }
    fn send(
        &self,
        session: &AgentSession,
        req: &AgentPrompt,
    ) -> project_agent::AgentResult<AgentTask> {
        *self.last_model.lock().unwrap_or_else(|e| e.into_inner()) =
            req.model.clone().map(|m| (m.provider_id, m.model_id));
        self.inner.send(session, req)
    }
    fn cancel(&self, session: &AgentSession) -> project_agent::AgentResult<()> {
        self.inner.cancel(session)
    }
    fn status(&self) -> AgentStatus {
        self.inner.status()
    }
    fn shutdown(&self) -> project_agent::AgentResult<()> {
        self.inner.shutdown()
    }
}

#[test]
fn run_agent_uses_the_selected_model() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![Artifact {
        path: "workspace/index.html".into(),
        kind: ArtifactKind::Web,
        byte_size: 1,
        sha256: None,
    }]);
    let last = Arc::new(Mutex::new(None));
    let recording = RecordingEngine::new(engine, Arc::clone(&last));
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        recording,
        FakeTunnel::new(),
        default_connector(),
        FakeRestarter::new(),
    );
    let p = state.create_project("P").expect("create");
    let file = tmp
        .path()
        .join("projects")
        .join(&p.id)
        .join("workspace")
        .join("index.html");
    std::fs::create_dir_all(file.parent().expect("parent")).expect("dirs");
    std::fs::write(&file, b"<h1>").expect("artifact");
    state.run_agent(&p.id, "crea algo").expect("run");
    assert_eq!(
        *last.lock().unwrap_or_else(|e| e.into_inner()),
        Some(("opencode".into(), "big-pickle".into()))
    );
}

#[test]
fn run_agent_without_free_model_asks_for_choice() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let connector = FakeProviderConnector::new()
        .with_provider(provider("openai", "ChatGPT"))
        .with_model(model("openai", "gpt-4o", false, true));
    let state = app(tmp.path(), connector, FakeRestarter::new());
    let p = state.create_project("P").expect("create");
    let err = state.run_agent(&p.id, "hola").expect_err("no model");
    assert_eq!(err.code, ErrorCode::ModelUnavailable);
}

// -- Secret-free DTOs across the whole provider surface ------------------------

#[test]
fn provider_dtos_never_carry_secrets() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let connector = default_connector();
    let state = app(tmp.path(), connector.clone(), FakeRestarter::new());
    state
        .provider_connect_key("openai", &SecretString::new("sk-abc123".into()), None)
        .expect("connect");

    let list = state.provider_list().expect("list");
    let encoded = serde_json::to_string(&list).expect("json");
    assert!(!encoded.contains("sk-abc123"));

    let detail = state.provider_detail("openai").expect("detail");
    let encoded = serde_json::to_string(&detail).expect("json");
    assert!(!encoded.contains("sk-abc123"));

    let selected = state.model_get_selected().expect("selected");
    let encoded = serde_json::to_string(&selected).expect("json");
    assert!(!encoded.contains("sk-abc123"));
}

#[test]
fn provider_list_highlights_featured_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let connector = FakeProviderConnector::new()
        .with_provider(provider("openai", "ChatGPT"))
        .with_provider(provider("other", "Other"));
    let state = app(tmp.path(), connector, FakeRestarter::new());
    let list = state.provider_list().expect("list");
    let by_id = |id: &str| {
        list.iter()
            .find(|p: &&ProviderSummary| p.id == id)
            .expect(id)
    };
    assert!(by_id("openai").highlighted);
    assert!(!by_id("other").highlighted);
}
