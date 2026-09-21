//! Exhaustive / corpus-wide Knowledge retrieval contract (A–R).

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

struct FixedIntent {
    intent: project_app::Intent,
}

impl project_app::IntentClassifier for FixedIntent {
    fn classify(
        &self,
        _input: &project_app::ClassifierInput,
    ) -> Result<project_app::ClassifierDecision, project_app::IntentClassificationError> {
        Ok(project_app::ClassifierDecision {
            intent: self.intent,
            modifiers: Vec::new(),
            confidence: 0.9,
            reason_code: project_app::ReasonCode::SemanticClassifier,
            provenance: project_app::ClassifierProvenance::SemanticSuccess,
        })
    }
}

fn force_normal_semantic(
    state: &AppState<RecordingEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
) {
    state.set_test_classifier(project_app::SemanticIntentClassifier::new(FixedIntent {
        intent: project_app::Intent::NormalSemantic,
    }));
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

/// Realistic meeting prose used to give each thematic-test source a corpus
/// footprint much larger than the selected evidence. Each document uses
/// disjoint logistics vocabulary so no distractor term recurs across meetings:
/// a word that appears in exactly one document can never become a recurring
/// theme, and the recurring themes below appear only in each document's final
/// "Cierre" section — never in the opening paragraph.
fn thematic_unique_body(index: usize) -> String {
    let openers = [
        "Se revisaron las planillas de asistencia y se cargaron los registros en la carpeta compartida.",
        "Se actualizaron las planillas del proyecto y se ordenaron los avances en las carpetas.",
        "La docente repartió las guías nuevas y se distribuyeron los cuadernillos impresos.",
        "Se organizaron las rotaciones de práctica y se prepararon los materiales de cada sala.",
        "Se compró material de librería y se acomodaron los útiles en los cajones.",
        "Se revisaron las inscripciones y se actualizaron los listados de cada comisión.",
        "Se acomodaron las aulas y se verificaron los proyectores de los salones.",
        "La secretaria compartió el cronograma y se confirmaron las reservas de la biblioteca.",
        "Se calibraron los equipos de audio y se probaron los micrófonos del auditorio.",
        "Se organizó la cartelera y se imprimieron los afiches del evento cultural.",
        "El personal ordenó el depósito y se inventariaron los muebles escolares.",
        "Se actualizó el registro de préstamos y se reordenaron los estantes de la sala.",
        "La coordinadora revisó las encuestas y se tabularon las respuestas anónimas.",
        "Se prepararon las actas y se archivaron los expedientes del ciclo.",
    ][index % 14];
    format!("{}\n{}\n", openers, openers)
}

#[test]
fn a_normal_semantic_retrieval_stays_compact_and_non_exhaustive() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Se acordó un aumento y se planteó una alternativa de becas.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    force_normal_semantic(&state);
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
    assert_eq!(
        metrics.source_names,
        vec!["2026-07-30 … 2026-07-30-Notas-de-Gemini.md".to_owned()]
    );
}

#[test]
fn b_multi_source_semantic_keeps_identifiable_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Se repiten dificultades de pronunciación y tiempos verbales.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    force_normal_semantic(&state);
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
            "¿Qué dificultades de inglés aparecen en varias reuniones?",
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
    assert_eq!(
        metrics.source_names,
        vec![
            "2026-07-30 … 2026-07-30-Notas-de-Gemini.md".to_owned(),
            "2026-07-31 … 2026-07-31-Notas-de-Gemini.md".to_owned(),
        ]
    );
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
    assert_eq!(
        metrics.source_names,
        vec!["2026-07-30 … 2026-07-30-Notas-de-Gemini.md".to_owned()]
    );
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
    assert_eq!(metrics.source_names, vec!["k.md".to_owned()]);
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
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(
        metrics.source_names,
        vec!["2026-07-31 … 2026-07-31-Notas-de-Gemini.md".to_owned()]
    );
    assert!(
        metrics
            .source_names
            .iter()
            .all(|name| !name.contains("/home/"))
    );
    assert!(
        metrics
            .source_names
            .iter()
            .all(|name| !name.contains(tmp.path().to_str().unwrap()))
    );
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
    force_normal_semantic(&state);
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
    assert_eq!(metrics.source_names, vec!["hit.md".to_owned()]);
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
    let names = metrics.source_names.clone();
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
    let _ = state
        .send_message(
            &project.id,
            "¿Se habló en alguna reunión de Kubernetes u OpenShift?",
            &[],
        )
        .unwrap();
    assert_eq!(
        state
            .last_turn_metrics(&project.id)
            .unwrap()
            .unwrap()
            .source_names,
        vec!["notas.md".to_owned()]
    );
}

#[test]
fn m_corpus_wide_theme_question_routes_to_thematic_synthesis_across_15_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message(
        "Los temas recurrentes son el presupuesto, la gramática, los horarios y las evaluaciones."
            .into(),
    );
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("M").unwrap();
    // 14 documents with recurring themes distributed across them; one document
    // carries only a unique, high-salience theme and must never be cited.
    // Each recurring theme appears only in the document's final section and
    // each closing sentence is worded differently, so the fixture does not
    // plant the answer in the opening paragraph nor rely on identical passages.
    let presupuesto_closings = [
        "Se aprobó el presupuesto mensual y se repasó la gramática de la semana.",
        "El presupuesto del trimestre quedó firme y la gramática se trabajó en clase.",
        "Se confirmó el presupuesto aprobado y se practicó la gramática con ejercicios.",
        "La docente validó el presupuesto final y repasó la gramática con el grupo.",
        "Se ajustó el presupuesto previsto y se explicó la gramática del examen.",
        "Quedó aprobado el presupuesto anual y se practicó la gramática en la pizarra.",
        "Se revisó el presupuesto de materiales y se trabajó la gramática en parejas.",
        "El presupuesto se cerró en la reunión y la gramática se repasó en casa.",
        "Se votó el presupuesto del ciclo y se ejercitó la gramática con audios.",
    ];
    let horarios_closings = [
        "Se fijaron los horarios definitivos y se programaron las evaluaciones de junio.",
        "Los horarios quedaron confirmados y las evaluaciones se agendaron por grupo.",
        "Se ajustaron los horarios de la tarde y se planificaron las evaluaciones parciales.",
        "Quedaron cerrados los horarios semanales y se anunciaron las evaluaciones orales.",
        "Se publicaron los horarios nuevos y se confirmaron las evaluaciones escritas.",
    ];
    for (index, closing) in presupuesto_closings.iter().enumerate() {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("2026-08-0{} reunion-presupuesto.md", index + 1),
            &format!(
                "Reunión {}.\n{}\n## Cierre\n{}\n",
                index + 1,
                thematic_unique_body(index),
                closing
            ),
        );
    }
    for (index, closing) in horarios_closings.iter().enumerate() {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("2026-08-{} reunion-horarios.md", index + 10),
            &format!(
                "Reunión {}.\n{}\n## Cierre\n{}\n",
                index + 10,
                thematic_unique_body(index + 9),
                closing
            ),
        );
    }
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "2026-08-15 reunion-extraordinaria.md",
        "Se realizó una reunión extraordinaria dedicada exclusivamente a certificaciones internacionales de inglés avanzado.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿Cuáles son los temas principales que aparecen repetidamente en las 15 reuniones?\nCitá únicamente los archivos que realmente aportan evidencia para cada tema.",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    assert_eq!(
        metrics.exhaustive_coverage.as_deref(),
        Some("not_requested")
    );
    // Broader evidence than ordinary K3/K4 (8 entries / 2 per document).
    assert!(
        metrics.materials_inspected.unwrap() > 8,
        "thematic synthesis must span more than the ordinary K4 cap: {}",
        metrics.materials_inspected.unwrap()
    );
    assert_eq!(
        metrics.materials_inspected.unwrap(),
        14,
        "all recurring-theme contributors must be inspected"
    );
    assert!(metrics.eligible_materials.unwrap() == 15);
    assert!(metrics.retrieval_candidate_count.unwrap() >= 2);
    // Remote context is bounded well below the raw corpus.
    assert!(metrics.evidence_est_tokens.unwrap() < metrics.corpus_est_tokens.unwrap());
    assert!(metrics.evidence_est_tokens.unwrap() <= 8_000);
    // Exactly one remote synthesis call for the whole thematic question.
    assert_eq!(calls.lock().unwrap().len(), 1);
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("<knowledge_evidence"));
    assert!(prompt.contains("thematic_candidates="));
    // Recurring themes are actually discoverable across sources.
    for theme in ["presupuesto", "gramática", "horarios", "evaluaciones"] {
        assert!(
            prompt.contains(theme),
            "recurring theme {theme} must appear in the thematic evidence"
        );
    }
    // The unique single-document theme never enters the evidence.
    assert!(!prompt.contains("certificaciones"));
    assert!(!prompt.contains("reunion-extraordinaria"));
    let names = metrics.source_names.clone();
    assert_eq!(
        names.len(),
        14,
        "sources must list only contributing files: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name.contains("extraordinaria")),
        "a source with no recurring theme must not be cited: {names:?}"
    );
    assert!(names.iter().any(|name| name.contains("presupuesto")));
    assert!(names.iter().any(|name| name.contains("horarios")));
}

#[test]
fn m_single_document_theme_question_falls_back_to_normal_semantic() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message(
        "Los temas principales que aparecen repetidamente son la retroalimentación y la planeación."
            .into(),
    );
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("M").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "reunion-temas.md",
        &format!(
            "Pregunta: ¿Cuáles son los temas principales que aparecen repetidamente en las 15 reuniones?\nCitá únicamente los archivos que realmente aportan evidencia para cada tema.\nLos temas principales son la retroalimentacion y la planeacion.\n\n{}",
            filler_corpus()
        ),
    );
    let run = state
        .send_message(
            &project.id,
            "¿Cuáles son los temas principales que aparecen repetidamente en las 15 reuniones?\nCitá únicamente los archivos que realmente aportan evidencia para cada tema.",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    // A single READY document cannot exhibit recurring themes across distinct
    // meetings, so the corpus-wide synthesis falls back to compact K3/K4.
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("normal"));
    assert_eq!(
        metrics.exhaustive_coverage.as_deref(),
        Some("not_requested")
    );
    assert!(metrics.selected_evidence_count.unwrap() > 0);
    assert!(metrics.evidence_est_tokens.unwrap() < metrics.corpus_est_tokens.unwrap());
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("<knowledge_evidence"));
    assert!(prompt.contains("reunion-temas.md"));
    assert_eq!(metrics.source_names, vec!["reunion-temas.md".to_owned()]);
}

#[test]
fn n_grammar_term_exhaustive_has_one_source_and_compact_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, hubo una reunión sobre presente continuo.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("N").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "presente-continuo.md",
        "En esta clase se explicó presente continuo con ejemplos y ejercicios.\n",
    );
    add_file(&state, tmp.path(), &project.id, "otra.md", &filler_corpus());
    let run = state
        .send_message(
            &project.id,
            "¿Qué reuniones mencionan presente continuo?\nIndicame la fecha y el archivo exacto de cada una.",
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
    assert!(prompt.contains("presente continuo"));
    assert!(prompt.contains("presente-continuo.md"));
    assert!(prompt.contains("exhaustive_coverage=complete"));
    assert!(!prompt.contains("otra.md"));
    assert_eq!(
        metrics.source_names,
        vec!["presente-continuo.md".to_owned()]
    );
}

#[test]
fn o_two_technology_exhaustive_lists_both_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, se habló de Kubernetes y OpenShift.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("O").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "k8s.md",
        "En la reunion se definio el rollout de Kubernetes.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "o1.md",
        "En otra reunion se comparo OpenShift con el resto del stack.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "relleno.md",
        &filler_corpus(),
    );
    let run = state
        .send_message(
            &project.id,
            "¿Se habló en alguna de las 15 reuniones de Kubernetes u OpenShift?\nRevisá todo el corpus antes de responder.",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    assert_eq!(metrics.lexical_hits, Some(2));
    assert_eq!(metrics.materials_inspected, Some(3));
    assert_eq!(metrics.eligible_materials, Some(3));
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("exhaustive_coverage=complete"));
    assert!(prompt.contains("k8s.md"));
    assert!(prompt.contains("o1.md"));
    assert!(!prompt.contains("relleno.md"));
    let names = metrics.source_names.clone();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"k8s.md".to_owned()));
    assert!(names.contains(&"o1.md".to_owned()));
}

#[test]
fn p_english_theme_question_routes_to_thematic_across_multiple_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("The recurring themes were feedback and planning.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("P").unwrap();
    let english_bodies = [
        "The assistant took the register and collected the attendance sheets from every classroom.",
        "The coordinator updated the timetable and posted the revised notices on the board.",
        "The teachers reviewed the exam copies and organised the marking booklets.",
        "The secretary filed the minutes and archived the correspondence of the term.",
    ];
    // Each meeting closes with a distinctly worded sentence; the topical terms
    // (feedback, planning, pronunciation) still recur across all four sources.
    let english_closings = [
        "In the closing we reviewed the feedback on pronunciation and planned the activities for the next unit.",
        "During the closing the group discussed pronunciation feedback and the planning calendar.",
        "Feedback about pronunciation appeared at closing together with the planning notes.",
        "The meeting ended with pronunciation feedback and the planning of the monthly review.",
    ];
    let english_fillers = [
        "The hall keeper checked the lockers and the coats.",
        "The lab ordered the flasks and the racks.",
        "The library sorted the shelves and the carriages.",
        "The yard crew painted the benches and the nets.",
    ];
    for (index, body) in english_bodies.iter().enumerate() {
        // Long, realistic meeting transcripts (unique filler per source so no
        // distractor word recurs across meetings).
        let filler = std::iter::repeat_n(english_fillers[index], 30)
            .collect::<Vec<_>>()
            .join("\n\n");
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("meeting-{index}.md"),
            &format!(
                "Meeting {index}.\n{}\n\n{filler}\n## Closing\n{}\n",
                body, english_closings[index]
            ),
        );
    }
    let run = state
        .send_message(
            &project.id,
            "What recurring themes appear across all meetings?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    assert_eq!(
        metrics.exhaustive_coverage.as_deref(),
        Some("not_requested")
    );
    assert_eq!(metrics.materials_inspected.unwrap(), 4);
    assert!(metrics.selected_evidence_count.unwrap() > 0);
    assert!(metrics.evidence_est_tokens.unwrap() < metrics.corpus_est_tokens.unwrap());
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("feedback"));
    assert!(prompt.contains("planning"));
    for index in 0..4 {
        assert!(prompt.contains(&format!("meeting-{index}.md")));
    }
    assert_eq!(metrics.source_names.len(), 4);
}

#[test]
fn q_english_inventory_question_is_exhaustive_with_correct_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Kubernetes was discussed.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("Q").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "en.md",
        "Kubernetes was discussed for the cluster.\n",
    );
    add_file(&state, tmp.path(), &project.id, "rest.md", &filler_corpus());
    let run = state
        .send_message(&project.id, "Which meetings mention Kubernetes?", &[])
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    assert_eq!(metrics.lexical_hits, Some(1));
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("en.md"));
    assert!(!prompt.contains("rest.md"));
    assert_eq!(metrics.source_names, vec!["en.md".to_owned()]);
}

#[test]
fn r_single_source_distractor_theme_is_never_cited_or_forwarded() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Los temas recurrentes son presupuesto y horarios.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("R").unwrap();
    for index in 0..5 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("meeting-{index}.md"),
            &format!(
                "Reunión {index}: se revisó el presupuesto del mes y los horarios de la semana.\n"
            ),
        );
    }
    // A high-salience theme that exists in exactly one document.
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "solo-cluster.md",
        "En esta única reunión se planificó la migración completa del cluster de Kubernetes a OpenShift con certificaciones de seguridad.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿Qué temas se repiten en todas las reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    assert_eq!(metrics.materials_inspected.unwrap(), 5);
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("presupuesto"));
    assert!(prompt.contains("horarios"));
    // The single-source theme must not be forwarded as recurring evidence.
    assert!(!prompt.contains("kubernetes"));
    assert!(!prompt.contains("openshift"));
    assert!(!prompt.contains("certificaciones"));
    assert!(!prompt.contains("solo-cluster.md"));
    let names = metrics.source_names.clone();
    assert_eq!(
        names.len(),
        5,
        "only the five contributing meetings: {names:?}"
    );
    assert!(!names.iter().any(|name| name.contains("solo-cluster")));
}

#[test]
fn s_adversarial_routing_distinguishes_thematic_scope_from_lists_and_dates() {
    let tmp = tempfile::tempdir().unwrap();
    let project = {
        let state = recording_app(
            tmp.path(),
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        let project = state.create_project("S").unwrap();
        for index in 0..4 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("meeting-{index}.md"),
                &format!(
                    "Reunión {index}: se revisó el presupuesto del mes y los horarios de la semana.\n"
                ),
            );
        }
        project
    };

    let assert_mode = |prompt: &str, expected: &str| {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let inner = FakeAgentEngine::new();
        inner.set_message("ok".into());
        let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
        if expected == "normal" {
            // Ambiguous open questions are not locally forced to Knowledge;
            // the semantic classifier is the authority for NormalSemantic.
            force_normal_semantic(&state);
        }
        let run = state
            .send_message(&project.id, prompt, &[])
            .expect("completed run");
        assert_eq!(run.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(
            metrics.retrieval_mode.as_deref(),
            Some(expected),
            "expected {expected} for: {prompt}"
        );
    };

    assert_mode("¿Qué temas se repiten en todas las reuniones?", "thematic");
    assert_mode(
        "Resumí los temas recurrentes en estos 15 archivos.",
        "thematic",
    );
    assert_mode(
        "¿Cuáles fueron los principales acuerdos de las 15 reuniones?",
        "thematic",
    );
    assert_mode(
        "¿Qué dificultades de inglés aparecen en las 15 reuniones?",
        "thematic",
    );
    assert_mode(
        "What recurring themes appear across all meetings?",
        "thematic",
    );
    // Lists and dates are not corpus-wide thematic synthesis.
    assert_mode("¿Cuáles son las 5 ideas principales?", "normal");
    assert_mode("¿Qué ocurrió el 15 de julio?", "normal");
    // Concrete presence/inventory stays exhaustive.
    assert_mode("¿Qué reuniones mencionan presente continuo?", "exhaustive");
    assert_mode(
        "¿Se habló en alguna de las 15 reuniones de Kubernetes u OpenShift?",
        "exhaustive",
    );
}

#[test]
fn t_thematic_reuses_persisted_document_summaries_across_restart() {
    use project_core::ProjectId;
    use project_knowledge::{
        KnowledgeStore, SummaryContent, SummaryItem, SummaryLevel, SummaryNode, SummaryState,
    };

    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().to_path_buf();
    let project = {
        let state = recording_app(
            &base,
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        let project = state.create_project("T").unwrap();
        let mut material_ids = Vec::new();
        for index in 0..3 {
            material_ids.push(add_file(
                &state,
                &base,
                &project.id,
                &format!("meeting-{index}.md"),
                &format!(
                    "Reunión {index}: se revisó el presupuesto del mes y los horarios de la semana.\n"
                ),
            ));
        }
        // Persist Ready per-document K6 summaries (simulating a prior cached
        // hierarchical summarization run).
        let root = base.join("projects").join(&project.id);
        let mut store =
            KnowledgeStore::open(&root, &ProjectId::parse(&project.id).unwrap()).unwrap();
        for (index, material_id) in material_ids.iter().enumerate() {
            let document_id = store
                .document_for_material(material_id)
                .unwrap()
                .expect("indexed");
            store
                .store_summary(&SummaryNode {
                    summary_id: format!("doc-summary-{index}"),
                    level: SummaryLevel::Document,
                    state: SummaryState::Ready,
                    failure: None,
                    content: Some(SummaryContent {
                        summary: "Se habló de presupuesto y horarios.".to_owned(),
                        topics: vec![SummaryItem {
                            text: "presupuesto".to_owned(),
                            evidence: vec!["E1".to_owned()],
                        }],
                        decisions: vec![],
                        action_items: vec![],
                        questions: vec![],
                    }),
                    source_ids: vec![document_id],
                    source_chunk_ids: Vec::new(),
                    parent_summary_id: None,
                    input_fingerprint: format!("fp-{index}"),
                    output_fingerprint: format!("out-{index}"),
                    generation_id: "generation-1".to_owned(),
                    model_id: None,
                    provider_id: None,
                    contract_version: project_knowledge::SUMMARY_CONTRACT_VERSION.to_owned(),
                    created_at: 1,
                    updated_at: 1,
                })
                .unwrap();
        }
        project
    };

    // First thematic run reuses all three cached summaries.
    let first_calls = Arc::new(Mutex::new(Vec::new()));
    let first_inner = FakeAgentEngine::new();
    first_inner.set_message("Los temas recurrentes son presupuesto y horarios.".into());
    let first_state = recording_app(&base, RecordingEngine(first_inner, first_calls.clone()));
    let run = first_state
        .send_message(
            &project.id,
            "¿Qué temas se repiten en todas las reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = first_state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    let first_prompt = first_calls.lock().unwrap()[0].clone();
    assert!(first_prompt.contains("summaries_reused=3"));
    assert!(first_prompt.contains("presupuesto"));
    assert_eq!(first_calls.lock().unwrap().len(), 1);

    // Simulated restart: a brand-new AppState over the same durable base dir.
    let second_calls = Arc::new(Mutex::new(Vec::new()));
    let second_inner = FakeAgentEngine::new();
    second_inner.set_message("Los temas recurrentes son presupuesto y horarios.".into());
    let second_state = recording_app(&base, RecordingEngine(second_inner, second_calls.clone()));
    let run = second_state
        .send_message(
            &project.id,
            "¿Qué temas se repiten en todas las reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = second_state
        .last_turn_metrics(&project.id)
        .unwrap()
        .unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    let second_prompt = second_calls.lock().unwrap()[0].clone();
    assert!(
        second_prompt.contains("summaries_reused=3"),
        "cached per-document summaries must survive restart"
    );
    assert_eq!(second_calls.lock().unwrap().len(), 1);
}

/// Unique per-document realistic meeting openers. Vocabulary is disjoint across
/// documents so a single-document word can never become a recurring theme, and
/// the fixtures never plant the answer in the opening paragraph.
const THEMATIC_UNIQUE_OPENERS: [&str; 15] = [
    "El equipo revisó las planillas de la cafetería y las notas del pasillo.",
    "Se actualizaron los roperos del gimnasio y se contaron las llaves.",
    "La biblioteca organizó los lomos de los libros y ordenó los carritos.",
    "Se revisaron los termómetros del laboratorio y las gradillas.",
    "El auditorio probó los focos del escenario y las butacas.",
    "Se limpiaron las pizarras del taller y se guardaron las tizas.",
    "La recepción registró los paquetes del correo y las cajas.",
    "Se pintaron los bancos del patio y se colgaron las redes.",
    "El depósito inventarió los tubos del invernadero y las macetas.",
    "Se empapelaron los murales del pasillo y los carteles.",
    "La cocina ordenó las ollas del comedor y los platos.",
    "Se revisaron los extintores del garaje y las mangueras.",
    "El vestuario colgó los bolsos del equipo y los chalecos.",
    "Se sellaron las cajas del archivo y los sobres.",
    "La oficina etiquetó los legajos del despacho y los folios.",
];

/// Deterministic per-document-unique meeting prose for large corpora. Words are
/// drawn from bounded pools cycled by document index, so any single pool word
/// recurs in only a few documents (document frequency well below the planted
/// recurring themes), keeping the corpus realistic without a per-document
/// answer-planted opening.
fn generated_unique_body(index: usize) -> String {
    const NOUNS: [&str; 18] = [
        "cuadernillos",
        "muebles",
        "estantes",
        "listados",
        "insumos",
        "registros",
        "cronogramas",
        "afiches",
        "expedientes",
        "proyectores",
        "micrófonos",
        "útiles",
        "equipos",
        "turnos",
        "planos",
        "informes",
        "cuadernos",
        "armarios",
    ];
    const VERBS: [&str; 18] = [
        "organizaron",
        "revisaron",
        "actualizaron",
        "imprimieron",
        "calibraron",
        "inventariaron",
        "archivaron",
        "tabularon",
        "confirmaron",
        "registraron",
        "etiquetaron",
        "reordenaron",
        "verificaron",
        "prepararon",
        "distribuyeron",
        "acomodaron",
        "publicaron",
        "firmaron",
    ];
    const OBJECTS: [&str; 18] = [
        "depósito",
        "auditorio",
        "laboratorio",
        "salón",
        "taller",
        "patio",
        "archivo",
        "pasillo",
        "almacén",
        "garaje",
        "sótano",
        "vestuario",
        "estudio",
        "gabinete",
        "despacho",
        "departamento",
        "invernadero",
        "comedor",
    ];
    let noun = NOUNS[index % NOUNS.len()];
    let verb = VERBS[index % VERBS.len()];
    let object = OBJECTS[index % OBJECTS.len()];
    format!(
        "Reunión {index}: se {verb} los {noun} de la semana y se {verb} los {noun} del {object}.\n"
    )
}

#[test]
fn a_theme_appearing_only_late_is_discovered_and_evidenced() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("El tema recurrente es el pasado continuo.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("A").unwrap();
    let closings = [
        "Al cierre se trabajó el pasado continuo con diálogos grabados.",
        "En la parte final se explicó el pasado continuo con ejemplos escritos.",
        "El cierre se dedicó al pasado continuo con juegos de roles.",
        "Se practicó el pasado continuo al final con escenas cortas.",
        "La última actividad mostró el pasado continuo en una línea del tiempo.",
        "El cierre presentó el pasado continuo mediante un esquema visual.",
        "Se modeló el pasado continuo con fragmentos de la película.",
        "Al final se contrastó el pasado continuo con el pasado simple.",
        "Se ejercitó el pasado continuo con descripciones de fotografías.",
        "El cierre incluyó el pasado continuo dentro de un diálogo.",
        "Se repasó el pasado continuo con completar los huecos.",
        "La parte final usó el pasado continuo en oraciones negativas.",
        "Se enseñó el pasado continuo con situaciones cotidianas.",
        "El cierre evaluó el pasado continuo con preguntas orales.",
        "Se reforzó el pasado continuo con tareas para la casa.",
    ];
    for index in 0..15 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("doc{index:02}.md"),
            &format!(
                "Reunión {index}.\n{}\n## Cierre\n{}\n",
                THEMATIC_UNIQUE_OPENERS[index], closings[index]
            ),
        );
    }
    let run = state
        .send_message(
            &project.id,
            "¿Cuáles son los temas principales que aparecen repetidamente en las 15 reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    assert_eq!(calls.lock().unwrap().len(), 1);
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(
        prompt.contains("pasado continuo"),
        "the recurring late theme must be discovered"
    );
    let names = metrics.source_names.clone();
    assert_eq!(
        names.len(),
        15,
        "all supporting documents must be cited: {names:?}"
    );
}

#[test]
fn b_late_lexicographic_sources_can_enter_thematic_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Se habló de kubernetes.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("B").unwrap();
    // 50 documents. Four late lexicographic files support a recurring theme;
    // doc48 is the strongest supporter (five late mentions). The rest of the
    // corpus carries per-document-unique logistics material.
    for index in 0..50 {
        if index == 2 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                "doc02.md",
                &format!(
                    "Reunión 2.\n{}\n## Cierre\nEn el cierre se desplegó el clúster de kubernetes para producción.\n",
                    generated_unique_body(index)
                ),
            );
        } else if index == 17 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                "doc17.md",
                &format!(
                    "Reunión 17.\n{}\n## Cierre\nSe documentó el rollout de kubernetes y se revisaron los pods.\n",
                    generated_unique_body(index)
                ),
            );
        } else if index == 31 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                "doc31.md",
                &format!(
                    "Reunión 31.\n{}\n## Cierre\nLa operación de kubernetes se supervisó con nuevos paneles.\n",
                    generated_unique_body(index)
                ),
            );
        } else if index == 48 {
            let mut body = format!("Reunión 48.\n{}\n## Cierre\n", generated_unique_body(index));
            for i in 0..5 {
                body.push_str(&format!(
                    "Se migró el servicio a kubernetes en el paso {i} y se probó la réplica.\n"
                ));
            }
            add_file(&state, tmp.path(), &project.id, "doc48.md", &body);
        } else {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("doc{index:02}.md"),
                &format!("Reunión {index}.\n{}\n", generated_unique_body(index)),
            );
        }
    }
    let run = state
        .send_message(
            &project.id,
            "¿Qué temas se repiten en todas estas reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    assert_eq!(calls.lock().unwrap().len(), 1);
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(
        prompt.contains("kubernetes"),
        "the recurring theme must appear in the evidence"
    );
    let names = metrics.source_names.clone();
    assert!(
        names.contains(&"doc17.md".to_owned()),
        "a late lexicographic supporting source must enter evidence (doc02 is earlier but must not monopolize selection): {names:?}"
    );
    assert!(
        names.len() <= 20,
        "remote evidence must remain bounded: {names:?}"
    );
    assert!(
        metrics.evidence_est_tokens.unwrap() < metrics.corpus_est_tokens.unwrap(),
        "thematic evidence must stay far below the raw 50-document corpus"
    );
}

#[test]
fn c_hundred_document_corpus_keeps_themes_evidence_and_calls_bounded() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Los temas recurrentes fueron presupuesto y horarios.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("C").unwrap();
    for index in 0..100 {
        let closing = if index < 40 {
            "## Cierre\nSe aprobó el presupuesto del ciclo y se repasó la gramática.\n"
        } else if index < 70 {
            "## Cierre\nSe fijaron los horarios finales y se programaron las evaluaciones.\n"
        } else {
            "## Cierre\nSe practicó el vocabulario nuevo y se explicaron los ejercicios.\n"
        };
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("doc{index:03}.md"),
            &format!(
                "Reunión {index}.\n{}\n{closing}",
                generated_unique_body(index)
            ),
        );
    }
    let run = state
        .send_message(
            &project.id,
            "¿Cuáles son los temas principales que aparecen repetidamente en las 100 reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    assert!(
        metrics.retrieval_candidate_count.unwrap() <= 20,
        "candidate theme count must be explicitly bounded: {}",
        metrics.retrieval_candidate_count.unwrap()
    );
    assert!(
        metrics.selected_evidence_count.unwrap() <= 20,
        "evidence entry count must be explicitly bounded: {}",
        metrics.selected_evidence_count.unwrap()
    );
    assert!(
        metrics.evidence_est_tokens.unwrap() <= 8_000,
        "remote context must remain bounded"
    );
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "exactly one remote synthesis call for the whole thematic question"
    );
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(
        prompt.contains("<knowledge_evidence"),
        "evidence is sent as bounded entries, never the raw corpus"
    );
    let corpus = prompt.len();
    assert!(
        corpus < 30_000,
        "the prompt must not forward the raw corpus"
    );
    let names = metrics.source_names.clone();
    assert_eq!(names.len(), metrics.selected_evidence_count.unwrap());
    assert!(names.len() <= 20);
}

#[test]
fn d_single_document_high_salience_topic_never_becomes_recurring() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Los temas recurrentes son presupuesto y horarios.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("D").unwrap();
    let closings = [
        "## Cierre\nSe revisó el presupuesto del mes y los horarios de la mañana.\n",
        "## Cierre\nSe aprobó el presupuesto previsto y se ajustaron los horarios.\n",
        "## Cierre\nEl presupuesto quedó firme y los horarios se confirmaron.\n",
        "## Cierre\nSe detalló el presupuesto final y los horarios del turno.\n",
        "## Cierre\nSe votó el presupuesto del ciclo y los horarios semanales.\n",
    ];
    for index in 0..5 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("meeting-{index}.md"),
            &format!(
                "Reunión {index}.\n{}\n{}\n",
                THEMATIC_UNIQUE_OPENERS[index], closings[index]
            ),
        );
    }
    // A single document mentions a technology twenty times; document-frequency
    // recurrence (>= 2 distinct documents) must still reject it.
    let mut solo = String::new();
    for _ in 0..20 {
        solo.push_str("El clúster de kubernetes se amplió y se replicó el servicio.\n");
    }
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "solo-tecnologia.md",
        &format!("Reunión única.\n{}\n", solo),
    );
    let run = state
        .send_message(
            &project.id,
            "¿Qué temas se repiten en todas las reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("presupuesto"));
    assert!(prompt.contains("horarios"));
    assert!(
        !prompt.contains("kubernetes"),
        "a single-document topic must not become a recurring corpus theme"
    );
    assert!(!prompt.contains("solo-tecnologia.md"));
    let names = metrics.source_names.clone();
    assert_eq!(
        names.len(),
        5,
        "only the five contributing meetings: {names:?}"
    );
    assert!(!names.iter().any(|name| name.contains("solo-tecnologia")));
}

#[test]
fn e_duplicate_chunks_inside_one_document_do_not_inflate_recurrence() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("El tema recurrente es verbos irregulares.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("E").unwrap();
    let mut duplicated = String::new();
    for _ in 0..6 {
        duplicated.push_str(
            "Se repitió el tema de los verbos irregulares en esta parte de la clase.\n\n",
        );
    }
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "duplicado.md",
        &format!("Reunión duplicada.\n{}\n", duplicated),
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "una-vez.md",
        &format!(
            "Reunión única.\n{}\n## Cierre\nSe mencionó una vez el tema de los verbos irregulares.\n",
            THEMATIC_UNIQUE_OPENERS[0]
        ),
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "sin-tema.md",
        &format!("Reunión neutra.\n{}\n", THEMATIC_UNIQUE_OPENERS[1]),
    );
    let run = state
        .send_message(
            &project.id,
            "¿Qué temas se repiten en todas las reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(
        prompt.contains("irregulares"),
        "the recurring theme must be discovered at document-frequency two"
    );
    let names = metrics.source_names.clone();
    assert!(
        names.contains(&"duplicado.md".to_owned()),
        "the duplicated source is one distinct supporting document"
    );
    assert!(
        names.contains(&"una-vez.md".to_owned()),
        "the single-mention source is the second distinct supporting document"
    );
    assert!(
        !names.iter().any(|name| name == "sin-tema.md"),
        "a document without the recurring theme must not be cited: {names:?}"
    );
}

#[test]
fn f_multiple_recurring_themes_rank_and_keep_correct_provenance() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Los temas recurrentes son presupuesto, evaluaciones y vocabulario.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("F").unwrap();
    let closings = [
        "Al cierre se aprobó el presupuesto del curso y se anotó el total.",
        "Se confirmó el presupuesto previsto y se ajustaron los gastos.",
        "El cierre fijó el presupuesto final y los montos asignados.",
        "Se revisó el presupuesto aprobado y se firmó la planilla.",
        "La última parte detalló el presupuesto del mes y los saldos.",
        "El cierre programó las evaluaciones parciales y las fechas.",
        "Se corrigieron las evaluaciones de junio y los puntajes.",
        "La coordinadora revisó las evaluaciones finales y las notas.",
        "Se publicaron las evaluaciones del bimestre y los promedios.",
        "El cierre analizó las evaluaciones escritas y los criterios.",
        "El cierre practicó el vocabulario de la unidad y las frases.",
        "Se repasó el vocabulario nuevo y los sinónimos.",
        "La docente presentó el vocabulario del tema y los ejemplos.",
        "Se ejercitó el vocabulario cotidiano y las expresiones.",
        "El cierre amplió el vocabulario técnico y los términos.",
    ];
    for index in 0..15 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("doc{index:02}.md"),
            &format!(
                "Reunión {index}.\n{}\n## Cierre\n{}\n",
                THEMATIC_UNIQUE_OPENERS[index], closings[index]
            ),
        );
    }
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "reunion-extraordinaria.md",
        "La reunión extraordinaria se dedicó a certificaciones internacionales de inglés.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿Cuáles son los temas principales que aparecen repetidamente en las 15 reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    assert_eq!(calls.lock().unwrap().len(), 1);
    let prompt = calls.lock().unwrap()[0].clone();
    for theme in ["presupuesto", "evaluaciones", "vocabulario"] {
        assert!(
            prompt.contains(theme),
            "recurring theme {theme} must appear in the thematic evidence"
        );
    }
    assert!(
        !prompt.contains("certificaciones"),
        "the single-document theme must never enter the evidence"
    );
    let names = metrics.source_names.clone();
    assert_eq!(
        names.len(),
        15,
        "each supporting source must be cited exactly once: {names:?}"
    );
    assert!(!names.iter().any(|name| name.contains("extraordinaria")));
}

#[test]
fn h_k6_summaries_are_reused_for_candidates_but_evidence_stays_grounded() {
    use project_core::ProjectId;
    use project_knowledge::{
        KnowledgeStore, SummaryContent, SummaryItem, SummaryLevel, SummaryNode, SummaryState,
    };

    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().to_path_buf();
    let project = {
        let state = recording_app(
            &base,
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        let project = state.create_project("H").unwrap();
        let mut material_ids = Vec::new();
        let closings = [
            "## Cierre\nSe aprobó el presupuesto del trimestre.\n",
            "## Cierre\nSe confirmó el presupuesto del bimestre.\n",
            "## Cierre\nSe revisó el presupuesto del semestre.\n",
        ];
        for index in 0..3 {
            material_ids.push(add_file(
                &state,
                &base,
                &project.id,
                &format!("meeting-{index}.md"),
                &format!(
                    "Reunión {index}.\n{}\n{}\n",
                    THEMATIC_UNIQUE_OPENERS[index], closings[index]
                ),
            ));
        }
        // Persist Ready per-document K6 summaries. The summary text mentions a
        // topic that does NOT appear in the source chunks; it may broaden the
        // candidate set, but evidence must remain grounded in the chunks.
        let root = base.join("projects").join(&project.id);
        let mut store =
            KnowledgeStore::open(&root, &ProjectId::parse(&project.id).unwrap()).unwrap();
        for (index, material_id) in material_ids.iter().enumerate() {
            let document_id = store
                .document_for_material(material_id)
                .unwrap()
                .expect("indexed");
            store
                .store_summary(&SummaryNode {
                    summary_id: format!("doc-summary-{index}"),
                    level: SummaryLevel::Document,
                    state: SummaryState::Ready,
                    failure: None,
                    content: Some(SummaryContent {
                        summary: "Se aprobó el presupuesto y los horarios.".to_owned(),
                        topics: vec![SummaryItem {
                            text: "presupuesto".to_owned(),
                            evidence: vec!["E1".to_owned()],
                        }],
                        decisions: vec![],
                        action_items: vec![],
                        questions: vec![],
                    }),
                    source_ids: vec![document_id],
                    source_chunk_ids: Vec::new(),
                    parent_summary_id: None,
                    input_fingerprint: format!("fp-{index}"),
                    output_fingerprint: format!("out-{index}"),
                    generation_id: "generation-1".to_owned(),
                    model_id: None,
                    provider_id: None,
                    contract_version: project_knowledge::SUMMARY_CONTRACT_VERSION.to_owned(),
                    created_at: 1,
                    updated_at: 1,
                })
                .unwrap();
        }
        project
    };

    let run_calls = Arc::new(Mutex::new(Vec::new()));
    let run_inner = FakeAgentEngine::new();
    run_inner.set_message("Los temas recurrentes son presupuesto y horarios.".into());
    let state = recording_app(&base, RecordingEngine(run_inner, run_calls.clone()));
    let run = state
        .send_message(
            &project.id,
            "¿Qué temas se repiten en todas las reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    assert_eq!(run_calls.lock().unwrap().len(), 1);
    let prompt = run_calls.lock().unwrap()[0].clone();
    assert!(
        prompt.contains("summaries_reused=3"),
        "persisted K6 summaries must be read and reused"
    );
    assert!(
        prompt.contains("presupuesto"),
        "the chunk-grounded theme must appear in the evidence"
    );
    assert!(
        !prompt.contains("Se aprobó el presupuesto y los horarios."),
        "K6 summary text must never be sent as evidence by itself"
    );
    let names = metrics.source_names.clone();
    assert_eq!(
        names.len(),
        3,
        "provenance comes only from the selected chunk evidence: {names:?}"
    );
}

#[test]
fn i_no_k6_fallback_stays_corpus_wide_and_grounded() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Los temas recurrentes son presupuesto y horarios.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("I").unwrap();
    let closings = [
        "## Cierre\nSe aprobó el presupuesto del mes y se fijaron los horarios.\n",
        "## Cierre\nEl presupuesto del año y los horarios quedaron confirmados.\n",
        "## Cierre\nSe revisó el presupuesto trimestral y los horarios de clase.\n",
        "## Cierre\nSe detalló el presupuesto nuevo y los horarios del taller.\n",
    ];
    for index in 0..4 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("meeting-{index}.md"),
            &format!(
                "Reunión {index}.\n{}\n{}\n",
                THEMATIC_UNIQUE_OPENERS[index], closings[index]
            ),
        );
    }
    let run = state
        .send_message(
            &project.id,
            "¿Qué temas se repiten en todas las reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(
        metrics.retrieval_mode.as_deref(),
        Some("thematic"),
        "without K6 summaries the thematic route must still engage"
    );
    assert_eq!(metrics.materials_inspected.unwrap(), 4);
    assert_eq!(calls.lock().unwrap().len(), 1);
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(
        prompt.contains("summaries_reused=0"),
        "without persisted K6 summaries nothing is reused"
    );
    assert!(prompt.contains("presupuesto"));
    assert!(prompt.contains("horarios"));
    assert!(
        metrics.evidence_est_tokens.unwrap() < metrics.corpus_est_tokens.unwrap(),
        "the no-K6 fallback must remain bounded and corpus-wide, not collapse to a tiny top-k"
    );
    assert_eq!(metrics.source_names.len(), 4);
}

#[test]
fn j_sources_dropped_by_the_evidence_budget_never_appear_in_fuentes() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("El tema recurrente es presupuesto.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("J").unwrap();
    // 30 documents all support the same recurring theme; the bounded evidence
    // budget (20 entries, at most one excerpt per distinct document) cannot
    // admit every supporter, so some candidate-producing sources must be
    // dropped. Dropped sources must never appear in Fuentes.
    for index in 0..30 {
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &format!("support-{index:02}.md"),
            &format!(
                "Reunión {index}.\n{}\n## Cierre\nSe aprobó el presupuesto del mes {index} y se anotaron los montos.\n",
                generated_unique_body(index)
            ),
        );
    }
    let run = state
        .send_message(
            &project.id,
            "¿Qué temas se repiten en todas las reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    let evidence_entries = metrics.selected_evidence_count.unwrap();
    assert!(evidence_entries <= 20, "evidence must be bounded");
    let assistant = run.message.unwrap();
    let names = metrics.source_names.clone();
    assert!(
        names.len() <= evidence_entries,
        "sources must never exceed the evidence actually sent (a source may
         support two themes and therefore contribute two excerpts, which
         collapse to one distinct source): {} vs {}",
        names.len(),
        evidence_entries
    );
    assert!(
        names.len() < 30,
        "some candidate-producing sources must be dropped by the budget"
    );
    let prompt = calls.lock().unwrap()[0].clone();
    for name in &names {
        assert!(
            prompt.contains(name),
            "every cited source must actually appear in the sent evidence"
        );
    }
    // The sources that were dropped from the final package must not be cited.
    for index in 0..30 {
        let name = format!("support-{index:02}.md");
        if !names.contains(&name) {
            assert!(
                !assistant.contains(&name),
                "a source dropped by the evidence budget must never be cited: {name}"
            );
        }
    }
}

/// Realistic Fedora/Gemini reproduction: 15 long Spanish meeting transcripts
/// that share conversational chatter in nearly every document (greetings,
/// interruptions, acknowledgements, references to students/classes, scheduling,
/// logistics and meeting transitions), while meaningful recurring pedagogical
/// topics appear in *fewer* documents and in varied positions. This reproduces
/// the class of human Fedora failure where common discourse words and generic
/// meeting chatter crowded out the pedagogical themes. The test is adversarial:
/// shared conversational words occur in all 15 documents, the pedagogical
/// themes occur in only 5–8 documents, and unrelated repeated distractors
/// compete. The ranking must keep the pedagogical topics selected and must
/// never cite the single-document distractor or the conversational filler.
#[test]
fn fedora_realistic_meeting_corpus_selects_recurring_pedagogical_topics() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Los temas recurrentes de las reuniones son las estructuras gramaticales y la pronunciación.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("Fedora").unwrap();

    // Pedagogical theme sentences, kept minimal and bracketting the theme with
    // function/scaffold words so they never fabricate spurious cross-document
    // bigrams. Theme => (support, doc set, sentence).
    let theme_sentence = |theme: usize| -> &'static str {
        match theme {
            0 => "Se explicó el presente continuo.",
            1 => "Se revisó el pasado simple.",
            2 => "Se comentó la comprensión auditiva.",
            3 => "Se plantearon preguntas en pasado.",
            4 => "Se retomó la pronunciación.",
            _ => unreachable!(),
        }
    };
    // Deterministic theme membership with the exact support targets:
    // presente continuo 8, pasado simple 7, comprensión auditiva 6,
    // preguntas en pasado 5, pronunciación 5.
    let has_theme = |doc: usize, theme: usize| -> bool {
        match theme {
            0 => [0, 2, 4, 6, 8, 10, 12, 13].contains(&doc),
            1 => [1, 3, 5, 7, 9, 11, 13].contains(&doc),
            2 => [0, 3, 6, 9, 12, 13].contains(&doc),
            3 => [1, 4, 7, 10, 13].contains(&doc),
            4 => [2, 5, 8, 11, 13].contains(&doc),
            _ => false,
        }
    };

    // Shared conversational chatter in every document. These use only the
    // categorized discourse/politeness fillers ("bueno", "claro", "gracias",
    // "verdad"), the meeting-participant role noun "alumnos", and light meeting
    // verbs, all of which are function/scaffold words that can never become
    // standalone recurring themes.
    let chatter = [
        "Bueno, la verdad es que claro, gracias, vamos a empezar.",
        "Entonces los alumnos y el equipo ya están al tanto de la agenda.",
        "Vamos, retomamos la clase y anotamos las novedades.",
    ];
    let openers = [
        "Reunión {n}: inicio de la sesión con la agenda del día.",
        "Reunión {n}: apertura del encuentro y novedades del equipo.",
        "Reunión {n}: inicio del encuentro con la agenda de la semana.",
    ];
    let logistics = [
        "Se comentó la agenda del próximo encuentro y se anotaron las pautas.",
        "Se revisaron los materiales y se coordinaron las pautas del curso.",
        "Se anotaron los puntos y se retomaron las consignas.",
    ];
    // Scaffold-only filler narration used to build long middle sections. These
    // sentences contain only categorized function/scaffold words, so they add
    // realistic meeting length and competition without inventing new recurring
    // themes.
    let fillers = [
        "Se comentaron los avances y se anotaron las dudas del equipo.",
        "Se revisaron las consignas y se coordinaron los próximos pasos.",
        "Se acordó retomar el tema y se plantearon las consultas.",
        "Se compartieron las novedades y se resumieron los puntos.",
        "Se retomaron la agenda y se continuaron las pautas.",
        "Se terminaron los puntos y se siguieron las pautas del día.",
    ];

    for doc in 0..15 {
        // Document 14 is a single-document distractor: it shares the same
        // conversational chatter but its only topical content is a unique,
        // high-salience topic that appears nowhere else. It must never become a
        // recurring theme nor be cited as a supporting source.
        if doc == 14 {
            let paragraphs = [
                chatter[doc % chatter.len()].to_owned(),
                "Este ciclo se planificó la certificación cambridge internacional para el grupo avanzado.".to_owned(),
                "La certificación cambridge requiere inscripción previa y un examen final con simulacros mensuales.".to_owned(),
                "La certificación cambridge se evaluó con simulacros de lectura, escritura y entrevistas.".to_owned(),
            ];
            let name = format!("2026-08-{doc:02} reunión docente.md");
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &name,
                &paragraphs.join("\n\n"),
            );
            continue;
        }

        let mut paragraphs = Vec::new();
        paragraphs.push(openers[doc % openers.len()].replace("{n}", &doc.to_string()));
        paragraphs.push(chatter[doc % chatter.len()].to_owned());
        paragraphs.push(logistics[doc % logistics.len()].to_owned());
        // Unrelated repeated distractors (repeated, but at far lower document
        // frequency than any pedagogical theme, so they never crowd it out).
        if [1, 5, 9].contains(&doc) {
            paragraphs.push("Se prepararon las fotocopias.".to_owned());
        }
        if [2, 8].contains(&doc) {
            paragraphs.push("Se reservó la biblioteca.".to_owned());
        }

        // Varied positions: early (right after the opening), middle (after
        // several narration paragraphs), or late (after a long middle section),
        // so discovery must work across the whole supported window.
        let mut theme_paras = Vec::new();
        for theme in 0..5 {
            if has_theme(doc, theme) {
                theme_paras.push(theme_sentence(theme).to_owned());
            }
        }
        match doc % 3 {
            0 => {
                paragraphs.extend(theme_paras);
                for i in 0..6 {
                    paragraphs.push(fillers[(doc + i) % fillers.len()].to_owned());
                }
            }
            1 => {
                for i in 0..6 {
                    paragraphs.push(fillers[(doc + i) % fillers.len()].to_owned());
                }
                paragraphs.extend(theme_paras);
                for i in 0..6 {
                    paragraphs.push(fillers[(doc + i + 2) % fillers.len()].to_owned());
                }
            }
            _ => {
                for i in 0..14 {
                    paragraphs.push(fillers[(doc + i) % fillers.len()].to_owned());
                }
                paragraphs.extend(theme_paras);
            }
        }
        paragraphs.push(
            "El cierre retomó los acuerdos y se saludaron hasta la próxima reunión.".to_owned(),
        );
        let name = format!("2026-08-{doc:02} reunión docente.md");
        add_file(
            &state,
            tmp.path(),
            &project.id,
            &name,
            &paragraphs.join("\n\n"),
        );
    }

    let run = state
        .send_message(
            &project.id,
            "¿Cuáles son los temas principales que aparecen repetidamente en las 15 reuniones?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
    assert_eq!(
        metrics.exhaustive_coverage.as_deref(),
        Some("not_requested")
    );
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "exactly one remote synthesis call"
    );
    let prompt = calls.lock().unwrap()[0].clone();
    assert!(prompt.contains("<knowledge_evidence"));
    for theme in [
        "presente continuo",
        "pasado simple",
        "comprensión auditiva",
        "preguntas en pasado",
        "pronunciación",
    ] {
        assert!(
            prompt.contains(theme),
            "recurring pedagogical topic {theme:?} must be selected and evidenced"
        );
    }
    // Conversational filler must never be *selected* as a recurring theme. The
    // chatter still appears inside other themes' evidence chunks, so the
    // selected-theme set (not the raw prompt) is the correct assertion target.
    {
        use project_core::ProjectId;
        use project_knowledge::KnowledgeStore;
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &ProjectId::parse(&project.id).unwrap()).unwrap();
        let report = store.thematic_synthesis_evidence().unwrap();
        for noise in ["bueno", "claro", "gracias", "verdad", "alumnos"] {
            assert!(
                !report.selected_themes.iter().any(|theme| theme == noise),
                "conversational word {noise:?} must never be a selected theme: {:?}",
                report.selected_themes
            );
        }
        for theme in [
            "presente continuo",
            "pasado simple",
            "comprensión auditiva",
            "preguntas en pasado",
            "pronunciación",
        ] {
            assert!(
                report.selected_themes.iter().any(|t| t == theme),
                "pedagogical theme {theme:?} must be selected: {:?}",
                report.selected_themes
            );
        }
    }
    // The single-document distractor must never enter the evidence.
    assert!(
        !prompt.contains("cambridge"),
        "the single-document distractor must never become recurring evidence"
    );
    assert!(
        metrics.selected_evidence_count.unwrap() <= 20,
        "evidence must remain bounded"
    );
    let names = metrics.source_names.clone();
    assert!(!names.is_empty());
    assert!(names.len() <= 20);
    assert!(
        !names.iter().any(|name| name.contains("2026-08-14")),
        "the distractor source must never be cited: {names:?}"
    );
}

#[test]
fn depend_on_presence_query_is_found_not_a_false_negative() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, se practicó \"depend on\" en el material de phrasal verbs.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("DepOn").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "phrasal-verbs.md",
        "En esta clase se practicó el phrasal verb depend on con ejemplos.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "gramatica.md",
        &filler_corpus(),
    );
    let run = state
        .send_message(
            &project.id,
            "¿En qué archivos aparece o se practica \"depend on\"?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    assert!(metrics.lexical_hits.unwrap() >= 1);
    assert_eq!(metrics.source_names, vec!["phrasal-verbs.md".to_owned()]);
    let text = run.message.unwrap();
    assert!(
        !text.contains("No encontré menciones"),
        "a present phrase must never yield a negative answer: {text}"
    );
}

#[test]
fn unicode_curly_quoted_presence_query_is_found_not_a_false_negative() {
    // The same protected needle must be recovered from typographic quotes that
    // a non-technical user would paste from a rich-text editor or mobile
    // keyboard. The corpus contains the literal phrase, so this must be a
    // lexical hit, never a false negative.
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, se practicó \"depend on\" en el material de phrasal verbs.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("DepOnUnicode").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "phrasal-verbs.md",
        "En esta clase se practicó el phrasal verb depend on con ejemplos.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "gramatica.md",
        &filler_corpus(),
    );
    let run = state
        .send_message(
            &project.id,
            "¿En qué archivos aparece o se practica \u{201c}depend on\u{201d}?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    assert!(metrics.lexical_hits.unwrap() >= 1);
    assert!(metrics.selected_evidence_count.unwrap() > 0);
    assert_eq!(metrics.source_names, vec!["phrasal-verbs.md".to_owned()]);
    let text = run.message.unwrap();
    assert!(
        !text.contains("No encontré menciones"),
        "a present phrase must never yield a negative answer: {text}"
    );
}

#[test]
fn unicode_quoted_absent_phrase_is_a_truthful_true_negative() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("no debe usarse".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("UnicodeTrueNegative").unwrap();
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
        "Otra reunion sobre vocabulario.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿En qué archivos aparece \u{201c}zxqv-not-in-corpus\u{201d}?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    assert!(calls.lock().unwrap().is_empty());
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    assert_eq!(metrics.lexical_hits, Some(0));
    assert_eq!(metrics.selected_evidence_count, Some(0));
    assert_eq!(metrics.source_names, Vec::<String>::new());
    let text = run.message.unwrap();
    assert!(text.contains("No encontré menciones"));
    assert!(!text.contains("Fuentes:"));
}

#[test]
fn depend_on_in_exactly_one_document_reports_only_that_source() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, aparece en un solo archivo.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("DepOne").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "hit.md",
        "Los estudiantes depend on la agenda para organizarse.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "otro.md",
        "Reunion de gramatica, precios y horarios.\n",
    );
    let _ = state
        .send_message(
            &project.id,
            "¿En qué archivos aparece o se practica \"depend on\"?",
            &[],
        )
        .unwrap();
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_eq!(metrics.lexical_hits, Some(1));
    assert_eq!(metrics.source_names, vec!["hit.md".to_owned()]);
}

#[test]
fn depend_on_in_multiple_documents_reports_all_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, aparece en varios archivos.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("DepMany").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "a.md",
        "En esta clase se usó depend on en la consigna.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "b.md",
        "Se explicó depend on junto con otros phrasal verbs.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "c.md",
        "Practicamos depend on en los ejercicios de cierre.\n",
    );
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "relleno.md",
        "Solo gramatica y precios.\n",
    );
    let _ = state
        .send_message(
            &project.id,
            "¿En qué archivos aparece o se practica \"depend on\"?",
            &[],
        )
        .unwrap();
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.lexical_hits, Some(3));
    let mut names = metrics.source_names.clone();
    names.sort();
    assert_eq!(names, vec!["a.md", "b.md", "c.md"]);
}

#[test]
fn multi_word_phrasal_verbs_are_found_generically() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, se encontró.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("Phrasal").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "phrasals.md",
        "We teach think about as a phrasal verb.\n\
         We teach look for as a phrasal verb.\n\
         We teach arrive at as a phrasal verb.\n\
         We teach be going to as a future form.\n",
    );
    for phrase in ["think about", "look for", "arrive at", "be going to"] {
        let query = format!("¿En qué archivos se practica \"{phrase}\"?");
        let run = state
            .send_message(&project.id, &query, &[])
            .unwrap_or_else(|_| panic!("{query}"));
        assert_eq!(run.status, "completed", "{query}");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(
            metrics.retrieval_mode.as_deref(),
            Some("exhaustive"),
            "{query}"
        );
        assert!(metrics.lexical_hits.unwrap() >= 1, "{query}");
        assert_eq!(
            metrics.source_names,
            vec!["phrasals.md".to_owned()],
            "{query}"
        );
    }
}

#[test]
fn dependency_near_miss_is_not_positive_evidence_for_depend_on() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("no debe usarse".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("NearMiss").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "dependencia.md",
        "Se analizó la dependencia entre el aprendizaje y la práctica de los verbos.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿En qué archivos aparece o se practica \"depend on\"?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    assert!(
        calls.lock().unwrap().is_empty(),
        "a true negative must not forward the corpus: {:?}",
        calls.lock().unwrap()
    );
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    assert_eq!(metrics.lexical_hits, Some(0));
    let text = run.message.unwrap();
    assert!(text.contains("No encontré menciones"));
    assert!(!text.contains("dependencia.md"));
    assert!(!text.contains("Fuentes:"));
}

#[test]
fn depend_on_true_negative_is_local_complete_and_zero_source() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("no debe usarse".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("DepNeg").unwrap();
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
        "Otra reunion sobre vocabulario.\n",
    );
    let run = state
        .send_message(
            &project.id,
            "¿En qué archivos aparece o se practica \"depend on\"?",
            &[],
        )
        .unwrap();
    assert_eq!(run.status, "completed");
    assert!(calls.lock().unwrap().is_empty());
    let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    assert_eq!(metrics.lexical_hits, Some(0));
    assert_eq!(metrics.source_names, Vec::<String>::new());
    let text = run.message.unwrap();
    assert!(text.contains("No encontré menciones"));
    assert!(!text.contains("Fuentes:"));
}

#[test]
fn depend_on_phrase_query_survives_restart_without_rework() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Sí, se practicó depend on.".into());
    let state = recording_app(tmp.path(), RecordingEngine(inner, calls.clone()));
    let project = state.create_project("DepRestart").unwrap();
    add_file(
        &state,
        tmp.path(),
        &project.id,
        "phrasal-verbs.md",
        "En esta clase se practicó depend on con ejemplos.\n",
    );
    let _ = state
        .send_message(
            &project.id,
            "¿En qué archivos aparece o se practica \"depend on\"?",
            &[],
        )
        .unwrap();
    let before = state.last_turn_metrics(&project.id).unwrap().unwrap();
    assert_eq!(before.retrieval_mode.as_deref(), Some("exhaustive"));
    assert!(before.lexical_hits.unwrap() >= 1);
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
    assert_eq!(after.source_names, before.source_names);
    assert!(fresh_calls.lock().unwrap().is_empty());
    let _ = fresh.open_project(&project.id).unwrap();
    assert_eq!(fresh_calls.lock().unwrap().len(), 0);
}
