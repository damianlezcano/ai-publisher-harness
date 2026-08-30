//! Offline HTTP tests for `OpenCodeProviderConnector` against the shared fake server.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fake_opencode_server::FakeServer;
use project_opencode::OpenCodeBackend;
use project_provider::{
    AuthMethodKind, ConnectionTestOutcome, OpenCodeProviderConnector, ProviderConnector,
    ProviderError, SecretString,
};
use serde_json::json;

fn unique_config_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "project-provider-adapter-{}-{n}",
        std::process::id()
    ))
}

fn ready_backend(server: &FakeServer) -> Arc<OpenCodeBackend> {
    let backend = OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), unique_config_dir(), 0);
    backend.set_base_url(server.base_url());
    backend.ensure_ready().expect("ready");
    Arc::new(backend)
}

fn connector(server: &FakeServer) -> OpenCodeProviderConnector {
    OpenCodeProviderConnector::new(ready_backend(server))
        .with_scratch_root(unique_config_dir())
        .with_task_timeout(Duration::from_secs(2))
}

#[test]
fn list_providers_maps_methods_and_featured() {
    let server = FakeServer::start();
    let connector = OpenCodeProviderConnector::new(ready_backend(&server)).with_featured(vec![
        "openai".into(),
        "does-not-exist".into(),
        "google".into(),
    ]);
    let list = connector.list_providers().expect("list");
    assert_eq!(list.len(), 3);

    let openai = list.iter().find(|p| p.id == "openai").expect("openai");
    assert_eq!(openai.name, "OpenAI");
    assert!(openai.highlighted);
    assert!(!openai.connected);
    assert!(openai.connection_label.is_none());
    assert_eq!(openai.auth_methods.len(), 2);
    assert_eq!(openai.auth_methods[0].kind, AuthMethodKind::ApiKey);
    assert!(openai.auth_methods[0].method_id.is_none());
    assert_eq!(openai.auth_methods[1].kind, AuthMethodKind::Account);
    assert_eq!(
        openai.auth_methods[1].method_id.as_deref(),
        Some("chatgpt-browser")
    );
    assert_eq!(openai.auth_methods[1].prompts.len(), 1);
    assert!(
        !openai
            .auth_methods
            .iter()
            .any(|m| format!("{:?}", m).to_lowercase().contains("env"))
    );

    let google = list.iter().find(|p| p.id == "google").expect("google");
    assert!(google.highlighted);
    assert_eq!(google.auth_methods.len(), 1);
    assert_eq!(google.auth_methods[0].kind, AuthMethodKind::ApiKey);

    let opencode = list.iter().find(|p| p.id == "opencode").expect("opencode");
    assert!(!opencode.highlighted);

    let ids: Vec<_> = list.iter().map(|p| p.id.as_str()).collect();
    assert!(!ids.contains(&"does-not-exist"));
}

#[test]
fn list_providers_connected_label_from_first_connection() {
    let server = FakeServer::start();
    server.set_integrations(vec![json!({
        "id": "openai",
        "name": "OpenAI",
        "methods": [{"type": "key"}],
        "connections": [
            {"id": "cred-a", "label": "Primera"},
            {"id": "cred-b", "label": "Segunda"}
        ]
    })]);
    let list = connector(&server).list_providers().expect("list");
    assert!(list[0].connected);
    assert_eq!(list[0].connection_label.as_deref(), Some("Primera"));
}

#[test]
fn connect_key_sends_secret_and_returns_connection_view() {
    let server = FakeServer::start();
    let connector = connector(&server);
    let key = SecretString::new("sk-super-secret-test-key-xyz".into());
    let state = connector
        .connect_api_key("openai", &key, Some("Mi clave"))
        .expect("connect");
    assert!(state.connected);
    let view = state.connection.expect("connection");
    assert_eq!(view.label.as_deref(), Some("Mi clave"));
    assert!(!view.id.is_empty());
    assert_eq!(
        server.last_connect_key().as_deref(),
        Some("sk-super-secret-test-key-xyz")
    );
    assert_eq!(server.last_connect_label().as_deref(), Some("Mi clave"));

    let json = serde_json::to_string(&view).expect("serialize");
    assert!(!json.contains("sk-super-secret-test-key-xyz"));
    let summaries = connector.list_providers().expect("list");
    let encoded = serde_json::to_string(&summaries).expect("serialize summaries");
    assert!(!encoded.contains("sk-super-secret-test-key-xyz"));
}

#[test]
fn connect_key_401_is_credential_invalid() {
    let server = FakeServer::start();
    server.set_connect_key_status(401);
    let err = connector(&server)
        .connect_api_key("openai", &SecretString::new("sk-bad".into()), None)
        .expect_err("401");
    assert_eq!(err, ProviderError::CredentialInvalid);
}

#[test]
fn connect_key_403_is_credential_invalid() {
    let server = FakeServer::start();
    server.set_connect_key_status(403);
    let err = connector(&server)
        .connect_api_key("openai", &SecretString::new("sk-bad".into()), None)
        .expect_err("403");
    assert_eq!(err, ProviderError::CredentialInvalid);
}

#[test]
fn connect_key_5xx_is_connect_failed() {
    let server = FakeServer::start();
    server.set_connect_key_status(500);
    let err = connector(&server)
        .connect_api_key("openai", &SecretString::new("sk-bad".into()), None)
        .expect_err("500");
    assert!(
        matches!(err, ProviderError::ConnectFailed(ref s) if s == "500"),
        "{err:?}"
    );
}

#[test]
fn oauth_round_trip_and_status_mapping() {
    let server = FakeServer::start();
    let connector = connector(&server);
    let attempt = connector
        .begin_oauth("openai", "chatgpt-browser")
        .expect("begin");
    assert!(!attempt.attempt_id.is_empty());
    assert!(attempt.url.starts_with("https://example.test/oauth/"));

    let pending = connector
        .oauth_status(&attempt.attempt_id)
        .expect("pending");
    assert_eq!(pending.status, project_provider::OAuthStatusKind::Pending);

    server.set_oauth_status(&attempt.attempt_id, "failed");
    let failed = connector.oauth_status(&attempt.attempt_id).expect("failed");
    assert_eq!(failed.status, project_provider::OAuthStatusKind::Failed);

    server.set_oauth_status(&attempt.attempt_id, "expired");
    let expired = connector
        .oauth_status(&attempt.attempt_id)
        .expect("expired");
    assert_eq!(expired.status, project_provider::OAuthStatusKind::Expired);

    server.set_oauth_status(&attempt.attempt_id, "pending");
    let state = connector
        .complete_oauth(&attempt.attempt_id, Some("code-1"))
        .expect("complete");
    assert!(state.connected);
    assert!(state.connection.is_some());

    let other = connector
        .begin_oauth("openai", "chatgpt-browser")
        .expect("begin 2");
    connector.cancel_oauth(&other.attempt_id).expect("cancel");
    let err = connector
        .oauth_status(&other.attempt_id)
        .expect_err("cancelled");
    assert!(matches!(err, ProviderError::OAuthFailed(_)), "{err:?}");
}

#[test]
fn disconnect_unknown_credential_is_not_found() {
    let server = FakeServer::start();
    let err = connector(&server)
        .disconnect("missing-cred")
        .expect_err("not found");
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[test]
fn disconnect_removes_credential() {
    let server = FakeServer::start();
    let connector = connector(&server);
    let state = connector
        .connect_api_key("google", &SecretString::new("AIza-test".into()), None)
        .expect("connect");
    let id = state.connection.expect("view").id;
    connector.disconnect(&id).expect("disconnect");
    let detail = connector.provider_detail("google").expect("detail");
    assert!(detail.connections.is_empty());
}

#[test]
fn list_models_maps_free_recommended_deprecated() {
    let server = FakeServer::start();
    let models = connector(&server).list_models().expect("models");
    let ids: Vec<_> = models.iter().map(|m| m.model_id.as_str()).collect();
    assert!(!ids.contains(&"hidden"));

    let gpt = models.iter().find(|m| m.model_id == "gpt-4o").expect("gpt");
    assert!(!gpt.free);
    assert!(gpt.recommended);
    assert!(!gpt.deprecated);

    let old = models
        .iter()
        .find(|m| m.model_id == "gpt-old")
        .expect("old");
    assert!(old.deprecated);
    assert!(!old.recommended);

    let pickle = models
        .iter()
        .find(|m| m.model_id == "big-pickle")
        .expect("pickle");
    assert!(pickle.free);
    assert!(pickle.recommended);

    let free2 = models
        .iter()
        .find(|m| m.model_id == "free-2")
        .expect("free2");
    assert!(free2.free);
    assert!(!free2.recommended);
}

#[test]
fn test_connection_connected() {
    let server = FakeServer::start();
    let result = connector(&server)
        .test_connection("opencode", "big-pickle")
        .expect("test");
    assert_eq!(result.outcome, ConnectionTestOutcome::Connected);
    assert_eq!(result.message, "Conectado.");
    assert!(server.prompt_called());
}

#[test]
fn test_connection_401_is_credential_invalid() {
    let server = FakeServer::start();
    server.set_prompt_status(401);
    let result = connector(&server)
        .test_connection("openai", "gpt-4o")
        .expect("test");
    assert_eq!(result.outcome, ConnectionTestOutcome::CredentialInvalid);
}

#[test]
fn test_connection_403_is_credential_invalid() {
    let server = FakeServer::start();
    server.set_prompt_status(403);
    let result = connector(&server)
        .test_connection("openai", "gpt-4o")
        .expect("test");
    assert_eq!(result.outcome, ConnectionTestOutcome::CredentialInvalid);
}

#[test]
fn test_connection_404_is_no_compatible_model() {
    let server = FakeServer::start();
    server.set_prompt_status(404);
    let result = connector(&server)
        .test_connection("openai", "missing")
        .expect("test");
    assert_eq!(result.outcome, ConnectionTestOutcome::NoCompatibleModel);
}

#[test]
fn test_connection_5xx_is_provider_unavailable() {
    let server = FakeServer::start();
    server.set_prompt_status(500);
    let result = connector(&server)
        .test_connection("openai", "gpt-4o")
        .expect("test");
    assert_eq!(result.outcome, ConnectionTestOutcome::ProviderUnavailable);
}

#[test]
fn test_connection_timeout_is_provider_unavailable() {
    let server = FakeServer::start();
    server.set_status_sequence(&["working"]);
    server.set_status_delay(Duration::from_millis(40));
    let connector = OpenCodeProviderConnector::new(ready_backend(&server))
        .with_scratch_root(unique_config_dir())
        .with_task_timeout(Duration::from_millis(80));
    let result = connector.test_connection("openai", "gpt-4o").expect("test");
    assert_eq!(result.outcome, ConnectionTestOutcome::ProviderUnavailable);
}

#[test]
fn test_connection_refused_is_network_error() {
    let backend = OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), unique_config_dir(), 0);
    backend.set_base_url("http://127.0.0.1:1".into());
    let connector = OpenCodeProviderConnector::new(Arc::new(backend))
        .with_scratch_root(unique_config_dir())
        .with_task_timeout(Duration::from_secs(1));
    let result = connector.test_connection("openai", "gpt-4o");
    match result {
        Ok(test) => assert_eq!(test.outcome, ConnectionTestOutcome::NetworkError),
        Err(err) => assert_eq!(err, ProviderError::NetworkError),
    }
}

#[test]
fn malformed_integration_json_does_not_panic() {
    let server = FakeServer::start();
    server.set_malformed_integrations();
    let err = connector(&server).list_providers().expect_err("malformed");
    assert!(
        matches!(
            err,
            ProviderError::Internal(_) | ProviderError::ProviderUnavailable
        ),
        "{err:?}"
    );
    let displayed = err.to_string();
    assert!(!displayed.contains("not-json"));
}

#[test]
fn provider_detail_unknown_is_not_found() {
    let server = FakeServer::start();
    let err = connector(&server)
        .provider_detail("no-such")
        .expect_err("missing");
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}
