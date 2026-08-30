//! M7 provider lifecycle suite (design §16/§26): restart-on-credential-mutation,
//! model switch without restart, and selection persistence across reload.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fake_opencode_server::FakeServer;
use project_opencode::{BackendStatus, OpenCodeBackend};
use project_provider::{
    BackendRestarter, FakeProviderConnector, FakeRestarter, ModelSummary, ProviderError,
    ProviderService, SecretString,
};

fn unique_config_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("project-provider-life-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn unique_settings() -> PathBuf {
    unique_config_dir().join("settings.json")
}

fn provider(id: &str, name: &str) -> project_provider::ProviderDetail {
    project_provider::ProviderDetail {
        id: id.to_owned(),
        name: name.to_owned(),
        auth_methods: Vec::new(),
        connections: Vec::new(),
    }
}

fn free_model(provider_id: &str, model_id: &str) -> ModelSummary {
    ModelSummary {
        provider_id: provider_id.to_owned(),
        model_id: model_id.to_owned(),
        name: model_id.to_owned(),
        free: true,
        recommended: true,
        deprecated: false,
    }
}

/// Restarter that actually shuts down a shared `OpenCodeBackend`, mirroring the
/// production `SharedBackendRestarter` (design §16).
#[derive(Clone)]
struct ShutdownRestarter {
    backend: Arc<OpenCodeBackend>,
}

impl ShutdownRestarter {
    fn new(backend: Arc<OpenCodeBackend>) -> Self {
        Self { backend }
    }
}

impl BackendRestarter for ShutdownRestarter {
    fn restart(&self) -> project_provider::ProviderResult<()> {
        self.backend
            .shutdown()
            .map_err(|err| ProviderError::Internal(err.to_string()))
    }
}

fn shared_components(
    server: &FakeServer,
) -> (
    Arc<OpenCodeBackend>,
    ProviderService<project_provider::OpenCodeProviderConnector, ShutdownRestarter>,
) {
    let backend = Arc::new(OpenCodeBackend::new(
        PathBuf::from("/usr/bin/true"),
        unique_config_dir(),
        0,
    ));
    backend.set_base_url(server.base_url());
    backend.ensure_ready().expect("ready");
    let connector = project_provider::OpenCodeProviderConnector::new(Arc::clone(&backend));
    let restarter = ShutdownRestarter::new(Arc::clone(&backend));
    let service = ProviderService::new(connector, restarter, unique_settings());
    (backend, service)
}

#[test]
fn credential_mutation_restarts_the_shared_backend() {
    let server = FakeServer::start();
    let (backend, service) = shared_components(&server);
    assert_eq!(backend.status(), BackendStatus::Ready);

    service
        .connect_api_key("openai", &SecretString::new("sk-x".into()), None)
        .expect("connect");
    assert_eq!(
        backend.status(),
        BackendStatus::Stopped,
        "a connect must restart (shutdown) the shared backend"
    );

    // Lazy respawn on next use.
    backend.ensure_ready().expect("respawn");
    assert_eq!(backend.status(), BackendStatus::Ready);
}

#[test]
fn oauth_complete_and_disconnect_restart_but_cancel_does_not() {
    let server = FakeServer::start();
    let (backend, service) = shared_components(&server);

    let attempt = service
        .begin_oauth("openai", "chatgpt-browser")
        .expect("begin");
    assert_eq!(
        backend.status(),
        BackendStatus::Ready,
        "begin must not restart"
    );

    service.cancel_oauth(&attempt.attempt_id).expect("cancel");
    assert_eq!(
        backend.status(),
        BackendStatus::Ready,
        "cancel must not restart"
    );

    let attempt = service
        .begin_oauth("openai", "chatgpt-browser")
        .expect("begin 2");
    service
        .complete_oauth(&attempt.attempt_id, None)
        .expect("complete");
    assert_eq!(
        backend.status(),
        BackendStatus::Stopped,
        "oauth complete must restart"
    );

    backend.ensure_ready().expect("respawn");
    let view = service
        .connect_api_key("openai", &SecretString::new("sk-y".into()), None)
        .expect("connect");
    assert_eq!(
        backend.status(),
        BackendStatus::Stopped,
        "connect must restart"
    );
    backend.ensure_ready().expect("respawn");

    service.disconnect(&view.id).expect("disconnect");
    assert_eq!(
        backend.status(),
        BackendStatus::Stopped,
        "disconnect must restart"
    );
}

#[test]
fn model_selection_does_not_restart_the_backend() {
    let server = FakeServer::start();
    let (backend, service) = shared_components(&server);
    service
        .select_model("opencode", "big-pickle")
        .expect("select");
    assert_eq!(
        backend.status(),
        BackendStatus::Ready,
        "model selection applies per prompt without restart (design §16)"
    );
}

#[test]
fn failed_connect_does_not_restart_the_backend() {
    // The service must restart only on a successful mutation; an invalid key
    // (CredentialInvalid) leaves the backend untouched.
    let fake = FakeProviderConnector::new()
        .with_provider(provider("openai", "ChatGPT"))
        .with_model(free_model("opencode", "big-pickle"));
    fake.set_connect_error(ProviderError::CredentialInvalid);
    let restarter = FakeRestarter::new();
    let service = ProviderService::new(fake, restarter.clone(), unique_settings());
    let err = service
        .connect_api_key("openai", &SecretString::new("sk-bad".into()), None)
        .expect_err("invalid");
    assert_eq!(err, ProviderError::CredentialInvalid);
    assert_eq!(
        restarter.restart_count(),
        0,
        "failed connect must not restart"
    );
}

#[test]
fn selection_persists_across_service_reload() {
    let settings = unique_settings();
    let connector = FakeProviderConnector::new()
        .with_provider(provider("opencode", "Gratis"))
        .with_model(free_model("opencode", "big-pickle"));
    let first = ProviderService::new(connector.clone(), FakeRestarter::new(), settings.clone());
    first
        .select_model("opencode", "big-pickle")
        .expect("select");

    let second = ProviderService::new(connector, FakeRestarter::new(), settings);
    let selected = second.get_selected_model().expect("reload");
    assert_eq!(selected.model.provider_id, "opencode");
    assert_eq!(selected.model.model_id, "big-pickle");
    assert!(!selected.requires_choice);
}
