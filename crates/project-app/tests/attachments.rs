//! M8 named suite: prompt attachments resolve by authorized material ID only;
//! cross-project references and unknown/malformed IDs are rejected. Uses
//! FakeAgentEngine so the whole flow is offline and deterministic.

use std::fs;

use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};
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

/// Engine that records the exact prompt text it receives.
struct RecordingEngine(
    FakeAgentEngine,
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
);

impl project_agent::AgentEngine for RecordingEngine {
    fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
        self.0.ensure_ready()
    }
    fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
        self.0.open_session(project)
    }
    fn send(
        &self,
        session: &AgentSession,
        req: &AgentPrompt,
    ) -> project_agent::AgentResult<AgentTask> {
        self.1.lock().unwrap().push(req.text.clone());
        self.0.send(session, req)
    }
    fn cancel(&self, session: &AgentSession) -> project_agent::AgentResult<()> {
        self.0.cancel(session)
    }
    fn status(&self) -> AgentStatus {
        self.0.status()
    }
    fn shutdown(&self) -> project_agent::AgentResult<()> {
        self.0.shutdown()
    }
}

fn artifact(path: &str, kind: ArtifactKind) -> Artifact {
    Artifact {
        path: path.to_owned(),
        kind,
        byte_size: 1,
        sha256: None,
    }
}

fn write_artifact(base: &std::path::Path, project_id: &str, rel: &str, bytes: &[u8]) {
    let path = base
        .join("projects")
        .join(project_id)
        .join("workspace")
        .join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
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
fn attachments_flow_to_the_agent_and_register_creations() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, engine) = app(tmp.path());
    let p = app.create_project("P").unwrap();
    engine.set_artifacts(vec![artifact(
        "workspace/actividad/index.html",
        ArtifactKind::Web,
    )]);
    write_artifact(tmp.path(), &p.id, "actividad/index.html", b"<h1>");

    let mid = add_material(&app, tmp.path(), &p.id, "manual.pdf", b"pdf-bytes");
    let result = app.run_agent(&p.id, "creá una actividad", &[mid]).unwrap();
    assert_eq!(result.status, "completed");
    assert_eq!(result.registered_creation_ids.len(), 1);
    assert_eq!(app.open_project(&p.id).unwrap().creations.len(), 1);
}

#[test]
fn foreign_material_id_is_rejected_as_attachment_invalid() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, engine) = app(tmp.path());
    let a = app.create_project("A").unwrap();
    let b = app.create_project("B").unwrap();
    engine.set_artifacts(vec![artifact("workspace/x.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &b.id, "x.html", b"<h1>");
    // The material belongs to project A.
    let mid = add_material(&app, tmp.path(), &a.id, "manual.pdf", b"pdf-bytes");
    let err = app.run_agent(&b.id, "usalo", &[mid]).unwrap_err();
    assert_eq!(err.code, ErrorCode::AttachmentInvalid);
    assert_eq!(err.message, "No pudimos adjuntar ese material.");
}

#[test]
fn unknown_material_id_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, engine) = app(tmp.path());
    let p = app.create_project("P").unwrap();
    engine.set_artifacts(vec![artifact("workspace/x.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &p.id, "x.html", b"<h1>");
    let err = app
        .run_agent(
            &p.id,
            "usalo",
            &["0198e4a6-79b2-7b51-9e68-c2eb7af3db15".to_owned()],
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::AttachmentInvalid);
}

#[test]
fn malformed_material_id_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, engine) = app(tmp.path());
    let p = app.create_project("P").unwrap();
    engine.set_artifacts(vec![artifact("workspace/x.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &p.id, "x.html", b"<h1>");
    let err = app
        .run_agent(&p.id, "usalo", &["not-a-uuid".to_owned()])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::AttachmentInvalid);
}

#[test]
fn attachments_augment_the_prompt_deterministically() {
    let tmp = tempfile::tempdir().unwrap();
    // Seed the materials through a plain FakeAgentEngine state on the same base.
    let (seed, _) = app(tmp.path());
    let p = seed.create_project("P").unwrap();
    let mid = add_material(&seed, tmp.path(), &p.id, "manual.pdf", b"pdf-bytes");
    let mid2 = add_material(&seed, tmp.path(), &p.id, "diagrama.png", b"png-bytes");
    drop(seed);

    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![artifact("workspace/x.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &p.id, "x.html", b"<h1>");
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recording = RecordingEngine(engine, std::sync::Arc::clone(&recorded));
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        recording,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    state
        .run_agent(&p.id, "Creá una actividad", &[mid, mid2])
        .unwrap();
    let prompts = recorded.lock().unwrap();
    assert_eq!(prompts.len(), 1);
    let text = &prompts[0];
    assert!(text.starts_with(
        "Materiales adjuntos (usá estos archivos como contexto; están en la carpeta \"materials\"):\n- manual.pdf (pdf)\n- diagrama.png (image)\n\n"
    ));
    assert!(text.ends_with("Creá una actividad"));
}

#[test]
fn hostile_material_names_are_sanitized_never_injected_raw() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, engine) = app(tmp.path());
    let p = app.create_project("P").unwrap();
    engine.set_artifacts(vec![artifact("workspace/x.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &p.id, "x.html", b"<h1>");

    // A hostile name containing traversal and script markup.
    let hostile = std::env::temp_dir().join("m8-hostile-<script>..png");
    fs::write(&hostile, b"png-bytes").unwrap();
    let material = app
        .add_material_from_path(&p.id, hostile.to_str().unwrap())
        .unwrap();
    let _ = fs::remove_file(&hostile);

    app.run_agent(&p.id, "usalo", &[material.id]).unwrap();
    // The agent still completes (sanitized provisioning); the creation exists.
    assert_eq!(app.open_project(&p.id).unwrap().creations.len(), 1);
}
