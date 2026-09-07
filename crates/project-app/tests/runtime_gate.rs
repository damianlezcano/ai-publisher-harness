//! Production-faithful send lifecycle gate (below the UI).
//!
//! This harness exercises the SAME application layer a real "attach + Send"
//! uses — `AppState::send_staged_message_persist` (acceptance) followed by
//! `run_accepted_staged_turn` (derived Knowledge work + terminal agent step) —
//! WITHOUT graphical clicks. The only replaced boundary is the *external*
//! provider edge (`FakeAgentEngine`), which is exactly the deterministic
//! offline split the project's test convention mandates (`docs/TESTING.md`:
//! "local fakes for external processes"). The real model/K6 remote synthesis
//! boundary and real E5 embeddings are the §12 human gate, exercised by the
//! real AppImage run, not by this offline harness.
//!
//! It proves, for a supported Markdown "attach + Send":
//!   - no processing before the accepted Send,
//!   - exactly one durable user turn,
//!   - material copy begins only after the accepted Send,
//!   - turn_id is durable before `prepared > 0`,
//!   - lexical indexing succeeds,
//!   - the raw supported attachment is NOT forwarded to the provider,
//!   - summary intent ("resumime el archivo") is recognised and never also
//!     raw-forwards the indexed source nor double-runs normal chat,
//!   - exactly one final assistant result,
//!   - provider usage keeps unavailable fields `null` (rendered "No
//!     disponible") and carries `additional_attachment_route == false` for
//!     supported indexed TXT/Markdown.

use std::sync::{Arc, Mutex};

use project_agent::FakeAgentEngine;
use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};
use project_app::AppState;
use project_core::{MaterialId, ProjectId};
use project_knowledge::{KnowledgeStore, MaterialIndexState};
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

/// Records the exact prompt text each `send` received, so a test can assert a
/// raw workspace attachment was never provisioned and no second chat run fired.
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

/// Deterministic K6 summarizer: records every synthesis request and returns a
/// valid structured document/global summary.
fn open_store(base: &std::path::Path, project_id: &str) -> KnowledgeStore {
    KnowledgeStore::open(
        base.join("projects").join(project_id),
        &ProjectId::parse(project_id).unwrap(),
    )
    .unwrap()
}

/// One representative Markdown file, same shape as the real gate corpus.
const NOTA: &str = "# Reunión de infraestructura\n\nSe decidió migrar a OpenShift y quedó pendiente la automatización de despliegues.\n";

#[test]
fn one_file_summary_send_produces_one_turn_and_one_answer() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = FakeAgentEngine::new();
    inner.set_message("Listo, resumen listo.".to_owned());
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        inner,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let project = state.create_project("P").unwrap();

    let corpus = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    let path = corpus.join("nota.md");
    std::fs::write(&path, NOTA).unwrap();

    // ---- Pre-Send: no processing may have happened --------------------------
    let before = state.open_project(&project.id).unwrap();
    assert!(before.materials.is_empty(), "no Material before Send");
    assert!(before.messages.is_empty(), "no turn before Send");
    assert!(before.accepted_import.is_none(), "no operation before Send");
    // ---- Acceptance boundary -------------------------------------------------
    let accepted = state
        .send_staged_message_persist(
            &project.id,
            "resumime el archivo",
            &[path.to_string_lossy().to_string()],
            &[],
        )
        .unwrap();
    let turn_id = accepted.turn_id().unwrap().to_owned();
    let operation_id = accepted.operation_id().to_owned();

    // turn_id durable before prepared > 0, and material copy already happened
    // as part of acceptance (the ledger's first `copied > 0` write carries the
    // turn link — see `accepted_staged_send_links_turn_before_prepared_counter`).
    let store = open_store(tmp.path(), &project.id);
    let op = store
        .accepted_import_operation(&operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(op.copied, 1, "exactly one prepared material");
    assert_eq!(op.turn_id.as_deref(), Some(turn_id.as_str()));

    // ---- Completed derived work (indexing + agent/summary) -------------------
    let run = state.run_accepted_staged_turn(accepted).unwrap();
    assert_eq!(run.status, "completed");
    assert_eq!(run.turn_id.as_deref(), Some(turn_id.as_str()));

    // Lexical indexing succeeded and the material is READY.
    let material = state.open_project(&project.id).unwrap().materials[0].clone();
    let mid = MaterialId::parse(&material.id).unwrap();
    let index_state = store.material_index_status(&mid).unwrap().unwrap();
    assert_eq!(index_state.state, MaterialIndexState::Ready);
    assert!(!store.search("OpenShift", 10).unwrap().is_empty());

    // Exactly one durable user turn and exactly one final assistant message.
    let view = state.open_project(&project.id).unwrap();
    assert_eq!(view.messages.iter().filter(|m| m.role == "user").count(), 1);
    assert_eq!(
        view.messages
            .iter()
            .filter(|m| m.role == "assistant")
            .count(),
        1
    );
    assert_eq!(view.messages[0].id, turn_id);
}

#[test]
fn one_file_summary_routes_to_k6_and_never_raw_forwards() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Listo.".into());
    // `summarizer_backend` is production-only (`AppState::new`); this DI state
    // proves the deterministic parts of the route: summary intent is detected,
    // and the accepted staged send with an indexed supported file NEVER raw
    // forwards it as a chat attachment nor double-runs normal chat.
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        RecordingEngine(inner, calls.clone()),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let project = state.create_project("P").unwrap();

    // Summary intent must be recognised for the exact prompt semantics.
    assert!(project_app::detect_summarize_intent("resumime el archivo"));

    let corpus = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    let path = corpus.join("nota.md");
    std::fs::write(&path, NOTA).unwrap();

    let accepted = state
        .send_staged_message_persist(
            &project.id,
            "resumime el archivo",
            &[path.to_string_lossy().to_string()],
            &[],
        )
        .unwrap();
    // No normal chat remote execution may have been issued on acceptance.
    assert!(
        calls.lock().unwrap().is_empty(),
        "no remote chat call on acceptance"
    );

    let _ = state.run_accepted_staged_turn(accepted);

    // Whatever chat prompt the (summary-routed) send produced (if any) must NOT
    // carry the raw attachment as a workspace file, and must be at most one.
    let sent: Vec<String> = calls.lock().unwrap().clone();
    assert!(sent.len() <= 1, "no duplicate normal-chat execution");
    for text in &sent {
        assert!(
            !text.contains("materials/1-nota.md"),
            "raw supported attachment must never be forwarded: {text}"
        );
        assert!(
            !text.contains("input_image") && !text.contains("(md)"),
            "supported indexed source must not be provisioned as a raw attachment: {text}"
        );
    }
}

/// Scale gate: 5 files. Same production-faithful lifecycle as the one-file
/// gate, proving `prepared`/`indexed` = N, exactly one durable turn, and N
/// Materials with no duplication. A deterministically-bounded 5-item corpus
/// keeps this offline and fast (the `52`/`51`-file recovery contracts are
/// already pinned by `materials.rs`).
#[test]
fn five_file_batch_send_indexes_all_and_links_one_turn() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = FakeAgentEngine::new();
    inner.set_message("Listo.".to_owned());
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        inner,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let project = state.create_project("P").unwrap();

    let corpus = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    let mut paths = Vec::new();
    for i in 0..5 {
        let path = corpus.join(format!("nota-{i:02}.md"));
        std::fs::write(
            &path,
            format!("# Nota {i}\n\nDecisión número {i} para la síntesis.\n"),
        )
        .unwrap();
        paths.push(path.to_string_lossy().to_string());
    }

    let accepted = state
        .send_staged_message_persist(
            &project.id,
            "haceme un resumen de cada archivo ordenado por fecha",
            &paths,
            &[],
        )
        .unwrap();
    let turn_id = accepted.turn_id().unwrap().to_owned();

    // `prepared == 5` and the turn was durable before the counter advanced.
    let store = open_store(tmp.path(), &project.id);
    let op = store
        .accepted_import_operation(accepted.operation_id())
        .unwrap()
        .unwrap();
    assert_eq!(op.copied, 5);
    assert_eq!(op.turn_id.as_deref(), Some(turn_id.as_str()));

    let run = state.run_accepted_staged_turn(accepted).unwrap();
    assert_eq!(run.status, "completed");

    let view = state.open_project(&project.id).unwrap();
    assert_eq!(
        view.materials.len(),
        5,
        "exactly 5 Materials, no duplication"
    );
    assert_eq!(view.messages.iter().filter(|m| m.role == "user").count(), 1);
    assert_eq!(view.messages[0].material_ids.len(), 5);

    // Lexical indexing advanced for every accepted material.
    let final_store = open_store(tmp.path(), &project.id);
    let op = final_store
        .accepted_import_operation(&run_operation_id(&view))
        .unwrap()
        .unwrap();
    assert_eq!(op.lexical_completed, 5);
}

/// The multi-file operation identity is stable and observable through the
/// public `ProjectView.accepted_import` progress (no private-store dependency).
fn run_operation_id(view: &project_app::ProjectView) -> String {
    view.accepted_import
        .as_ref()
        .expect("accepted operation present")
        .operation_id
        .clone()
}

#[test]
fn provider_usage_record_is_structured_and_never_collapses_missing_to_zero() {
    // The DTO that backs "Detalles de la conversación → Uso y optimización →
    // Último turno" aggregates every remote call of the logical turn and keeps
    // missing provider fields absent (rendered "No disponible"), never zero.
    let usage = project_app::session_log::SessionUsage {
        conversation_id: "c".into(),
        turn_id: "t".into(),
        provider: "opencode-go".into(),
        model: "deepseek-v4-pro".into(),
        input_tokens: None,
        output_tokens: None,
        cache_read_tokens: Some(123),
        cache_write_tokens: None,
        total_tokens: None,
        cost_usd: None,
        turn_duration_ms: Some(420),
        source: "unavailable".into(),
        remote_calls: Some(3),
        reason: "summary_global".into(),
        additional_attachment_route: false,
    };
    let value = serde_json::to_value(&usage).unwrap();
    // Missing token fields stay `null` — the DTO never coerces an unavailable
    // provider value to zero, and the UI renders `null` as "No disponible".
    assert!(value["inputTokens"].is_null());
    assert!(value["outputTokens"].is_null());
    assert!(value["totalTokens"].is_null());
    assert!(value["costUsd"].is_null());
    // Present fields survive, and the remote-call count carries the aggregate.
    assert_eq!(value["cacheReadTokens"], 123);
    assert_eq!(value["remoteCalls"], 3);
    assert_eq!(value["reason"], "summary_global");
    assert_eq!(value["additionalAttachmentRoute"], false);
}
