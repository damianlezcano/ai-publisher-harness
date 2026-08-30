//! M7 app-facade security suite (design §20 threats 1, 2, 8, 10, 11).

use std::collections::BTreeMap;
use std::path::Path;

use project_agent::FakeAgentEngine;
use project_app::{AppError, AppState, ErrorCode};
use project_provider::{
    FakeProviderConnector, FakeRestarter, ModelSummary, ProviderDetail, SecretString,
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

fn connector() -> FakeProviderConnector {
    FakeProviderConnector::new()
        .with_provider(provider("openai", "ChatGPT"))
        .with_provider(provider("google", "Gemini"))
        .with_model(ModelSummary {
            provider_id: "opencode".into(),
            model_id: "big-pickle".into(),
            name: "big-pickle".into(),
            free: true,
            recommended: true,
            deprecated: false,
        })
}

fn app(
    base: &Path,
    connector: FakeProviderConnector,
) -> AppState<FakeAgentEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
    AppState::with_components(
        base.to_path_buf(),
        FakeAgentEngine::new(),
        FakeTunnel::new(),
        connector,
        FakeRestarter::new(),
    )
}

fn snapshot_dir(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut map = BTreeMap::new();
    fn walk(root: &Path, dir: &Path, map: &mut BTreeMap<String, Vec<u8>>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, map);
                } else if let Ok(bytes) = std::fs::read(&path) {
                    let rel = path
                        .strip_prefix(root)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    map.insert(rel, bytes);
                }
            }
        }
    }
    walk(root, root, &mut map);
    map
}

// Threat 1 (§20): project metadata/files are byte-unchanged by connect/disconnect.
#[test]
fn connect_and_disconnect_leave_project_bytes_unchanged() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = app(tmp.path(), connector());
    let p = state.create_project("Fotosíntesis").expect("create");

    let project_dir = tmp.path().join("projects").join(&p.id);
    let before = snapshot_dir(&project_dir);

    let view = state
        .provider_connect_key("openai", &SecretString::new("sk-secret".into()), None)
        .expect("connect");
    let after_connect = snapshot_dir(&project_dir);
    assert_eq!(
        after_connect, before,
        "a credential connect must not touch any project file"
    );

    state.provider_disconnect(&view.id).expect("disconnect");
    let after_disconnect = snapshot_dir(&project_dir);
    assert_eq!(
        after_disconnect, before,
        "a disconnect must not touch any project file"
    );
    assert!(
        !serde_json::to_string(&after_connect)
            .expect("json")
            .contains("sk-secret")
    );
}

// Threat 8 (§20): credentials are app-global; no project ever receives them.
#[test]
fn two_projects_remain_credential_free_after_connect() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = app(tmp.path(), connector());
    let a = state.create_project("A").expect("create a");
    let b = state.create_project("B").expect("create b");

    state
        .provider_connect_key(
            "openai",
            &SecretString::new("sk-global-secret".into()),
            None,
        )
        .expect("connect");

    for id in [&a.id, &b.id] {
        let project_dir = tmp.path().join("projects").join(id);
        let snapshot = snapshot_dir(&project_dir);
        let raw = serde_json::to_string(&snapshot).expect("json");
        assert!(
            !raw.contains("sk-global-secret"),
            "project {id} leaked the credential"
        );
    }
}

// Threat 11 (§20): the persisted settings bundle contains no credential values.
#[test]
fn app_settings_json_never_contains_credentials() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = app(tmp.path(), connector());
    state
        .provider_connect_key("openai", &SecretString::new("sk-app-secret".into()), None)
        .expect("connect");
    state
        .model_select("opencode", "big-pickle")
        .expect("select");

    let settings_path = tmp.path().join("settings.json");
    assert!(settings_path.exists(), "selection must persist");
    let raw = std::fs::read_to_string(&settings_path).expect("read settings");
    for fragment in ["sk-app-secret", "AIza", "gsk_", "Bearer ", "auth.json"] {
        assert!(
            !raw.contains(fragment),
            "settings leaked {fragment:?}: {raw}"
        );
    }
}

// Threat 2 (§20): no facade read returns a secret; ids from the frontend are
// validated against the live lists and never echoed in raw form.
#[test]
fn facade_reads_never_expose_secrets_or_echo_hostile_ids() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = app(tmp.path(), connector());
    state
        .provider_connect_key("openai", &SecretString::new("sk-read-secret".into()), None)
        .expect("connect");

    let list = serde_json::to_string(&state.provider_list().expect("list")).expect("json");
    let detail =
        serde_json::to_string(&state.provider_detail("openai").expect("detail")).expect("json");
    let selected =
        serde_json::to_string(&state.model_get_selected().expect("selected")).expect("json");
    for value in [list, detail, selected] {
        assert!(!value.contains("sk-read-secret"));
    }

    let err = state
        .provider_detail("../../etc/passwd")
        .expect_err("hostile");
    assert_eq!(err.code, ErrorCode::ProviderNotFound);
    assert_eq!(err.message, "Ese proveedor no está disponible.");
    assert!(!err.message.contains("etc/passwd"));
}

// Threat 6 (§20): after a disconnect the provider is disconnected, and a
// revoked credential surfaces the human "volver a conectar" message.
#[test]
fn disconnect_clears_connection_and_revoked_error_is_human() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = app(tmp.path(), connector());
    let view = state
        .provider_connect_key("openai", &SecretString::new("sk-x".into()), Some("clave"))
        .expect("connect");
    assert!(
        state
            .provider_detail("openai")
            .expect("detail")
            .connections
            .len()
            == 1
    );

    state.provider_disconnect(&view.id).expect("disconnect");
    let detail = state.provider_detail("openai").expect("detail");
    assert!(
        detail.connections.is_empty(),
        "disconnect must clear the connection"
    );

    // A revoked credential maps to the "volver a conectar" prompt (ADR-0008).
    let err = AppError::from_provider(project_provider::ProviderError::CredentialRevoked);
    assert_eq!(err.code, ErrorCode::CredentialRevoked);
    assert_eq!(err.message, "Necesitás volver a conectar tu cuenta.");
}
