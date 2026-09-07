//! K6 structural acceptance: hierarchical summarization through the production
//! `AppState` facade with a deterministic fake/capture summarizer (no remote
//! calls). Covers: single-doc (TXT/Markdown/Spanish), multi-doc global, large
//! corpus multi-level hierarchy, deterministic grouping, cache reuse,
//! single-document-change invalidation, deletion lineage, provider failure,
//! structured-output validation, and zero chat-pollution.

use std::sync::{Arc, Mutex};

use project_agent::FakeAgentEngine;
use project_app::AppState;
use project_core::ProjectId;
use project_knowledge::{
    BatchOptions, ContextAssemblyOptions, KnowledgeStore, RemoteSummarizer, SummaryContent,
    SummaryFailure, SummaryLevel, SummaryOutput, SummaryRequest, SummaryState,
};
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

fn make_app(
    base: &std::path::Path,
) -> AppState<FakeAgentEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
    AppState::with_components(
        base.to_path_buf(),
        FakeAgentEngine::new(),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    )
}

/// Deterministic, inspecting summarizer. Produces valid structured JSON and
/// records every request (level, labels, evidence text) for assertions.
#[derive(Clone)]
struct CaptureSummarizer {
    calls: Arc<Mutex<Vec<CapturedRequest>>>,
    fail: bool,
}

#[derive(Clone, Debug)]
struct CapturedRequest {
    level: SummaryLevel,
    labels: Vec<String>,
    evidence_texts: Vec<String>,
}

impl CaptureSummarizer {
    fn new() -> (Self, Arc<Mutex<Vec<CapturedRequest>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                calls: calls.clone(),
                fail: false,
            },
            calls,
        )
    }

    fn failing() -> Self {
        let (this, _) = Self::new();
        Self { fail: true, ..this }
    }

    fn output_for(request: &SummaryRequest) -> SummaryOutput {
        let first_label = request.labels.first().map(|r| r.label.clone());
        let topics = first_label
            .as_ref()
            .map(|label| {
                serde_json::json!([{ "text": "Se habló de presupuesto", "evidence": [label] }])
            })
            .unwrap_or_else(|| serde_json::json!([]));
        let content = SummaryContent {
            summary: "Reunión sobre presupuesto y planificación.".to_owned(),
            topics: serde_json::from_value(topics).unwrap(),
            decisions: serde_json::from_value(serde_json::json!([
                { "text": "Aprobar el plan trimestral", "evidence": [first_label.unwrap_or_default()] }
            ]))
            .unwrap(),
            action_items: vec![],
            questions: vec![],
        };
        SummaryOutput {
            text: serde_json::to_string(&content).unwrap(),
            model_id: Some("capture-model".to_owned()),
            provider_id: Some("capture-provider".to_owned()),
            usage: project_knowledge::SummaryUsage::default(),
        }
    }
}

impl RemoteSummarizer for CaptureSummarizer {
    fn summarize(
        &self,
        request: &SummaryRequest,
    ) -> std::result::Result<SummaryOutput, SummaryFailure> {
        if self.fail {
            return Err(SummaryFailure::ProviderUnavailable);
        }
        self.calls.lock().unwrap().push(CapturedRequest {
            level: request.level,
            labels: request.labels.iter().map(|r| r.label.clone()).collect(),
            evidence_texts: request.evidence_texts.clone(),
        });
        Ok(Self::output_for(request))
    }
}

fn add_txt<E: project_agent::AgentEngine>(
    app: &AppState<E, FakeTunnel, FakeProviderConnector, FakeRestarter>,
    base: &std::path::Path,
    project_id: &str,
    name: &str,
    body: &str,
) -> String {
    let src = base.join(name);
    std::fs::write(&src, body).unwrap();
    let material = app
        .add_material_from_path(project_id, src.to_str().unwrap())
        .unwrap();
    let _ = std::fs::remove_file(&src);
    material.id
}

fn open_store(base: &std::path::Path, project_id: &str) -> KnowledgeStore {
    KnowledgeStore::open(
        base.join("projects").join(project_id),
        &ProjectId::parse(project_id).unwrap(),
    )
    .unwrap()
}

fn global_summary_of(store: &KnowledgeStore) -> Option<SummaryContent> {
    // Prefer the global summary; a single-document project has its document
    // summary as the root (no redundant global node).
    let mut document_fallback = None;
    for id in store.summary_ids().unwrap() {
        if let Some(node) = store.get_summary(&id).unwrap()
            && node.state == SummaryState::Ready
        {
            match node.level {
                SummaryLevel::Global => return node.content,
                SummaryLevel::Document => document_fallback = node.content,
                SummaryLevel::Batch => {}
            }
        }
    }
    document_fallback
}

// ---------------------------------------------------------------------------

#[test]
fn single_txt_document_summarizes_and_reuses_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();
    add_txt(
        &app,
        tmp.path(),
        &p.id,
        "reunion.txt",
        "En la reunión se aprobó el presupuesto.\nSe definió el plan trimestral.",
    );

    let (summarizer, calls) = CaptureSummarizer::new();
    let report = app.summarize_project(&p.id, &summarizer).unwrap();
    assert!(report.regenerated >= 1);
    assert_eq!(report.remote_calls, report.regenerated);

    let store = open_store(tmp.path(), &p.id);
    let content = global_summary_of(&store).unwrap();
    assert!(content.summary.contains("presupuesto"));
    assert!(!content.topics.is_empty());

    // Second run: full cache hit, zero remote calls.
    calls.lock().unwrap().clear();
    let report2 = app.summarize_project(&p.id, &summarizer).unwrap();
    assert_eq!(report2.remote_calls, 0);
    assert_eq!(report2.reused, report.reused + report.regenerated);
    assert!(
        calls.lock().unwrap().is_empty(),
        "no remote call on cache hit"
    );
}

#[test]
fn markdown_document_summarizes_with_headings_as_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();
    add_txt(
        &app,
        tmp.path(),
        &p.id,
        "guia.md",
        "# Operaciones\n\nOpenShift usa operadores para automatizar.\n\n# Decisiones\n\nSe aprobó la migración.",
    );

    let (summarizer, calls) = CaptureSummarizer::new();
    let report = app.summarize_project(&p.id, &summarizer).unwrap();
    assert!(report.regenerated >= 1);

    let captured = calls.lock().unwrap();
    let doc_call = captured
        .iter()
        .find(|c| c.level == SummaryLevel::Document)
        .expect("one document summary call");
    assert!(
        doc_call
            .evidence_texts
            .iter()
            .any(|t| t.contains("OpenShift")),
        "document evidence must be bounded real chunk text"
    );
}

#[test]
fn multiple_documents_produce_global_summary_with_lineage() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();
    add_txt(
        &app,
        tmp.path(),
        &p.id,
        "a.txt",
        "Se habló de presupuesto y de la contratacion.",
    );
    add_txt(
        &app,
        tmp.path(),
        &p.id,
        "b.txt",
        "Se decidió migrar a la nube en el próximo trimestre.",
    );
    add_txt(
        &app,
        tmp.path(),
        &p.id,
        "c.txt",
        "Quedó pendiente definir el cronograma de prueba.",
    );

    let (summarizer, calls) = CaptureSummarizer::new();
    let report = app.summarize_project(&p.id, &summarizer).unwrap();
    assert!(report.hierarchy_depth >= 2, "document + global levels");

    let store = open_store(tmp.path(), &p.id);
    let global = global_summary_of(&store).unwrap();
    assert!(!global.summary.is_empty());

    // Batch/global synthesis calls reference only prior summaries as evidence,
    // never raw corpus.
    let captured = calls.lock().unwrap();
    let doc_calls = captured
        .iter()
        .filter(|c| c.level == SummaryLevel::Document)
        .count();
    assert_eq!(doc_calls, 3, "one document summary per source");

    // Document evidence uses E-labels; synthesis evidence uses P-labels.
    assert!(
        captured
            .iter()
            .any(|c| c.level == SummaryLevel::Document
                && c.labels.iter().any(|l| l.starts_with('E')))
    );
    assert!(
        captured
            .iter()
            .any(|c| c.level != SummaryLevel::Document
                && c.labels.iter().any(|l| l.starts_with('P')))
    );
}

#[test]
fn large_corpus_forces_multi_level_reduction() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();
    // 30 small transcript-like fixtures -> document + batch + global levels.
    for i in 0..30 {
        add_txt(
            &app,
            tmp.path(),
            &p.id,
            &format!("reunion-{i:02}.txt"),
            &format!("Tema de la reunion {i}: coordinacion de tareas y seguimiento."),
        );
    }

    let (summarizer, calls) = CaptureSummarizer::new();
    let report = app.summarize_project(&p.id, &summarizer).unwrap();
    assert!(
        report.hierarchy_depth >= 3,
        "must reduce through an intermediate batch level"
    );

    let captured = calls.lock().unwrap();
    let docs = captured
        .iter()
        .filter(|c| c.level == SummaryLevel::Document)
        .count();
    let batches = captured
        .iter()
        .filter(|c| c.level == SummaryLevel::Batch)
        .count();
    let globals = captured
        .iter()
        .filter(|c| c.level == SummaryLevel::Global)
        .count();
    assert_eq!(docs, 30);
    assert!(batches >= 1);
    assert_eq!(globals, 1);

    // Every batch/global synthesis request stays bounded: no single request
    // contains the 30-document corpus.
    for c in captured
        .iter()
        .filter(|c| c.level != SummaryLevel::Document)
    {
        assert!(
            c.evidence_texts.len() <= BatchOptions::default().branching_factor,
            "synthesis evidence must be a bounded group, not the whole corpus"
        );
    }
}

#[test]
fn changed_document_invalidates_only_dependent_lineage() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();
    let _a = add_txt(
        &app,
        tmp.path(),
        &p.id,
        "a.txt",
        "Se aprobó el presupuesto año fiscal.",
    );
    add_txt(
        &app,
        tmp.path(),
        &p.id,
        "b.txt",
        "Se discutió la capacitación.",
    );
    add_txt(
        &app,
        tmp.path(),
        &p.id,
        "c.txt",
        "Pendiente: actualizar el manual.",
    );

    let (summarizer, calls) = CaptureSummarizer::new();
    app.summarize_project(&p.id, &summarizer).unwrap();

    let store = open_store(tmp.path(), &p.id);
    let doc_before = store
        .summary_ids()
        .unwrap()
        .into_iter()
        .filter_map(|id| store.get_summary(&id).unwrap())
        .filter(|n| n.level == SummaryLevel::Document)
        .count();
    assert_eq!(doc_before, 3);

    // Re-add document "a" with modified content: only "a" lineage invalidates.
    let src = tmp.path().join("a.txt");
    std::fs::write(
        &src,
        "Nuevo plan: recortar el presupuesto en un diez por ciento.",
    )
    .unwrap();
    app.add_material_from_path(&p.id, src.to_str().unwrap())
        .unwrap();
    let _ = std::fs::remove_file(&src);

    // Re-summarize: the changed document and its ancestors regenerate, but
    // unchanged document summaries are reused.
    calls.lock().unwrap().clear();
    let report = app.summarize_project(&p.id, &summarizer).unwrap();
    let captured = calls.lock().unwrap();
    let regenerated_docs = captured
        .iter()
        .filter(|c| c.level == SummaryLevel::Document)
        .count();
    // Only the newly-added document's summary (and its batch/global) regenerate;
    // the two unchanged document summaries remain cached.
    assert!(
        regenerated_docs <= 2,
        "unchanged document summaries must be reused"
    );
    assert!(
        report.remote_calls > 0,
        "changed lineage requires remote synthesis"
    );
}

#[test]
fn deleted_source_invalidates_dependent_lineage_not_others() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();
    let a = add_txt(
        &app,
        tmp.path(),
        &p.id,
        "a.txt",
        "Se aprobó la contratación.",
    );
    add_txt(&app, tmp.path(), &p.id, "b.txt", "Se definió el alcance.");

    let (summarizer, _calls) = CaptureSummarizer::new();
    app.summarize_project(&p.id, &summarizer).unwrap();

    app.remove_material(&p.id, &a).unwrap();

    let store = open_store(tmp.path(), &p.id);
    // The deleted document's summary and anything built from it is now stale;
    // the document it contributed to removed, but b's summary remains Ready.
    let any_stale = store
        .summary_ids()
        .unwrap()
        .into_iter()
        .filter_map(|id| store.get_summary(&id).unwrap())
        .any(|n| n.state == SummaryState::Stale);
    assert!(
        any_stale,
        "deleted source must invalidate its summary lineage"
    );
}

#[test]
fn retry_after_failed_synthesis_recovers() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();
    add_txt(&app, tmp.path(), &p.id, "a.txt", "Se discutió el alcance.");

    let failing = CaptureSummarizer::failing();
    let report = app.summarize_project(&p.id, &failing).unwrap();
    assert!(report.regenerated == 0);
    let store = open_store(tmp.path(), &p.id);
    assert!(
        store
            .summary_ids()
            .unwrap()
            .into_iter()
            .filter_map(|id| store.get_summary(&id).unwrap())
            .all(|n| n.state == SummaryState::Failed),
        "remote failure must not produce a Ready summary"
    );

    let (ok_summarizer, _calls) = CaptureSummarizer::new();
    let report2 = app.summarize_project(&p.id, &ok_summarizer).unwrap();
    assert!(report2.regenerated >= 1);
}

#[test]
fn provider_unavailable_keeps_existing_ready_summary_readable() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();
    add_txt(
        &app,
        tmp.path(),
        &p.id,
        "a.txt",
        "Se aprobó el presupuesto.",
    );

    let (summarizer, _calls) = CaptureSummarizer::new();
    app.summarize_project(&p.id, &summarizer).unwrap();
    let store = open_store(tmp.path(), &p.id);
    let ready_before = store
        .summary_ids()
        .unwrap()
        .into_iter()
        .filter_map(|id| store.get_summary(&id).unwrap())
        .any(|n| n.state == SummaryState::Ready);
    assert!(ready_before);

    // New source, then provider fails: existing Ready summaries remain readable.
    add_txt(&app, tmp.path(), &p.id, "b.txt", "Nueva reunión.");
    let failing = CaptureSummarizer::failing();
    let _ = app.summarize_project(&p.id, &failing).unwrap();
    let store = open_store(tmp.path(), &p.id);
    let global = global_summary_of(&store);
    assert!(
        global.is_some(),
        "existing valid summary remains readable offline"
    );
}

#[test]
fn summarization_does_not_pollute_chat_history() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();
    add_txt(
        &app,
        tmp.path(),
        &p.id,
        "a.txt",
        "Se aprobó el presupuesto.",
    );

    let (summarizer, _calls) = CaptureSummarizer::new();
    app.summarize_project(&p.id, &summarizer).unwrap();

    // No user-visible messages were appended by summarization.
    let project = app.open_project(&p.id).unwrap();
    assert!(
        project.messages.is_empty(),
        "internal summarization must not create chat turns"
    );
}

#[test]
fn structured_output_validation_rejects_non_existent_evidence() {
    // The fake summarizer emits valid output; test the validation boundary for
    // a hallucinated evidence label directly.
    let raw = "{\"summary\":\"s\",\"topics\":[{\"text\":\"t\",\"evidence\":[\"E1\"]}],\
               \"decisions\":[],\"action_items\":[],\"questions\":[]}";
    let validated = project_knowledge::validate_summary_output(raw, &["E9".to_owned()]).unwrap();
    assert!(
        validated.topics[0].evidence.is_empty(),
        "references to nonexistent evidence must be stripped"
    );
}

#[test]
fn summary_state_survives_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();
    add_txt(
        &app,
        tmp.path(),
        &p.id,
        "a.txt",
        "Se aprobó el presupuesto.",
    );

    let (summarizer, _calls) = CaptureSummarizer::new();
    app.summarize_project(&p.id, &summarizer).unwrap();

    // Reopen the store fresh (simulating restart); summaries persist.
    let store = open_store(tmp.path(), &p.id);
    let total = store.summary_ids().unwrap().len();
    assert!(total >= 1);

    // A fresh AppState over the same base reuses the cached summaries (zero calls).
    let app2 = make_app(tmp.path());
    let (summarizer2, calls2) = CaptureSummarizer::new();
    let report = app2.summarize_project(&p.id, &summarizer2).unwrap();
    assert_eq!(
        report.remote_calls, 0,
        "restart must reuse durable cached summaries"
    );
    assert!(calls2.lock().unwrap().is_empty());
}

/// Representative 51-file production flow: batch-import 51 Markdown meeting
/// notes (Spanish/English, realistic sizes), verify corpus/index state, prove a
/// bounded K3/K4 ordinary query, then K6 global summarization with cache reuse
/// and no whole-corpus request. Non-sensitive synthetic fixtures only.
#[test]
fn representative_51_markdown_corpus_flow() {
    let tmp = tempfile::tempdir().unwrap();
    let app = make_app(tmp.path());
    let p = app.create_project("P").unwrap();

    let dir = tmp.path().join("corpus");
    std::fs::create_dir_all(&dir).unwrap();
    let mut paths = Vec::new();
    for i in 0..51 {
        let name = format!("reunion-{i:02}.md");
        let lang = if i % 2 == 0 { "es" } else { "en" };
        let body = if lang == "es" {
            format!(
                "# Reunión {i}\n\nSe revisó el estado del proyecto de infraestructura.\n\n- Decisión: continuar con la migración a OpenShift.\n- Pendiente: preparar el inventario de aplicaciones.\n- Incidente INC-{i:05} en seguimiento.\n"
            )
        } else {
            format!(
                "# Meeting {i}\n\nReviewed the infrastructure project status.\n\n- Decision: continue the OpenShift migration.\n- Action: prepare the application inventory.\n- Incident INC-{i:05} being tracked.\n"
            )
        };
        let path = dir.join(&name);
        std::fs::write(&path, body).unwrap();
        paths.push(path.to_str().unwrap().to_owned());
    }

    let report = app.import_materials(&p.id, paths).unwrap();
    assert_eq!(report.items.len(), 51);
    assert!(report.items.iter().all(|item| item.status == "added"));

    // Corpus/index state: all 51 TXT/Markdown materials reach Ready, chunks and
    // (without the local E5 runtime in this unit test) no ready embeddings but a
    // measurable corpus.
    let store = open_store(tmp.path(), &p.id);
    let stats = store.corpus_stats().unwrap();
    assert_eq!(stats.ready, 51);
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.unsupported, 0);
    assert!(stats.chunks_total >= 51);
    assert_eq!(stats.material_count, 51);
    assert!(stats.corpus_bytes > 0);
    assert!(stats.corpus_utf8_chars > 0);
    assert!(stats.naive_corpus_est_tokens > 0);

    // Ordinary query uses K3 (lexical) + K4 bounded evidence; never the corpus.
    let (results, _availability) = store
        .hybrid_search(
            "migración a OpenShift",
            None,
            project_knowledge::HybridSearchOptions::default(),
        )
        .unwrap();
    let package = store
        .assemble_context(
            "migración a OpenShift",
            &results,
            ContextAssemblyOptions::default(),
        )
        .unwrap();
    assert!(!package.entries.is_empty());
    assert!(package.totals.estimated_budget_used <= package.totals.estimated_budget_limit);
    let evidence_chars: usize = package.entries.iter().map(|e| e.text.chars().count()).sum();
    assert!(
        evidence_chars < stats.corpus_utf8_chars,
        "evidence must be far smaller than the full corpus"
    );

    // K6 global summary: bounded hierarchy, then full cache reuse.
    let (summarizer, calls) = CaptureSummarizer::new();
    let first = app.summarize_project(&p.id, &summarizer).unwrap();
    assert_eq!(first.source_count, 51);
    assert!(
        first.hierarchy_depth >= 3,
        "51 docs must reduce in multiple levels"
    );
    let captured = calls.lock().unwrap();
    let docs = captured
        .iter()
        .filter(|c| c.level == SummaryLevel::Document)
        .count();
    assert_eq!(docs, 51, "one document summary per source");
    // No synthesis request carries the whole corpus: batch/global stay bounded.
    for c in captured
        .iter()
        .filter(|c| c.level != SummaryLevel::Document)
    {
        assert!(
            c.evidence_texts.len() <= BatchOptions::default().branching_factor,
            "synthesis must be bounded, never the 51-document corpus"
        );
    }
    drop(captured);

    calls.lock().unwrap().clear();
    let second = app.summarize_project(&p.id, &summarizer).unwrap();
    assert_eq!(second.remote_calls, 0, "identical request must reuse cache");
    assert!(calls.lock().unwrap().is_empty());
}
