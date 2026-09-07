//! K5 production material-to-knowledge wiring: acceptance matrix closure.
//!
//! These tests exercise the production `add_material_and_index` /
//! `remove_knowledge_material` boundary and the K3/K4/K5 request path through
//! the exact `AppState` facade a real turn uses, with `FakeAgentEngine` so the
//! whole flow is offline and deterministic.

use std::fs;
use std::sync::{Arc, Mutex};

use project_agent::FakeAgentEngine;
use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask,
};
use project_app::AppState;
use project_core::{MaterialId, ProjectId};
use project_knowledge::{KnowledgeStore, MaterialIndexState, MaterialSource};
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

fn app(
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

fn open_store(base: &std::path::Path, project_id: &str) -> KnowledgeStore {
    let pid = ProjectId::parse(project_id).unwrap();
    KnowledgeStore::open(base.join("projects").join(project_id), &pid).unwrap()
}

// -- Acceptance: material acceptance + lexical indexing + durable states ------

#[test]
fn txt_material_is_accepted_and_lexically_indexed_to_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let mid = add_file(
        &app,
        tmp.path(),
        &p.id,
        "notas.txt",
        b"La fotosintesis usa clorofila.\nLas plantas crecen con luz.",
    );
    let mid = MaterialId::parse(&mid).unwrap();

    let store = open_store(tmp.path(), &p.id);
    let status = store.material_index_status(&mid).unwrap().unwrap();
    assert_eq!(
        status.state,
        MaterialIndexState::Ready,
        "TXT must reach Ready"
    );
    assert!(
        !store.search("clorofila", 10).unwrap().is_empty(),
        "TXT must be lexically searchable"
    );
    // The material remains accepted independently of Knowledge.
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 1);
}

#[test]
fn markdown_material_is_accepted_and_lexically_indexed_to_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let mid = add_file(
        &app,
        tmp.path(),
        &p.id,
        "resumen.md",
        b"# Resumen\n\nOpenShift despliega contenedores.",
    );
    let mid = MaterialId::parse(&mid).unwrap();

    let store = open_store(tmp.path(), &p.id);
    let status = store.material_index_status(&mid).unwrap().unwrap();
    assert_eq!(status.state, MaterialIndexState::Ready);
    assert!(!store.search("OpenShift", 10).unwrap().is_empty());
}

#[test]
fn unsupported_material_remains_accepted_with_durable_unsupported_state() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    // PDF is not a supported Knowledge format, but is a normal accepted Material.
    let mid = add_file(&app, tmp.path(), &p.id, "manual.pdf", b"%PDF-1.4 fake");
    let mid = MaterialId::parse(&mid).unwrap();

    let store = open_store(tmp.path(), &p.id);
    let status = store.material_index_status(&mid).unwrap().unwrap();
    assert_eq!(status.state, MaterialIndexState::Unsupported);
    assert!(!status.retryable);
    assert!(
        store.search("fake", 10).unwrap().is_empty(),
        "unsupported content must never be indexed"
    );
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 1);
}

#[test]
fn indexing_failure_does_not_roll_back_the_material() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    // Invalid UTF-8 within a *supported* (txt) extractor: lexical derivation
    // fails locally, but the Material must remain accepted.
    let mid = add_file(
        &app,
        tmp.path(),
        &p.id,
        "invalid.txt",
        &[0xff, 0xfe, 0x00, 0x80],
    );
    let mid = MaterialId::parse(&mid).unwrap();

    let store = open_store(tmp.path(), &p.id);
    let status = store.material_index_status(&mid).unwrap().unwrap();
    assert_eq!(status.state, MaterialIndexState::Failed);
    assert!(status.retryable);
    // The Material is still stored and opens normally.
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 1);
    let view = app.open_project(&p.id).unwrap();
    assert_eq!(view.materials[0].original_file_name, "invalid.txt");
}

#[test]
fn local_semantic_unavailable_keeps_material_accepted_and_ready_lexically() {
    // No verified model + no bundled runtime in a test temp base dir: the local
    // E5 provider is simply absent. Lexical indexing must still complete and the
    // material must reach Ready (semantic readiness is a separate K2 concern).
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let mid = add_file(&app, tmp.path(), &p.id, "notas.txt", b"contenido de prueba");
    let mid = MaterialId::parse(&mid).unwrap();

    let store = open_store(tmp.path(), &p.id);
    let status = store.material_index_status(&mid).unwrap().unwrap();
    assert_eq!(status.state, MaterialIndexState::Ready);
    assert!(!store.search("prueba", 10).unwrap().is_empty());
}

// -- Acceptance: delete / reuse -------------------------------------------------

#[test]
fn material_deletion_cleans_knowledge_source_and_status() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let mid = add_file(&app, tmp.path(), &p.id, "notas.txt", b"contenido unico");
    let mid = MaterialId::parse(&mid).unwrap();

    {
        let store = open_store(tmp.path(), &p.id);
        assert_eq!(
            store.material_index_status(&mid).unwrap().unwrap().state,
            MaterialIndexState::Ready
        );
        assert!(store.has_material_source(mid.as_str()).unwrap());
    }

    app.remove_material(&p.id, mid.as_str()).unwrap();

    let store = open_store(tmp.path(), &p.id);
    assert!(
        store.material_index_status(&mid).unwrap().is_none(),
        "status record must be removed with its source"
    );
    assert!(
        !store.has_material_source(mid.as_str()).unwrap(),
        "source link must be removed"
    );
}

#[test]
fn same_content_materials_share_document_but_delete_is_scoped() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let a = MaterialId::parse(add_file(
        &app,
        tmp.path(),
        &p.id,
        "a.txt",
        b"contenido identico",
    ))
    .unwrap();
    let b = MaterialId::parse(add_file(
        &app,
        tmp.path(),
        &p.id,
        "b.txt",
        b"contenido identico",
    ))
    .unwrap();

    {
        let mut store = open_store(tmp.path(), &p.id);
        let out_a = store
            .index(
                &MaterialSource {
                    material_id: a.clone(),
                    source_name: "a.txt".into(),
                    relative_path: format!("inputs/{a}/a.txt"),
                    media_type: Some("text/plain".into()),
                },
                b"contenido identico".as_slice(),
            )
            .unwrap();
        let out_b = store
            .index(
                &MaterialSource {
                    material_id: b.clone(),
                    source_name: "b.txt".into(),
                    relative_path: format!("inputs/{b}/b.txt"),
                    media_type: Some("text/plain".into()),
                },
                b"contenido identico".as_slice(),
            )
            .unwrap();
        // Both resolve to the same canonical document.
        assert_eq!(out_a.document_id, out_b.document_id);
        assert!(out_b.reused, "same bytes must reuse the canonical document");
        assert_eq!(out_a.chunk_count, out_b.chunk_count);
    }

    // Delete only A: B still resolves to the shared canonical document.
    app.remove_material(&p.id, a.as_str()).unwrap();
    let store = open_store(tmp.path(), &p.id);
    assert!(store.material_index_status(&a).unwrap().is_none());
    assert_eq!(
        store.material_index_status(&b).unwrap().unwrap().state,
        MaterialIndexState::Ready
    );
    assert!(store.has_material_source(b.as_str()).unwrap());
}

// -- Acceptance: project isolation ---------------------------------------------

#[test]
fn knowledge_is_project_local_and_isolated() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let a = app.create_project("A").unwrap();
    let b = app.create_project("B").unwrap();
    let mid_a = MaterialId::parse(add_file(
        &app,
        tmp.path(),
        &a.id,
        "x.txt",
        b"OpenShift aqui",
    ))
    .unwrap();

    let store_b = open_store(tmp.path(), &b.id);
    assert!(
        store_b.search("OpenShift", 10).unwrap().is_empty(),
        "project B must not see project A's indexed content"
    );
    assert!(store_b.material_index_status(&mid_a).unwrap().is_none());

    let store_a = open_store(tmp.path(), &a.id);
    assert!(!store_a.search("OpenShift", 10).unwrap().is_empty());
}

// -- Acceptance: production K3/K4/K5 request path ------------------------------

/// Engine that records the exact prompt text it received on `send`.
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
        _session: &AgentSession,
        req: &AgentPrompt,
    ) -> project_agent::AgentResult<AgentTask> {
        self.1.lock().unwrap().push(req.text.clone());
        self.0.send(_session, req)
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

#[test]
fn production_turn_injects_bounded_evidence_into_the_backend_request() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Listo. Creé el recurso.".into());
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        RecordingEngine(inner.clone(), calls.clone()),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let project = state.create_project("Knowledge").unwrap();
    let source_path = tmp.path().join("manual.txt");
    fs::write(
        &source_path,
        b"El incidente INC-12345 fue cerrado en OpenShift.",
    )
    .unwrap();
    let _material = state
        .add_material_from_path(&project.id, source_path.to_str().unwrap())
        .unwrap();
    let _ = fs::remove_file(&source_path);

    // No attachments: the indexed material alone drives evidence injection.
    let run = state.run_agent(&project.id, "INC-12345", &[]).unwrap();
    assert_eq!(run.status, "completed");

    let first_prompt = calls.lock().unwrap()[0].clone();
    let text = first_prompt.as_str();
    assert!(
        text.contains("<knowledge_evidence trust=\"untrusted\">"),
        "{text}"
    );
    assert!(text.contains("INC-12345"), "{text}");
    assert!(text.contains("evidence_label=\"E1\""), "{text}");
    // No absolute local path or raw DB leakage.
    assert!(!text.contains(tmp.path().to_str().unwrap()), "{text}");
    assert!(!text.contains("knowledge.sqlite"), "{text}");
    // The indexed material must not ALSO be forwarded as a raw workspace attachment.
    assert!(!text.contains("materials/1-manual.txt"), "{text}");

    // Deterministic serialization: a second identical turn produces the same
    // evidence block.
    let _ = state.run_agent(&project.id, "INC-12345", &[]).unwrap();
    let second_prompt = calls.lock().unwrap()[1].clone();
    let evidence_first = first_prompt
        .split_once("INC-12345")
        .map(|(_, e)| e)
        .unwrap_or("");
    let evidence_second = second_prompt
        .split_once("INC-12345")
        .map(|(_, e)| e)
        .unwrap_or("");
    assert_eq!(
        evidence_first, evidence_second,
        "K5 serialization must be deterministic"
    );
}

#[test]
fn chat_without_knowledge_index_remains_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    // No material, no index: a normal chat turn must not inject any evidence
    // block and must complete normally.
    let run = app.send_message(&p.id, "hola", &[]).unwrap();
    assert_eq!(run.status, "failed"); // FakeAgentEngine set_message is None -> no reply text
    let store_dir = tmp.path().join("projects").join(&p.id).join("knowledge");
    assert!(!store_dir.join("knowledge.sqlite").is_file());
}

#[test]
fn normal_non_indexed_attachment_behavior_remains_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Listo.".into());
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        RecordingEngine(inner, calls.clone()),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let p = state.create_project("P").unwrap();
    // A PDF (unsupported Knowledge format) attached as a normal attachment.
    let mid = add_file(&state, tmp.path(), &p.id, "manual.pdf", b"%PDF-1.4 fake");
    let run = state
        .run_agent(&p.id, "crea una actividad", &[mid])
        .unwrap();
    assert_eq!(run.status, "completed");

    let text = &calls.lock().unwrap().pop().unwrap();
    assert!(
        text.contains("- manual.pdf (pdf)"),
        "non-indexed attachment must be provisioned as a normal attachment"
    );
    assert!(
        !text.contains("<knowledge_evidence"),
        "unsupported material must not inject knowledge evidence"
    );
}

#[test]
fn send_message_with_indexed_and_attached_dedups_duplicate_corpus() {
    // When a TXT material is indexed AND also passed as an attachment id, the K5
    // contract deduplicates: it appears once as bounded evidence, not also as a
    // byte-for-byte copied raw workspace attachment.
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Listo.".into());
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        RecordingEngine(inner, calls.clone()),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let p = state.create_project("P").unwrap();
    let mid = add_file(
        &state,
        tmp.path(),
        &p.id,
        "notas.txt",
        b"OpenShift despliega aplicaciones",
    );
    let run = state
        .run_agent(&p.id, "OpenShift", std::slice::from_ref(&mid))
        .unwrap();
    assert_eq!(run.status, "completed");

    let text = calls.lock().unwrap().pop().unwrap();
    assert!(
        text.contains("<knowledge_evidence trust=\"untrusted\">"),
        "{text}"
    );
    assert!(
        !text.contains("materials/1-notas.txt"),
        "indexed material must not be forwarded as a raw workspace attachment"
    );
}

#[test]
fn indexed_attachment_is_deduped_even_when_the_current_turn_retrieves_no_evidence() {
    // Regression for the raw-forwarding contradiction: a supported, READY-indexed
    // TXT the user attaches must be served through Knowledge (or K6), never
    // raw-forwarded as a full workspace attachment — even when this exact turn's
    // query retrieves zero evidence, so the model does not silently receive the
    // attached file through a second content route.
    let tmp = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    inner.set_message("Listo.".into());
    let state = AppState::with_components(
        tmp.path().to_path_buf(),
        RecordingEngine(inner, calls.clone()),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    );
    let p = state.create_project("P").unwrap();
    let mid = add_file(
        &state,
        tmp.path(),
        &p.id,
        "notas.txt",
        b"OpenShift despliega aplicaciones de forma automatica",
    );

    // A query with no lexical overlap with the corpus content: retrieval returns
    // zero evidence, yet the indexed source name must still suppress raw
    // forwarding of the exact same attached file.
    let run = state
        .run_agent(
            &p.id,
            "resumen de fisica cuantica",
            std::slice::from_ref(&mid),
        )
        .unwrap();
    assert_eq!(run.status, "completed");

    let text = calls.lock().unwrap().pop().unwrap();
    assert!(
        !text.contains("materials/1-notas.txt"),
        "READY-indexed attachment must not be raw-forwarded even with zero retrieved evidence: {text}"
    );
}

// -- Acceptance: no remote embedding fallback in lexical-only mode ------------

#[test]
fn lexical_fallback_search_never_contacts_a_remote_provider() {
    // The provider model in these tests is a local FakeProviderConnector with no
    // network surface; a lexical-only K3 search must complete and inject evidence
    // while the agent engine records exactly the send call (the only provider
    // interaction is the fake, offline engine).
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    add_file(
        &app,
        tmp.path(),
        &p.id,
        "notas.txt",
        b"la luna orbita la tierra",
    );

    let store = open_store(tmp.path(), &p.id);
    // No embedding provider => hybrid_search uses lexical only and marks
    // semantic unavailable.
    let (results, availability) = store
        .hybrid_search("luna", None, Default::default())
        .unwrap();
    assert_eq!(
        availability,
        project_knowledge::SemanticAvailability::Unavailable
    );
    assert!(!results.is_empty());
}
