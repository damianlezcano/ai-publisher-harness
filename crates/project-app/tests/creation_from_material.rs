//! Creation-from-material routing: same-turn attachment, explicit filename,
//! demonstration/regression matrix, clarification, and multi-material bounding.
//!
//! These tests exercise the production `send_message` / accepted-staged path
//! through the exact `AppState` facade a real turn uses, with `FakeAgentEngine`
//! so the whole flow is offline and deterministic. The critical staged
//! production-seam proof lives in the `app.rs` unit test module
//! (`creation_from_material_tests`) where the test-only embedding-provider
//! seam can assert "embeddings only during ingestion, never during creation".

use std::fs;
use std::sync::{Arc, Mutex};

use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};
use project_agent::{Artifact, ArtifactKind, FakeAgentEngine};
use project_app::AppState;
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

/// Records the exact prompt text each `send` received so a test can assert the
/// creation context grounded the turn and no raw workspace attachment leaked.
struct RecordingEngine(FakeAgentEngine, Arc<Mutex<Vec<String>>>);

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

fn recording_app(
    base: &std::path::Path,
    recorded: Arc<Mutex<Vec<String>>>,
) -> (
    AppState<RecordingEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
    FakeAgentEngine,
) {
    let inner = FakeAgentEngine::new();
    let state = AppState::with_components(
        base.to_path_buf(),
        RecordingEngine(inner.clone(), recorded),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    (state, inner)
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
    fs::create_dir_all(path.parent().expect("parent")).expect("workspace dirs");
    fs::write(&path, bytes).expect("write artifact");
}

fn add_file<E: project_agent::AgentEngine>(
    app: &AppState<E, FakeTunnel, FakeProviderConnector, FakeRestarter>,
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

fn seed_web_creation(base: &std::path::Path, project_id: &str, engine: &FakeAgentEngine) {
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web)]);
    write_artifact(base, project_id, "index.html", b"<html></html>");
    engine.set_message("Listo. Creé el recurso a partir del material.".into());
}

/// A bounded Markdown body long enough to produce several chunks.
fn long_body(name: &str, sentinel: &str) -> String {
    let mut body = format!(
        "# {name}\n\nContenido de ejemplo para generar una creación a partir del material.\n\n"
    );
    for index in 0..30 {
        body.push_str(&format!(
            "## Parte {index}\n\nLa parte {index} agrega contenido suficiente para fragmentar el documento en varios fragmentos con una longitud razonable.\n\n"
        ));
    }
    body.push_str(sentinel);
    body.push('\n');
    body
}

// -- Test 1: same-turn attachment wins ----------------------------------------

#[test]
fn same_turn_attached_readme_is_the_creation_target() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let (app, engine) = recording_app(tmp.path(), recorded.clone());
    let p = app.create_project("P").unwrap();
    let mid = add_file(
        &app,
        tmp.path(),
        &p.id,
        "README.md",
        long_body("README.md", "cierre del material adjunto.").as_bytes(),
    );
    seed_web_creation(tmp.path(), &p.id, &engine);

    let run = app
        .send_message(
            &p.id,
            "me podes armar una presentacion interactiva para presentar esto?",
            &[mid],
        )
        .expect("send");
    assert_eq!(run.status, "completed");
    assert!(
        !run.registered_creation_ids.is_empty(),
        "a real artifact must be created"
    );

    let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_eq!(metrics.turn_kind.as_deref(), Some("creation_from_material"));
    assert_eq!(
        metrics.local_mode.as_deref(),
        Some("creation_from_material")
    );
    assert_eq!(
        metrics.retrieval_mode, None,
        "creation action is not retrieval_mode=hybrid/normal"
    );

    let usage = project_app::session_log::list();
    assert!(
        usage
            .iter()
            .filter_map(|entry| entry.usage.as_ref())
            .any(|usage| usage.conversation_id == p.id && usage.reason == "creation_from_material"),
        "reason=creation_from_material must be recorded"
    );
    assert!(
        usage.iter().any(|entry| {
            entry.message.contains("creation_from_material=true")
                && entry.message.contains("target_count=1")
                && entry.message.contains("no_reembedding=true")
        }),
        "bounded creation telemetry must be recorded"
    );

    // The exact target source is README.md and the prompt grounds the creation
    // on it without raw-forwarding, and never claims the directory is empty.
    let prompt = recorded.lock().unwrap().last().cloned().unwrap_or_default();
    assert!(
        prompt.contains("README.md"),
        "target README.md in context: {prompt}"
    );
    assert!(
        prompt.contains("material already available in Knowledge"),
        "creation directive: {prompt}"
    );
    assert!(
        prompt.contains("empty filesystem does not mean there is no source material"),
        "anti-empty-directory directive: {prompt}"
    );
    assert!(
        !prompt.contains("materials/1-README.md"),
        "raw attachment must not be duplicated: {prompt}"
    );
    let view = app.open_project(&p.id).unwrap();
    assert_eq!(view.creations.len(), 1);
}

// -- Test 2: explicit filename ------------------------------------------------

#[test]
fn explicit_filename_is_resolved_without_a_new_attachment() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let (app, engine) = recording_app(tmp.path(), recorded.clone());
    let p = app.create_project("P").unwrap();
    add_file(
        &app,
        tmp.path(),
        &p.id,
        "README.md",
        long_body("README.md", "fin del readme.").as_bytes(),
    );
    seed_web_creation(tmp.path(), &p.id, &engine);

    // No new attachment: deterministic source-name resolution.
    let run = app
        .send_message(
            &p.id,
            "me podes armar una presentacion interactiva para presentar este README.md?",
            &[],
        )
        .expect("send");
    assert_eq!(run.status, "completed");
    assert!(!run.registered_creation_ids.is_empty());

    let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_eq!(metrics.turn_kind.as_deref(), Some("creation_from_material"));
    assert_eq!(metrics.retrieval_mode, None);
    let prompt = recorded.lock().unwrap().last().cloned().unwrap_or_default();
    assert!(
        prompt.contains("README.md"),
        "explicit filename target: {prompt}"
    );
}

// -- Test 3: summary regression -----------------------------------------------

#[test]
fn summary_wording_is_not_stolen_by_creation() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let (app, engine) = recording_app(tmp.path(), recorded.clone());
    let p = app.create_project("P").unwrap();
    let mid = add_file(
        &app,
        tmp.path(),
        &p.id,
        "notas.txt",
        b"Resumen de la reunion: se decidio migrar a OpenShift.",
    );
    engine.set_message("Resumen del archivo: migración a OpenShift.".into());
    engine.set_artifacts(vec![]);

    let run = app
        .send_message(&p.id, "me resumís el archivo?", &[mid])
        .expect("send");
    assert_eq!(run.status, "completed");
    let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_ne!(
        metrics.turn_kind.as_deref(),
        Some("creation_from_material"),
        "a summary request must never route to creation"
    );
    assert_eq!(
        metrics.retrieval_mode.as_deref(),
        Some("normal"),
        "summary wording stays on the ordinary semantic path"
    );
}

// -- Test 4: NormalSemantic regression ----------------------------------------

#[test]
fn ordinary_qa_is_not_creation() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().unwrap();
    let (app, engine) = recording_app(tmp.path(), Arc::new(Mutex::new(Vec::new())));
    let p = app.create_project("P").unwrap();
    let mid = add_file(
        &app,
        tmp.path(),
        &p.id,
        "README.md",
        b"# Grok\n\nGrok es un modelo conversacional. Los agentes usan funciones.",
    );
    engine.set_message("Grok es un modelo conversacional.".into());
    engine.set_artifacts(vec![]);

    let run = app
        .send_message(&p.id, "¿qué dice el README sobre Grok?", &[mid])
        .expect("send");
    assert_eq!(run.status, "completed");
    let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_ne!(metrics.turn_kind.as_deref(), Some("creation_from_material"));
    assert_eq!(
        metrics.retrieval_mode, None,
        "an open question is not locally forced to Knowledge retrieval"
    );
}

// -- Test 5: missing target clarification -------------------------------------

#[test]
fn missing_target_returns_clarification_without_topk_retrieval() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let (app, engine) = recording_app(tmp.path(), recorded.clone());
    let p = app.create_project("P").unwrap();
    // No attachment, no persisted referent, no Knowledge store at all.
    let run = app
        .send_message(&p.id, "creame una presentación sobre esto", &[])
        .expect("send");
    assert_eq!(run.status, "completed");
    assert!(
        run.registered_creation_ids.is_empty(),
        "clarification must create nothing"
    );
    let message = run.message.unwrap_or_default();
    assert!(
        message.contains("Adjuntá el archivo o decime su nombre"),
        "user must be asked to identify the target: {message}"
    );

    let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_eq!(metrics.turn_kind.as_deref(), Some("creation_from_material"));
    assert_eq!(
        metrics.local_mode.as_deref(),
        Some("creation_from_material")
    );
    assert_eq!(metrics.retrieval_mode, None, "no top-K retrieval");
    assert_eq!(metrics.remote_calls, Some(0), "no provider call");
    assert!(
        !engine.calls().contains(&project_agent::FakeCall::Send),
        "a clarification must never reach the agent engine"
    );
    assert!(
        recorded.lock().unwrap().is_empty(),
        "a clarification must never serialize a provider prompt"
    );
}

// -- Test 5b: demonstrative resolves to a compatible prior MaterialSet --------

#[test]
fn demonstrative_resolves_to_a_prior_material_set_without_a_new_attachment() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let (app, engine) = recording_app(tmp.path(), recorded.clone());
    let p = app.create_project("P").unwrap();
    add_file(
        &app,
        tmp.path(),
        &p.id,
        "reunion-a.md",
        b"# Reunion A\n\ncontenido de la reunion A",
    );
    add_file(
        &app,
        tmp.path(),
        &p.id,
        "reunion-b.md",
        b"# Reunion B\n\ncontenido de la reunion B",
    );
    // The inventory list persists a durable MaterialSet referent.
    let inventory = app
        .send_message(&p.id, "listame los archivos", &[])
        .expect("inventory");
    assert_eq!(inventory.status, "completed");

    seed_web_creation(tmp.path(), &p.id, &engine);
    // No new attachment: the demonstrative must bind the prior MaterialSet.
    let run = app
        .send_message(&p.id, "creame una presentación sobre esto", &[])
        .expect("send");
    assert_eq!(run.status, "completed");
    assert!(!run.registered_creation_ids.is_empty());

    let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_eq!(metrics.turn_kind.as_deref(), Some("creation_from_material"));
    assert_eq!(metrics.retrieval_mode, None);
    let prompt = recorded.lock().unwrap().last().cloned().unwrap_or_default();
    assert!(
        prompt.contains("reunion-a.md") && prompt.contains("reunion-b.md"),
        "the prior MaterialSet must ground the creation: {prompt}"
    );
}

// -- Test 6: multiple materials are bounded -----------------------------------

#[test]
fn multiple_materials_use_bounded_compact_representations() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let (app, engine) = recording_app(tmp.path(), recorded.clone());
    let p = app.create_project("P").unwrap();
    let body_a = long_body("archivo-a.md", "sentinel A");
    let body_b = long_body("archivo-b.md", "sentinel B");
    let mid_a = add_file(&app, tmp.path(), &p.id, "archivo-a.md", body_a.as_bytes());
    let mid_b = add_file(&app, tmp.path(), &p.id, "archivo-b.md", body_b.as_bytes());
    seed_web_creation(tmp.path(), &p.id, &engine);

    let run = app
        .send_message(
            &p.id,
            "creame una presentación de estos archivos",
            &[mid_a, mid_b],
        )
        .expect("send");
    assert_eq!(run.status, "completed");
    assert!(!run.registered_creation_ids.is_empty());

    let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_eq!(metrics.turn_kind.as_deref(), Some("creation_from_material"));
    assert_eq!(metrics.retrieval_mode, None);

    let prompt = recorded.lock().unwrap().last().cloned().unwrap_or_default();
    assert!(prompt.contains("archivo-a.md"), "first target: {prompt}");
    assert!(prompt.contains("archivo-b.md"), "second target: {prompt}");
    // No raw concatenation: the full bodies never reach the prompt verbatim.
    assert!(
        !prompt.contains("La parte 29 agrega contenido"),
        "raw files must not be concatenated: {prompt}"
    );
    // The evidence is bounded: it is far smaller than the combined raw files.
    let evidence_start = prompt.find("<knowledge_evidence").unwrap_or(0);
    let evidence_end = prompt.find("</knowledge_evidence>").unwrap_or(prompt.len());
    let evidence_chars = prompt[evidence_start..evidence_end].chars().count();
    assert!(
        evidence_chars < body_a.chars().count() + body_b.chars().count(),
        "multi-target context must stay compact: {evidence_chars} chars"
    );
    assert!(
        !prompt.contains("materials/1-archivo-a.md")
            && !prompt.contains("materials/1-archivo-b.md"),
        "indexed targets must not be raw-forwarded: {prompt}"
    );
}

// -- Test 7: inventory regression ---------------------------------------------

#[test]
fn inventory_question_is_not_creation() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = recording_app(tmp.path(), Arc::new(Mutex::new(Vec::new())));
    let p = app.create_project("P").unwrap();
    add_file(&app, tmp.path(), &p.id, "notas.txt", b"contenido");
    let run = app
        .send_message(&p.id, "¿cuántos archivos tengo?", &[])
        .expect("send");
    assert_eq!(run.status, "completed");
    let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_eq!(metrics.local_mode.as_deref(), Some("inventory"));
    assert_eq!(metrics.remote_calls, Some(0));
    assert_ne!(metrics.turn_kind.as_deref(), Some("creation_from_material"));
    assert!(run.message.unwrap_or_default().contains("materiales"));
}

// -- Test 8: exhaustive regression --------------------------------------------

#[test]
fn exhaustive_question_is_not_creation() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().unwrap();
    let (app, engine) = recording_app(tmp.path(), Arc::new(Mutex::new(Vec::new())));
    let p = app.create_project("P").unwrap();
    add_file(
        &app,
        tmp.path(),
        &p.id,
        "reunion.md",
        b"Kubernetes orquesta contenedores en la infraestructura.",
    );
    engine.set_message("Sí, se menciona Kubernetes en reunion.md.".into());
    engine.set_artifacts(vec![]);
    let run = app
        .send_message(&p.id, "¿qué archivos mencionan Kubernetes?", &[])
        .expect("send");
    assert_eq!(run.status, "completed");
    let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_ne!(metrics.turn_kind.as_deref(), Some("creation_from_material"));
}

// -- Test 9: alternative artifacts --------------------------------------------

#[test]
fn alternative_artifact_nouns_route_to_creation() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    for prompt in [
        "haceme una actividad interactiva usando este archivo",
        "generame una página web basada en este README",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let (app, engine) = recording_app(tmp.path(), Arc::new(Mutex::new(Vec::new())));
        let p = app.create_project("P").unwrap();
        let mid = add_file(
            &app,
            tmp.path(),
            &p.id,
            "README.md",
            b"# README\n\nContenido del material.",
        );
        seed_web_creation(tmp.path(), &p.id, &engine);
        let run = app.send_message(&p.id, prompt, &[mid]).expect("send");
        assert_eq!(run.status, "completed", "{prompt}");
        let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
        assert_eq!(
            metrics.turn_kind.as_deref(),
            Some("creation_from_material"),
            "{prompt}"
        );
        assert_eq!(metrics.retrieval_mode, None, "{prompt}");
        assert!(!run.registered_creation_ids.is_empty(), "{prompt}");
    }
}

// -- Test 10: creation + summary wording --------------------------------------

#[test]
fn creation_with_summary_wording_wins_over_summary_gate() {
    project_app::session_log::configure_from_args(["--debug".to_owned()]);
    let tmp = tempfile::tempdir().unwrap();
    let (app, engine) = recording_app(tmp.path(), Arc::new(Mutex::new(Vec::new())));
    let p = app.create_project("P").unwrap();
    let mid = add_file(
        &app,
        tmp.path(),
        &p.id,
        "README.md",
        "# README\n\nContenido del material para la presentación.".as_bytes(),
    );
    seed_web_creation(tmp.path(), &p.id, &engine);
    let run = app
        .send_message(
            &p.id,
            "haceme una presentación resumiendo este README",
            &[mid],
        )
        .expect("send");
    assert_eq!(run.status, "completed");
    let metrics = app.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_eq!(metrics.turn_kind.as_deref(), Some("creation_from_material"));
    assert_eq!(metrics.retrieval_mode, None);
    assert!(!run.registered_creation_ids.is_empty());
}
