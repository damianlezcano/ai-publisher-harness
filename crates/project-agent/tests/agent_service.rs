use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use project_agent::model::{
    AgentBackendInfo, AgentKnowledgeContext, AgentProject, AgentPrompt, AgentSession, AgentStatus,
    AgentTask, Artifact, ArtifactKind, TaskStatus,
};
use project_agent::{
    AgentEngine, AgentError, AgentRequest, AgentService, CreationRegistrar, FakeAgentEngine,
    FakeCall, knowledge_answer_grounding_instruction,
};

#[derive(Clone, Debug)]
struct RecordedCreation {
    kind: ArtifactKind,
    visibility: &'static str,
    display_name: String,
    file_name: String,
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct FakeRegistrar {
    next_id: Arc<AtomicU32>,
    records: Arc<Mutex<Vec<RecordedCreation>>>,
    fail: Arc<Mutex<bool>>,
}

impl FakeRegistrar {
    fn new() -> Self {
        Self {
            next_id: Arc::new(AtomicU32::new(1)),
            records: Arc::new(Mutex::new(Vec::new())),
            fail: Arc::new(Mutex::new(false)),
        }
    }

    fn fail_next(&self) {
        *self.fail.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }

    fn records(&self) -> Vec<RecordedCreation> {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl CreationRegistrar for FakeRegistrar {
    fn register_turn(
        &self,
        _project_id: &str,
        artifacts: &[project_agent::RegisteredArtifact],
    ) -> project_agent::AgentResult<Vec<String>> {
        if *self.fail.lock().unwrap_or_else(|e| e.into_inner()) {
            return Err(AgentError::RegistrationFailed("injected".into()));
        }
        // Mirror the real registrar's grouping contract at the service
        // boundary: changed assets in the same workspace directory as a web
        // entry belong to that web lineage and are not separate Creations.
        let web_dir = artifacts
            .iter()
            .find(|a| a.kind == ArtifactKind::Web)
            .and_then(|a| std::path::Path::new(&a.path).parent())
            .map(|p| p.to_path_buf());
        let mut ids = Vec::new();
        for artifact in artifacts {
            let in_web_dir = web_dir.as_deref().is_some_and(|dir| {
                std::path::Path::new(&artifact.path)
                    .parent()
                    .is_some_and(|p| p == dir)
            });
            let standalone_doc = matches!(
                artifact.kind,
                ArtifactKind::Document
                    | ArtifactKind::Spreadsheet
                    | ArtifactKind::Presentation
                    | ArtifactKind::Pdf
                    | ArtifactKind::Text
            );
            if in_web_dir && !standalone_doc && artifact.kind != ArtifactKind::Web {
                continue;
            }
            let file_name = std::path::Path::new(&artifact.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_owned();
            let display_name = std::path::Path::new(&file_name)
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or(&file_name)
                .to_owned();
            self.records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(RecordedCreation {
                    kind: artifact.kind,
                    visibility: "private",
                    display_name,
                    file_name,
                    bytes: artifact.bytes.clone(),
                });
            let n = self.next_id.fetch_add(1, Ordering::SeqCst);
            ids.push(format!("creation-{n}"));
        }
        Ok(ids)
    }
}

struct SlowEngine {
    inner: FakeAgentEngine,
    inflight: Mutex<u32>,
    max_inflight: Arc<Mutex<u32>>,
}

impl SlowEngine {
    fn new(inner: FakeAgentEngine, max_inflight: Arc<Mutex<u32>>) -> Self {
        Self {
            inner,
            inflight: Mutex::new(0),
            max_inflight,
        }
    }
}

impl AgentEngine for SlowEngine {
    fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
        self.inner.ensure_ready()
    }

    fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
        self.inner.open_session(project)
    }

    fn invalidate_cached_session(&self, project_id: &str) {
        self.inner.invalidate_cached_session(project_id);
    }

    fn send(
        &self,
        session: &AgentSession,
        req: &AgentPrompt,
    ) -> project_agent::AgentResult<AgentTask> {
        {
            let mut n = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            *n += 1;
            let mut max = self.max_inflight.lock().unwrap_or_else(|e| e.into_inner());
            if *n > *max {
                *max = *n;
            }
        }
        thread::sleep(Duration::from_millis(80));
        let result = self.inner.send(session, req);
        *self.inflight.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        result
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

fn artifact(path: &str, kind: ArtifactKind, bytes_hint: u64) -> Artifact {
    Artifact {
        path: path.to_owned(),
        kind,
        byte_size: bytes_hint,
        sha256: None,
    }
}

fn prompt() -> AgentPrompt {
    AgentPrompt {
        text: "create an activity".into(),
        model: None,
        knowledge: None,
        conversation_context: None,
    }
}

fn write_artifact(base: &std::path::Path, project_id: &str, rel: &str, bytes: &[u8]) -> PathBuf {
    let path = base
        .join("projects")
        .join(project_id)
        .join("workspace")
        .join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("workspace dirs");
    }
    fs::write(&path, bytes).expect("write artifact");
    path
}

#[test]
fn run_registers_scripted_artifacts_as_private() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![
        artifact("workspace/actividad/index.html", ArtifactKind::Web, 4),
        artifact("workspace/guia.pdf", ArtifactKind::Pdf, 3),
    ]);
    write_artifact(tmp.path(), "proj-7", "actividad/index.html", b"<h1>");
    write_artifact(tmp.path(), "proj-7", "guia.pdf", b"pdf");
    let registrar = FakeRegistrar::new();
    let service = AgentService::new(engine, registrar.clone(), tmp.path().to_path_buf());
    let result = service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: Vec::new(),
        })
        .expect("run");
    assert_eq!(result.registered, vec!["creation-1", "creation-2"]);
    assert_eq!(result.task.status, TaskStatus::Completed);
    let records = registrar.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, ArtifactKind::Web);
    assert_eq!(records[0].visibility, "private");
    assert_eq!(records[0].display_name, "index");
    assert_eq!(records[0].file_name, "index.html");
    assert_eq!(records[0].bytes, b"<h1>");
    assert_eq!(records[1].kind, ArtifactKind::Pdf);
    assert_eq!(records[1].visibility, "private");
    assert_eq!(records[1].bytes, b"pdf");
}

#[test]
fn same_project_runs_are_serialized() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fake = FakeAgentEngine::new();
    fake.set_artifacts(vec![]);
    let max_inflight = Arc::new(Mutex::new(0));
    let engine = SlowEngine::new(fake, Arc::clone(&max_inflight));
    let registrar = FakeRegistrar::new();
    let service = Arc::new(AgentService::new(
        engine,
        registrar,
        tmp.path().to_path_buf(),
    ));
    let start = Instant::now();
    let a = {
        let service = Arc::clone(&service);
        thread::spawn(move || {
            service.run(AgentRequest {
                project_id: "proj-7".into(),
                prompt: prompt(),
                attachments: Vec::new(),
            })
        })
    };
    let b = {
        let service = Arc::clone(&service);
        thread::spawn(move || {
            service.run(AgentRequest {
                project_id: "proj-7".into(),
                prompt: prompt(),
                attachments: Vec::new(),
            })
        })
    };
    a.join().expect("join a").expect("run a");
    b.join().expect("join b").expect("run b");
    assert!(
        start.elapsed() >= Duration::from_millis(140),
        "runs should not fully overlap"
    );
    assert_eq!(*max_inflight.lock().unwrap_or_else(|e| e.into_inner()), 1);
}

#[test]
fn traversal_artifact_path_is_rejected_and_not_registered() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![artifact(
        "workspace/../secret.txt",
        ArtifactKind::Text,
        1,
    )]);
    let secret = tmp
        .path()
        .join("projects")
        .join("proj-7")
        .join("secret.txt");
    fs::create_dir_all(secret.parent().unwrap()).expect("dirs");
    fs::write(&secret, b"nope").expect("secret");
    let registrar = FakeRegistrar::new();
    let service = AgentService::new(engine, registrar.clone(), tmp.path().to_path_buf());
    let err = match service.run(AgentRequest {
        project_id: "proj-7".into(),
        prompt: prompt(),
        attachments: Vec::new(),
    }) {
        Err(err) => err,
        Ok(_) => panic!("traversal must fail"),
    };
    assert!(matches!(err, AgentError::RegistrationFailed(_)), "{err:?}");
    assert!(registrar.records().is_empty());
}

#[test]
fn cancel_calls_engine_cancel_for_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![]);
    let calls = engine.clone();
    let service = AgentService::new(engine, FakeRegistrar::new(), tmp.path().to_path_buf());
    service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: Vec::new(),
        })
        .expect("run");
    service.cancel("proj-7").expect("cancel");
    assert!(calls.calls().contains(&FakeCall::Cancel));
}

#[test]
fn run_lazily_ensures_ready() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![]);
    let calls = engine.clone();
    let service = AgentService::new(engine, FakeRegistrar::new(), tmp.path().to_path_buf());
    assert_eq!(service.engine_status(), AgentStatus::Stopped);
    service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: Vec::new(),
        })
        .expect("run");
    assert_eq!(calls.calls()[0], FakeCall::Ready);
    assert_eq!(service.engine_status(), AgentStatus::Ready);
}

#[test]
fn failing_registrar_leaves_workspace_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![artifact("workspace/notes.txt", ArtifactKind::Text, 5)]);
    let file = write_artifact(tmp.path(), "proj-7", "notes.txt", b"hello");
    let registrar = FakeRegistrar::new();
    registrar.fail_next();
    let service = AgentService::new(engine, registrar, tmp.path().to_path_buf());
    let err = match service.run(AgentRequest {
        project_id: "proj-7".into(),
        prompt: prompt(),
        attachments: Vec::new(),
    }) {
        Err(err) => err,
        Ok(_) => panic!("expected registration failure"),
    };
    assert!(matches!(err, AgentError::RegistrationFailed(_)), "{err:?}");
    assert!(file.is_file(), "workspace artifact must remain");
    assert_eq!(fs::read(&file).expect("read"), b"hello");
}

#[test]
fn workspace_scan_does_not_register_preexisting_files_when_diff_is_empty() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![]);
    write_artifact(tmp.path(), "proj-7", "index.html", b"<h1>juego</h1>");
    let registrar = FakeRegistrar::new();
    let service = AgentService::new(engine, registrar.clone(), tmp.path().to_path_buf());
    let result = service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: Vec::new(),
        })
        .expect("run");
    assert!(result.registered.is_empty());
    let records = registrar.records();
    assert!(records.is_empty());
}

#[test]
fn web_sidecar_assets_are_not_separate_creations() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![
        artifact("workspace/index.html", ArtifactKind::Web, 4),
        artifact("workspace/app.js", ArtifactKind::Other, 2),
        artifact("workspace/guia.pdf", ArtifactKind::Pdf, 3),
    ]);
    write_artifact(tmp.path(), "proj-7", "index.html", b"<h1>");
    write_artifact(tmp.path(), "proj-7", "app.js", b"{}");
    write_artifact(tmp.path(), "proj-7", "guia.pdf", b"pdf");
    let registrar = FakeRegistrar::new();
    let service = AgentService::new(engine, registrar.clone(), tmp.path().to_path_buf());
    let result = service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: Vec::new(),
        })
        .expect("run");
    assert_eq!(result.registered.len(), 2);
    let records = registrar.records();
    assert_eq!(records.len(), 2);
    let kinds: Vec<_> = records.iter().map(|r| r.kind).collect();
    assert!(kinds.contains(&ArtifactKind::Web));
    assert!(kinds.contains(&ArtifactKind::Pdf));
    assert!(!kinds.contains(&ArtifactKind::Other));
}

#[test]
fn later_turn_does_not_reregister_prior_workspace_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web, 4)]);
    write_artifact(tmp.path(), "proj-7", "index.html", b"<h1>one</h1>");
    let registrar = FakeRegistrar::new();
    let service = AgentService::new(engine.clone(), registrar.clone(), tmp.path().to_path_buf());
    let first = service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: Vec::new(),
        })
        .expect("turn 1");
    assert_eq!(first.registered.len(), 1);

    engine.set_artifacts(vec![artifact(
        "workspace/actividad-2/index.html",
        ArtifactKind::Web,
        4,
    )]);
    write_artifact(
        tmp.path(),
        "proj-7",
        "actividad-2/index.html",
        b"<h1>two</h1>",
    );
    let second = service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: Vec::new(),
        })
        .expect("turn 2");
    assert_eq!(second.registered.len(), 1);
    assert_eq!(registrar.records().len(), 2);
}

#[test]
fn later_turn_prompt_asks_to_revise_existing_web() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web, 4)]);
    write_artifact(tmp.path(), "proj-8", "index.html", b"<h1>one</h1>");
    let registrar = FakeRegistrar::new();
    let service = AgentService::new(engine.clone(), registrar, tmp.path().to_path_buf());
    service
        .run(AgentRequest {
            project_id: "proj-8".into(),
            prompt: prompt(),
            attachments: Vec::new(),
        })
        .expect("turn 1");
    engine.set_artifacts(vec![artifact("workspace/index.html", ArtifactKind::Web, 4)]);
    write_artifact(tmp.path(), "proj-8", "index.html", b"<h1>two</h1>");
    service
        .run(AgentRequest {
            project_id: "proj-8".into(),
            prompt: prompt(),
            attachments: Vec::new(),
        })
        .expect("turn 2");
    let text = engine.last_prompt_text().expect("prompt");
    assert!(text.contains("modificá ESA misma actividad"), "{text}");
}

/// Models the REAL sidecar behavior for Finding A: the agent edits the existing
/// `index.html` in place and copies the attached image into the workspace, and
/// `/diff` reports NOTHING (empty) for the committed in-place edit.
struct ModifyDuringSendEngine {
    inner: FakeAgentEngine,
    workspace: PathBuf,
    material_bytes: Vec<u8>,
    updated_html: Vec<u8>,
}

impl AgentEngine for ModifyDuringSendEngine {
    fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
        self.inner.ensure_ready()
    }
    fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
        self.inner.open_session(project)
    }
    fn invalidate_cached_session(&self, project_id: &str) {
        self.inner.invalidate_cached_session(project_id);
    }
    fn send(
        &self,
        session: &AgentSession,
        req: &AgentPrompt,
    ) -> project_agent::AgentResult<AgentTask> {
        fs::write(self.workspace.join("encabezado.png"), &self.material_bytes)
            .expect("agent copies the attached image into the workspace");
        fs::write(self.workspace.join("index.html"), &self.updated_html)
            .expect("agent edits the existing activity in place");
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
fn attached_image_copy_is_input_not_a_creation_and_in_place_update_is_registered() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let png: Vec<u8> = b"\x89PNG\r\n\x1a\nuser-uploaded-image-bytes".to_vec();

    // Existing Creation C1 from a previous turn: index.html already in the
    // workspace, and the user's material already imported under inputs/.
    write_artifact(tmp.path(), "proj-9", "index.html", b"<h1>ORIGINAL</h1>");
    let inputs_dir = tmp
        .path()
        .join("projects/proj-9/inputs/0198e4a6-79b2-7b51-9e68-c2eb7af3db15");
    fs::create_dir_all(&inputs_dir).expect("inputs dir");
    fs::write(inputs_dir.join("images.png"), &png).expect("material");

    let inner = FakeAgentEngine::new();
    inner.set_artifacts(vec![]); // /diff is empty for committed in-place edits
    let engine = ModifyDuringSendEngine {
        inner,
        workspace: tmp.path().join("projects/proj-9/workspace"),
        material_bytes: png.clone(),
        updated_html: b"<h1>UPDATED</h1><img src=\"encabezado.png\">".to_vec(),
    };
    let registrar = FakeRegistrar::new();
    let service = AgentService::new(engine, registrar.clone(), tmp.path().to_path_buf());
    let result = service
        .run(AgentRequest {
            project_id: "proj-9".into(),
            prompt: prompt(),
            attachments: vec![project_agent::AgentAttachment {
                display_name: "images.png".into(),
                kind: "image".into(),
                bytes: png.clone(),
            }],
        })
        .expect("turn 2");

    // C1 is re-registered with the NEW content (in-place update detected).
    let records = registrar.records();
    assert_eq!(
        records.len(),
        1,
        "only the web creation, never a phantom Image"
    );
    assert_eq!(records[0].kind, ArtifactKind::Web);
    assert_eq!(records[0].file_name, "index.html");
    assert_eq!(
        records[0].bytes,
        b"<h1>UPDATED</h1><img src=\"encabezado.png\">"
    );
    assert_eq!(result.registered.len(), 1);
}

#[test]
fn attached_image_copy_reported_by_diff_is_still_input_not_a_creation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let png: Vec<u8> = b"\x89PNG\r\n\x1a\nuser-uploaded-image-bytes".to_vec();

    write_artifact(tmp.path(), "proj-10", "index.html", b"<h1>ORIGINAL</h1>");
    let inputs_dir = tmp
        .path()
        .join("projects/proj-10/inputs/0198e4a6-79b2-7b51-9e68-c2eb7af3db15");
    fs::create_dir_all(&inputs_dir).expect("inputs dir");
    fs::write(inputs_dir.join("images.png"), &png).expect("material");

    // Here `/diff` DOES report the copied image (the variant where the sidecar
    // surfaces the new file): the provenance filter must still classify the
    // byte-identical copy as INPUT and never register it as a Creation.
    let inner = FakeAgentEngine::new();
    inner.set_artifacts(vec![artifact(
        "workspace/encabezado.png",
        ArtifactKind::Image,
        png.len() as u64,
    )]);
    let engine = ModifyDuringSendEngine {
        inner,
        workspace: tmp.path().join("projects/proj-10/workspace"),
        material_bytes: png.clone(),
        updated_html: b"<h1>UPDATED</h1><img src=\"encabezado.png\">".to_vec(),
    };
    let registrar = FakeRegistrar::new();
    let service = AgentService::new(engine, registrar.clone(), tmp.path().to_path_buf());
    let result = service
        .run(AgentRequest {
            project_id: "proj-10".into(),
            prompt: prompt(),
            attachments: vec![project_agent::AgentAttachment {
                display_name: "images.png".into(),
                kind: "image".into(),
                bytes: png,
            }],
        })
        .expect("turn 2");

    let records = registrar.records();
    assert_eq!(
        records.len(),
        1,
        "no standalone Image creation for the input copy"
    );
    assert_eq!(records[0].kind, ArtifactKind::Web);
    assert_eq!(result.registered.len(), 1);
}

#[derive(Clone)]
struct SessionKindEngine {
    inner: FakeAgentEngine,
    kinds: Arc<Mutex<Vec<&'static str>>>,
}

impl AgentEngine for SessionKindEngine {
    fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
        self.inner.ensure_ready()
    }

    fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
        self.kinds.lock().unwrap().push("conversational");
        self.inner.open_session(project)
    }

    fn invalidate_cached_session(&self, project_id: &str) {
        self.inner.invalidate_cached_session(project_id);
    }

    fn open_fresh_session(
        &self,
        project: &AgentProject,
    ) -> project_agent::AgentResult<AgentSession> {
        self.kinds.lock().unwrap().push("ephemeral");
        Ok(AgentSession {
            id: format!("fresh-{}", project.project_id),
            project_id: project.project_id.clone(),
        })
    }

    fn send(
        &self,
        session: &AgentSession,
        req: &AgentPrompt,
    ) -> project_agent::AgentResult<AgentTask> {
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

fn knowledge_prompt(mode: Option<&str>) -> AgentPrompt {
    AgentPrompt {
        text: "pregunta".into(),
        model: None,
        knowledge: mode.map(|mode| AgentKnowledgeContext {
            entries: Vec::new(),
            evidence_budget_used: 0,
            evidence_budget_limit: 100,
            indexed_source_names: Vec::new(),
            citation_map: Vec::new(),
            retrieval_mode: Some(mode.to_owned()),
            exhaustive_coverage: Some("not_requested".to_owned()),
            structural_note: None,
            local_answer: None,
            authorize_negative: false,
            citation_source_names: Vec::new(),
            creation_from_material: false,
        }),
        conversation_context: None,
    }
}

fn creation_evidence_prompt() -> AgentPrompt {
    AgentPrompt {
        text: "armá una presentación interactiva".into(),
        model: None,
        knowledge: Some(AgentKnowledgeContext {
            entries: vec![project_agent::model::AgentKnowledgeEntry {
                label: "E1".to_owned(),
                source_label: "S1".to_owned(),
                source_name: "README.md".to_owned(),
                chunk_label: "C1".to_owned(),
                line_start: None,
                line_end: None,
                heading_path: Vec::new(),
                text: "contenido del material".to_owned(),
                source_id: Some("material-1".to_owned()),
                evidence_kind: Some("creation_material".to_owned()),
            }],
            evidence_budget_used: 40,
            evidence_budget_limit: 1200,
            indexed_source_names: vec!["README.md".to_owned()],
            citation_map: Vec::new(),
            retrieval_mode: None,
            exhaustive_coverage: Some("not_requested".to_owned()),
            structural_note: Some("creation_from_material=true".to_owned()),
            local_answer: None,
            authorize_negative: false,
            citation_source_names: vec!["README.md".to_owned()],
            creation_from_material: true,
        }),
        conversation_context: None,
    }
}

fn populated_knowledge_prompt(mode: &str) -> AgentPrompt {
    let mut prompt = creation_evidence_prompt();
    if let Some(knowledge) = prompt.knowledge.as_mut() {
        knowledge.retrieval_mode = Some(mode.to_owned());
        knowledge.creation_from_material = false;
        knowledge.structural_note = Some(format!("retrieval_mode={mode} coverage=complete"));
        knowledge.authorize_negative = false;
    }
    prompt.text = match mode {
        "exhaustive" => "¿Qué archivos mencionan preposiciones?".into(),
        "thematic" => {
            "¿Cuáles son los temas principales que se repiten entre estos archivos?".into()
        }
        _ => "¿Qué se decidió?".into(),
    };
    prompt
}

#[test]
fn knowledge_answer_grounding_is_shared_and_ordinary_chat_stays_ungrounded() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    let service = AgentService::new(
        engine.clone(),
        FakeRegistrar::new(),
        tmp.path().to_path_buf(),
    );
    for mode in ["normal", "thematic", "exhaustive"] {
        service
            .run(AgentRequest {
                project_id: format!("proj-{mode}"),
                prompt: populated_knowledge_prompt(mode),
                attachments: Vec::new(),
            })
            .expect(mode);
        let prompt = engine.last_prompt_text().expect(mode);
        assert!(
            prompt.contains(knowledge_answer_grounding_instruction()),
            "{mode} must carry the shared Knowledge grounding"
        );
        assert!(prompt.contains("<knowledge_evidence trust=\"untrusted\">"));
    }
}

#[test]
fn knowledge_answer_is_one_send_per_turn() {
    for mode in ["normal", "thematic", "exhaustive"] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let engine = FakeAgentEngine::new();
        let service = AgentService::new(
            engine.clone(),
            FakeRegistrar::new(),
            tmp.path().to_path_buf(),
        );
        service
            .run(AgentRequest {
                project_id: format!("proj-once-{mode}"),
                prompt: populated_knowledge_prompt(mode),
                attachments: Vec::new(),
            })
            .expect(mode);
        let sends = engine
            .calls()
            .into_iter()
            .filter(|call| *call == FakeCall::Send)
            .count();
        assert_eq!(
            sends, 1,
            "{mode} must not add a retry or extra provider send"
        );
    }
}

#[test]
fn ordinary_and_creation_prompts_do_not_use_knowledge_answer_grounding() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    let service = AgentService::new(
        engine.clone(),
        FakeRegistrar::new(),
        tmp.path().to_path_buf(),
    );
    service
        .run(AgentRequest {
            project_id: "proj-ordinary-grounding".into(),
            prompt: knowledge_prompt(None),
            attachments: Vec::new(),
        })
        .expect("ordinary");
    let ordinary = engine.last_prompt_text().expect("ordinary prompt");
    assert!(!ordinary.contains(knowledge_answer_grounding_instruction()));
    service
        .run(AgentRequest {
            project_id: "proj-creation-grounding".into(),
            prompt: creation_evidence_prompt(),
            attachments: Vec::new(),
        })
        .expect("creation");
    let creation = engine.last_prompt_text().expect("creation prompt");
    assert!(!creation.contains(knowledge_answer_grounding_instruction()));
    assert!(creation.contains("material already available in Knowledge"));
}

#[test]
fn knowledge_synthesis_opens_ephemeral_sessions_ordinary_chat_reuses_conversation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let kinds = Arc::new(Mutex::new(Vec::new()));
    let inner = FakeAgentEngine::new();
    let engine = SessionKindEngine {
        inner,
        kinds: Arc::clone(&kinds),
    };
    let service = AgentService::new(engine, FakeRegistrar::new(), tmp.path().to_path_buf());
    for mode in ["normal", "exhaustive", "thematic"] {
        service
            .run(AgentRequest {
                project_id: format!("proj-{mode}"),
                prompt: knowledge_prompt(Some(mode)),
                attachments: Vec::new(),
            })
            .expect(mode);
    }
    service
        .run(AgentRequest {
            project_id: "proj-chat".into(),
            prompt: knowledge_prompt(None),
            attachments: Vec::new(),
        })
        .expect("ordinary");
    assert_eq!(
        *kinds.lock().unwrap(),
        vec!["ephemeral", "ephemeral", "ephemeral", "conversational"]
    );
}

#[test]
fn ephemeral_send_failure_restores_conversational_cancel_target() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let inner = FakeAgentEngine::new();
    inner.set_artifacts(vec![]);
    let kinds = Arc::new(Mutex::new(Vec::new()));
    let engine = SessionKindEngine {
        inner: inner.clone(),
        kinds,
    };
    let service = AgentService::new(engine, FakeRegistrar::new(), tmp.path().to_path_buf());
    service
        .run(AgentRequest {
            project_id: "proj-restore".into(),
            prompt: knowledge_prompt(None),
            attachments: Vec::new(),
        })
        .expect("ordinary");
    assert_eq!(
        service.cancel_target_role("proj-restore"),
        Some("conversational")
    );
    inner.fail_send();
    let err = match service.run(AgentRequest {
        project_id: "proj-restore".into(),
        prompt: knowledge_prompt(Some("normal")),
        attachments: Vec::new(),
    }) {
        Err(err) => err,
        Ok(_) => panic!("ephemeral send must fail"),
    };
    assert!(matches!(err, AgentError::TaskFailed(_)));
    assert_eq!(
        service.cancel_target_role("proj-restore"),
        Some("conversational")
    );
    service
        .cancel("proj-restore")
        .expect("cancel conversational");
    assert_eq!(
        inner.cancelled_session_ids(),
        vec!["session-proj-restore".to_owned()]
    );
}

#[test]
fn cancel_during_ephemeral_send_targets_ephemeral_then_restores() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;

    let tmp = tempfile::tempdir().expect("tempdir");
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(Mutex::new(Vec::new()));
    let engine = HoldingEngine {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
        cancelled: Arc::clone(&cancelled),
    };
    let service = Arc::new(AgentService::new(
        engine,
        FakeRegistrar::new(),
        tmp.path().to_path_buf(),
    ));
    service
        .run(AgentRequest {
            project_id: "proj-hold".into(),
            prompt: knowledge_prompt(None),
            attachments: Vec::new(),
        })
        .expect("ordinary");
    let service_thread = Arc::clone(&service);
    let handle = thread::spawn(move || {
        service_thread.run(AgentRequest {
            project_id: "proj-hold".into(),
            prompt: knowledge_prompt(Some("normal")),
            attachments: Vec::new(),
        })
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !started.load(Ordering::SeqCst) {
        assert!(std::time::Instant::now() < deadline, "send did not start");
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        service.cancel_target_role("proj-hold"),
        Some("ephemeral_knowledge")
    );
    service.cancel("proj-hold").expect("cancel ephemeral");
    release.store(true, Ordering::SeqCst);
    handle.join().expect("join").expect("ephemeral completed");
    assert_eq!(
        *cancelled.lock().unwrap(),
        vec!["fresh-proj-hold".to_owned()]
    );
    assert_eq!(
        service.cancel_target_role("proj-hold"),
        Some("conversational")
    );
}

#[derive(Clone)]
struct HoldingEngine {
    started: Arc<std::sync::atomic::AtomicBool>,
    release: Arc<std::sync::atomic::AtomicBool>,
    cancelled: Arc<Mutex<Vec<String>>>,
}

impl AgentEngine for HoldingEngine {
    fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
        Ok(AgentBackendInfo {
            version: "hold".into(),
        })
    }

    fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
        Ok(AgentSession {
            id: format!("session-{}", project.project_id),
            project_id: project.project_id.clone(),
        })
    }

    fn open_fresh_session(
        &self,
        project: &AgentProject,
    ) -> project_agent::AgentResult<AgentSession> {
        Ok(AgentSession {
            id: format!("fresh-{}", project.project_id),
            project_id: project.project_id.clone(),
        })
    }

    fn send(
        &self,
        session: &AgentSession,
        _req: &AgentPrompt,
    ) -> project_agent::AgentResult<AgentTask> {
        if session.id.starts_with("fresh-") {
            self.started
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !self.release.load(std::sync::atomic::Ordering::SeqCst) {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        Ok(AgentTask {
            id: format!("{}-task", session.id),
            status: project_agent::TaskStatus::Completed,
            artifacts: Vec::new(),
            message: Some("ok".into()),
            usage: project_agent::RemoteUsage::default(),
        })
    }

    fn cancel(&self, session: &AgentSession) -> project_agent::AgentResult<()> {
        self.cancelled.lock().unwrap().push(session.id.clone());
        Ok(())
    }

    fn status(&self) -> AgentStatus {
        AgentStatus::Ready
    }

    fn shutdown(&self) -> project_agent::AgentResult<()> {
        Ok(())
    }
}

#[test]
fn creation_serialized_evidence_rotates_cached_session_before_ordinary_chat() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    let service = AgentService::new(
        engine.clone(),
        FakeRegistrar::new(),
        tmp.path().to_path_buf(),
    );
    service
        .run(AgentRequest {
            project_id: "proj-create".into(),
            prompt: creation_evidence_prompt(),
            attachments: Vec::new(),
        })
        .expect("creation");
    let creation_id = engine
        .sent_session_ids()
        .last()
        .cloned()
        .expect("creation session");
    assert_eq!(creation_id, "session-proj-create");
    let prompt = engine.last_prompt_text().expect("creation prompt");
    assert!(prompt.contains("<knowledge_evidence"));
    service
        .run(AgentRequest {
            project_id: "proj-create".into(),
            prompt: knowledge_prompt(None),
            attachments: Vec::new(),
        })
        .expect("ordinary");
    let ordinary_id = engine
        .sent_session_ids()
        .last()
        .cloned()
        .expect("ordinary session");
    assert_ne!(creation_id, ordinary_id);
    assert_eq!(ordinary_id, "session-proj-create-2");
    let ordinary_prompt = engine.last_prompt_text().expect("ordinary prompt");
    assert!(!ordinary_prompt.contains("<knowledge_evidence"));
}

#[test]
fn creation_serialized_evidence_failure_does_not_reuse_tainted_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.fail_send();
    let service = AgentService::new(
        engine.clone(),
        FakeRegistrar::new(),
        tmp.path().to_path_buf(),
    );
    assert!(
        service
            .run(AgentRequest {
                project_id: "proj-fail".into(),
                prompt: creation_evidence_prompt(),
                attachments: Vec::new(),
            })
            .is_err()
    );
    let tainted = engine
        .sent_session_ids()
        .last()
        .cloned()
        .expect("tainted session");
    service
        .run(AgentRequest {
            project_id: "proj-fail".into(),
            prompt: knowledge_prompt(None),
            attachments: Vec::new(),
        })
        .expect("ordinary after failure");
    let next = engine
        .sent_session_ids()
        .last()
        .cloned()
        .expect("next session");
    assert_ne!(tainted, next);
    assert!(
        !engine
            .last_prompt_text()
            .unwrap()
            .contains("<knowledge_evidence")
    );
}

#[test]
fn creation_without_serialized_evidence_keeps_conversational_reuse() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    let service = AgentService::new(
        engine.clone(),
        FakeRegistrar::new(),
        tmp.path().to_path_buf(),
    );
    service
        .run(AgentRequest {
            project_id: "proj-plain".into(),
            prompt: knowledge_prompt(None),
            attachments: Vec::new(),
        })
        .expect("first");
    let first = engine.sent_session_ids().last().cloned().unwrap();
    service
        .run(AgentRequest {
            project_id: "proj-plain".into(),
            prompt: knowledge_prompt(None),
            attachments: Vec::new(),
        })
        .expect("second");
    let second = engine.sent_session_ids().last().cloned().unwrap();
    assert_eq!(first, second);
}

#[derive(Clone)]
struct HoldingConversationalEngine {
    inner: FakeAgentEngine,
    started: Arc<std::sync::atomic::AtomicBool>,
    release: Arc<std::sync::atomic::AtomicBool>,
}

impl AgentEngine for HoldingConversationalEngine {
    fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
        self.inner.ensure_ready()
    }

    fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
        self.inner.open_session(project)
    }

    fn invalidate_cached_session(&self, project_id: &str) {
        self.inner.invalidate_cached_session(project_id);
    }

    fn send(
        &self,
        session: &AgentSession,
        req: &AgentPrompt,
    ) -> project_agent::AgentResult<AgentTask> {
        self.started
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !self.release.load(std::sync::atomic::Ordering::SeqCst) {
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
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
fn creation_serialized_evidence_cancel_does_not_reuse_tainted_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let inner = FakeAgentEngine::new();
    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let engine = HoldingConversationalEngine {
        inner: inner.clone(),
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    };
    let service = Arc::new(AgentService::new(
        engine,
        FakeRegistrar::new(),
        tmp.path().to_path_buf(),
    ));
    let handle = thread::spawn({
        let service = Arc::clone(&service);
        move || {
            service.run(AgentRequest {
                project_id: "proj-cancel".into(),
                prompt: creation_evidence_prompt(),
                attachments: Vec::new(),
            })
        }
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !started.load(Ordering::SeqCst) {
        if Instant::now() >= deadline {
            panic!("send never started");
        }
        thread::sleep(Duration::from_millis(5));
    }
    let _ = service.cancel("proj-cancel");
    release.store(true, Ordering::SeqCst);
    let _ = handle.join().expect("join");
    let tainted = inner
        .sent_session_ids()
        .first()
        .cloned()
        .expect("tainted session");
    service
        .run(AgentRequest {
            project_id: "proj-cancel".into(),
            prompt: knowledge_prompt(None),
            attachments: Vec::new(),
        })
        .expect("ordinary after cancel/send");
    let next = inner.sent_session_ids().last().cloned().unwrap();
    assert_ne!(tainted, next);
}
