//! Offline tests for `ProviderService` selection, persistence, test-connection,
//! featured highlighting, and restart-on-mutation.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use project_provider::{
    ConnectionTest, ConnectionTestOutcome, ConnectionView, FakeProviderConnector, FakeRestarter,
    ModelSelection, ModelSummary, OAuthAttempt, OAuthStatus, ProviderConnector, ProviderDetail,
    ProviderError, ProviderService, ProviderSummary, SecretString, Settings, SettingsStore,
};

fn unique_settings_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "project-provider-service-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp settings dir");
    dir.join("settings.json")
}

fn provider_detail(id: &str, name: &str) -> ProviderDetail {
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

fn catalog_connector() -> FakeProviderConnector {
    FakeProviderConnector::new()
        .with_provider(provider_detail("opencode", "Gratis"))
        .with_provider(provider_detail("openai", "ChatGPT"))
        .with_provider(provider_detail("google", "Gemini"))
        .with_model(model("opencode", "big-pickle", true, true))
        .with_model(model("opencode", "other-free", true, false))
        .with_model(model("openai", "gpt-paid", false, true))
}

fn service(
    connector: FakeProviderConnector,
    restarter: FakeRestarter,
    path: PathBuf,
) -> ProviderService<FakeProviderConnector, FakeRestarter> {
    ProviderService::new(connector, restarter, path)
}

/// Records `test_connection` model ids without changing `FakeProviderConnector`.
struct RecordingConnector {
    inner: FakeProviderConnector,
    last_test_model: Arc<Mutex<Option<String>>>,
}

impl RecordingConnector {
    fn new(inner: FakeProviderConnector) -> Self {
        Self {
            inner,
            last_test_model: Arc::new(Mutex::new(None)),
        }
    }
}

impl ProviderConnector for RecordingConnector {
    fn list_providers(&self) -> project_provider::ProviderResult<Vec<ProviderSummary>> {
        self.inner.list_providers()
    }

    fn provider_detail(
        &self,
        provider_id: &str,
    ) -> project_provider::ProviderResult<ProviderDetail> {
        self.inner.provider_detail(provider_id)
    }

    fn connect_api_key(
        &self,
        provider_id: &str,
        key: &SecretString,
        label: Option<&str>,
    ) -> project_provider::ProviderResult<project_provider::ConnectionState> {
        self.inner.connect_api_key(provider_id, key, label)
    }

    fn begin_oauth(
        &self,
        provider_id: &str,
        method_id: &str,
    ) -> project_provider::ProviderResult<OAuthAttempt> {
        self.inner.begin_oauth(provider_id, method_id)
    }

    fn oauth_status(&self, attempt_id: &str) -> project_provider::ProviderResult<OAuthStatus> {
        self.inner.oauth_status(attempt_id)
    }

    fn complete_oauth(
        &self,
        attempt_id: &str,
        code: Option<&str>,
    ) -> project_provider::ProviderResult<project_provider::ConnectionState> {
        self.inner.complete_oauth(attempt_id, code)
    }

    fn cancel_oauth(&self, attempt_id: &str) -> project_provider::ProviderResult<()> {
        self.inner.cancel_oauth(attempt_id)
    }

    fn disconnect(&self, credential_id: &str) -> project_provider::ProviderResult<()> {
        self.inner.disconnect(credential_id)
    }

    fn list_models(&self) -> project_provider::ProviderResult<Vec<ModelSummary>> {
        self.inner.list_models()
    }

    fn test_connection(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> project_provider::ProviderResult<ConnectionTest> {
        *self
            .last_test_model
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(model_id.to_owned());
        self.inner.test_connection(provider_id, model_id)
    }
}

#[test]
fn default_selection_uses_opencode_free_recommended_and_does_not_persist() {
    let path = unique_settings_path();
    let svc = service(catalog_connector(), FakeRestarter::new(), path.clone());
    let selected = svc.get_selected_model().expect("select");
    assert_eq!(selected.model.provider_id, "opencode");
    assert_eq!(selected.model.model_id, "big-pickle");
    assert!(selected.notice.is_none());
    assert!(!selected.requires_choice);
    assert!(
        !path.exists(),
        "first-launch default must not write settings"
    );
}

#[test]
fn stored_live_selection_is_returned_unchanged() {
    let path = unique_settings_path();
    SettingsStore::new(path.clone())
        .save(&Settings {
            selected_model: Some(ModelSelection {
                provider_id: "opencode".into(),
                model_id: "other-free".into(),
            }),
            featured_order: None,
        })
        .expect("save");
    let svc = service(catalog_connector(), FakeRestarter::new(), path);
    let selected = svc.get_selected_model().expect("select");
    assert_eq!(selected.model.model_id, "other-free");
    assert!(selected.notice.is_none());
    assert!(!selected.requires_choice);
}

#[test]
fn disappeared_model_falls_back_to_same_provider_free_and_persists() {
    let path = unique_settings_path();
    SettingsStore::new(path.clone())
        .save(&Settings {
            selected_model: Some(ModelSelection {
                provider_id: "opencode".into(),
                model_id: "gone".into(),
            }),
            featured_order: None,
        })
        .expect("save");
    let svc = service(catalog_connector(), FakeRestarter::new(), path.clone());
    let selected = svc.get_selected_model().expect("select");
    assert_eq!(selected.model.provider_id, "opencode");
    assert_eq!(selected.model.model_id, "big-pickle");
    assert_eq!(
        selected.notice.as_deref(),
        Some("Este modelo ya no está disponible; usamos el recomendado.")
    );
    assert!(!selected.requires_choice);
    let reloaded = SettingsStore::new(path).load();
    assert_eq!(
        reloaded.selected_model,
        Some(ModelSelection {
            provider_id: "opencode".into(),
            model_id: "big-pickle".into(),
        })
    );
}

#[test]
fn disappeared_model_with_only_paid_same_provider_requires_choice() {
    let path = unique_settings_path();
    SettingsStore::new(path.clone())
        .save(&Settings {
            selected_model: Some(ModelSelection {
                provider_id: "openai".into(),
                model_id: "gone-gpt".into(),
            }),
            featured_order: None,
        })
        .expect("save");
    let connector = FakeProviderConnector::new()
        .with_provider(provider_detail("openai", "ChatGPT"))
        .with_model(model("openai", "gpt-paid", false, true))
        .with_model(model("openai", "gpt-paid-2", false, false));
    let svc = service(connector, FakeRestarter::new(), path.clone());
    let selected = svc.get_selected_model().expect("select");
    assert!(selected.requires_choice);
    assert_eq!(
        selected.notice.as_deref(),
        Some("Este modelo ya no está disponible. Elegí otro.")
    );
    assert_ne!(selected.model.model_id, "gpt-paid");
    assert_ne!(selected.model.model_id, "gpt-paid-2");
    let reloaded = SettingsStore::new(path).load();
    assert_eq!(
        reloaded
            .selected_model
            .as_ref()
            .map(|s| s.model_id.as_str()),
        Some("gone-gpt")
    );
}

#[test]
fn select_model_persists_across_service_reload() {
    let path = unique_settings_path();
    let connector = catalog_connector();
    let svc = service(connector.clone(), FakeRestarter::new(), path.clone());
    let chosen = svc
        .select_model("openai", "gpt-paid")
        .expect("explicit paid selection is allowed");
    assert_eq!(chosen.model_id, "gpt-paid");
    assert!(path.exists());

    let reloaded = service(connector, FakeRestarter::new(), path);
    let selected = reloaded.get_selected_model().expect("reload");
    assert_eq!(selected.model.provider_id, "openai");
    assert_eq!(selected.model.model_id, "gpt-paid");
    assert!(selected.notice.is_none());
    assert!(!selected.requires_choice);
}

#[test]
fn select_unknown_model_is_unavailable() {
    let path = unique_settings_path();
    let svc = service(catalog_connector(), FakeRestarter::new(), path.clone());
    let err = svc.select_model("openai", "does-not-exist").unwrap_err();
    assert_eq!(err, ProviderError::ModelUnavailable);
    assert!(!path.exists());
}

#[test]
fn test_connection_explicit_and_resolved_free_model() {
    let path = unique_settings_path();
    let inner = FakeProviderConnector::new()
        .with_provider(provider_detail("openai", "ChatGPT"))
        .with_model(model("openai", "gpt-paid", false, true))
        .with_model(model("openai", "gpt-free", true, false))
        .with_model(model("openai", "gpt-free-rec", true, true));
    inner.set_test_outcome(ConnectionTestOutcome::Connected, "Conectado.");
    let recording = RecordingConnector::new(inner);
    let last = recording.last_test_model.clone();
    let svc = ProviderService::new(recording, FakeRestarter::new(), path);

    let explicit = svc
        .test_connection("openai", Some("gpt-paid"))
        .expect("explicit");
    assert_eq!(explicit.outcome, ConnectionTestOutcome::Connected);
    assert_eq!(explicit.message, "Conectado.");
    assert_eq!(
        last.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        Some("gpt-paid".into())
    );

    let resolved = svc.test_connection("openai", None).expect("resolved");
    assert_eq!(resolved.outcome, ConnectionTestOutcome::Connected);
    assert_eq!(
        last.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        Some("gpt-free-rec".into())
    );
}

#[test]
fn test_connection_none_without_free_model_is_no_compatible() {
    let path = unique_settings_path();
    let connector = FakeProviderConnector::new()
        .with_provider(provider_detail("openai", "ChatGPT"))
        .with_model(model("openai", "gpt-paid", false, true));
    let svc = service(connector, FakeRestarter::new(), path);
    let test = svc.test_connection("openai", None).expect("none");
    assert_eq!(test.outcome, ConnectionTestOutcome::NoCompatibleModel);
    assert_eq!(
        test.message,
        "No encontramos un modelo disponible para este proveedor."
    );
}

#[test]
fn credential_mutations_restart_non_mutations_and_failures_do_not() {
    let path = unique_settings_path();
    let connector = catalog_connector();
    let restarter = FakeRestarter::new();
    let svc = service(connector.clone(), restarter.clone(), path);

    let view = svc
        .connect_api_key("openai", &SecretString::new("sk-test".into()), Some("lab"))
        .expect("connect");
    assert_eq!(view.label.as_deref(), Some("lab"));
    assert!(!format!("{view:?}").contains("sk-test"));
    assert_eq!(restarter.restart_count(), 1);

    let attempt = svc.begin_oauth("openai", "browser").expect("begin");
    assert_eq!(restarter.restart_count(), 1);
    let _ = svc.oauth_status(&attempt.attempt_id).expect("status");
    assert_eq!(restarter.restart_count(), 1);
    let _ = svc
        .complete_oauth(&attempt.attempt_id, None)
        .expect("complete");
    assert_eq!(restarter.restart_count(), 2);

    let connections = connector.connections("openai");
    svc.disconnect(&connections[0].id).expect("disconnect");
    assert_eq!(restarter.restart_count(), 3);

    let attempt2 = svc.begin_oauth("openai", "browser").expect("begin2");
    svc.cancel_oauth(&attempt2.attempt_id).expect("cancel");
    assert_eq!(restarter.restart_count(), 3);
    let _ = svc.oauth_status(&attempt2.attempt_id);
    assert_eq!(restarter.restart_count(), 3);

    connector.set_connect_error(ProviderError::CredentialInvalid);
    let failed = svc.connect_api_key("openai", &SecretString::new("bad".into()), None);
    assert!(failed.is_err());
    assert_eq!(restarter.restart_count(), 3);
}

#[test]
fn list_providers_recomputes_highlighted_from_featured_order() {
    let path = unique_settings_path();
    SettingsStore::new(path.clone())
        .save(&Settings {
            selected_model: None,
            featured_order: Some(vec![
                "google".into(),
                "does-not-exist".into(),
                "opencode".into(),
            ]),
        })
        .expect("save");
    let connector = FakeProviderConnector::new()
        .with_provider(provider_detail("openai", "ChatGPT"))
        .with_provider(provider_detail("google", "Gemini"))
        .with_provider(provider_detail("opencode", "Gratis"))
        .with_featured("openai");
    let svc = service(connector, FakeRestarter::new(), path);
    let list = svc.list_providers().expect("list");
    let by_id = |id: &str| list.iter().find(|p| p.id == id).expect(id);
    assert!(!by_id("openai").highlighted);
    assert!(by_id("google").highlighted);
    assert!(by_id("opencode").highlighted);
}

#[test]
fn list_providers_uses_default_featured_when_settings_empty() {
    let path = unique_settings_path();
    let connector = FakeProviderConnector::new()
        .with_provider(provider_detail("openai", "ChatGPT"))
        .with_provider(provider_detail("google", "Gemini"))
        .with_provider(provider_detail("other", "Other"));
    let svc = service(connector, FakeRestarter::new(), path);
    let list = svc.list_providers().expect("list");
    let by_id = |id: &str| list.iter().find(|p| p.id == id).expect(id);
    assert!(by_id("openai").highlighted);
    assert!(by_id("google").highlighted);
    assert!(!by_id("other").highlighted);
}

#[test]
fn corrupt_settings_load_as_empty() {
    let path = unique_settings_path();
    std::fs::write(&path, "{not json").expect("corrupt");
    let loaded = SettingsStore::new(path).load();
    assert_eq!(loaded, Settings::default());
}

#[test]
fn pass_through_detail_and_models() {
    let path = unique_settings_path();
    let svc = service(catalog_connector(), FakeRestarter::new(), path);
    let detail = svc.provider_detail("openai").expect("detail");
    assert_eq!(detail.name, "ChatGPT");
    let models = svc.list_models().expect("models");
    assert!(models.iter().any(|m| m.model_id == "big-pickle"));
}

#[test]
fn connection_view_never_contains_secret() {
    let view = ConnectionView {
        id: "cred-1".into(),
        label: Some("lab".into()),
    };
    let json = serde_json::to_string(&view).expect("json");
    assert!(!json.contains("sk-"));
}
