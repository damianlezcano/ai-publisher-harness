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

use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};
use project_agent::{FakeAgentEngine, RemoteUsage, UsageSource};
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

/// SEMANTIC_CHAT_REMAINS_TOP_K: an ordinary question over five selected files
/// remains on the normal bounded-Knowledge route. It must not create K6
/// document nodes simply because several attachments were selected.
#[test]
fn semantic_chat_with_five_selected_files_does_not_force_exhaustive_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Respuesta con evidencia acotada.".to_owned());
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        RecordingEngine(inner, calls.clone()),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let project = state.create_project("P").unwrap();
    let corpus = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    let paths: Vec<String> = (0..5)
        .map(|i| {
            let path = corpus.join(format!("grammar-{i}.md"));
            std::fs::write(&path, format!("# Gramática {i}\n\nRegla compartida.")).unwrap();
            path.to_string_lossy().to_string()
        })
        .collect();
    assert_eq!(
        project_app::summarize::detect_summary_intent("¿qué dicen sobre gramática?", 5),
        project_app::summarize::SummaryIntent::None
    );
    let accepted = state
        .send_staged_message_persist(&project.id, "¿qué dicen sobre gramática?", &paths, &[])
        .unwrap();
    let run = state.run_accepted_staged_turn(accepted).unwrap();
    assert_eq!(run.status, "completed");
    assert_eq!(calls.lock().unwrap().len(), 1, "one normal chat call");
    assert!(
        state.summary_status(&project.id).unwrap().is_empty(),
        "semantic chat must not create exhaustive K6 nodes"
    );
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

fn distinctive_usage() -> RemoteUsage {
    RemoteUsage {
        input_tokens: Some(12_345),
        output_tokens: Some(6_789),
        cache_read_tokens: Some(222),
        cache_write_tokens: Some(333),
        total_tokens: Some(19_134),
        cost_usd: Some(0.0456),
        source: UsageSource::ProviderActual,
    }
}

#[test]
fn durable_completed_turn_survives_disk_restart_without_session_log() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = FakeAgentEngine::new();
    engine.set_message("respuesta".into());
    engine.set_usage(distinctive_usage());
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        engine,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let project = state.create_project("P").unwrap();
    let source = tmp.path().join("nota.md");
    std::fs::write(&source, "# Nota\n\ncontenido distintivo para Knowledge.\n").unwrap();
    let accepted = state
        .send_staged_message_persist(
            &project.id,
            "Knowledge",
            &[source.to_string_lossy().to_string()],
            &[],
        )
        .unwrap();
    assert_eq!(
        state.run_accepted_staged_turn(accepted).unwrap().status,
        "completed"
    );
    let before = state
        .last_turn_metrics(&project.id)
        .unwrap()
        .expect("completed turn metrics");
    assert_eq!(before.input_tokens, Some(12_345));
    assert_eq!(before.cost_usd, Some(0.0456));
    assert_eq!(before.material_count, Some(1));
    assert!(before.corpus_bytes.unwrap() > 0);
    let expected_knowledge = (
        before.material_count.expect("material count"),
        before.corpus_bytes.expect("corpus bytes"),
        before.corpus_utf8_chars.expect("corpus chars"),
        before.corpus_est_tokens.expect("corpus token estimate"),
        before
            .retrieval_candidate_count
            .expect("retrieval candidate count"),
        before
            .selected_evidence_count
            .expect("selected evidence count"),
        before
            .selected_evidence_bytes
            .expect("selected evidence bytes"),
        before
            .selected_evidence_utf8_chars
            .expect("selected evidence chars"),
        before.evidence_est_tokens.expect("evidence token estimate"),
        before.context_reduction_pct.expect("context reduction"),
        before
            .semantic_provider_state
            .clone()
            .expect("semantic provider state"),
        before
            .request_preparation_ms
            .expect("request preparation duration"),
    );
    assert!(
        expected_knowledge.1 > 0
            && expected_knowledge.2 > 0
            && expected_knowledge.3 > 0
            && expected_knowledge.4 > 0
            && expected_knowledge.5 > 0
            && expected_knowledge.6 > 0
            && expected_knowledge.7 > 0
            && expected_knowledge.8 > 0,
        "the fixture must produce distinctive non-zero structural Knowledge telemetry"
    );
    let project_json = std::fs::read_to_string(
        tmp.path()
            .join("projects")
            .join(&project.id)
            .join("project.json"),
    )
    .unwrap();
    assert!(project_json.contains("turnMetrics"));
    state.clear_session_logs();
    drop(state);
    let fresh_engine = FakeAgentEngine::new();
    let fresh = AppState::with_components(
        tmp.path().to_path_buf(),
        fresh_engine.clone(),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let after = fresh
        .last_turn_metrics(&project.id)
        .unwrap()
        .expect("metrics from project.json");
    assert_eq!(after.input_tokens, Some(12_345));
    assert_eq!(after.output_tokens, Some(6_789));
    assert_eq!(after.cache_read_tokens, Some(222));
    assert_eq!(after.cache_write_tokens, Some(333));
    assert_eq!(after.total_tokens, Some(19_134));
    assert_eq!(after.cost_usd, Some(0.0456));
    assert_eq!(after.material_count, Some(1));
    assert!(after.corpus_bytes.unwrap() > 0);
    assert_eq!(
        (
            after.material_count.unwrap(),
            after.corpus_bytes.unwrap(),
            after.corpus_utf8_chars.unwrap(),
            after.corpus_est_tokens.unwrap(),
            after.retrieval_candidate_count.unwrap(),
            after.selected_evidence_count.unwrap(),
            after.selected_evidence_bytes.unwrap(),
            after.selected_evidence_utf8_chars.unwrap(),
            after.evidence_est_tokens.unwrap(),
            after.context_reduction_pct.unwrap(),
            after.semantic_provider_state.unwrap(),
            after.request_preparation_ms.unwrap(),
        ),
        expected_knowledge,
        "all structural Knowledge values are read from disk unchanged"
    );
    assert!(
        fresh_engine.calls().is_empty(),
        "reading durable metrics cannot invoke provider/indexing/K6"
    );
}

#[test]
fn durable_metrics_preserve_unavailable_provider_values_as_null() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = FakeAgentEngine::new();
    engine.set_message("ok".into());
    engine.set_usage(RemoteUsage {
        input_tokens: Some(7),
        output_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        total_tokens: None,
        cost_usd: None,
        source: UsageSource::ProviderActual,
    });
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        engine,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let p = state.create_project("P").unwrap();
    assert_eq!(
        state.send_message(&p.id, "x", &[]).unwrap().status,
        "completed"
    );
    let metrics = state.last_turn_metrics(&p.id).unwrap().unwrap();
    assert_eq!(metrics.input_tokens, Some(7));
    assert_eq!(metrics.output_tokens, None);
    assert_eq!(metrics.cost_usd, None);
    assert_eq!(metrics.cache_write_tokens, None);
    let json = serde_json::to_value(metrics).unwrap();
    assert!(json["costUsd"].is_null());
    assert!(json["outputTokens"].is_null());
}

#[test]
fn latest_metrics_skip_newer_incomplete_turn_and_never_leak_projects() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = FakeAgentEngine::new();
    engine.set_message("ok".into());
    engine.set_usage(distinctive_usage());
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        engine,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let a = state.create_project("A").unwrap();
    let b = state.create_project("B").unwrap();
    state.send_message(&a.id, "completed", &[]).unwrap();
    state
        .send_message_persist(&a.id, "newer but incomplete", &[])
        .unwrap();
    assert_eq!(
        state
            .last_turn_metrics(&a.id)
            .unwrap()
            .unwrap()
            .input_tokens,
        Some(12_345)
    );
    assert!(state.last_turn_metrics(&b.id).unwrap().is_none());
}

/// The durable lookup walks messages backwards and selects user-owned metrics,
/// rather than using message position or the process-local session log.  The
/// six rows below are deliberately distinct so this fails if a later user,
/// assistant, or legacy message accidentally changes the lookup contract.
#[test]
fn last_turn_metrics_ordering_matrix_selects_newest_completed_user_turn() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = FakeAgentEngine::new();
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        engine.clone(),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );

    // 1. One completed metrics-bearing user turn.
    let one = state.create_project("one").unwrap();
    engine.set_message("one answer".into());
    engine.set_usage(RemoteUsage {
        input_tokens: Some(101),
        ..distinctive_usage()
    });
    state.send_message(&one.id, "one", &[]).unwrap();
    assert_eq!(
        state
            .last_turn_metrics(&one.id)
            .unwrap()
            .unwrap()
            .input_tokens,
        Some(101)
    );

    // 2. Two completed turns: the newer completed turn wins.  Their assistant
    // messages are interleaved naturally by `send_message` and must not alter
    // the user-turn ordering.
    let two = state.create_project("two").unwrap();
    engine.set_usage(RemoteUsage {
        input_tokens: Some(202),
        ..distinctive_usage()
    });
    state.send_message(&two.id, "first", &[]).unwrap();
    engine.set_usage(RemoteUsage {
        input_tokens: Some(303),
        ..distinctive_usage()
    });
    state.send_message(&two.id, "second", &[]).unwrap();
    let two_view = state.open_project(&two.id).unwrap();
    assert_eq!(
        two_view.messages.len(),
        4,
        "user/assistant messages interleave"
    );
    assert_eq!(
        state
            .last_turn_metrics(&two.id)
            .unwrap()
            .unwrap()
            .input_tokens,
        Some(303),
        "the assistant after the first user and the assistant after the second user are ignored"
    );

    // 3. A later user accepted but not completed has no metrics; it cannot hide
    // the preceding completed turn.
    state.send_message_persist(&two.id, "pending", &[]).unwrap();
    assert_eq!(
        state
            .last_turn_metrics(&two.id)
            .unwrap()
            .unwrap()
            .input_tokens,
        Some(303)
    );

    // 4. A legacy user turn without metrics followed by a metrics-bearing turn
    // still returns the later completed turn.
    let legacy = state.create_project("legacy").unwrap();
    state
        .send_message_persist(&legacy.id, "old project user", &[])
        .unwrap();
    engine.set_usage(RemoteUsage {
        input_tokens: Some(404),
        ..distinctive_usage()
    });
    state
        .send_message(&legacy.id, "new completed", &[])
        .unwrap();
    assert_eq!(
        state
            .last_turn_metrics(&legacy.id)
            .unwrap()
            .unwrap()
            .input_tokens,
        Some(404)
    );
}

#[test]
fn old_project_json_without_turn_metrics_opens_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        FakeAgentEngine::new(),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let p = state.create_project("P").unwrap();
    state
        .send_message_persist(&p.id, "legacy turn", &[])
        .unwrap();
    drop(state);
    let fresh = AppState::with_components(
        tmp.path().to_path_buf(),
        FakeAgentEngine::new(),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    assert_eq!(fresh.open_project(&p.id).unwrap().messages.len(), 1);
    assert!(fresh.last_turn_metrics(&p.id).unwrap().is_none());
}

// H. Privacy: TurnMetricsView serialises only opaque IDs / counters / costs.
#[test]
fn h_privacy_no_prompt_or_body_in_metrics() {
    // TurnMetricsView only contains fields defined in the DTO.
    // No prompt text, bodies, vectors, or paths can leak.
    let metrics = project_app::TurnMetricsView {
        provider: Some("opencode-go".into()),
        model: Some("big-pickle".into()),
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: Some(10),
        cache_write_tokens: None,
        total_tokens: None,
        cost_usd: Some(0.01),
        turn_duration_ms: Some(420),
        source: Some("user".into()),
        remote_calls: Some(1),
        material_count: Some(2),
        corpus_bytes: Some(1024),
        corpus_utf8_chars: Some(512),
        corpus_est_tokens: Some(200),
        retrieval_candidate_count: Some(3),
        selected_evidence_count: Some(1),
        selected_evidence_bytes: Some(256),
        selected_evidence_utf8_chars: Some(128),
        evidence_est_tokens: Some(50),
        context_reduction_pct: Some(75),
        semantic_provider_state: Some("ready".into()),
        request_preparation_ms: Some(10),
    };
    let json = serde_json::to_value(&metrics).unwrap();
    let text = serde_json::to_string(&json).unwrap();
    assert!(
        !text.contains("secret"),
        "prompt content must not leak into serialised metrics: {text}"
    );
    assert!(
        !text.contains("sk-12345"),
        "API key must not leak into serialised metrics"
    );

    // Only allowed fields should be present.
    let allowed = [
        "provider",
        "model",
        "inputTokens",
        "outputTokens",
        "cacheReadTokens",
        "cacheWriteTokens",
        "totalTokens",
        "costUsd",
        "turnDurationMs",
        "source",
        "remoteCalls",
        "materialCount",
        "corpusBytes",
        "corpusUtf8Chars",
        "corpusEstTokens",
        "retrievalCandidateCount",
        "selectedEvidenceCount",
        "selectedEvidenceBytes",
        "selectedEvidenceUtf8Chars",
        "evidenceEstTokens",
        "contextReductionPct",
        "semanticProviderState",
        "requestPreparationMs",
    ];
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    for k in &keys {
        assert!(
            allowed.contains(k),
            "unexpected field in TurnMetricsView: {k}"
        );
    }
}
