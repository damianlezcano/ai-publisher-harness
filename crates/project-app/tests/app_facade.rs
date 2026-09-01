use std::fs;
use std::path::Path;

use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};
use project_agent::{Artifact, ArtifactKind, FakeAgentEngine};
use project_app::{AppState, ErrorCode};
use project_provider::{FakeProviderConnector, FakeRestarter, ModelSummary, ProviderDetail};
use project_tunnel::FakeTunnel;

/// A fake connector seeded with a free recommended model so `run_agent` (which
/// resolves the global model) and the provider surface work offline.
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
    base: &Path,
) -> (
    AppState<FakeAgentEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
    FakeAgentEngine,
    FakeTunnel,
) {
    let engine = FakeAgentEngine::new();
    let tunnel = FakeTunnel::new();
    let state = AppState::with_components(
        base.to_path_buf(),
        engine.clone(),
        tunnel.clone(),
        connector(),
        FakeRestarter::new(),
    );
    (state, engine, tunnel)
}

/// Engine whose `send` always returns `Cancelled`, modeling a user abort.
struct CancellingEngine(FakeAgentEngine);

impl project_agent::AgentEngine for CancellingEngine {
    fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
        self.0.ensure_ready()
    }
    fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
        self.0.open_session(project)
    }
    fn send(
        &self,
        _session: &AgentSession,
        _req: &AgentPrompt,
    ) -> project_agent::AgentResult<AgentTask> {
        Err(project_agent::AgentError::Cancelled)
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

fn write_artifact(base: &Path, project_id: &str, rel: &str, bytes: &[u8]) {
    let path = base
        .join("projects")
        .join(project_id)
        .join("workspace")
        .join(rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("workspace dirs");
    fs::write(&path, bytes).expect("write artifact");
}

// -- Projects ---------------------------------------------------------------

#[test]
fn project_lifecycle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let p = app.create_project("  Fotosíntesis  ").expect("create");
    assert_eq!(p.name, "Fotosíntesis");
    assert_eq!(app.list_projects().expect("list").len(), 1);
    let renamed = app.rename_project(&p.id, "Sistema solar").expect("rename");
    assert_eq!(renamed.name, "Sistema solar");
    app.delete_project(&p.id).expect("delete");
    assert!(app.list_projects().expect("list").is_empty());
}

#[test]
fn delete_unpublishes_before_removing_data() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let p = app.create_project("Fotosíntesis").expect("create");
    app.publish(&p.id).expect("publish");
    assert!(app.publication_status(&p.id).expect("status").state == "published");

    app.delete_project(&p.id).expect("delete");

    assert!(app.list_projects().expect("list").is_empty());
    assert_eq!(
        app.publication_status(&p.id).expect("status").state,
        "local"
    );
}

#[test]
fn delete_removes_project_tree_and_preserves_other_projects() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let a = app.create_project("A").expect("create");
    let b = app.create_project("B").expect("create");

    engine.set_artifacts(vec![artifact("workspace/guia.pdf", ArtifactKind::Pdf)]);
    write_artifact(tmp.path(), &a.id, "guia.pdf", b"pdf");
    app.run_agent(&a.id, "hacé una guía", &[]).expect("run");

    let a_dir = tmp.path().join("projects").join(&a.id);
    let b_dir = tmp.path().join("projects").join(&b.id);
    assert!(a_dir.exists());
    assert!(b_dir.exists());

    app.delete_project(&a.id).expect("delete");

    assert!(!a_dir.exists());
    assert!(b_dir.exists());
    assert_eq!(app.list_projects().expect("list").len(), 1);
    assert_eq!(app.open_project(&b.id).expect("open").name, "B");
}

#[test]
fn delete_is_idempotent_with_respect_to_publication_state() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let p = app.create_project("X").expect("create");

    // Deleting a local project should not fail because unpublish returns AlreadyLocal.
    app.delete_project(&p.id).expect("delete local");
    assert!(app.list_projects().expect("list").is_empty());
}

#[test]
fn delete_persists_after_restart() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let id = {
        let (app, _, _) = app(tmp.path());
        let p = app.create_project("ToDelete").expect("create");
        app.delete_project(&p.id).expect("delete");
        p.id
    };

    let (app_after, _, _) = app(tmp.path());
    assert!(app_after.list_projects().expect("list").is_empty());
    assert!(matches!(
        app_after.open_project(&id),
        Err(project_app::AppError {
            code: project_app::ErrorCode::NotFound,
            ..
        })
    ));
}

#[test]
fn rename_persists_and_preserves_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (state, _, _) = app(tmp.path());
    let p = state.create_project("Original").expect("create");
    let id = p.id.clone();
    let _first_updated = p.updated_at.clone();

    let renamed = state.rename_project(&id, "Renombrado").expect("rename");
    assert_eq!(renamed.id, id);
    assert_eq!(renamed.name, "Renombrado");
    assert!(!renamed.updated_at.is_empty());

    let (state_after, _, _) = app(tmp.path());
    let view = state_after.open_project(&id).expect("open");
    assert_eq!(view.name, "Renombrado");
}

#[test]
fn blank_name_is_rejected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let err = app.create_project("   ").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
}

// -- Materials ---------------------------------------------------------------

#[test]
fn material_add_copies_file_and_rejects_symlink_and_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let p = app.create_project("P").expect("create");

    let src = tmp.path().join("manual.pdf");
    fs::write(&src, b"pdf-bytes").expect("write");
    let m = app
        .add_material_from_path(&p.id, src.to_str().expect("path"))
        .expect("add");
    assert_eq!(m.original_file_name, "manual.pdf");
    assert_eq!(m.kind, "pdf");
    assert_eq!(m.byte_size, 9);
    // The original is never modified.
    assert_eq!(fs::read(&src).expect("read"), b"pdf-bytes");

    // Symlink is rejected before it is read.
    #[cfg(unix)]
    {
        let link = tmp.path().join("link.pdf");
        std::os::unix::fs::symlink(&src, &link).expect("symlink");
        let err = app
            .add_material_from_path(&p.id, link.to_str().expect("path"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::MaterialFailed);
    }

    // Directory is rejected.
    let err = app
        .add_material_from_path(&p.id, tmp.path().to_str().expect("path"))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);

    // Missing file maps to the human material message.
    let err = app
        .add_material_from_path(&p.id, "/no/such/file.pdf")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MaterialFailed);
    assert_eq!(err.message, "No pudimos agregar ese archivo.");
}

// -- Creations ---------------------------------------------------------------

#[test]
fn run_agent_registers_creation_private_by_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Fotosíntesis").expect("create");
    engine.set_artifacts(vec![artifact(
        "workspace/actividad/index.html",
        ArtifactKind::Web,
    )]);
    write_artifact(tmp.path(), &p.id, "actividad/index.html", b"<h1>");
    let result = app
        .run_agent(&p.id, "crea una actividad", &[])
        .expect("run");
    assert_eq!(result.status, "completed");
    assert_eq!(result.registered_creation_ids.len(), 1);

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.creations.len(), 1);
    assert_eq!(view.creations[0].kind, "web");
    assert_eq!(view.creations[0].visibility, "private");
}

#[test]
fn set_creation_visibility_toggles() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("P").expect("create");
    engine.set_artifacts(vec![artifact("workspace/doc.pdf", ArtifactKind::Pdf)]);
    write_artifact(tmp.path(), &p.id, "doc.pdf", b"x");
    app.run_agent(&p.id, "doc", &[]).expect("run");
    let cid = app.open_project(&p.id).expect("open").creations[0]
        .id
        .clone();

    let c = app.set_creation_visibility(&p.id, &cid, true).expect("set");
    assert_eq!(c.visibility, "public");
    let c = app
        .set_creation_visibility(&p.id, &cid, false)
        .expect("set");
    assert_eq!(c.visibility, "private");
}

#[test]
fn creation_path_rejects_cross_project_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let a = app.create_project("A").expect("create");
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &a.id, "index.html", b"x");
    app.run_agent(&a.id, "web", &[]).expect("run");
    let creation_id = app.open_project(&a.id).expect("open").creations[0]
        .id
        .clone();

    let b = app.create_project("B").expect("create");
    let err = app.creation_path(&b.id, &creation_id).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(app.creation_path(&a.id, &creation_id).is_ok());
}

// -- Agent errors ------------------------------------------------------------

#[test]
fn agent_failure_maps_to_human_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("X").expect("create");
    engine.fail_ready();
    let err = app.run_agent(&p.id, "hola", &[]).unwrap_err();
    assert_eq!(err.code, ErrorCode::AiUnavailable);
    assert_eq!(err.message, "No se pudo iniciar el asistente de IA.");
}

#[test]
fn empty_prompt_is_rejected_without_touching_engine() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("X").expect("create");
    let err = app.run_agent(&p.id, "   ", &[]).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert!(!engine.calls().contains(&project_agent::FakeCall::Ready));
}

// -- Publication -------------------------------------------------------------

#[test]
fn publish_returns_public_url_and_status() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let p = app.create_project("Fotosíntesis").expect("create");
    let view = app.publish(&p.id).expect("publish");
    assert_eq!(view.state, "published");
    let url = view.public_url.clone().expect("url");
    assert!(url.starts_with("https://fake-tunnel.trycloudflare.com/"));
    assert!(url.contains("fotosintesis"));

    let status = app.publication_status(&p.id).expect("status");
    assert_eq!(status.state, "published");
    assert_eq!(status.public_url, Some(url));
}

#[test]
fn unpublish_one_keeps_other_published() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let a = app.create_project("A").expect("create");
    let b = app.create_project("B").expect("create");
    app.publish(&a.id).expect("publish a");
    app.publish(&b.id).expect("publish b");
    assert_eq!(app.publication_status(&a.id).expect("a").state, "published");
    assert_eq!(app.publication_status(&b.id).expect("b").state, "published");

    app.unpublish(&a.id).expect("unpublish a");
    assert_eq!(app.publication_status(&a.id).expect("a").state, "local");
    assert_eq!(app.publication_status(&b.id).expect("b").state, "published");
}

#[test]
fn publish_failure_maps_to_human_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, tunnel) = app(tmp.path());
    let p = app.create_project("X").expect("create");
    tunnel.fail_start();
    let err = app.publish(&p.id).unwrap_err();
    assert_eq!(err.code, ErrorCode::PublishFailed);
    assert_eq!(err.message, "No se pudo publicar en Internet.");
}

// -- Restart / persistence ----------------------------------------------------

#[test]
fn restart_persists_projects_and_resets_publication_to_local() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let id = {
        let (app, engine, _) = app(tmp.path());
        let p = app.create_project("Fotosíntesis").expect("create");
        engine.set_artifacts(vec![artifact("workspace/guia.pdf", ArtifactKind::Pdf)]);
        write_artifact(tmp.path(), &p.id, "guia.pdf", b"pdf");
        app.run_agent(&p.id, "hacé una guía", &[]).expect("run");
        app.publish(&p.id).expect("publish");
        p.id
    };

    let (app2, _, _) = app(tmp.path());
    let view = app2.open_project(&id).expect("open");
    assert_eq!(view.name, "Fotosíntesis");
    assert_eq!(view.creations.len(), 1);
    assert_eq!(view.publication.state, "local");
    assert!(view.publication.public_url.is_none());
}

// -- Security ----------------------------------------------------------------

#[test]
fn malformed_ids_are_rejected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let err = app.open_project("not-a-uuid").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    let p = app.create_project("X").expect("create");
    let err = app.set_creation_visibility(&p.id, "bad", true).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    let err = app.creation_path(&p.id, "bad").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
}

#[test]
fn hostile_names_are_treated_as_opaque_text() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let name = "<img src=x onerror=alert(1)>";
    let p = app.create_project(name).expect("create");
    assert_eq!(p.name, name);
    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.name, name);
}

#[test]
fn cancelled_agent_run_is_a_normal_cancelled_outcome() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = CancellingEngine(FakeAgentEngine::new());
    let tunnel = FakeTunnel::new();
    let app = AppState::with_components(
        tmp.path().to_path_buf(),
        engine,
        tunnel,
        connector(),
        FakeRestarter::new(),
    );
    let p = app.create_project("P").expect("create");
    let result = app.run_agent(&p.id, "hacé algo", &[]).expect("run");
    assert_eq!(result.status, "cancelled");
    assert!(result.registered_creation_ids.is_empty());
}

// -- Durable messages ------------------------------------------------------

#[test]
fn send_message_persists_user_and_assistant_messages() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Fotosíntesis").expect("create");
    engine.set_message("Listo.".to_owned());
    engine.set_artifacts(vec![artifact(
        "workspace/actividad/index.html",
        ArtifactKind::Web,
    )]);
    write_artifact(tmp.path(), &p.id, "actividad/index.html", b"<h1>");

    let run = app
        .send_message(&p.id, "crea una actividad", &[])
        .expect("send");
    assert_eq!(run.status, "completed");
    assert_eq!(run.registered_creation_ids.len(), 1);

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.messages.len(), 2);
    assert_eq!(view.messages[0].role, "user");
    assert_eq!(view.messages[0].text, "crea una actividad");
    assert_eq!(view.messages[0].status, "ok");
    assert!(view.messages[0].material_ids.is_empty());
    assert_eq!(view.messages[1].role, "assistant");
    assert_eq!(view.messages[1].status, "ok");
    assert_eq!(view.messages[1].text, "Listo.");
    assert_eq!(view.messages[1].creation_ids, run.registered_creation_ids);
}

#[test]
fn failed_run_persists_failed_assistant_message() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("P").expect("create");
    engine.fail_send();

    let run = app.send_message(&p.id, "hacé algo", &[]).expect("send");
    assert_eq!(run.status, "failed");

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.messages.len(), 2);
    assert_eq!(view.messages[0].role, "user");
    assert_eq!(view.messages[0].text, "hacé algo");
    assert_eq!(view.messages[0].status, "ok");
    assert_eq!(view.messages[1].role, "assistant");
    assert_eq!(view.messages[1].status, "failed");
}

#[test]
fn cancel_persists_cancelled_message() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = CancellingEngine(FakeAgentEngine::new());
    let tunnel = FakeTunnel::new();
    let app = AppState::with_components(
        tmp.path().to_path_buf(),
        engine,
        tunnel,
        connector(),
        FakeRestarter::new(),
    );
    let p = app.create_project("P").expect("create");

    let run = app.send_message(&p.id, "hacé algo", &[]).expect("send");
    assert_eq!(run.status, "cancelled");

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.messages.len(), 2);
    assert_eq!(view.messages[0].role, "user");
    assert_eq!(view.messages[1].role, "assistant");
    assert_eq!(view.messages[1].status, "cancelled");
}

#[test]
fn project_summary_includes_timestamps_and_shared() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let p = app.create_project("P").expect("create");

    let summaries = app.list_projects().expect("list");
    assert_eq!(summaries.len(), 1);
    let s = &summaries[0];
    assert_eq!(s.id, p.id);
    assert!(!s.created_at.is_empty());
    assert!(!s.updated_at.is_empty());
    assert!(!s.shared);

    app.publish(&p.id).expect("publish");
    let summaries = app.list_projects().expect("list");
    assert!(summaries[0].shared);
}

#[test]
fn project_view_includes_messages() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("P").expect("create");
    engine.set_message("Listo.".to_owned());

    app.send_message(&p.id, "hola", &[]).expect("send");

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.messages.len(), 2);
    assert_eq!(view.messages[0].role, "user");
    assert_eq!(view.messages[1].role, "assistant");
}

#[test]
fn message_append_is_durable_before_agent_run() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("P").expect("create");
    engine.set_message("Listo.".to_owned());

    let inputs = app
        .send_message_persist(&p.id, "hola", &[])
        .expect("persist");

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.messages.len(), 1);
    assert_eq!(view.messages[0].role, "user");
    assert_eq!(view.messages[0].text, "hola");
    assert_eq!(view.messages[0].status, "ok");

    let run = app.send_message_run(inputs).expect("run");
    assert_eq!(run.status, "completed");

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.messages.len(), 2);
    assert_eq!(view.messages[1].role, "assistant");
}
