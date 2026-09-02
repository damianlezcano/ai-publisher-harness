//! M8 named suite: in-app preview authorization and caps. Web preview
//! isolation is covered by project-preview's own suites; here we verify that
//! the facade resolves IDs within the project only, enforces the 2 MB cap, and
//! tears web previews down by token.

use std::fs;

use project_agent::{Artifact, ArtifactKind, FakeAgentEngine};
use project_app::{AppState, ErrorCode};
use project_provider::{FakeProviderConnector, FakeRestarter, ModelSummary, ProviderDetail};
use project_tunnel::FakeTunnel;

fn connector() -> FakeProviderConnector {
    FakeProviderConnector::new()
        .with_provider(ProviderDetail {
            id: "opencode".into(),
            name: "Gratis".into(),
            auth_methods: Vec::new(),
            connections: Vec::new(),
        })
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
    base: &std::path::Path,
) -> (
    AppState<FakeAgentEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
    FakeAgentEngine,
) {
    let engine = FakeAgentEngine::new();
    let state = AppState::with_components(
        base.to_path_buf(),
        engine.clone(),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    (state, engine)
}

fn add_material(
    app: &AppState<FakeAgentEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
    base: &std::path::Path,
    project_id: &str,
    name: &str,
    bytes: &[u8],
) -> String {
    let src = base.join(name);
    fs::write(&src, bytes).unwrap();
    let material = app
        .add_material_from_path(project_id, src.to_str().unwrap())
        .unwrap();
    let _ = fs::remove_file(&src);
    material.id
}

#[test]
fn preview_data_material_image_returns_base64_and_type() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let bytes = b"\x89PNG\r\n\x1a\npayload";
    let mid = add_material(&app, tmp.path(), &p.id, "foto.png", bytes);
    let data = app.preview_data(&p.id, "material", &mid).unwrap();
    assert_eq!(data.content_type, "image/png");
    assert!(!data.data_base64.is_empty());
}

#[test]
fn preview_data_unknown_material_is_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let err = app
        .preview_data(&p.id, "material", "0198e4a6-79b2-7b51-9e68-c2eb7af3db15")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn preview_data_foreign_material_is_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = app(tmp.path());
    let a = app.create_project("A").unwrap();
    let b = app.create_project("B").unwrap();
    let mid = add_material(&app, tmp.path(), &a.id, "foto.png", b"x");
    let err = app.preview_data(&b.id, "material", &mid).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn preview_data_malformed_id_is_invalid_input() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let err = app
        .preview_data(&p.id, "material", "not-a-uuid")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
}

#[test]
fn preview_data_unknown_kind_is_invalid() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let err = app
        .preview_data(&p.id, "weird", "0198e4a6-79b2-7b51-9e68-c2eb7af3db15")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
}

#[test]
fn preview_data_oversized_resource_is_preview_too_large() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let big = vec![0u8; 2 * 1024 * 1024 + 1];
    let mid = add_material(&app, tmp.path(), &p.id, "grande.txt", &big);
    let err = app.preview_data(&p.id, "material", &mid).unwrap_err();
    assert_eq!(err.code, ErrorCode::PreviewTooLarge);
}

// -- Web preview --------------------------------------------------------------

/// Registers a web creation through the agent flow and returns its id. The
/// workspace artifact (matching the FakeAgentEngine artifact) must exist before
/// `run_agent` so the registrar can copy it into `outputs/<id>`.
fn create_web_creation(
    state: &AppState<FakeAgentEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
    base: &std::path::Path,
    project_id: &str,
    artifact_rel: &str,
    files: &[(&str, &[u8])],
) -> String {
    let workspace_file = base
        .join("projects")
        .join(project_id)
        .join("workspace")
        .join(artifact_rel);
    fs::create_dir_all(workspace_file.parent().unwrap()).unwrap();
    for (name, bytes) in files {
        let target = workspace_file.parent().unwrap().join(name);
        fs::write(target, bytes).unwrap();
    }
    let result = state.run_agent(project_id, "creá una web", &[]).unwrap();
    result.registered_creation_ids[0].clone()
}

#[test]
fn web_preview_starts_and_closes_by_token() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, engine) = app(tmp.path());
    let p = app.create_project("P").unwrap();
    engine.set_artifacts(vec![Artifact {
        path: "workspace/app/index.html".into(),
        kind: ArtifactKind::Web,
        byte_size: 1,
        sha256: None,
    }]);
    let cid = create_web_creation(
        &app,
        tmp.path(),
        &p.id,
        "app/index.html",
        &[("index.html", b"<h1>hola</h1>")],
    );
    let web = app.preview_open_web(&p.id, &cid).unwrap();
    assert!(web.url.starts_with("http://127.0.0.1:"));
    assert!(web.url.contains(&format!("/preview/{}/", web.token)));
    // The URL is reachable and serves the copy.
    let url = format!("{url_prefix}index.html", url_prefix = web.url);
    let resp = reqwest::blocking::get(&url).unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().unwrap(), "<h1>hola</h1>");
    let root = reqwest::blocking::get(&web.url).unwrap();
    assert_eq!(root.status(), 200);
    assert_eq!(root.text().unwrap(), "<h1>hola</h1>");
    // Close tears it down (the server stops and its token is invalidated).
    app.preview_close(&web.token).unwrap();
}

#[test]
fn web_preview_foreign_creation_is_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, engine) = app(tmp.path());
    let a = app.create_project("A").unwrap();
    let b = app.create_project("B").unwrap();
    engine.set_artifacts(vec![Artifact {
        path: "workspace/app/index.html".into(),
        kind: ArtifactKind::Web,
        byte_size: 1,
        sha256: None,
    }]);
    let cid = create_web_creation(
        &app,
        tmp.path(),
        &a.id,
        "app/index.html",
        &[("index.html", b"x")],
    );
    let err = app.preview_open_web(&b.id, &cid).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn web_preview_unknown_token_close_is_unavailable() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = app(tmp.path());
    let err = app
        .preview_close("deadbeefdeadbeefdeadbeefdeadbeef")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PreviewUnavailable);
}
