use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask, Artifact,
    ArtifactKind, TaskStatus,
};
use project_agent::{
    AgentEngine, AgentError, AgentRequest, AgentService, CreationRegistrar, FakeAgentEngine,
    FakeCall,
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
    fn register(
        &self,
        _project_id: &str,
        artifact: &Artifact,
        bytes: Vec<u8>,
    ) -> project_agent::AgentResult<String> {
        if *self.fail.lock().unwrap_or_else(|e| e.into_inner()) {
            return Err(AgentError::RegistrationFailed("injected".into()));
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
                bytes,
            });
        let n = self.next_id.fetch_add(1, Ordering::SeqCst);
        Ok(format!("creation-{n}"))
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
            })
        })
    };
    let b = {
        let service = Arc::clone(&service);
        thread::spawn(move || {
            service.run(AgentRequest {
                project_id: "proj-7".into(),
                prompt: prompt(),
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
    }) {
        Err(err) => err,
        Ok(_) => panic!("expected registration failure"),
    };
    assert!(matches!(err, AgentError::RegistrationFailed(_)), "{err:?}");
    assert!(file.is_file(), "workspace artifact must remain");
    assert_eq!(fs::read(&file).expect("read"), b"hello");
}
