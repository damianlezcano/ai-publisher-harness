//! M7 provider unit tests: model serialization, SecretString redaction, and
//! FakeProviderConnector behavior (connect/OAuth/disconnect/models/test).

use project_provider::{
    AuthMethodKind, AuthMethodView, AuthPrompt, AuthPromptKind, ConnectionTestOutcome,
    ConnectionView, FakeProviderConnector, ModelSummary, OAuthMode, OAuthStatusKind, ProviderCall,
    ProviderConnector, ProviderDetail, ProviderError, ProviderSummary, ScriptedOAuth, SecretString,
};

fn api_key_method() -> AuthMethodView {
    AuthMethodView {
        kind: AuthMethodKind::ApiKey,
        method_id: None,
        label: "Clave de acceso".into(),
        prompts: Vec::new(),
    }
}

fn account_method(method_id: &str) -> AuthMethodView {
    AuthMethodView {
        kind: AuthMethodKind::Account,
        method_id: Some(method_id.to_owned()),
        label: "Conectá tu cuenta".into(),
        prompts: vec![AuthPrompt {
            key: "url".into(),
            message: "URL de tu organización".into(),
            kind: AuthPromptKind::Text,
            options: Vec::new(),
            placeholder: Some("https://...".into()),
            optional: true,
        }],
    }
}

fn provider_detail(id: &str, name: &str) -> ProviderDetail {
    ProviderDetail {
        id: id.to_owned(),
        name: name.to_owned(),
        auth_methods: vec![api_key_method()],
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

// -- Serialization -----------------------------------------------------------

#[test]
fn provider_summary_serializes_camel_case() {
    let summary = ProviderSummary {
        id: "openai".into(),
        name: "ChatGPT".into(),
        auth_methods: vec![api_key_method(), account_method("chatgpt-browser")],
        connected: true,
        connection_label: Some("Mi clave".into()),
        highlighted: true,
    };
    let json = serde_json::to_value(&summary).expect("serialize");
    let obj = json.as_object().expect("object");
    assert!(obj.contains_key("authMethods"));
    assert!(obj.contains_key("connectionLabel"));
    assert!(!obj.contains_key("auth_methods"));
    assert_eq!(obj["authMethods"][0]["kind"], "api_key");
    assert_eq!(obj["authMethods"][1]["kind"], "account");
    assert_eq!(obj["authMethods"][1]["methodId"], "chatgpt-browser");
    assert_eq!(obj["authMethods"][1]["prompts"][0]["kind"], "text");
    assert_eq!(obj["connected"], true);
    assert_eq!(obj["highlighted"], true);
}

#[test]
fn oauth_and_test_outcome_serialize_snake_case() {
    assert_eq!(
        serde_json::to_value(OAuthMode::Auto).expect("auto"),
        serde_json::json!("auto")
    );
    assert_eq!(
        serde_json::to_value(OAuthStatusKind::Pending).expect("pending"),
        serde_json::json!("pending")
    );
    assert_eq!(
        serde_json::to_value(ConnectionTestOutcome::CredentialInvalid).expect("invalid"),
        serde_json::json!("credential_invalid")
    );
    assert_eq!(
        serde_json::to_value(ConnectionTestOutcome::NoCompatibleModel).expect("no model"),
        serde_json::json!("no_compatible_model")
    );
    assert_eq!(
        serde_json::to_value(ConnectionTestOutcome::ProviderUnavailable).expect("unavailable"),
        serde_json::json!("provider_unavailable")
    );
    assert_eq!(
        serde_json::to_value(ConnectionTestOutcome::NetworkError).expect("network"),
        serde_json::json!("network_error")
    );
}

#[test]
fn connection_view_never_carries_a_secret_field() {
    let view = ConnectionView {
        id: "cred-1".into(),
        label: Some("clave".into()),
    };
    let obj = serde_json::to_value(&view)
        .expect("serialize")
        .as_object()
        .expect("object")
        .clone();
    assert_eq!(obj.len(), 2);
    assert!(!obj.contains_key("key"));
    assert!(!obj.contains_key("secret"));
    assert!(!obj.contains_key("token"));
}

// -- SecretString (redaction tests also live in src/secret.rs) ---------------

#[test]
fn connect_key_never_reaches_dtos() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("openai", "ChatGPT"));
    let secret = SecretString::new("sk-super-secret-value".into());
    let state = fake
        .connect_api_key("openai", &secret, Some("mi clave"))
        .expect("connect");
    let serialized = serde_json::to_string(&state).expect("serialize");
    assert!(!serialized.contains("sk-super-secret-value"));
    assert!(state.connected);
    let connection = state.connection.expect("connection");
    assert_eq!(connection.id, "cred-1");
    assert_eq!(connection.label.as_deref(), Some("mi clave"));
}

// -- FakeProviderConnector behavior ------------------------------------------

#[test]
fn list_providers_projects_connection_state_and_highlighted() {
    let fake = FakeProviderConnector::new()
        .with_provider(provider_detail("openai", "ChatGPT"))
        .with_provider(provider_detail("deepseek", "DeepSeek"))
        .with_featured("openai");
    fake.connect_api_key("openai", &SecretString::new("sk-x".into()), Some("clave"))
        .expect("connect");

    let providers = fake.list_providers().expect("list");
    assert_eq!(providers.len(), 2);
    let openai = providers.iter().find(|p| p.id == "openai").expect("openai");
    assert!(openai.connected);
    assert!(openai.highlighted);
    assert_eq!(openai.connection_label.as_deref(), Some("clave"));
    let deepseek = providers
        .iter()
        .find(|p| p.id == "deepseek")
        .expect("deepseek");
    assert!(!deepseek.connected);
    assert!(!deepseek.highlighted);
    assert!(fake.is_connected("openai"));
    assert!(!fake.is_connected("deepseek"));
}

#[test]
fn connect_api_key_records_key_and_label() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("anthropic", "Claude"));
    let secret = SecretString::new("sk-ant-abc".into());
    fake.connect_api_key("anthropic", &secret, Some("cuenta principal"))
        .expect("connect");
    assert_eq!(fake.last_connect_key().as_deref(), Some("sk-ant-abc"));
    assert_eq!(
        fake.last_connect_label().as_deref(),
        Some("cuenta principal")
    );
    assert!(fake.calls().contains(&ProviderCall::ConnectApiKey));
}

#[test]
fn connect_api_key_unknown_provider_is_not_found() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("openai", "ChatGPT"));
    let err = fake
        .connect_api_key("nope", &SecretString::new("sk-x".into()), None)
        .expect_err("unknown provider");
    assert!(
        matches!(err, ProviderError::NotFound(ref id) if id == "nope"),
        "{err:?}"
    );
}

#[test]
fn connect_error_injection_maps_through() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("google", "Gemini"));
    fake.set_connect_error(ProviderError::CredentialInvalid);
    let err = fake
        .connect_api_key("google", &SecretString::new("AIza-bad".into()), None)
        .expect_err("invalid");
    assert_eq!(err, ProviderError::CredentialInvalid);
    assert!(!fake.is_connected("google"));
}

#[test]
fn oauth_begin_status_complete_flow() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("openai", "ChatGPT"));
    let attempt = fake
        .begin_oauth("openai", "chatgpt-browser")
        .expect("begin");
    assert_eq!(attempt.mode, OAuthMode::Auto);
    assert!(attempt.url.starts_with("https://"));
    assert!(attempt.instructions.is_some());

    let status = fake.oauth_status(&attempt.attempt_id).expect("status");
    assert_eq!(status.status, OAuthStatusKind::Pending);

    let state = fake
        .complete_oauth(&attempt.attempt_id, None)
        .expect("complete");
    assert!(state.connected);
    assert!(fake.is_connected("openai"));
    assert_eq!(fake.connections("openai").len(), 1);
}

#[test]
fn oauth_seeded_failed_or_expired_cannot_complete() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("openai", "ChatGPT"));
    for status in [OAuthStatusKind::Failed, OAuthStatusKind::Expired] {
        fake.seed_oauth(ScriptedOAuth {
            attempt_id: "attempt-fail".into(),
            provider_id: "openai".into(),
            url: "https://example.test".into(),
            instructions: None,
            mode: OAuthMode::Code,
            status,
            connection: None,
        });
        let err = fake
            .complete_oauth("attempt-fail", Some("123456"))
            .expect_err("failed oauth");
        assert!(matches!(err, ProviderError::OAuthFailed(_)), "{err:?}");
        assert!(!fake.is_connected("openai"));
    }
}

#[test]
fn oauth_cancel_removes_attempt() {
    let fake =
        FakeProviderConnector::new().with_provider(provider_detail("opencode", "OpenCode Zen"));
    let attempt = fake.begin_oauth("opencode", "zen").expect("begin");
    fake.cancel_oauth(&attempt.attempt_id).expect("cancel");
    let err = fake.oauth_status(&attempt.attempt_id).expect_err("gone");
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[test]
fn begin_oauth_unknown_provider_or_injection() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("openai", "ChatGPT"));
    let err = fake.begin_oauth("nope", "m").expect_err("unknown provider");
    assert!(
        matches!(err, ProviderError::NotFound(ref id) if id == "nope"),
        "{err:?}"
    );

    fake.set_begin_oauth_error(ProviderError::OAuthFailed("injected".into()));
    let err = fake
        .begin_oauth("openai", "chatgpt-browser")
        .expect_err("injected");
    assert!(matches!(err, ProviderError::OAuthFailed(_)), "{err:?}");
}

#[test]
fn disconnect_removes_only_target_credential() {
    let fake = FakeProviderConnector::new()
        .with_provider(provider_detail("google", "Gemini"))
        .with_provider(provider_detail("deepseek", "DeepSeek"));
    fake.connect_api_key("google", &SecretString::new("AIza-a".into()), None)
        .expect("connect google");
    fake.connect_api_key("deepseek", &SecretString::new("sk-d".into()), None)
        .expect("connect deepseek");

    fake.disconnect("cred-1").expect("disconnect");
    assert!(!fake.is_connected("google"));
    assert!(fake.is_connected("deepseek"));
    assert_eq!(fake.connections("google").len(), 0);

    let err = fake.disconnect("cred-1").expect_err("already gone");
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[test]
fn connection_ids_are_unique_per_connect() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("openai", "ChatGPT"));
    fake.connect_api_key("openai", &SecretString::new("sk-a".into()), None)
        .expect("first");
    fake.connect_api_key("openai", &SecretString::new("sk-b".into()), None)
        .expect("second");
    let ids: Vec<_> = fake
        .connections("openai")
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, vec!["cred-1".to_owned(), "cred-2".to_owned()]);
    assert_eq!(fake.connections("openai").len(), 2);
}

#[test]
fn disconnect_error_injection() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("openai", "ChatGPT"));
    fake.connect_api_key("openai", &SecretString::new("sk-x".into()), None)
        .expect("connect");
    fake.set_disconnect_error(ProviderError::DisconnectFailed("injected".into()));
    let err = fake.disconnect("cred-1").expect_err("injected");
    assert!(matches!(err, ProviderError::DisconnectFailed(_)), "{err:?}");
    assert!(fake.is_connected("openai"));
}

#[test]
fn list_models_returns_scripted_models() {
    let fake = FakeProviderConnector::new()
        .with_model(free_model("opencode", "nemotron-free"))
        .with_model(free_model("opencode", "mimo-free"));
    let models = fake.list_models().expect("list");
    assert_eq!(models.len(), 2);
    assert!(models.iter().all(|m| m.free));
    assert!(fake.calls().contains(&ProviderCall::ListModels));
}

#[test]
fn test_connection_returns_scripted_outcome() {
    let fake = FakeProviderConnector::new();
    for (outcome, message) in [
        (ConnectionTestOutcome::Connected, "Conectado."),
        (
            ConnectionTestOutcome::CredentialInvalid,
            "Esta clave no es válida.",
        ),
        (
            ConnectionTestOutcome::ProviderUnavailable,
            "No pudimos conectarnos con el proveedor.",
        ),
        (
            ConnectionTestOutcome::NoCompatibleModel,
            "Este modelo ya no está disponible.",
        ),
        (
            ConnectionTestOutcome::NetworkError,
            "No hay conexión con el proveedor.",
        ),
    ] {
        fake.set_test_outcome(outcome, message);
        let test = fake.test_connection("openai", "gpt-4.1").expect("test");
        assert_eq!(test.outcome, outcome);
        assert_eq!(test.message, message);
    }
}

#[test]
fn provider_detail_returns_connections() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("anthropic", "Claude"));
    fake.connect_api_key(
        "anthropic",
        &SecretString::new("sk-ant".into()),
        Some("clave"),
    )
    .expect("connect");
    let detail = fake.provider_detail("anthropic").expect("detail");
    assert_eq!(detail.connections.len(), 1);
    assert_eq!(detail.connections[0].id, "cred-1");
}

#[test]
fn provider_detail_unknown_is_not_found() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("openai", "ChatGPT"));
    let err = fake.provider_detail("nope").expect_err("unknown");
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[test]
fn serde_round_trips_all_design_strings() {
    use project_provider::{ConnectionState, OAuthAttempt, OAuthStatus};

    for (kind, json) in [
        (OAuthStatusKind::Pending, "pending"),
        (OAuthStatusKind::Complete, "complete"),
        (OAuthStatusKind::Failed, "failed"),
        (OAuthStatusKind::Expired, "expired"),
    ] {
        assert_eq!(
            serde_json::to_value(kind).expect("to"),
            serde_json::json!(json)
        );
        let back: OAuthStatusKind = serde_json::from_value(serde_json::json!(json)).expect("back");
        assert_eq!(back, kind);
    }

    assert_eq!(
        serde_json::from_value::<OAuthMode>(serde_json::json!("code")).expect("code"),
        OAuthMode::Code
    );
    assert_eq!(
        serde_json::from_value::<AuthPromptKind>(serde_json::json!("select")).expect("select"),
        AuthPromptKind::Select
    );
    assert_eq!(
        serde_json::from_value::<AuthMethodKind>(serde_json::json!("account")).expect("account"),
        AuthMethodKind::Account
    );
    assert_eq!(
        serde_json::from_value::<ConnectionTestOutcome>(serde_json::json!("connected"))
            .expect("connected"),
        ConnectionTestOutcome::Connected
    );

    let summary = ProviderSummary {
        id: "opencode".into(),
        name: "Gratis".into(),
        auth_methods: Vec::new(),
        connected: true,
        connection_label: None,
        highlighted: true,
    };
    let json = serde_json::to_value(&summary).expect("summary to");
    let back: ProviderSummary = serde_json::from_value(json).expect("summary back");
    assert_eq!(back, summary);

    let model = free_model("opencode", "mimo-free");
    let json = serde_json::to_value(&model).expect("model to");
    assert_eq!(json["providerId"], "opencode");
    assert_eq!(json["modelId"], "mimo-free");
    assert_eq!(json["free"], true);
    let back: ModelSummary = serde_json::from_value(json).expect("model back");
    assert_eq!(back, model);

    let status = OAuthStatus {
        status: OAuthStatusKind::Pending,
        message: Some("esperando".into()),
    };
    let json = serde_json::to_value(&status).expect("status to");
    assert_eq!(json["status"], "pending");
    let back: OAuthStatus = serde_json::from_value(json).expect("status back");
    assert_eq!(back, status);

    let attempt = OAuthAttempt {
        attempt_id: "a1".into(),
        url: "https://example.test".into(),
        instructions: None,
        mode: OAuthMode::Code,
    };
    let json = serde_json::to_value(&attempt).expect("attempt to");
    assert_eq!(json["mode"], "code");
    let back: OAuthAttempt = serde_json::from_value(json).expect("attempt back");
    assert_eq!(back, attempt);

    let state = ConnectionState {
        connected: true,
        connection: Some(ConnectionView {
            id: "cred-1".into(),
            label: None,
        }),
    };
    let json = serde_json::to_value(&state).expect("state to");
    assert_eq!(json["connected"], true);
    assert_eq!(json["connection"]["id"], "cred-1");
    let back: ConnectionState = serde_json::from_value(json).expect("state back");
    assert_eq!(back, state);
}

#[test]
fn model_catalog_disappearance_is_scriptable() {
    let fake = FakeProviderConnector::new()
        .with_model(free_model("opencode", "nemotron-free"))
        .with_model(free_model("opencode", "mimo-free"));
    assert_eq!(fake.list_models().expect("list").len(), 2);

    fake.set_models(vec![free_model("opencode", "mimo-free")]);
    let models = fake.list_models().expect("list");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].model_id, "mimo-free");

    fake.set_models(Vec::new());
    assert!(fake.list_models().expect("list").is_empty());
}

#[test]
fn fake_public_debug_redacts_retained_key() {
    let fake = FakeProviderConnector::new().with_provider(provider_detail("openai", "ChatGPT"));
    fake.connect_api_key("openai", &SecretString::new("sk-super-secret".into()), None)
        .expect("connect");
    let debug = format!("{fake:?}");
    assert!(!debug.contains("sk-super-secret"));
    assert!(debug.contains("[REDACTED]"));
}
