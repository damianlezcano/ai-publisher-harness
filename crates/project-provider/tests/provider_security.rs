//! M7 provider security suite (design §20 threats at the provider domain level).

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fake_opencode_server::FakeServer;
use project_opencode::OpenCodeBackend;
use project_provider::{
    FakeProviderConnector, ModelSelection, OpenCodeProviderConnector, ProviderConnector,
    ProviderError, ProviderSummary, SecretString, Settings, SettingsStore, redact_credentials,
};

fn unique_path() -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("project-provider-sec-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join("settings.json")
}

fn unique_config_dir() -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "project-provider-sec-cfg-{}-{n}",
        std::process::id()
    ))
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

// Threat 3 (§20): SecretString, the defensive scrubber, and the adapter's own
// `[provider]` log line never leak the value.
#[test]
fn secret_string_and_scrubber_redact() {
    let secret = SecretString::new("sk-abc123".into());
    assert!(!format!("{secret:?}").contains("sk-abc123"));
    assert!(!format!("{secret}").contains("sk-abc123"));

    // Matches the adapter's log format (`[provider] {event}`) with a secret
    // accidentally embedded; the §19 belt-and-suspenders scrubber removes it.
    let log_line = "[provider] connected key=sk-abc123 token=AIzaSy0 gsk_xyz Bearer e30";
    let scrubbed = redact_credentials(log_line);
    assert!(!scrubbed.contains("sk-abc123"));
    assert!(!scrubbed.contains("AIzaSy0"));
    assert!(!scrubbed.contains("gsk_xyz"));
    assert!(!scrubbed.contains("Bearer e30"));
    assert!(scrubbed.contains("[REDACTED]"));
}

// Threat 5 (§20): malicious ids are rejected; the human-facing Display never
// echoes them.
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
            !err.to_string().contains(bad),
            "Display must never echo the raw hostile id: {err}"
        );
    }
}

// Threat 5 (§20), adapter path: hostile ids interpolated into the connect URL
// resolve to not-found (the fake server only knows real integration ids), and
// the error's Display still never echoes the raw id.
#[test]
fn malicious_id_through_the_adapter_is_not_found_not_echoed() {
    let server = FakeServer::start();
    let backend = Arc::new(OpenCodeBackend::new(
        std::path::PathBuf::from("/usr/bin/true"),
        unique_config_dir(),
        0,
    ));
    backend.set_base_url(server.base_url());
    backend.ensure_ready().expect("ready");
    let connector = OpenCodeProviderConnector::new(Arc::clone(&backend));
    for bad in ["../../etc/passwd", "openai; rm -rf /"] {
        let err = connector
            .connect_api_key(bad, &SecretString::new("sk-x".into()), None)
            .expect_err("hostile id");
        assert!(
            matches!(err, ProviderError::NotFound(ref id) if id == bad),
            "unknown ids must map to not_found, got {err:?}"
        );
        assert!(
            !err.to_string().contains(bad),
            "Display must never echo the raw hostile id: {err}"
        );
    }
}

// Threat 4 (§20): the backend child env never carries a secret or a
// secret-shaped variable; the serve argv is loopback-only; and the connect/key
// body only ever reaches the loopback server (captured by our fake server).
#[test]
fn backend_env_and_loopback_never_carry_secrets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_dir = temp.path().join("app-data").join("opencode");
    let env = project_opencode::build_env(&config_dir);
    for (key, value) in &env {
        for fragment in [
            "TOKEN",
            "API_KEY",
            "SECRET",
            "PASSWORD",
            "CREDENTIAL",
            "AWS",
        ] {
            assert!(
                !key.to_ascii_uppercase().contains(fragment),
                "backend env must not expose {fragment}: {key}"
            );
        }
        assert!(
            !value.contains("sk-"),
            "backend env must not carry a secret: {value}"
        );
    }

    let argv = project_opencode::build_argv(4567);
    assert!(argv.windows(2).any(|w| w == ["--hostname", "127.0.0.1"]));
    assert!(!argv.iter().any(|a| a == "--mdns"));
    assert!(!argv.iter().any(|a| a == "0.0.0.0"));

    // The connect/key body is sent only to the loopback server we control.
    let server = FakeServer::start();
    assert!(server.base_url().starts_with("http://127.0.0.1:"));
    let backend = Arc::new(OpenCodeBackend::new(
        std::path::PathBuf::from("/usr/bin/true"),
        unique_config_dir(),
        0,
    ));
    backend.set_base_url(server.base_url());
    backend.ensure_ready().expect("ready");
    let connector = OpenCodeProviderConnector::new(Arc::clone(&backend));
    connector
        .connect_api_key(
            "openai",
            &SecretString::new("sk-loopback-only".into()),
            None,
        )
        .expect("connect");
    assert_eq!(
        server.last_connect_key().as_deref(),
        Some("sk-loopback-only"),
        "connect/key must have reached the loopback server"
    );
}

// Threat 6 (§20): a 401/403 on a provider that already has a stored credential
// is a revocation, surfaced as CredentialRevoked (the app maps it to "volver a
// conectar").
#[test]
fn revoked_credential_is_detected_by_connection_test() {
    let server = FakeServer::start();
    let backend = Arc::new(OpenCodeBackend::new(
        std::path::PathBuf::from("/usr/bin/true"),
        unique_config_dir(),
        0,
    ));
    backend.set_base_url(server.base_url());
    backend.ensure_ready().expect("ready");
    let connector = OpenCodeProviderConnector::new(Arc::clone(&backend));

    connector
        .connect_api_key("openai", &SecretString::new("sk-ok".into()), None)
        .expect("connect");

    // The provider now holds a credential; a 401 on the next test is a revocation.
    server.set_prompt_status(401);
    let err = connector
        .test_connection("openai", "gpt-4o")
        .expect_err("revoked");
    assert_eq!(err, ProviderError::CredentialRevoked);

    // Without a stored credential the same signal is an invalid key.
    let server2 = FakeServer::start();
    let backend2 = Arc::new(OpenCodeBackend::new(
        std::path::PathBuf::from("/usr/bin/true"),
        unique_config_dir(),
        0,
    ));
    backend2.set_base_url(server2.base_url());
    backend2.ensure_ready().expect("ready");
    let connector2 = OpenCodeProviderConnector::new(Arc::clone(&backend2));
    server2.set_prompt_status(401);
    let test = connector2
        .test_connection("google", "gemini-2")
        .expect("test");
    assert_eq!(
        test.outcome,
        project_provider::ConnectionTestOutcome::CredentialInvalid
    );
}

// Threat 9 (§20): the isolated OpenCode config/data/cache/state stay under the
// managed root; they never resolve to the developer's real opencode dirs or a
// project dir.
#[test]
fn managed_root_isolates_opencode_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_dir = temp.path().join("app-data").join("opencode");
    let env = project_opencode::build_env(&config_dir);

    let value = |key: &str| {
        env.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    assert_eq!(value("XDG_CONFIG_HOME"), config_dir.display().to_string());
    assert_eq!(
        value("XDG_DATA_HOME"),
        config_dir.join("data").display().to_string()
    );
    assert_eq!(
        value("XDG_CACHE_HOME"),
        config_dir.join("cache").display().to_string()
    );
    assert_eq!(
        value("XDG_STATE_HOME"),
        config_dir.join("state").display().to_string()
    );

    let home = std::env::var("HOME").unwrap_or_default();
    let real_config = std::path::Path::new(&home).join(".config/opencode");
    let real_data = std::path::Path::new(&home).join(".local/share/opencode");
    for (_, v) in &env {
        assert_ne!(v, &real_config.display().to_string());
        assert_ne!(v, &real_data.display().to_string());
        assert!(
            !std::path::Path::new(v).starts_with(temp.path().join("projects")),
            "backend state must never land in a project dir"
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

// Threat 10 (§20): the credential boundary is write/delete-only by
// construction. Assert the only credential references that cross the boundary
// are opaque `{id, label}` shapes with no secret field.
#[test]
fn credential_references_are_opaque_id_label_only() {
    let view = project_provider::ConnectionView {
        id: "cred-1".into(),
        label: Some("clave".into()),
    };
    let obj = serde_json::to_value(&view)
        .expect("serialize")
        .as_object()
        .expect("object")
        .clone();
    assert_eq!(obj.len(), 2, "ConnectionView must expose only id + label");
    for key in ["key", "secret", "token", "apiKey"] {
        assert!(
            !obj.contains_key(key),
            "ConnectionView must not expose {key}"
        );
    }
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
