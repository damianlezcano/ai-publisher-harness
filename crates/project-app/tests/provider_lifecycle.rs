//! M7 app-facade lifecycle suite (design §16/§26): the shared backend restart
//! drops stale agent sessions, and model selection never restarts the backend.

use std::path::PathBuf;
use std::sync::Arc;

use fake_opencode_server::FakeServer;
use project_agent::FakeAgentEngine;
use project_agent::model::{AgentProject, AgentStatus};
use project_agent::{AgentEngine, OpenCodeAgentEngine};
use project_app::{AppState, SharedBackendRestarter};
use project_opencode::{BackendStatus, OpenCodeBackend};
use project_provider::{
    FakeProviderConnector, FakeRestarter, ModelSummary, ProviderDetail, ProviderService,
    SecretString,
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

fn free_model() -> ModelSummary {
    ModelSummary {
        provider_id: "opencode".into(),
        model_id: "big-pickle".into(),
        name: "big-pickle".into(),
        free: true,
        recommended: true,
        deprecated: false,
    }
}

fn app(
    base: &std::path::Path,
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

// Design §16: a credential mutation restarts the shared backend and the engine
// drops its stale session cache; the next use recreates a fresh session.
#[test]
fn shared_backend_restart_drops_stale_sessions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let server = FakeServer::start();

    let backend = Arc::new(OpenCodeBackend::new(
        PathBuf::from("/usr/bin/true"),
        tmp.path().join("oc"),
        0,
    ));
    backend.set_base_url(server.base_url());
    let engine = OpenCodeAgentEngine::from_backend(Arc::clone(&backend));
    let connector = project_provider::OpenCodeProviderConnector::new(Arc::clone(&backend));
    let restarter = SharedBackendRestarter::new(Arc::clone(&backend));
    let service = ProviderService::new(connector, restarter, tmp.path().join("settings.json"));

    engine.ensure_ready().expect("ready");
    assert_eq!(engine.status(), AgentStatus::Ready);
    let project = AgentProject {
        project_id: "proj-7".into(),
        directory: PathBuf::from("/tmp/proj-7/workspace"),
    };
    let first = engine.open_session(&project).expect("session");
    assert_eq!(first.id, "ses-1");

    // Credential mutation -> restart (shared backend shut down).
    service
        .connect_api_key("openai", &SecretString::new("sk-x".into()), None)
        .expect("connect");
    assert_eq!(backend.status(), BackendStatus::Stopped);

    // New backend generation (test seam: repoint the loopback base URL).
    server.set_session_id("ses-2");
    backend.set_base_url(server.base_url());
    engine.ensure_ready().expect("respawn ready");

    // The stale session must be gone: a fresh session id is created, not "ses-1".
    let second = engine.open_session(&project).expect("session 2");
    assert_eq!(second.id, "ses-2");
    assert_ne!(second.id, first.id);
}

// Design §16: model selection applies per prompt without restarting the backend.
#[test]
fn model_selection_does_not_restart_the_backend() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let restarter = FakeRestarter::new();
    let state = app(
        tmp.path(),
        FakeProviderConnector::new()
            .with_provider(provider("opencode", "Gratis"))
            .with_model(free_model()),
        restarter.clone(),
    );
    state
        .model_select("opencode", "big-pickle")
        .expect("select");
    assert_eq!(restarter.restart_count(), 0);
    assert!(tmp.path().join("settings.json").exists());
}

// App restart persists the global selection (design §23).
#[test]
fn app_restart_persists_selection() {
    let tmp = tempfile::tempdir().expect("tempdir");
    {
        let state = app(
            tmp.path(),
            FakeProviderConnector::new().with_model(free_model()),
            FakeRestarter::new(),
        );
        state
            .model_select("opencode", "big-pickle")
            .expect("select");
    }
    let reloaded = app(
        tmp.path(),
        FakeProviderConnector::new().with_model(free_model()),
        FakeRestarter::new(),
    );
    let selected = reloaded.model_get_selected().expect("reload");
    assert_eq!(selected.model.provider_id, "opencode");
    assert_eq!(selected.model.model_id, "big-pickle");
}
