//! Exhaustive / corpus-wide Knowledge retrieval contract (A–L).

use std::fs;
use std::sync::{Arc, Mutex};

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

fn filler_corpus() -> String {
    let mut body = String::new();
    for i in 0..80 {
        body.push_str(&format!(
            "Parrafo {i} de relleno sobre gramatica, clases y la agenda semanal de la docente.\n"
        ));
    }
    body.push_str("Qué se acordó respecto del aumento del precio de las clases y se planteó una alternativa de becas.\n");
    body.push_str("Delfina explicó pasado simple y pasado continuo con ejemplos.\n");
    body
}

#[test]
fn a_normal_semantic_retrieval_stays_compact_and_non_exhaustive() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Se acordó un aumento y se planteó una alternativa de becas.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("A").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "2026-07-30 Notas de Gemini.md",
        &filler_corpus(),
    );
    let run = state
        .send_message(
            &project.id,
            "¿Qué se acordó respecto del aumento del precio de las clases?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("normal"));
    assert_eq!(
        metrics.exhaustive_coverage.as_deref(),
        Some("not_requested")
    );
    assert!(metrics.selected_evidence_count.unwrap() > 0);
    assert!(metrics.evidence_est_tokens.unwrap() < metrics.corpus_est_tokens.unwrap());
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("<knowledge_evidence"));
    assert!(prompt.contains("2026-07-30-Notas-de-Gemini.md"));
    assert!(!prompt.contains(tmp.path().to_str().unwrap()));
    assert!(!prompt.contains("materials/"));
    let assistant = run.message.unwrap();
    assert!(assistant.contains("Fuentes:\n- 2026-07-30 … 2026-07-30-Notas-de-Gemini.md\n"));
}

#[test]
fn b_multi_source_semantic_keeps_identifiable_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Se repiten dificultades de pronunciación y tiempos verbales.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("B").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "2026-07-30 Notas de Gemini.md",
        &(format!(
            "Qué dificultades de inglés aparecen repetidamente en varias reuniones: pasado simple.\n\n{}",
            (0..60)
                .map(|i| format!("Parrafo extra {i} de gramatica y vocabulario de la semana.\n\n"))
                .collect::<String>()
        )),
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "2026-07-31 Notas de Gemini.md",
        &(format!(
            "Qué dificultades de inglés aparecen repetidamente en varias reuniones: pronunciación.\n\n{}",
            (0..60)
                .map(|i| format!("Parrafo extra {i} de listening y escritura.\n\n"))
                .collect::<String>()
        )),
    );
    let run = state
        .send_message(
            &project.id,
            "¿Qué dificultades de inglés aparecen repetidamente en varias reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("normal"));
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("2026-07-30-Notas-de-Gemini.md"));
    assert!(prompt.contains("2026-07-31-Notas-de-Gemini.md"));
    assert!(metrics.evidence_est_tokens.unwrap() < metrics.corpus_est_tokens.unwrap());
    let assistant = run.message.unwrap();
    assert!(assistant.contains("Fuentes:\n- 2026-07-30 … 2026-07-30-Notas-de-Gemini.md\n- 2026-07-31 … 2026-07-31-Notas-de-Gemini.md\n"));
}

#[test]
fn c_exact_exhaustive_positive_is_compact() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, se habló de Google Workspace.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("C").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "2026-07-30 Notas de Gemini.md",
        "Se habló de Google Workspace para el correo institucional.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "otros.md",
        "Solo se habló de precios de las clases.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿Se habló de Google Workspace en alguna reunión?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    assert_eq!(metrics.lexical_hits, Some(1));
    assert_eq!(metrics.materials_inspected, metrics.eligible_materials);
    let prompt = calls.lock().unwrap()[0].clone();
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert!(prompt.contains("Google Workspace"));
    assert!(prompt.contains("exhaustive_coverage=complete"));
    assert!(!prompt.contains("Solo se habló de precios de las clases.\nSolo se habló"));
    assert!(!prompt.contains("materials/"));
    let assistant = run.message.unwrap();
    assert!(assistant.contains("Fuentes:\n- 2026-07-30 … 2026-07-30-Notas-de-Gemini.md\n"));
    assert!(!assistant.contains("otros.md"));
}

#[test]
fn alternative_exhaustive_terms_do_not_require_a_contiguous_phrase() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, se habló de Kubernetes.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("Alt").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "k.md",
        "En esta reunion solo se hablo de Kubernetes.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿Se habló en alguna reunión de Kubernetes u OpenShift?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.lexical_hits, Some(1));
    assert!(metrics.lexical_hits.unwrap() > 0);
    let text = run.message.unwrap();
    assert!(!text.contains("No encontré menciones"));
    assert!(text.contains("Fuentes:\n- k.md\n"));
}

#[test]
fn d_exhaustive_negative_is_local_complete_and_does_not_forward_corpus() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("esto no debe enviarse".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("D").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "a.md",
        "Reunion de gramatica y precios.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "b.md",
        "Otra reunion sobre Google Workspace y clases.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿Se habló en alguna reunión de Kubernetes u OpenShift?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    assert!(
        calls.lock().unwrap().is_empty(),
        "absence must not be proven by a remote corpus dump: {:?}",
        calls.lock().unwrap()
    );
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    assert_eq!(metrics.materials_inspected, metrics.eligible_materials);
    assert_eq!(metrics.lexical_hits, Some(0));
    assert_eq!(metrics.remote_calls, Some(0));
    let text = run.message.unwrap();
    assert!(text.contains("No encontré menciones"));
    assert!(text.contains("2 materiales"));
    assert!(!text.contains("esto no debe enviarse"));
    assert!(!text.contains("Fuentes:"));
    assert!(!text.contains("a.md"));
    assert!(!text.contains("b.md"));
}

#[test]
fn e_exhaustive_incomplete_must_not_claim_global_absence() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("no debe usarse".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("E").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "notas.md",
        "Solo gramatica y precios.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "manual.pdf",
        "%PDF-1.4 fake Kubernetes",
    );
    let run = state
        .send_message(
            &project.id,
            "¿Se habló en alguna reunión de Kubernetes u OpenShift?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    assert!(calls.lock().unwrap().is_empty());
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("incomplete"));
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    let text = run.message.unwrap();
    assert!(text.contains("no pude verificar exhaustivamente todo el corpus"));
    assert!(!text.to_lowercase().contains("no se mencionó"));
    assert!(!text.contains("No encontré menciones de kubernetes"));
    assert!(!text.contains("Fuentes:"));
    assert!(!text.contains("notas.md"));
    assert!(!text.contains("manual.pdf"));
}

#[test]
fn f_historical_project_isolation() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("local".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let current = state.create_project("Current").unwrap();
    let historical = state.create_project("Historical").unwrap();
    add_file(
        &state,
        tmp.path(),
        &historical.id,
        "viejo.md",
        "Kubernetes aparece solo en el historial ajeno.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &current.id,
        "actual.md",
        "Reunion de gramatica sin contenedores.\n",
    );
    let run = state
        .send_message(
            &current.id,
            "¿Se habló en alguna reunión de Kubernetes u OpenShift?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&current.id).unwrap().unwrap();
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    assert_eq!(metrics.lexical_hits, Some(0));
    let text = run.message.unwrap();
    assert!(text.contains("No encontré menciones"));
    assert!(!text.contains("viejo.md"));
    assert!(!text.contains("Fuentes:"));
}

#[test]
fn g_source_traceability_has_filename_without_absolute_path() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Respuesta.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("G").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "2026-07-31 Notas de Gemini.md",
        "Se habló de Google Workspace en detalle.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿Se habló de Google Workspace en alguna reunión?",
            &[],
        )
        .unwrap();
    let assistant = run.message.unwrap();
    assert!(assistant.contains("Fuentes:\n- 2026-07-31 … 2026-07-31-Notas-de-Gemini.md\n"));
    assert!(!assistant.contains("/home/"));
    assert!(!assistant.contains(tmp.path().to_str().unwrap()));
}

#[test]
fn h_privacy_metrics_and_logs_omit_bodies_paths_and_secrets() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("ok".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("H").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "notas.md",
        "PROMPT_BODY_SENTINEL respuesta RESPONSE_BODY_SENTINEL DOCUMENT_TEXT_SENTINEL VECTOR_BLOB_SENTINEL sk-secret-api /home/damian/secret-path\nSe habló de Google Workspace.\n",
    );
    let _ = state
        .send_message(
            &project.id,
            "¿Se habló de Google Workspace en alguna reunión?",
            &[],
        )
        .unwrap();
    let disk = fs::read_to_string(
        tmp.path()
            .join("projects")
            .join(&project.id)
            .join("project.json"),
    )
    .unwrap();
    for forbidden in [
        "PROMPT_BODY_SENTINEL",
        "RESPONSE_BODY_SENTINEL",
        "DOCUMENT_TEXT_SENTINEL",
        "VECTOR_BLOB_SENTINEL",
        "/home/damian/secret-path",
        "sk-secret-api",
    ] {
        assert!(
            !disk.contains(forbidden),
            "{forbidden} leaked in project.json"
        );
    }
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    let serialized = serde_json::to_string(&metrics).unwrap();
    for forbidden in [
        "PROMPT_BODY_SENTINEL",
        "RESPONSE_BODY_SENTINEL",
        "DOCUMENT_TEXT_SENTINEL",
        "VECTOR_BLOB_SENTINEL",
        "/home/",
        "sk-secret-api",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "{forbidden} leaked in metrics: {serialized}"
        );
    }
}

#[test]
fn i_normal_semantic_context_is_materially_smaller_than_corpus() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("aumento".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("I").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "corpus.md",
        &filler_corpus(),
    );
    let _ = state
        .send_message(
            &project.id,
            "¿Qué se acordó respecto del aumento del precio de las clases?",
            &[],
        )
        .unwrap();
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("normal"));
    assert!(metrics.corpus_est_tokens.unwrap() > 400);
    assert!(metrics.evidence_est_tokens.unwrap() < metrics.corpus_est_tokens.unwrap() / 2);
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(!prompt.contains("Parrafo 40 de relleno"));
}

#[test]
fn j_exhaustive_negative_does_not_issue_per_document_remote_calls() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("remote".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("J").unwrap();
    for i in 0..4 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("n{i}.md"),
            &format!("Documento {i} de gramatica y precios.\n"),
        );
    }
    let _ = state
        .send_message(
            &project.id,
            "¿Se habló en alguna reunión de Kubernetes u OpenShift?",
            &[],
        )
        .unwrap();
    assert!(calls.lock().unwrap().is_empty());
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.remote_calls, Some(0));
    assert!(metrics.chunks_inspected.unwrap() >= 4);
    assert_eq!(metrics.evidence_est_tokens, Some(0));
}

#[test]
fn k_and_l_restart_durability_and_zero_work_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("ok".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("KL").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "a.md",
        "Se habló de Google Workspace.\n",
    );
    let _ = state
        .send_message(
            &project.id,
            "¿Se habló de Google Workspace en alguna reunión?",
            &[],
        )
        .unwrap();
    let before = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(before.retrieval_mode.as_deref(), Some("exhaustive"));
    let remote_before = calls.lock().unwrap().len();
    drop(state);
    let fresh_calls = Arc::new(Mutex::new(Vec::new()));
    let fresh_inner = FakeAgentEngine::new();
    let fresh = recording_app(
        tmp.path(),
        RecordingEngine(fresh_inner, fresh_calls.clone()),
    );
    let after = fresh.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(after.retrieval_mode, before.retrieval_mode);
    assert_eq!(after.exhaustive_coverage, before.exhaustive_coverage);
    assert_eq!(after.lexical_hits, before.lexical_hits);
    assert_eq!(after.material_count, before.material_count);
    assert!(fresh_calls.lock().unwrap().is_empty());
    let _ = fresh.open_project(&project.id).unwrap();
    assert_eq!(fresh_calls.lock().unwrap().len(), 0);
    assert_eq!(calls.lock().unwrap().len(), remote_before);
}

fn fuentes_names(text: &str) -> Vec<String> {
    let Some(block) = text.split("Fuentes:\n").nth(1) else {
        return Vec::new();
    };
    block
        .lines()
        .filter_map(|line| line.strip_prefix("- ").map(str::to_owned))
        .take_while(|line| !line.is_empty())
        .collect()
}

#[test]
fn five_document_corpus_does_not_emit_five_fuentes() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, se habló de Kubernetes.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("Five").unwrap();
    for i in 0..4 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("filler-{i}.md"),
            &format!("Documento {i} de gramatica, precios y la agenda semanal.\n"),
        );
    }
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "hit.md",
        "En esta reunion se hablo de Kubernetes y el cluster de produccion.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿Se habló en alguna reunión de Kubernetes u OpenShift?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_eq!(metrics.materials_inspected, Some(5));
    assert_eq!(metrics.eligible_materials, Some(5));
    assert_eq!(metrics.lexical_hits, Some(1));
    let assistant = run.message.unwrap();
    assert_eq!(fuentes_names(&assistant), vec!["hit.md".to_owned()]);
    assert!(!assistant.contains("filler-"));
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("hit.md"));
    assert!(!prompt.contains("filler-0.md"));
}

#[test]
fn budget_dropped_exhaustive_sources_are_not_fuentes() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, se habló de Kubernetes en varios documentos.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("Budget").unwrap();
    for i in 0..9 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("hit-{i:02}.md"),
            &format!(
                "En esta reunion {i} se hablo de Kubernetes como plataforma de contenedores.\n"
            ),
        );
    }
    let run = state
        .send_message(
            &project.id,
            "¿Se habló en alguna reunión de Kubernetes u OpenShift?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.lexical_hits, Some(9));
    assert_eq!(metrics.materials_inspected, Some(9));
    assert!(metrics.selected_evidence_count.unwrap() <= 8);
    assert!(metrics.selected_evidence_count.unwrap() < metrics.lexical_hits.unwrap());
    let assistant = run.message.unwrap();
    let names = fuentes_names(&assistant);
    assert_eq!(names.len(), metrics.selected_evidence_count.unwrap());
    assert!(!names.iter().any(|name| name == "hit-08.md"));
    assert!(!assistant.contains("hit-08.md"));
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(!prompt.contains("hit-08.md"));
    assert!(prompt.contains("hit-00.md"));
}

#[test]
fn duplicate_chunks_from_one_source_emit_one_fuente() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí.".into());
    let state = recording_app(
        tmp.path(),
        RecordingEngine(inner, Arc::new(Mutex::new(Vec::new()))),
    );
    let project = state.create_project("Dup").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "notas.md",
        "Se hablo de Kubernetes en la manana.\n\nMas tarde se volvio a mencionar Kubernetes.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿Se habló en alguna reunión de Kubernetes u OpenShift?",
            &[],
        )
        .unwrap();
    assert_eq!(
        fuentes_names(&run.message.unwrap()),
        vec!["notas.md".to_owned()]
    );
}
