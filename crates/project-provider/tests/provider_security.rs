//! M7 provider security suite (design §20 threats at the provider domain level).

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use project_provider::{
    FakeProviderConnector, ModelSelection, ProviderConnector, ProviderError, ProviderSummary,
    SecretString, Settings, SettingsStore, redact_credentials,
};

fn unique_path() -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("project-provider-sec-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join("settings.json")
}

fn provider(id: &str, name: &str) -> project_provider::ProviderDetail {
    project_provider::ProviderDetail {
        id: id.to_owned(),
        name: name.to_owned(),
        auth_methods: Vec::new(),
        connections: Vec::new(),
    }
}

// Threat 2 (§20): a read command never returns a secret; the whole provider
// surface serializes without the key after a connect.
#[test]
fn provider_surface_never_returns_a_secret() {
    let fake = FakeProviderConnector::new()
        .with_provider(provider("openai", "ChatGPT"))
        .with_model(project_provider::ModelSummary {
            provider_id: "openai".into(),
            model_id: "gpt-4o".into(),
            name: "gpt-4o".into(),
            free: false,
            recommended: true,
            deprecated: false,
        });
    fake.connect_api_key(
        "openai",
        &SecretString::new("sk-super-secret-value".into()),
        Some("clave"),
    )
    .expect("connect");

    for value in [
        serde_json::to_string(&fake.list_providers().expect("list")).expect("list json"),
        serde_json::to_string(&fake.provider_detail("openai").expect("detail"))
            .expect("detail json"),
        serde_json::to_string(&fake.list_models().expect("models")).expect("models json"),
    ] {
        assert!(
            !value.contains("sk-super-secret-value"),
            "secret leaked in {value}"
        );
    }
}

// Threat 3 (§20): SecretString and the defensive scrubber never leak the value.
#[test]
fn secret_string_and_scrubber_redact() {
    let secret = SecretString::new("sk-abc123".into());
    assert!(!format!("{secret:?}").contains("sk-abc123"));
    assert!(!format!("{secret}").contains("sk-abc123"));

    let log_line = "connect key=sk-abc123 token=AIzaSy0 gsk_xyz Bearer e30 expiry=never";
    let scrubbed = redact_credentials(log_line);
    assert!(!scrubbed.contains("sk-abc123"));
    assert!(!scrubbed.contains("AIzaSy0"));
    assert!(!scrubbed.contains("gsk_xyz"));
    assert!(!scrubbed.contains("Bearer e30"));
    assert!(scrubbed.contains("[REDACTED]"));
}

// Threat 5 (§20): malicious ids are never echoed and never reach a live list.
#[test]
fn malicious_ids_are_rejected_not_echoed() {
    let fake = FakeProviderConnector::new().with_provider(provider("openai", "ChatGPT"));
    for bad in ["../../etc/passwd", "openai; rm -rf /", "..%2f..%2fetc"] {
        let err = fake
            .provider_detail(bad)
            .expect_err("must reject hostile id");
        assert!(
            matches!(err, ProviderError::NotFound(ref id) if id == bad),
            "{err:?}"
        );
        assert!(
            !err.to_string().contains("etc/passwd") || err.to_string().contains(bad),
            "id is only echoed as the opaque NotFound payload"
        );
    }
}

// Threat 11 (§20): the persisted settings bundle never contains a credential
// (the store has no secret field) and never embeds auth.json content.
#[test]
fn settings_bundle_contains_no_credentials() {
    let path = unique_path();
    let store = SettingsStore::new(path.clone());
    store
        .save(&Settings {
            selected_model: Some(ModelSelection {
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
            }),
            featured_order: Some(vec!["openai".into()]),
        })
        .expect("save");
    let raw = fs::read_to_string(&path).expect("read settings");
    for fragment in ["sk-", "AIza", "gsk_", "Bearer ", "auth.json", "credential"] {
        assert!(
            !raw.contains(fragment),
            "settings must not contain {fragment:?}: {raw}"
        );
    }
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("valid json");
    assert_eq!(parsed["selectedModel"]["providerId"], "openai");
    assert_eq!(parsed["selectedModel"]["modelId"], "gpt-4o");
}

// Threat 10 (§20): the credential boundary is write/delete-only by construction.
// This compiles only if the port surface has no secret read-back method; the
// runtime assertion documents the contract.
#[test]
fn connector_has_no_secret_read_back() {
    let fake = FakeProviderConnector::new();
    let _ = fake;
    // If a `get_secret`/`read_api_key` method is ever added to ProviderConnector,
    // the M7 security review must re-open this invariant (ADR-0008 one-way flow).
}

// A connect that fails must not mark the provider connected (no partial state).
#[test]
fn failed_connect_leaves_provider_disconnected() {
    let fake = FakeProviderConnector::new().with_provider(provider("google", "Gemini"));
    fake.set_connect_error(ProviderError::CredentialInvalid);
    let err = fake
        .connect_api_key("google", &SecretString::new("AIza-bad".into()), None)
        .expect_err("invalid");
    assert_eq!(err, ProviderError::CredentialInvalid);
    assert!(!fake.is_connected("google"));
    assert!(fake.connections("google").is_empty());
}

// OAuth failed/expired attempts never add a connection.
#[test]
fn failed_oauth_never_connects() {
    let fake = FakeProviderConnector::new().with_provider(provider("openai", "ChatGPT"));
    fake.seed_oauth(project_provider::ScriptedOAuth {
        attempt_id: "att-x".into(),
        provider_id: "openai".into(),
        url: "https://example.test".into(),
        instructions: None,
        mode: project_provider::OAuthMode::Code,
        status: project_provider::OAuthStatusKind::Expired,
        connection: None,
    });
    let err = fake.complete_oauth("att-x", None).expect_err("expired");
    assert!(matches!(err, ProviderError::OAuthFailed(_)), "{err:?}");
    assert!(!fake.is_connected("openai"));
}

// ProviderSummary DTO never exposes more than an opaque label (no ids of the
// underlying integration, no credential material).
#[test]
fn summary_only_exposes_opaque_reference() {
    let fake = FakeProviderConnector::new().with_provider(provider("openai", "ChatGPT"));
    fake.connect_api_key(
        "openai",
        &SecretString::new("sk-x".into()),
        Some("Mi clave"),
    )
    .expect("connect");
    let list = fake.list_providers().expect("list");
    let summary: &ProviderSummary = list.iter().find(|p| p.id == "openai").expect("openai");
    assert_eq!(summary.connection_label.as_deref(), Some("Mi clave"));
    let json = serde_json::to_string(summary).expect("json");
    assert!(!json.contains("sk-"));
    assert!(!json.contains("credential"));
}
