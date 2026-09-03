use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

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
fn conversation_model_is_validated_persisted_isolated_and_clearable() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let first = app.create_project("A").expect("create A");
    let second = app.create_project("B").expect("create B");
    let unknown = app.conversation_model_select(&first.id, "missing", "model");
    assert_eq!(
        unknown.expect_err("unknown model").code,
        ErrorCode::ModelUnavailable
    );
    app.conversation_model_select(&first.id, "opencode", "big-pickle")
        .expect("select model");
    assert_eq!(
        app.open_project(&first.id)
            .expect("open")
            .model
            .unwrap()
            .model_id,
        "big-pickle"
    );
    assert!(
        app.open_project(&second.id)
            .expect("open B")
            .model
            .is_none()
    );
    app.conversation_model_clear(&first.id)
        .expect("clear model");
    assert!(app.open_project(&first.id).expect("reload").model.is_none());
}

#[test]
fn owned_material_and_creation_paths_reject_foreign_ids() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let first = app.create_project("A").expect("create");
    let second = app.create_project("B").expect("create");
    let source = tmp.path().join("note.txt");
    fs::write(&source, b"hello").expect("source");
    let material = app
        .add_material_from_path(&first.id, source.to_str().expect("utf8"))
        .expect("material");
    let path = app.material_path(&first.id, &material.id).expect("path");
    assert!(path.starts_with(tmp.path().join("projects").join(&first.id)));
    assert!(app.material_path(&second.id, &material.id).is_err());
    assert!(app.material_path(&first.id, "not-an-id").is_err());
    assert!(app.creation_path(&second.id, "not-an-id").is_err());
}

#[test]
fn folder_open_rejects_invalid_project_before_opening() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());

    // Malformed ids are rejected before any opener is invoked; the folder-open
    // command never opens an unvalidated path.
    assert_eq!(
        app.open_materials_folder("not-a-uuid").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        app.open_creations_folder("not-a-uuid").unwrap_err().code,
        ErrorCode::InvalidInput
    );
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
fn delete_aborts_when_unpublish_fails_leaving_project_intact() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, tunnel) = app(tmp.path());
    let p = app.create_project("Fotosíntesis").expect("create");
    app.publish(&p.id).expect("publish");

    tunnel.fail_stop();
    let err = app.delete_project(&p.id).unwrap_err();
    assert_eq!(err.code, ErrorCode::PublishFailed);

    assert_eq!(app.list_projects().expect("list").len(), 1);
    assert!(app.open_project(&p.id).is_ok());
}

#[test]
fn delete_waits_for_in_flight_agent_and_leaves_no_orphans() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_message("Listo.".to_owned());

    let barrier = Arc::new(Barrier::new(2));
    let entered_send = Arc::new(AtomicBool::new(false));

    #[derive(Clone)]
    struct BlockingEngine {
        inner: FakeAgentEngine,
        barrier: Arc<Barrier>,
        entered_send: Arc<AtomicBool>,
    }

    impl project_agent::AgentEngine for BlockingEngine {
        fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
            self.inner.ensure_ready()
        }
        fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
            self.inner.open_session(project)
        }
        fn send(
            &self,
            session: &AgentSession,
            req: &AgentPrompt,
        ) -> project_agent::AgentResult<AgentTask> {
            self.entered_send.store(true, Ordering::SeqCst);
            self.barrier.wait();
            self.inner.send(session, req)
        }
        fn cancel(&self, session: &AgentSession) -> project_agent::AgentResult<()> {
            self.inner.cancel(session)
        }
        fn status(&self) -> AgentStatus {
            self.inner.status()
        }
        fn shutdown(&self) -> project_agent::AgentResult<()> {
            self.inner.shutdown()
        }
    }

    let blocking_engine = BlockingEngine {
        inner: engine.clone(),
        barrier: barrier.clone(),
        entered_send: entered_send.clone(),
    };

    let run_app = AppState::with_components(
        tmp.path().to_path_buf(),
        blocking_engine,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let delete_app = app(tmp.path()).0;

    let p = run_app.create_project("A").expect("create");
    let run_id = p.id.clone();
    let delete_id = p.id.clone();

    let run_handle = thread::spawn(move || run_app.run_agent(&run_id, "hacé algo", &[]));

    while !entered_send.load(Ordering::SeqCst) {
        thread::yield_now();
    }

    let delete_handle = thread::spawn(move || delete_app.delete_project(&delete_id));

    thread::sleep(Duration::from_millis(50));
    barrier.wait();

    run_handle.join().expect("run thread").expect("run ok");
    delete_handle
        .join()
        .expect("delete thread")
        .expect("delete ok");

    let pd = tmp.path().join("projects").join(&p.id);
    assert!(!pd.exists());
}

#[test]
fn run_on_deleted_project_aborts_without_creating_orphans() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("A").expect("create");
    let pd = tmp.path().join("projects").join(&p.id);

    app.delete_project(&p.id).expect("delete");
    assert!(!pd.exists());

    engine.set_message("Listo.".to_owned());
    engine.set_artifacts(vec![artifact("workspace/guia.pdf", ArtifactKind::Pdf)]);
    let err = app.run_agent(&p.id, "hacé algo", &[]).unwrap_err();
    assert_eq!(err.code, ErrorCode::AiTaskFailed);

    assert!(!pd.exists());
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
    assert!(run.turn_id.as_deref().is_some_and(|id| !id.is_empty()));
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
fn publish_promotes_the_generated_web_creation_as_the_public_entry() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Rosco").expect("create");
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    write_artifact(
        tmp.path(),
        &p.id,
        "index.html",
        b"<html><body>JUEGO</body></html>",
    );
    engine.set_message("Listo. Creé el recurso usando el archivo que adjuntaste.".into());
    let run = app.send_message(&p.id, "creá el juego", &[]).expect("send");
    assert_eq!(run.registered_creation_ids.len(), 1);
    let cid = &run.registered_creation_ids[0];

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.messages[1].creation_ids, vec![cid.clone()]);
    assert_eq!(view.creations[0].visibility, "private");

    app.publish_creation(&p.id, Some(cid)).expect("share");
    let after = app.open_project(&p.id).expect("open");
    assert_eq!(after.creations[0].id, *cid);
    assert_eq!(after.creations[0].visibility, "public");

    let published = tmp
        .path()
        .join("projects")
        .join(&p.id)
        .join("publish")
        .join("index.html");
    let html = fs::read_to_string(&published).expect("published html");
    assert!(html.contains("JUEGO"), "{html}");
    assert!(
        !html.contains("Material del proyecto"),
        "published root must be the creation, not the empty materials landing"
    );
}

#[test]
fn publish_without_creation_id_still_promotes_the_latest_web() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Actividad").expect("create");
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &p.id, "index.html", b"<h1>actividad</h1>");
    app.run_agent(&p.id, "crea", &[]).expect("run");
    app.publish(&p.id).expect("publish");
    let html = fs::read_to_string(
        tmp.path()
            .join("projects")
            .join(&p.id)
            .join("publish")
            .join("index.html"),
    )
    .expect("html");
    assert!(html.contains("actividad"), "{html}");
    assert!(!html.contains("Material del proyecto"), "{html}");
}

#[test]
fn web_sidecar_sibling_is_copied_into_outputs_and_publish() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Actividad").expect("create");
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &p.id, "index.html", b"<h1>juego</h1>");
    write_artifact(tmp.path(), &p.id, "app.js", b"console.log(1)");
    let run = app.run_agent(&p.id, "crea", &[]).expect("run");
    assert_eq!(run.registered_creation_ids.len(), 1);
    let cid = &run.registered_creation_ids[0];

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.creations[0].display_name, "Actividad");

    let output_js = tmp
        .path()
        .join("projects")
        .join(&p.id)
        .join("outputs")
        .join(cid)
        .join("app.js");
    assert_eq!(
        fs::read_to_string(&output_js).expect("output js"),
        "console.log(1)"
    );

    app.publish_creation(&p.id, Some(cid)).expect("share");
    let published_js = tmp
        .path()
        .join("projects")
        .join(&p.id)
        .join("publish")
        .join("app.js");
    assert_eq!(
        fs::read_to_string(&published_js).expect("published js"),
        "console.log(1)"
    );
}

#[test]
fn failed_run_persists_failed_assistant_message() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("P").expect("create");
    engine.fail_send();

    let run = app.send_message(&p.id, "hacé algo", &[]).expect("send");
    assert_eq!(run.status, "failed");
    assert!(run.turn_id.as_deref().is_some_and(|id| !id.is_empty()));

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
    assert!(run.turn_id.as_deref().is_some_and(|id| !id.is_empty()));

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
fn missing_agent_text_does_not_become_misleading_listo() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let p = app.create_project("P").expect("create");

    let run = app.send_message(&p.id, "hola", &[]).expect("send");

    assert_eq!(run.status, "failed");
    assert_eq!(
        run.message.as_deref(),
        Some("No recibimos una respuesta. Probá de nuevo.")
    );
    let view = app.open_project(&p.id).expect("open");
    assert_eq!(
        view.messages[1].text,
        "No recibimos una respuesta. Probá de nuevo."
    );
    assert_ne!(view.messages[1].text, "Listo.");
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

#[test]
fn sequential_sends_keep_distinct_turn_ids_and_ordered_results() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("P").expect("create");
    engine.set_message("respuesta".to_owned());

    let first = app.send_message(&p.id, "primero", &[]).expect("first");
    let second = app.send_message(&p.id, "segundo", &[]).expect("second");

    assert_ne!(first.turn_id, second.turn_id);
    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.messages.len(), 4);
    assert_eq!(view.messages[0].text, "primero");
    assert_eq!(view.messages[1].text, "respuesta");
    assert_eq!(view.messages[2].text, "segundo");
    assert_eq!(view.messages[3].text, "respuesta");
}

#[test]
fn later_turn_updates_the_same_web_creation_and_refreshes_publish() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Actividad").expect("create");
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    write_artifact(
        tmp.path(),
        &p.id,
        "index.html",
        b"<html><body>ORIGINAL</body></html>",
    );
    engine.set_message("Listo. Creé el recurso.".into());
    let first = app
        .send_message(&p.id, "creá la actividad", &[])
        .expect("first");
    assert_eq!(first.registered_creation_ids.len(), 1);
    let cid = first.registered_creation_ids[0].clone();

    app.publish_creation(&p.id, Some(&cid)).expect("share");
    let published = tmp
        .path()
        .join("projects")
        .join(&p.id)
        .join("publish")
        .join("index.html");
    assert!(
        fs::read_to_string(&published)
            .expect("first snapshot")
            .contains("ORIGINAL")
    );

    write_artifact(
        tmp.path(),
        &p.id,
        "index.html",
        b"<html><body style=\"background:white\">UPDATED</body></html>",
    );
    engine.set_message("Listo. Ya está con fondo blanco.".into());
    let second = app
        .send_message(&p.id, "cambiá el fondo a blanco", &[])
        .expect("second");
    assert_eq!(second.registered_creation_ids, vec![cid.clone()]);

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.creations.len(), 1);
    assert_eq!(view.creations[0].id, cid);
    let html = fs::read_to_string(&published).expect("updated snapshot");
    assert!(html.contains("UPDATED"), "{html}");
    assert!(html.contains("background:white"), "{html}");
    assert!(!html.contains("ORIGINAL"), "{html}");
}

#[test]
fn distinct_web_activity_still_registers_a_new_creation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Actividad").expect("create");
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &p.id, "index.html", b"<h1>one</h1>");
    app.send_message(&p.id, "creá una", &[]).expect("first");

    engine.set_artifacts(vec![artifact(
        "workspace/actividad-2/index.html",
        ArtifactKind::Web,
    )]);
    write_artifact(tmp.path(), &p.id, "actividad-2/index.html", b"<h1>two</h1>");
    app.send_message(&p.id, "creá otra", &[]).expect("second");
    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.creations.len(), 2);
}

#[test]
fn new_distinct_web_does_not_replace_an_already_published_snapshot() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Actividad").expect("create");
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &p.id, "index.html", b"<h1>one</h1>");
    let first = app.send_message(&p.id, "creá una", &[]).expect("first");
    let cid = first.registered_creation_ids[0].clone();
    app.publish_creation(&p.id, Some(&cid)).expect("share");
    let published = tmp
        .path()
        .join("projects")
        .join(&p.id)
        .join("publish")
        .join("index.html");
    assert!(
        fs::read_to_string(&published)
            .expect("first snapshot")
            .contains("one")
    );

    engine.set_artifacts(vec![artifact(
        "workspace/actividad-2/index.html",
        ArtifactKind::Web,
    )]);
    write_artifact(tmp.path(), &p.id, "actividad-2/index.html", b"<h1>two</h1>");
    app.send_message(&p.id, "creá otra", &[]).expect("second");

    let html = fs::read_to_string(&published).expect("snapshot after second");
    assert!(html.contains("one"), "{html}");
    assert!(!html.contains("two"), "{html}");
    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.creations.len(), 2);
}

/// Turn-aware engine for the Finding A human scenario. Turn 1 is a plain
/// creation; from turn 2 on, `send` reproduces the REAL sidecar behavior: the
/// agent edits the existing `index.html` in place and copies the attached
/// image into the workspace, while `/diff` reports nothing (empty).
struct AttachmentEditEngine {
    inner: FakeAgentEngine,
    workspace: PathBuf,
    material_bytes: Vec<u8>,
    calls: Arc<AtomicU32>,
}

impl AttachmentEditEngine {
    fn new(inner: FakeAgentEngine, workspace: PathBuf, material_bytes: Vec<u8>) -> Self {
        Self {
            inner,
            workspace,
            material_bytes,
            calls: Arc::new(AtomicU32::new(0)),
        }
    }
}

impl project_agent::AgentEngine for AttachmentEditEngine {
    fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
        self.inner.ensure_ready()
    }
    fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
        self.inner.open_session(project)
    }
    fn send(
        &self,
        session: &AgentSession,
        req: &AgentPrompt,
    ) -> project_agent::AgentResult<AgentTask> {
        if self.calls.fetch_add(1, Ordering::SeqCst) >= 1 {
            fs::write(self.workspace.join("encabezado.png"), &self.material_bytes)
                .expect("agent copies the attached image into the workspace");
            fs::write(
                self.workspace.join("index.html"),
                b"<html><body>UPDATED</body><img src=\"encabezado.png\"></html>",
            )
            .expect("agent edits the existing creation in place");
        }
        self.inner.send(session, req)
    }
    fn cancel(&self, session: &AgentSession) -> project_agent::AgentResult<()> {
        self.inner.cancel(session)
    }
    fn status(&self) -> AgentStatus {
        self.inner.status()
    }
    fn shutdown(&self) -> project_agent::AgentResult<()> {
        self.inner.shutdown()
    }
}

#[test]
fn attached_input_image_updates_existing_creation_in_place_without_phantom_image() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let png: Vec<u8> = b"\x89PNG\r\n\x1a\nuser-image".to_vec();

    // Seed the project on the same base so the engine knows the real workspace
    // directory owned by the created conversation.
    let (seed, _, _) = app(tmp.path());
    let p = seed.create_project("Sopa de letras").expect("create");
    drop(seed);

    let inner = FakeAgentEngine::new();
    inner.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    inner.set_message("Listo. Creé la sopa de letras.".into());
    let workspace = tmp.path().join("projects").join(&p.id).join("workspace");
    let engine = AttachmentEditEngine::new(inner.clone(), workspace, png.clone());
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        engine,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    write_artifact(
        tmp.path(),
        &p.id,
        "index.html",
        b"<html><body>ORIGINAL</body></html>",
    );

    // T1: the existing Creation C1 is created and shared.
    let first = state
        .send_message(&p.id, "creá una sopa de letras", &[])
        .expect("turn 1");
    assert_eq!(first.registered_creation_ids.len(), 1);
    let cid = first.registered_creation_ids[0].clone();
    state.publish_creation(&p.id, Some(&cid)).expect("share");
    let published = tmp
        .path()
        .join("projects")
        .join(&p.id)
        .join("publish")
        .join("index.html");
    assert!(
        fs::read_to_string(&published)
            .expect("first snapshot")
            .contains("ORIGINAL")
    );

    // T2: the user attaches an image and asks to put it in the header.
    let src = tmp.path().join("images.png");
    fs::write(&src, &png).expect("png");
    let material = state
        .add_material_from_path(&p.id, src.to_str().expect("path"))
        .expect("material");
    let _ = fs::remove_file(&src);
    inner.set_artifacts(vec![]); // /diff is empty for the committed in-place edit
    inner.set_message(
        "Listo. Agregué la imagen del archivo que adjuntaste arriba del título.".into(),
    );
    let second = state
        .send_message(
            &p.id,
            "agregale esta imagen en el encabezado",
            &[material.id],
        )
        .expect("turn 2");

    // A1 stays INPUT material: no phantom "Imagen" Creation; C1 identity holds.
    assert_eq!(second.registered_creation_ids, vec![cid.clone()]);
    let view = state.open_project(&p.id).expect("open");
    assert_eq!(
        view.creations.len(),
        1,
        "attached PNG must not become a Creation"
    );
    assert_eq!(view.creations[0].id, cid);

    // C1 is re-registered in place and serves the image as a web sidecar.
    let outputs_dir = tmp
        .path()
        .join("projects")
        .join(&p.id)
        .join("outputs")
        .join(&cid);
    let html = fs::read_to_string(outputs_dir.join("index.html")).expect("updated html");
    assert!(html.contains("UPDATED"), "{html}");
    assert!(!html.contains("ORIGINAL"), "{html}");
    assert_eq!(
        fs::read(outputs_dir.join("encabezado.png")).expect("sidecar"),
        png
    );

    // Established republish semantics: the shared URL now shows the update.
    let published_html = fs::read_to_string(&published).expect("refreshed snapshot");
    assert!(published_html.contains("UPDATED"), "{published_html}");
    assert!(
        fs::read(published.parent().unwrap().join("encabezado.png")).expect("published sidecar")
            == png
    );
}

#[test]
fn agent_generated_image_can_be_a_creation_not_an_input_copy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let inner = FakeAgentEngine::new();
    // A standalone generated image (no web entry, no matching material) is a
    // legitimate agent OUTPUT: the provenance fix must never reject by extension.
    inner.set_artifacts(vec![artifact("workspace/portada.png", ArtifactKind::Image)]);
    let engine = AttachmentEditEngine::new(
        inner.clone(),
        tmp.path().join("projects").join("p-1").join("workspace"),
        Vec::new(),
    );
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        engine,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let p = state.create_project("Imagen").expect("create");
    write_artifact(tmp.path(), &p.id, "portada.png", b"generated-image-bytes");
    inner.set_message("Listo. Generé una imagen de portada.".into());
    let result = state
        .send_message(&p.id, "creá una imagen de portada", &[])
        .expect("send");
    assert_eq!(result.registered_creation_ids.len(), 1);
    let view = state.open_project(&p.id).expect("open");
    assert_eq!(view.creations.len(), 1);
    assert_eq!(view.creations[0].kind, "image");
}
