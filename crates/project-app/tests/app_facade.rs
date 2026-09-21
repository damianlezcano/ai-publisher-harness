use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};
use project_agent::{Artifact, ArtifactKind, FakeAgentEngine, RemoteUsage, UsageSource};
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
fn completed_turn_logs_sanitized_actual_usage_without_prompt_content() {
    project_app::session_log::clear();
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let project = app.create_project("A").expect("project");
    engine.set_message("Listo".into());
    engine.set_usage(RemoteUsage {
        input_tokens: Some(1834),
        output_tokens: Some(426),
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        total_tokens: Some(2260),
        cost_usd: Some(0.0018),
        source: UsageSource::ProviderActual,
    });

    app.send_message(&project.id, "PROMPT-PRIVATE-DO-NOT-LOG", &[])
        .expect("send");
    let usage = project_app::session_log::list()
        .into_iter()
        .find_map(|entry| {
            entry
                .usage
                .filter(|usage| usage.conversation_id == project.id)
        })
        .expect("usage record");
    assert_eq!(usage.conversation_id, project.id);
    assert_eq!(usage.input_tokens, Some(1834));
    assert_eq!(usage.total_tokens, Some(2260));
    assert_eq!(usage.source, "provider_actual");
    let rendered = format!("{usage:?}");
    assert!(!rendered.contains("PROMPT-PRIVATE-DO-NOT-LOG"));
}

#[test]
fn per_turn_metrics_are_distinct_and_accumulate_additively() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let project = app.create_project("A").expect("project");

    engine.set_message("Primera respuesta".into());
    engine.set_usage(RemoteUsage {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: Some(10),
        cache_write_tokens: Some(5),
        total_tokens: Some(165),
        cost_usd: Some(0.5),
        source: UsageSource::ProviderActual,
    });
    app.send_message(&project.id, "uno", &[]).expect("send");

    let first = app.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(first.input_tokens, Some(100));
    assert_eq!(first.output_tokens, Some(50));

    // A second turn with different provider usage must not overwrite the
    // first turn's per-turn record.
    engine.set_message("Segunda respuesta".into());
    engine.set_usage(RemoteUsage {
        input_tokens: Some(300),
        output_tokens: Some(150),
        cache_read_tokens: Some(30),
        cache_write_tokens: Some(15),
        total_tokens: Some(495),
        cost_usd: Some(0.25),
        source: UsageSource::ProviderActual,
    });
    app.send_message(&project.id, "dos", &[]).expect("send");

    let second = app.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(second.input_tokens, Some(300));
    assert_eq!(second.output_tokens, Some(150));
    assert_ne!(first.input_tokens, second.input_tokens);

    // Conversation totals are the additive sum of both turns, never a single
    // turn's values nor the latest turn only.
    let totals = app
        .accumulated_conversation_usage(&project.id)
        .unwrap()
        .unwrap();
    assert_eq!(totals.remote_calls, Some(2));
    assert_eq!(totals.input_tokens, Some(400));
    assert_eq!(totals.output_tokens, Some(200));
    assert_eq!(totals.cache_read_tokens, Some(40));
    assert_eq!(totals.cache_write_tokens, Some(20));
    assert_eq!(totals.total_tokens, Some(660));
    assert_eq!(totals.cost_usd, Some(0.75));
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
    // A non-web project shares the route root; root/latest are exposed too.
    assert_eq!(view.root_url.as_deref(), Some(url.as_str()));
    assert!(view.latest_url.as_deref().unwrap().ends_with("/latest/"));
    assert_eq!(view.current_version_id, None);

    let status = app.publication_status(&p.id).expect("status");
    assert_eq!(status.state, "published");
    assert_eq!(status.public_url, Some(url));
}

#[test]
fn share_publication_view_targets_the_immutable_current_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Actividad").expect("create");
    let publish = tmp.path().join("projects").join(&p.id).join("publish");

    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    engine.set_message("Listo.".into());
    write_artifact(tmp.path(), &p.id, "index.html", b"<html>V1</html>");
    let v1 = app
        .send_message(&p.id, "creá la actividad", &[])
        .expect("v1")
        .registered_creation_ids[0]
        .clone();
    app.publish_creation(&p.id, Some(&v1)).expect("share v1");
    let view = app.publication_status(&p.id).expect("status");
    let route = view
        .root_url
        .clone()
        .expect("root")
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .expect("route")
        .to_owned();
    let base = format!("https://fake-tunnel.trycloudflare.com/{route}");
    // The authoritative share URL is the immutable V1 URL, never root/latest.
    assert_eq!(
        view.public_url.as_deref(),
        Some(format!("{base}/{v1}/").as_str())
    );
    assert_eq!(view.root_url.as_deref(), Some(format!("{base}/").as_str()));
    assert_eq!(
        view.latest_url.as_deref(),
        Some(format!("{base}/latest/").as_str())
    );
    assert_eq!(view.current_version_id.as_deref(), Some(v1.as_str()));
    assert_ne!(view.public_url.as_deref(), view.root_url.as_deref());
    assert_ne!(view.public_url.as_deref(), view.latest_url.as_deref());

    // V2 becomes current; the share URL moves to the immutable V2 URL on the
    // SAME route, and the already-copied V1 URL still resolves to V1.
    engine.set_message("Listo. Cambié el título.".into());
    write_artifact(tmp.path(), &p.id, "index.html", b"<html>V2</html>");
    let v2 = app
        .send_message(&p.id, "cambiá el título", &[])
        .expect("v2")
        .registered_creation_ids[0]
        .clone();
    app.publish_creation(&p.id, Some(&v2)).expect("share v2");
    let view = app.publication_status(&p.id).expect("status v2");
    assert_eq!(
        view.public_url.as_deref(),
        Some(format!("{base}/{v2}/").as_str())
    );
    assert_eq!(view.current_version_id.as_deref(), Some(v2.as_str()));
    assert_eq!(view.root_url.as_deref(), Some(format!("{base}/").as_str()));
    assert_eq!(
        view.latest_url.as_deref(),
        Some(format!("{base}/latest/").as_str())
    );
    assert_ne!(view.public_url.as_deref(), view.root_url.as_deref());

    // V1 remains byte-identical and reachable under its immutable URL.
    let v1_html = fs::read_to_string(publish.join("versions").join(&v1).join("index.html"))
        .expect("v1 immutable");
    assert!(v1_html.contains("V1"), "{v1_html}");
    let v2_html = fs::read_to_string(publish.join("versions").join(&v2).join("index.html"))
        .expect("v2 immutable");
    assert!(v2_html.contains("V2"), "{v2_html}");
    // Root lists both and marks V2 Actual.
    let root_html = fs::read_to_string(publish.join("index.html")).expect("root");
    assert!(
        root_html.contains(&format!("href=\"{v1}/\""),),
        "{root_html}"
    );
    assert!(
        root_html.contains(&format!("href=\"{v2}/\""),),
        "{root_html}"
    );
    assert_eq!(root_html.matches("Actual").count(), 1, "{root_html}");
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

// -- Application shutdown / lifecycle ----------------------------------------

/// App-exit shutdown must terminate every owned runtime component (agent
/// engine / opencode backend, shared tunnel, local publisher) and be safe to
/// call more than once.
#[test]
fn app_shutdown_is_idempotent_and_stops_owned_children() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, tunnel) = app(tmp.path());
    let p = app.create_project("X").expect("create");
    app.publish(&p.id).expect("publish");

    app.shutdown();
    assert!(engine.calls().contains(&project_agent::FakeCall::Shutdown));
    assert!(tunnel.calls().contains(&project_tunnel::TunnelCall::Stop));
    assert!(!tunnel.running());

    // Idempotent: a second shutdown must not panic or regress state.
    app.shutdown();
    assert!(!tunnel.running());
    assert_eq!(
        app.publication_status(&p.id).expect("status").state,
        "local"
    );
}

/// A successful share must record the stage lifecycle in order:
/// requested -> prepared -> ready. All stage events carry the conversation id,
/// so the assertions are filtered by this test's unique project to stay
/// hermetic against the process-global session log (parallel integration
/// tests also publish into the same buffer).
#[test]
fn share_stage_logs_record_lifecycle_in_order() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, _) = app(tmp.path());
    let p = app.create_project("Fotosíntesis").expect("create");
    app.publish(&p.id).expect("publish");

    let conversation = format!("conversation_id={}", p.id);
    let share_events: Vec<String> = project_app::session_log::list()
        .into_iter()
        .map(|entry| entry.message)
        .filter(|message| message.contains(&conversation))
        .filter(|message| message.starts_with("[share]") || message.starts_with("[publish]"))
        .collect();
    assert!(
        share_events
            .iter()
            .any(|e| e.starts_with("[share] requested")),
        "missing share requested stage: {share_events:?}"
    );
    assert!(
        share_events
            .iter()
            .any(|e| e.starts_with("[publish] prepared")),
        "missing publish prepared stage: {share_events:?}"
    );
    assert!(
        share_events.iter().any(|e| e.starts_with("[share] ready")),
        "missing share ready stage: {share_events:?}"
    );
    let last_requested = share_events
        .iter()
        .rposition(|e| e.starts_with("[share] requested"))
        .expect("requested stage");
    let prepared_after = share_events[last_requested + 1..]
        .iter()
        .position(|e| e.starts_with("[publish] prepared"))
        .expect("prepared after requested");
    let ready_after = share_events[last_requested + 1 + prepared_after + 1..]
        .iter()
        .position(|e| e.starts_with("[share] ready"))
        .expect("ready after prepared");
    let _ = ready_after;
}

/// A failing tunnel start must be logged with the precise stage so a future
/// intermittent failure is localizable (tunnel_start, not a generic message).
#[test]
fn share_failure_logs_identify_tunnel_start_stage() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, tunnel) = app(tmp.path());
    let p = app.create_project("X").expect("create");
    tunnel.fail_start();
    let _ = app.publish(&p.id).unwrap_err();

    let messages: Vec<String> = project_app::session_log::list()
        .into_iter()
        .map(|entry| entry.message)
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("[share] failed stage=tunnel_start")),
        "tunnel_start failure stage not logged: {messages:?}"
    );
}

/// Unpublish is part of the share lifecycle: a failure must still be stage-
/// identifiable rather than a generic user-facing message only.
#[test]
fn unpublish_failure_logs_stage() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, _, tunnel) = app(tmp.path());
    let p = app.create_project("X").expect("create");
    app.publish(&p.id).expect("publish");
    tunnel.fail_stop();
    let _ = app.unpublish(&p.id).unwrap_err();

    let messages: Vec<String> = project_app::session_log::list()
        .into_iter()
        .map(|entry| entry.message)
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("[share] failed stage=unpublish")),
        "unpublish failure stage not logged: {messages:?}"
    );
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

    let published = tmp.path().join("projects").join(&p.id).join("publish");
    let html = fs::read_to_string(published.join("index.html")).expect("published html");
    // Root is the generated version-history landing page, never the raw
    // creation: the resource bytes live under the immutable version URL.
    assert!(!html.contains("JUEGO"), "{html}");
    assert!(html.contains("V1"), "{html}");
    assert!(html.contains("Actual"), "{html}");
    assert!(
        !html.contains("Material del proyecto"),
        "published root must be the version history, not the empty materials landing"
    );
    let v1_html = fs::read_to_string(published.join("versions").join(cid).join("index.html"))
        .expect("v1 html");
    assert!(v1_html.contains("JUEGO"), "{v1_html}");
}

#[test]
fn publish_without_creation_id_still_promotes_the_latest_web() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Actividad").expect("create");
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    write_artifact(tmp.path(), &p.id, "index.html", b"<h1>actividad</h1>");
    let run = app.run_agent(&p.id, "crea", &[]).expect("run");
    let cid = &run.registered_creation_ids[0];
    app.publish(&p.id).expect("publish");
    let publish = tmp.path().join("projects").join(&p.id).join("publish");
    let html = fs::read_to_string(publish.join("index.html")).expect("html");
    // Root is the version-history landing page (title = display name), the raw
    // resource bytes live under the immutable version URL.
    assert!(html.contains("Actividad"), "{html}");
    assert!(html.contains("V1"), "{html}");
    assert!(html.contains("Actual"), "{html}");
    assert!(!html.contains("<h1>actividad</h1>"), "{html}");
    assert!(!html.contains("Material del proyecto"), "{html}");
    let v1_html =
        fs::read_to_string(publish.join("versions").join(cid).join("index.html")).expect("v1 html");
    assert!(v1_html.contains("actividad"), "{v1_html}");
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
    // Sidecars land under the immutable version snapshot in publish/, never at
    // the root (the root is publication metadata only).
    let published_js = tmp
        .path()
        .join("projects")
        .join(&p.id)
        .join("publish")
        .join("versions")
        .join(cid)
        .join("app.js");
    assert_eq!(
        fs::read_to_string(&published_js).expect("published js"),
        "console.log(1)"
    );
    assert!(
        !tmp.path()
            .join("projects")
            .join(&p.id)
            .join("publish")
            .join("app.js")
            .exists()
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
fn quoted_and_shell_like_prompts_persist_and_reach_engine_verbatim() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("P").expect("create");
    engine.set_message("ok".to_owned());

    // User text is DATA: quotes, JSON-like text, and shell-like text must
    // round-trip through persistence and the engine prompt boundary untouched.
    let cases = [
        "\"hola\"",
        "{\"a\":\"b\"}",
        "$(touch /tmp/educai-should-not-exist)",
        r"C:\Users\test\archivo.txt",
        "línea uno\nlínea \"dos\"",
    ];
    for text in cases {
        let inputs = app.send_message_persist(&p.id, text, &[]).expect("persist");
        let view = app.open_project(&p.id).expect("open");
        let last = view.messages.last().expect("last message");
        assert_eq!(last.text, text, "persisted message must preserve {text:?}");
        let run = app.send_message_run(inputs).expect("run");
        assert_eq!(run.status, "completed");
        let engine_text = engine.last_prompt_text().unwrap_or_default();
        assert!(
            engine_text.ends_with(text),
            "engine prompt must keep user text byte-for-byte as its final segment; got ...{engine_text:?}"
        );
    }
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
fn later_turn_creates_a_new_immutable_version_and_refreshes_publish() {
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
    let v1 = first.registered_creation_ids[0].clone();

    app.publish_creation(&p.id, Some(&v1)).expect("share");
    let publish_dir = tmp.path().join("projects").join(&p.id).join("publish");
    // Root is the generated version-history landing page; the immutable V1
    // resource lives under versions/<v1>/.
    let root_html = fs::read_to_string(publish_dir.join("index.html")).expect("root");
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    assert!(!root_html.contains("ORIGINAL"), "{root_html}");
    let v1_published =
        fs::read_to_string(publish_dir.join("versions").join(&v1).join("index.html"))
            .expect("v1 snapshot");
    assert!(v1_published.contains("ORIGINAL"), "{v1_published}");

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
    assert_eq!(second.registered_creation_ids.len(), 1);
    let v2 = second.registered_creation_ids[0].clone();
    assert_ne!(
        v2, v1,
        "a modification must mint a NEW version, never mutate V1"
    );

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.creations.len(), 2, "V1 and V2 both exist");
    let v1_view = view.creations.iter().find(|c| c.id == v1).expect("V1 card");
    let v2_view = view.creations.iter().find(|c| c.id == v2).expect("V2 card");
    assert_eq!(v1_view.version_number, 1);
    assert_eq!(v2_view.version_number, 2);
    assert_eq!(v1_view.lineage_id, v2_view.lineage_id);
    assert_eq!(v2_view.parent_version_id.as_deref(), Some(v1.as_str()));
    assert!(!v1_view.is_current);
    assert!(v2_view.is_current);

    // V1 bytes remain byte-identical; V2 is the complete self-contained snapshot.
    let outputs = tmp.path().join("projects").join(&p.id).join("outputs");
    let v1_html = fs::read_to_string(outputs.join(&v1).join("index.html")).expect("v1 html");
    assert!(v1_html.contains("ORIGINAL"), "{v1_html}");
    assert!(!v1_html.contains("UPDATED"), "V1 must be immutable");
    let v2_html = fs::read_to_string(outputs.join(&v2).join("index.html")).expect("v2 html");
    assert!(v2_html.contains("UPDATED"), "{v2_html}");

    // The shared URL now serves the current version (V2) under its immutable URL:
    // the landing page lists both versions, and versions/<v2>/ carries the bytes.
    let root_html = fs::read_to_string(publish_dir.join("index.html")).expect("updated root");
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("V2"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    assert!(!root_html.contains("UPDATED"), "{root_html}");
    let v2_published =
        fs::read_to_string(publish_dir.join("versions").join(&v2).join("index.html"))
            .expect("v2 snapshot");
    assert!(v2_published.contains("UPDATED"), "{v2_published}");
    assert!(v2_published.contains("background:white"), "{v2_published}");
    assert!(!v2_published.contains("ORIGINAL"), "{v2_published}");
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
    write_artifact(tmp.path(), &p.id, "index.html", b"<h1>UNO</h1>");
    let first = app.send_message(&p.id, "creá una", &[]).expect("first");
    let cid = first.registered_creation_ids[0].clone();
    app.publish_creation(&p.id, Some(&cid)).expect("share");
    let publish = tmp.path().join("projects").join(&p.id).join("publish");
    let root_html = fs::read_to_string(publish.join("index.html")).expect("root");
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    assert!(!root_html.contains("UNO"), "{root_html}");
    let v1_html = fs::read_to_string(publish.join("versions").join(&cid).join("index.html"))
        .expect("v1 snapshot");
    assert!(v1_html.contains("UNO"), "{v1_html}");

    engine.set_artifacts(vec![artifact(
        "workspace/actividad-2/index.html",
        ArtifactKind::Web,
    )]);
    write_artifact(tmp.path(), &p.id, "actividad-2/index.html", b"<h1>DOS</h1>");
    app.send_message(&p.id, "creá otra", &[]).expect("second");

    let root_html = fs::read_to_string(publish.join("index.html")).expect("root after second");
    assert!(!root_html.contains("DOS"), "{root_html}");
    let v1_html = fs::read_to_string(publish.join("versions").join(&cid).join("index.html"))
        .expect("v1 still");
    assert!(v1_html.contains("UNO"), "{v1_html}");
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
    let v1 = first.registered_creation_ids[0].clone();
    state.publish_creation(&p.id, Some(&v1)).expect("share");
    let publish = tmp.path().join("projects").join(&p.id).join("publish");
    let root_html = fs::read_to_string(publish.join("index.html")).expect("first root");
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    assert!(!root_html.contains("ORIGINAL"), "{root_html}");
    assert!(
        fs::read_to_string(publish.join("versions").join(&v1).join("index.html"))
            .expect("v1 snapshot")
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

    // A1 stays INPUT material: no phantom "Imagen" Creation; the update is a
    // NEW immutable version (V2) of the same web lineage.
    assert_eq!(second.registered_creation_ids.len(), 1);
    let v2 = second.registered_creation_ids[0].clone();
    assert_ne!(v2, v1, "a modification must mint a new version");
    let view = state.open_project(&p.id).expect("open");
    assert_eq!(
        view.creations.len(),
        2,
        "attached PNG must not become a Creation; V1 and V2 exist"
    );
    let v1_view = view.creations.iter().find(|c| c.id == v1).expect("V1 card");
    let v2_view = view.creations.iter().find(|c| c.id == v2).expect("V2 card");
    assert_eq!(v1_view.version_number, 1);
    assert_eq!(v2_view.version_number, 2);
    assert_eq!(v1_view.lineage_id, v2_view.lineage_id);
    assert_eq!(v2_view.parent_version_id.as_deref(), Some(v1.as_str()));
    assert!(v2_view.is_current);

    // V2 serves the image as a web sidecar; V1 stays byte-identical.
    let outputs_dir = tmp.path().join("projects").join(&p.id).join("outputs");
    let v1_html = fs::read_to_string(outputs_dir.join(&v1).join("index.html")).expect("v1 html");
    assert!(
        v1_html.contains("ORIGINAL"),
        "V1 must be immutable: {v1_html}"
    );
    let v2_html =
        fs::read_to_string(outputs_dir.join(&v2).join("index.html")).expect("updated html");
    assert!(v2_html.contains("UPDATED"), "{v2_html}");
    assert!(!v2_html.contains("ORIGINAL"), "{v2_html}");
    assert_eq!(
        fs::read(outputs_dir.join(&v2).join("encabezado.png")).expect("sidecar"),
        png
    );

    // The shared history now shows V1..V2 with V2 Actual; the current resource
    // (with the image) lives under the immutable V2 URL.
    let root_html = fs::read_to_string(publish.join("index.html")).expect("refreshed root");
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("V2"), "{root_html}");
    assert_eq!(root_html.matches("Actual").count(), 1, "{root_html}");
    let v2_published = fs::read_to_string(publish.join("versions").join(&v2).join("index.html"))
        .expect("v2 published");
    assert!(v2_published.contains("UPDATED"), "{v2_published}");
    assert_eq!(
        fs::read(publish.join("versions").join(&v2).join("encabezado.png"))
            .expect("published sidecar"),
        png
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

/// The approved primary regression: a V1 web bundle (index.html + estilos.css +
/// app.js), then a CSS-only change (V2), then a JS-only change (V3). Every
/// version is a complete self-contained snapshot, earlier versions are
/// byte-identical, and the lineage is stable across the chain.
#[test]
fn css_then_js_changes_produce_complete_v1_v2_v3_chain() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Actividad").expect("create");
    let outputs = tmp.path().join("projects").join(&p.id).join("outputs");

    // V1: index.html + estilos.css + app.js created in one turn.
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    engine.set_message("Listo. Creé la actividad.".into());
    write_artifact(tmp.path(), &p.id, "index.html", b"<html>V1</html>");
    write_artifact(tmp.path(), &p.id, "estilos.css", b"body{color:red}");
    write_artifact(tmp.path(), &p.id, "app.js", b"console.log(1)");
    let first = app
        .send_message(&p.id, "creá la actividad", &[])
        .expect("v1");
    assert_eq!(first.registered_creation_ids.len(), 1);
    let v1 = first.registered_creation_ids[0].clone();

    // V2: only estilos.css changed. The diff names the changed sidecar.
    engine.set_artifacts(vec![artifact("workspace/estilos.css", ArtifactKind::Other)]);
    engine.set_message("Listo. Cambié el color.".into());
    write_artifact(tmp.path(), &p.id, "estilos.css", b"body{color:blue}");
    let second = app.send_message(&p.id, "cambiá el color", &[]).expect("v2");
    assert_eq!(second.registered_creation_ids.len(), 1);
    let v2 = second.registered_creation_ids[0].clone();
    assert_ne!(v2, v1);

    // V3: only app.js changed.
    engine.set_artifacts(vec![artifact("workspace/app.js", ArtifactKind::Other)]);
    engine.set_message("Listo. Cambié el comportamiento.".into());
    write_artifact(tmp.path(), &p.id, "app.js", b"console.log(3)");
    let third = app
        .send_message(&p.id, "cambiá el comportamiento", &[])
        .expect("v3");
    assert_eq!(third.registered_creation_ids.len(), 1);
    let v3 = third.registered_creation_ids[0].clone();
    assert_ne!(v3, v2);

    let view = app.open_project(&p.id).expect("open");
    assert_eq!(view.creations.len(), 3);
    let card = |id: &str| view.creations.iter().find(|c| c.id == id).expect("card");
    assert_eq!(card(&v1).version_number, 1);
    assert_eq!(card(&v2).version_number, 2);
    assert_eq!(card(&v3).version_number, 3);
    assert_eq!(card(&v1).lineage_id, card(&v2).lineage_id);
    assert_eq!(card(&v2).lineage_id, card(&v3).lineage_id);
    assert_eq!(card(&v2).parent_version_id.as_deref(), Some(v1.as_str()));
    assert_eq!(card(&v3).parent_version_id.as_deref(), Some(v2.as_str()));
    assert!(!card(&v1).is_current);
    assert!(!card(&v2).is_current);
    assert!(card(&v3).is_current);
    assert_eq!(
        card(&v3).available_version_ids,
        vec![v1.clone(), v2.clone(), v3.clone()]
    );

    // Every version is complete; earlier versions are byte-identical.
    assert_eq!(
        fs::read(outputs.join(&v1).join("index.html")).unwrap(),
        b"<html>V1</html>"
    );
    assert_eq!(
        fs::read(outputs.join(&v1).join("estilos.css")).unwrap(),
        b"body{color:red}"
    );
    assert_eq!(
        fs::read(outputs.join(&v1).join("app.js")).unwrap(),
        b"console.log(1)"
    );

    assert_eq!(
        fs::read(outputs.join(&v2).join("index.html")).unwrap(),
        b"<html>V1</html>"
    );
    assert_eq!(
        fs::read(outputs.join(&v2).join("estilos.css")).unwrap(),
        b"body{color:blue}"
    );
    assert_eq!(
        fs::read(outputs.join(&v2).join("app.js")).unwrap(),
        b"console.log(1)"
    );

    assert_eq!(
        fs::read(outputs.join(&v3).join("index.html")).unwrap(),
        b"<html>V1</html>"
    );
    assert_eq!(
        fs::read(outputs.join(&v3).join("estilos.css")).unwrap(),
        b"body{color:blue}"
    );
    assert_eq!(
        fs::read(outputs.join(&v3).join("app.js")).unwrap(),
        b"console.log(3)"
    );
}

#[test]
fn sharing_v3_publishes_all_historical_versions_and_republish_restores() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Rosco").expect("create");
    let outputs = tmp.path().join("projects").join(&p.id).join("outputs");
    let publish = tmp.path().join("projects").join(&p.id).join("publish");

    // V1 (private)
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    engine.set_message("Listo. Creé la actividad.".into());
    write_artifact(tmp.path(), &p.id, "index.html", b"<html>V1</html>");
    let v1 = app
        .send_message(&p.id, "creá la actividad", &[])
        .expect("v1")
        .registered_creation_ids[0]
        .clone();

    // V2 (private)
    engine.set_message("Listo. Cambié el título.".into());
    write_artifact(tmp.path(), &p.id, "index.html", b"<html>V2</html>");
    let v2 = app
        .send_message(&p.id, "cambiá el título", &[])
        .expect("v2")
        .registered_creation_ids[0]
        .clone();

    // V3 (private)
    engine.set_message("Listo. Cambié el color.".into());
    write_artifact(tmp.path(), &p.id, "index.html", b"<html>V3</html>");
    let v3 = app
        .send_message(&p.id, "cambiá el color", &[])
        .expect("v3")
        .registered_creation_ids[0]
        .clone();

    let before = app.open_project(&p.id).expect("open");
    assert!(
        before.creations.iter().all(|c| c.visibility == "private"),
        "all versions start private"
    );

    // First share targets V3 only.
    app.publish_creation(&p.id, Some(&v3)).expect("share v3");
    // Root is the generated version-history landing page, not a copied index.
    let root_html = fs::read_to_string(publish.join("index.html")).expect("root");
    assert!(root_html.contains("V3"), "{root_html}");
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    assert!(!root_html.contains("<html>V3</html>"), "{root_html}");
    assert_eq!(
        fs::read(publish.join("versions").join(&v1).join("index.html")).expect("v1"),
        b"<html>V1</html>"
    );
    assert_eq!(
        fs::read(publish.join("versions").join(&v2).join("index.html")).expect("v2"),
        b"<html>V2</html>"
    );
    assert_eq!(
        fs::read(publish.join("versions").join(&v3).join("index.html")).expect("v3"),
        b"<html>V3</html>"
    );

    // Unpublish removes the route but preserves durable outputs + history.
    app.unpublish(&p.id).expect("unpublish");
    assert_eq!(
        app.publication_status(&p.id).expect("status").state,
        "local"
    );
    assert!(outputs.join(&v1).join("index.html").exists());
    assert!(outputs.join(&v2).join("index.html").exists());
    assert!(outputs.join(&v3).join("index.html").exists());

    // Re-publish restores current + historical URLs.
    app.publish_creation(&p.id, Some(&v3)).expect("republish");
    assert_eq!(
        app.publication_status(&p.id).expect("status").state,
        "published"
    );
    let root_html = fs::read_to_string(publish.join("index.html")).expect("root again");
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("V3"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    assert!(!root_html.contains("<html>V3</html>"), "{root_html}");
    assert_eq!(
        fs::read(publish.join("versions").join(&v1).join("index.html")).expect("v1 again"),
        b"<html>V1</html>"
    );
    assert_eq!(
        fs::read(publish.join("versions").join(&v2).join("index.html")).expect("v2 again"),
        b"<html>V2</html>"
    );
    assert_eq!(
        fs::read(publish.join("versions").join(&v3).join("index.html")).expect("v3 again"),
        b"<html>V3</html>"
    );
}

#[test]
fn sharing_one_web_lineage_does_not_expose_an_unrelated_lineage() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Rosco").expect("create");
    let publish = tmp.path().join("projects").join(&p.id).join("publish");

    // Lineage A at the workspace root.
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    engine.set_message("Listo.".into());
    write_artifact(tmp.path(), &p.id, "index.html", b"<html>A</html>");
    let a = app
        .send_message(&p.id, "creá A", &[])
        .expect("a")
        .registered_creation_ids[0]
        .clone();

    // Lineage B in a subfolder.
    engine.set_artifacts(vec![artifact("workspace/b/index.html", ArtifactKind::Web)]);
    engine.set_message("Listo.".into());
    write_artifact(tmp.path(), &p.id, "b/index.html", b"<html>B</html>");
    let b = app
        .send_message(&p.id, "creá B", &[])
        .expect("b")
        .registered_creation_ids[0]
        .clone();

    app.publish_creation(&p.id, Some(&a)).expect("share A");
    let root_html = fs::read_to_string(publish.join("index.html")).expect("root");
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    assert!(!root_html.contains("<html>A</html>"), "{root_html}");
    assert!(
        publish
            .join("versions")
            .join(&a)
            .join("index.html")
            .exists()
    );
    assert!(
        !publish.join("versions").join(&b).exists(),
        "unrelated lineage B must not become public"
    );
    assert!(
        !root_html.contains("href=\"{b}/\""),
        "B must not appear in history"
    );
}

#[test]
fn two_same_display_name_web_lineages_keep_sidecars_isolated() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Juego").expect("create");
    let outputs = tmp.path().join("projects").join(&p.id).join("outputs");

    // Two independent bundles whose folders sanitize to the same display name.
    engine.set_artifacts(vec![artifact(
        "workspace/a/juego/index.html",
        ArtifactKind::Web,
    )]);
    engine.set_message("Listo.".into());
    write_artifact(tmp.path(), &p.id, "a/juego/index.html", b"<html>A1</html>");
    let a_v1 = app
        .send_message(&p.id, "creá A", &[])
        .expect("a")
        .registered_creation_ids[0]
        .clone();

    engine.set_artifacts(vec![artifact(
        "workspace/b/juego/index.html",
        ArtifactKind::Web,
    )]);
    engine.set_message("Listo.".into());
    write_artifact(tmp.path(), &p.id, "b/juego/index.html", b"<html>B1</html>");
    let b_v1 = app
        .send_message(&p.id, "creá B", &[])
        .expect("b")
        .registered_creation_ids[0]
        .clone();

    let view = app.open_project(&p.id).expect("open");
    let card = |id: &str| view.creations.iter().find(|c| c.id == id).expect("card");
    assert_eq!(card(&a_v1).display_name, card(&b_v1).display_name);
    assert_ne!(card(&a_v1).lineage_id, card(&b_v1).lineage_id);

    // A CSS-only change to lineage A.
    engine.set_artifacts(vec![artifact(
        "workspace/a/juego/estilos.css",
        ArtifactKind::Other,
    )]);
    engine.set_message("Listo. Cambié el color.".into());
    write_artifact(
        tmp.path(),
        &p.id,
        "a/juego/estilos.css",
        b"body{color:blue}",
    );
    let a_v2 = app
        .send_message(&p.id, "cambiá el color de A", &[])
        .expect("a v2")
        .registered_creation_ids[0]
        .clone();

    assert_ne!(a_v2, a_v1, "A must get a NEW version, never B");
    let view = app.open_project(&p.id).expect("open");
    let card = |id: &str| view.creations.iter().find(|c| c.id == id).expect("card");
    assert_eq!(card(&a_v2).lineage_id, card(&a_v1).lineage_id);
    assert_eq!(card(&a_v2).version_number, 2);

    // A's new version has the CSS; B's version does not.
    assert_eq!(
        fs::read(outputs.join(&a_v2).join("estilos.css")).expect("A css"),
        b"body{color:blue}"
    );
    assert_eq!(
        fs::read(outputs.join(&a_v2).join("index.html")).expect("A inherited html"),
        b"<html>A1</html>"
    );
    assert!(
        !outputs.join(&b_v1).join("estilos.css").exists(),
        "B must not receive A's CSS"
    );
    assert_eq!(
        fs::read(outputs.join(&b_v1).join("index.html")).expect("B html"),
        b"<html>B1</html>"
    );
}

#[test]
fn one_turn_can_update_two_web_lineages_independently() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Doble").expect("create");
    let outputs = tmp.path().join("projects").join(&p.id).join("outputs");

    engine.set_artifacts(vec![artifact("workspace/a/index.html", ArtifactKind::Web)]);
    engine.set_message("Listo.".into());
    write_artifact(tmp.path(), &p.id, "a/index.html", b"<html>A1</html>");
    let a_v1 = app
        .send_message(&p.id, "creá A", &[])
        .expect("a")
        .registered_creation_ids[0]
        .clone();

    engine.set_artifacts(vec![artifact("workspace/b/index.html", ArtifactKind::Web)]);
    engine.set_message("Listo.".into());
    write_artifact(tmp.path(), &p.id, "b/index.html", b"<html>B1</html>");
    let b_v1 = app
        .send_message(&p.id, "creá B", &[])
        .expect("b")
        .registered_creation_ids[0]
        .clone();

    // One turn changes A's CSS and B's JS at the same time.
    engine.set_artifacts(vec![
        artifact("workspace/a/estilos.css", ArtifactKind::Other),
        artifact("workspace/b/app.js", ArtifactKind::Other),
    ]);
    engine.set_message("Listo. Actualicé ambas.".into());
    write_artifact(tmp.path(), &p.id, "a/estilos.css", b"body{color:blue}");
    write_artifact(tmp.path(), &p.id, "b/app.js", b"console.log(3)");
    let result = app.send_message(&p.id, "cambiá ambas", &[]).expect("both");

    assert_eq!(
        result.registered_creation_ids.len(),
        2,
        "two lineages updated -> two new versions"
    );

    let view = app.open_project(&p.id).expect("open");
    let card = |id: &str| view.creations.iter().find(|c| c.id == id).expect("card");
    let a_v2 = result
        .registered_creation_ids
        .iter()
        .find(|id| card(id).lineage_id == card(&a_v1).lineage_id)
        .cloned()
        .expect("A v2");
    let b_v2 = result
        .registered_creation_ids
        .iter()
        .find(|id| card(id).lineage_id == card(&b_v1).lineage_id)
        .cloned()
        .expect("B v2");

    // A's new version has A's CSS but not B's JS; B's has B's JS but not A's CSS.
    assert_eq!(
        fs::read(outputs.join(&a_v2).join("estilos.css")).unwrap(),
        b"body{color:blue}"
    );
    assert!(!outputs.join(&a_v2).join("app.js").exists());
    assert_eq!(
        fs::read(outputs.join(&b_v2).join("app.js")).unwrap(),
        b"console.log(3)"
    );
    assert!(!outputs.join(&b_v2).join("estilos.css").exists());
}

#[test]
fn root_and_subfolder_bundles_are_owned_by_their_own_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Mixto").expect("create");
    let outputs = tmp.path().join("projects").join(&p.id).join("outputs");

    // Root bundle.
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    engine.set_message("Listo.".into());
    write_artifact(tmp.path(), &p.id, "index.html", b"<html>ROOT</html>");
    let root_v1 = app
        .send_message(&p.id, "creá raíz", &[])
        .expect("root")
        .registered_creation_ids[0]
        .clone();

    // Subfolder bundle.
    engine.set_artifacts(vec![artifact(
        "workspace/actividad/index.html",
        ArtifactKind::Web,
    )]);
    engine.set_message("Listo.".into());
    write_artifact(
        tmp.path(),
        &p.id,
        "actividad/index.html",
        b"<html>SUB</html>",
    );
    let sub_v1 = app
        .send_message(&p.id, "creá sub", &[])
        .expect("sub")
        .registered_creation_ids[0]
        .clone();

    // A root-level CSS change belongs to the root bundle, not the subfolder.
    engine.set_artifacts(vec![artifact("workspace/estilos.css", ArtifactKind::Other)]);
    engine.set_message("Listo.".into());
    write_artifact(tmp.path(), &p.id, "estilos.css", b"root-css");
    let root_v2 = app
        .send_message(&p.id, "cambiá raíz", &[])
        .expect("root v2")
        .registered_creation_ids[0]
        .clone();

    let view = app.open_project(&p.id).expect("open");
    let card = |id: &str| view.creations.iter().find(|c| c.id == id).expect("card");
    assert_eq!(card(&root_v2).lineage_id, card(&root_v1).lineage_id);
    assert_eq!(
        fs::read(outputs.join(&root_v2).join("estilos.css")).unwrap(),
        b"root-css"
    );
    assert!(!outputs.join(&sub_v1).join("estilos.css").exists());

    // A subfolder CSS change belongs to the subfolder bundle.
    engine.set_artifacts(vec![artifact(
        "workspace/actividad/estilos.css",
        ArtifactKind::Other,
    )]);
    engine.set_message("Listo.".into());
    write_artifact(tmp.path(), &p.id, "actividad/estilos.css", b"sub-css");
    let sub_v2 = app
        .send_message(&p.id, "cambiá sub", &[])
        .expect("sub v2")
        .registered_creation_ids[0]
        .clone();

    let view = app.open_project(&p.id).expect("open");
    let card = |id: &str| view.creations.iter().find(|c| c.id == id).expect("card");
    assert_eq!(card(&sub_v2).lineage_id, card(&sub_v1).lineage_id);
    assert_eq!(
        fs::read(outputs.join(&sub_v2).join("estilos.css")).unwrap(),
        b"sub-css"
    );
}

#[test]
fn bundle_ownership_survives_restart() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // First session creates a subfolder bundle.
    let (app, engine, _) = app(tmp.path());
    let p = app.create_project("Actividad").expect("create");
    engine.set_artifacts(vec![artifact(
        "workspace/actividad/index.html",
        ArtifactKind::Web,
    )]);
    engine.set_message("Listo.".into());
    write_artifact(
        tmp.path(),
        &p.id,
        "actividad/index.html",
        b"<html>V1</html>",
    );
    let v1 = app
        .send_message(&p.id, "creá", &[])
        .expect("v1")
        .registered_creation_ids[0]
        .clone();
    drop(app);
    drop(engine);

    // Restart on the same base.
    let (app, engine, _) = crate::app(tmp.path());
    engine.set_artifacts(vec![artifact(
        "workspace/actividad/estilos.css",
        ArtifactKind::Other,
    )]);
    engine.set_message("Listo.".into());
    write_artifact(
        tmp.path(),
        &p.id,
        "actividad/estilos.css",
        b"body{color:blue}",
    );
    let v2 = app
        .send_message(&p.id, "cambiá el color", &[])
        .expect("v2")
        .registered_creation_ids[0]
        .clone();

    let view = app.open_project(&p.id).expect("open");
    let card = |id: &str| view.creations.iter().find(|c| c.id == id).expect("card");
    assert_eq!(card(&v2).lineage_id, card(&v1).lineage_id);
    assert_eq!(card(&v2).version_number, 2);

    let outputs = tmp.path().join("projects").join(&p.id).join("outputs");
    assert_eq!(
        fs::read(outputs.join(&v2).join("estilos.css")).unwrap(),
        b"body{color:blue}"
    );
    assert_eq!(
        fs::read(outputs.join(&v2).join("index.html")).unwrap(),
        b"<html>V1</html>"
    );
}
