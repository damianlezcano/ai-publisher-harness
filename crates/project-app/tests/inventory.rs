//! Knowledge-inventory local command contracts.
//!
//! KnowledgeInventory is a LOCAL METADATA COMMAND, not a RAG retrieval mode.
//! Every pure inventory turn must:
//! - produce the local answer without any provider call (`remote_calls == 0`);
//! - never invoke the agent/OpenCode engine;
//! - generate no query embeddings and select no semantic evidence;
//! - never append a semantic `Fuentes:` provenance block;
//! - return ALL matching READY materials with no top-K truncation;
//! - keep current-turn attachment/import accounting separate from persisted
//!   Knowledge state.

use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use project_agent::FakeAgentEngine;
use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};
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
    engine: RecordingEngine,
) -> AppState<RecordingEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
    AppState::with_components(
        base.to_path_buf(),
        engine,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    )
}

fn add_file<E: project_agent::AgentEngine>(
    app: &AppState<E, FakeTunnel, FakeProviderConnector, FakeRestarter>,
    base: &std::path::Path,
    project_id: &str,
    name: &str,
    body: &str,
) -> String {
    let src = base.join(name);
    fs::write(&src, body).unwrap();
    let material = app
        .add_material_from_path(project_id, src.to_str().unwrap())
        .unwrap();
    let _ = fs::remove_file(&src);
    material.id
}

fn position(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("`{needle}` must appear in the inventory answer: {text}"))
}

#[test]
fn inventory_list_is_local_zero_remote_and_contains_all_materials() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let state = recording_app(
        tmp.path(),
        RecordingEngine(FakeAgentEngine::new(), calls.clone()),
    );
    let project = state.create_project("Inv").unwrap();
    for name in ["alfa.md", "bravo.md", "charlie.md", "delta.md", "echo.md"] {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            name,
            &format!("Contenido del material {name}.\n"),
        );
    }
    let run = state
        .send_message(&project.id, "listame todos los archivos", &[])
        .unwrap();
    assert_eq!(run.status, "completed");
    let message = run.message.expect("local inventory answer");
    assert!(message.contains("Tenés 5 materiales registrados y listos en Knowledge"));
    for name in ["alfa.md", "bravo.md", "charlie.md", "delta.md", "echo.md"] {
        assert!(
            message.contains(name),
            "every READY material must appear: {message}"
        );
    }
    assert!(
        !message.contains("Fuentes:"),
        "a pure inventory list is its own answer and must not carry a semantic Fuentes block: {message}"
    );
    assert!(
        calls.lock().unwrap().is_empty(),
        "the OpenCode agent engine must never be invoked for inventory"
    );
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.remote_calls, Some(0));
    assert_eq!(metrics.retrieval_mode, None);
    assert_eq!(metrics.local_mode.as_deref(), Some("inventory"));
    assert_eq!(metrics.retrieval_candidate_count, Some(0));
    assert_eq!(metrics.selected_evidence_count, Some(0));
    assert_eq!(
        metrics.semantic_provider_state.as_deref(),
        Some("not_requested")
    );
}

#[test]
fn inventory_count_is_exact_and_zero_remote() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let state = recording_app(
        tmp.path(),
        RecordingEngine(FakeAgentEngine::new(), calls.clone()),
    );
    let project = state.create_project("Count").unwrap();
    for index in 0..12 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("material-{index:02}.md"),
            &format!("Contenido del material {index}.\n"),
        );
    }
    let run = state
        .send_message(&project.id, "¿Cuántos archivos tengo?", &[])
        .unwrap();
    assert_eq!(run.status, "completed");
    let message = run.message.expect("local count answer");
    assert!(
        message.contains("Tenés 12 materiales registrados y listos en Knowledge."),
        "{message}"
    );
    assert!(!message.contains("Fuentes:"));
    assert!(calls.lock().unwrap().is_empty());
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.remote_calls, Some(0));
    assert_eq!(metrics.retrieval_mode, None);
    assert_eq!(metrics.local_mode.as_deref(), Some("inventory"));
}

#[test]
fn inventory_membership_is_exact_local_yes_no_without_provider() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let state = recording_app(
        tmp.path(),
        RecordingEngine(FakeAgentEngine::new(), calls.clone()),
    );
    let project = state.create_project("Member").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "reunion-2026-09-01.md",
        "Contenido de la reunión.\n",
    );
    let yes = state
        .send_message(&project.id, "¿Tengo el archivo reunion-2026-09-01.md?", &[])
        .unwrap();
    assert_eq!(yes.status, "completed");
    let yes_message = yes.message.expect("membership answer");
    assert!(
        yes_message
            .contains("Sí, tenés el material \"reunion-2026-09-01.md\" cargado en Knowledge."),
        "{yes_message}"
    );

    let no = state
        .send_message(
            &project.id,
            "¿Tengo el archivo inexistente-2026-01-01.pdf?",
            &[],
        )
        .unwrap();
    assert_eq!(no.status, "completed");
    let no_message = no.message.expect("membership answer");
    assert!(
        no_message.contains("No, no tenés ningún material"),
        "{no_message}"
    );
    assert!(!yes_message.contains("Fuentes:") && !no_message.contains("Fuentes:"));

    assert!(calls.lock().unwrap().is_empty());
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.remote_calls, Some(0));
    assert_eq!(metrics.local_mode.as_deref(), Some("inventory"));
}

#[test]
fn inventory_list_chronological_uses_index_timestamp_not_filename_or_insertion() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let state = recording_app(
        tmp.path(),
        RecordingEngine(FakeAgentEngine::new(), calls.clone()),
    );
    let project = state.create_project("Chrono").unwrap();
    // Non-chronological relative to names: c.md is indexed first (oldest),
    // then a.md, then b.md (newest). Filename dates are never parsed.
    for (name, body) in [
        ("c.md", "material c\n"),
        ("a.md", "material a\n"),
        ("b.md", "material b\n"),
    ] {
        add_file(&state, tmp.path(), &project.id, name, body);
        std::thread::sleep(Duration::from_millis(1100));
    }
    let run = state
        .send_message(&project.id, "listar los archivos en orden cronologico", &[])
        .unwrap();
    assert_eq!(run.status, "completed");
    let message = run.message.expect("chronological list answer");
    // Oldest (indexed first) must be listed first: c.md, then a.md, then b.md.
    assert!(
        position(&message, "c.md") < position(&message, "a.md")
            && position(&message, "a.md") < position(&message, "b.md"),
        "chronological order must follow the import/index timestamp: {message}"
    );
    assert!(calls.lock().unwrap().is_empty());
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.remote_calls, Some(0));
    assert_eq!(metrics.local_mode.as_deref(), Some("inventory"));
}

#[test]
fn large_inventory_list_is_not_truncated() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let state = recording_app(
        tmp.path(),
        RecordingEngine(FakeAgentEngine::new(), calls.clone()),
    );
    let project = state.create_project("Large").unwrap();
    for index in 0..105 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("material-{index:03}.md"),
            &format!("Contenido del material {index}.\n"),
        );
    }
    let run = state
        .send_message(&project.id, "listame todos los archivos", &[])
        .unwrap();
    assert_eq!(run.status, "completed");
    let message = run.message.expect("large inventory answer");
    assert!(message.contains("Tenés 105 materiales registrados y listos en Knowledge"));
    for index in 0..105 {
        assert!(
            message.contains(&format!("material-{index:03}.md")),
            "no top-K truncation allowed; material-{index:03}.md missing"
        );
    }
    assert!(!message.contains("Fuentes:"));
    assert!(calls.lock().unwrap().is_empty());
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.remote_calls, Some(0));
    assert_eq!(metrics.retrieval_candidate_count, Some(0));
    assert_eq!(metrics.selected_evidence_count, Some(0));
}

#[test]
fn inventory_survives_app_restart_with_identical_results() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().to_path_buf();
    let project = {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let state = recording_app(
            &base,
            RecordingEngine(FakeAgentEngine::new(), calls.clone()),
        );
        let project = state.create_project("Restart").unwrap();
        add_file(&state, &base, &project.id, "reunion-a.md", "contenido a\n");
        add_file(&state, &base, &project.id, "reunion-b.md", "contenido b\n");
        add_file(&state, &base, &project.id, "reunion-c.md", "contenido c\n");
        project
    };

    // Simulated restart: a brand-new AppState over the same durable base dir.
    let second_calls = Arc::new(Mutex::new(Vec::new()));
    let second = recording_app(
        &base,
        RecordingEngine(FakeAgentEngine::new(), second_calls.clone()),
    );
    let list = second
        .send_message(&project.id, "listame los archivos", &[])
        .unwrap();
    assert_eq!(list.status, "completed");
    let list_message = list.message.expect("restart list answer");
    assert!(list_message.contains("Tenés 3 materiales registrados y listos en Knowledge"));
    for name in ["reunion-a.md", "reunion-b.md", "reunion-c.md"] {
        assert!(list_message.contains(name), "{list_message}");
    }

    let count = second
        .send_message(&project.id, "¿Cuántos documentos tengo?", &[])
        .unwrap();
    assert_eq!(count.status, "completed");
    assert!(
        count
            .message
            .unwrap()
            .contains("Tenés 3 materiales registrados y listos en Knowledge.")
    );

    let member = second
        .send_message(&project.id, "¿Está cargado reunion-b.md?", &[])
        .unwrap();
    assert_eq!(member.status, "completed");
    assert!(
        member
            .message
            .unwrap()
            .contains("Sí, tenés el material \"reunion-b.md\"")
    );

    assert!(second_calls.lock().unwrap().is_empty());
    let metrics = second.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.remote_calls, Some(0));
    assert_eq!(metrics.local_mode.as_deref(), Some("inventory"));
}

#[test]
fn current_turn_attachment_accounting_stays_distinct_from_persisted_inventory() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let state = recording_app(
        tmp.path(),
        RecordingEngine(FakeAgentEngine::new(), calls.clone()),
    );
    let project = state.create_project("Distinct").unwrap();
    // Persisted Knowledge already has two READY materials.
    add_file(&state, tmp.path(), &project.id, "persistido-1.md", "uno\n");
    add_file(&state, tmp.path(), &project.id, "persistido-2.md", "dos\n");
    // A later turn attaches no new files: that turn's import accounting is 0
    // processed, but the persisted Knowledge inventory still reports both.
    let run = state
        .send_message(&project.id, "listame todos los archivos", &[])
        .unwrap();
    assert_eq!(run.status, "completed");
    let message = run.message.expect("inventory must read persisted store");
    assert!(message.contains("Tenés 2 materiales registrados y listos en Knowledge"));
    assert!(message.contains("persistido-1.md") && message.contains("persistido-2.md"));
    assert!(calls.lock().unwrap().is_empty());
}
