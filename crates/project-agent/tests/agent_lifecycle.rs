//! M5 agent lifecycle: lazy start/reuse, per-project serialization, cancel,
//! and failure-preserves-workspace behavior of `AgentService` over
//! `FakeAgentEngine`.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use project_agent::model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask, Artifact,
    ArtifactKind,
};
use project_agent::{
    AgentEngine, AgentError, AgentRequest, AgentService, CreationRegistrar, FakeAgentEngine,
    FakeCall,
};

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

fn request(project_id: &str) -> AgentRequest {
    AgentRequest {
        project_id: project_id.into(),
        prompt: prompt(),
        attachments: Vec::new(),
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

#[derive(Clone)]
struct FakeRegistrar {
    next_id: Arc<AtomicU32>,
    fail: Arc<Mutex<bool>>,
}

impl FakeRegistrar {
    fn new() -> Self {
        Self {
            next_id: Arc::new(AtomicU32::new(1)),
            fail: Arc::new(Mutex::new(false)),
        }
    }

    fn fail_next(&self) {
        *self.fail.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }
}

impl CreationRegistrar for FakeRegistrar {
    fn register(
        &self,
        _project_id: &str,
        _artifact: &Artifact,
        _bytes: Vec<u8>,
    ) -> project_agent::AgentResult<String> {
        if *self.fail.lock().unwrap_or_else(|e| e.into_inner()) {
            return Err(AgentError::RegistrationFailed("injected".into()));
        }
        let n = self.next_id.fetch_add(1, Ordering::SeqCst);
        Ok(format!("creation-{n}"))
    }
}

/// Delegates to `FakeAgentEngine` and counts successful readiness
/// transitions (Stopped -> Ready), i.e. actual backend starts.
struct RecordingEngine {
    inner: FakeAgentEngine,
    ready_ok: Arc<AtomicUsize>,
}

impl RecordingEngine {
    fn new(inner: FakeAgentEngine, ready_ok: Arc<AtomicUsize>) -> Self {
        Self { inner, ready_ok }
    }
}

impl AgentEngine for RecordingEngine {
    fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
        let result = self.inner.ensure_ready();
        if result.is_ok() {
            self.ready_ok.fetch_add(1, Ordering::SeqCst);
        }
        result
    }

    fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
        self.inner.open_session(project)
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

/// Slow engine that records concurrent `send` depth and the observed maximum.
struct SlowEngine {
    inner: FakeAgentEngine,
    active: Mutex<usize>,
    max_active: Arc<Mutex<usize>>,
}

impl SlowEngine {
    fn new(inner: FakeAgentEngine, max_active: Arc<Mutex<usize>>) -> Self {
        Self {
            inner,
            active: Mutex::new(0),
            max_active,
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
            let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
            *active += 1;
            let mut max = self.max_active.lock().unwrap_or_else(|e| e.into_inner());
            if *active > *max {
                *max = *active;
            }
        }
        thread::sleep(Duration::from_millis(80));
        let result = self.inner.send(session, req);
        *self.active.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
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

/// Engine that blocks inside `send` until released (used to hold a run in
/// flight so cancel can target it).
struct BlockingEngine {
    inner: FakeAgentEngine,
    entered: Arc<(Mutex<bool>, Condvar)>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl BlockingEngine {
    fn new(
        inner: FakeAgentEngine,
        entered: Arc<(Mutex<bool>, Condvar)>,
        release: Arc<(Mutex<bool>, Condvar)>,
    ) -> Self {
        Self {
            inner,
            entered,
            release,
        }
    }
}

fn wait_send_entered(gate: &Arc<(Mutex<bool>, Condvar)>, timeout: Duration) -> bool {
    let (lock, cv) = &**gate;
    let mut flag = lock.lock().unwrap_or_else(|e| e.into_inner());
    let deadline = Instant::now() + timeout;
    while !*flag && Instant::now() < deadline {
        let (guard, _) = cv
            .wait_timeout(flag, Duration::from_millis(10))
            .unwrap_or_else(|e| e.into_inner());
        flag = guard;
    }
    *flag
}

fn signal(gate: &Arc<(Mutex<bool>, Condvar)>) {
    let (lock, cv) = &**gate;
    *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
    cv.notify_all();
}

impl AgentEngine for BlockingEngine {
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
        let (entered_lock, entered_cv) = &*self.entered;
        {
            let mut flag = entered_lock.lock().unwrap_or_else(|e| e.into_inner());
            *flag = true;
            entered_cv.notify_all();
        }
        let (release_lock, release_cv) = &*self.release;
        let mut flag = release_lock.lock().unwrap_or_else(|e| e.into_inner());
        while !*flag {
            flag = release_cv.wait(flag).unwrap_or_else(|e| e.into_inner());
        }
        drop(flag);
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
fn lazy_start_and_reuse() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fake = FakeAgentEngine::new();
    fake.set_artifacts(vec![]);
    let calls = fake.clone();
    let ready_ok = Arc::new(AtomicUsize::new(0));
    let recorder = RecordingEngine::new(fake, Arc::clone(&ready_ok));
    let service = AgentService::new(recorder, FakeRegistrar::new(), tmp.path().to_path_buf());

    assert_eq!(service.engine_status(), AgentStatus::Stopped);

    service.run(request("proj-7")).expect("first run");
    assert_eq!(
        ready_ok.load(Ordering::SeqCst),
        1,
        "lazy start must ensure ready once"
    );
    assert_eq!(
        calls
            .calls()
            .iter()
            .filter(|c| **c == FakeCall::OpenSession)
            .count(),
        1
    );
    assert_eq!(service.engine_status(), AgentStatus::Ready);

    service.run(request("proj-7")).expect("second run");
    assert_eq!(
        ready_ok.load(Ordering::SeqCst),
        1,
        "a second run on the same project must reuse the backend, not re-ensure it"
    );
    assert_eq!(
        calls
            .calls()
            .iter()
            .filter(|c| **c == FakeCall::OpenSession)
            .count(),
        2
    );
    assert_eq!(service.engine_status(), AgentStatus::Ready);

    service.shutdown().expect("shutdown");
    assert_eq!(service.engine_status(), AgentStatus::Stopped);

    service.run(request("proj-7")).expect("run after shutdown");
    assert_eq!(
        ready_ok.load(Ordering::SeqCst),
        2,
        "a run after shutdown must re-ensure the backend"
    );
}

#[test]
fn same_project_serialized() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fake = FakeAgentEngine::new();
    fake.set_artifacts(vec![]);
    let max_active = Arc::new(Mutex::new(0));
    let engine = SlowEngine::new(fake, Arc::clone(&max_active));
    let service = Arc::new(AgentService::new(
        engine,
        FakeRegistrar::new(),
        tmp.path().to_path_buf(),
    ));

    let start = Instant::now();
    let a = {
        let service = Arc::clone(&service);
        thread::spawn(move || service.run(request("proj-7")))
    };
    let b = {
        let service = Arc::clone(&service);
        thread::spawn(move || service.run(request("proj-7")))
    };
    a.join().expect("join a").expect("run a");
    b.join().expect("join b").expect("run b");

    assert_eq!(
        *max_active.lock().unwrap_or_else(|e| e.into_inner()),
        1,
        "send calls for the same project must not overlap"
    );
    assert!(
        start.elapsed() >= Duration::from_millis(140),
        "serialized runs should take at least two slow sends"
    );
}

#[test]
fn cancel_aborts_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fake = FakeAgentEngine::new();
    fake.set_artifacts(vec![]);
    let calls = fake.clone();
    let entered = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let blocking = BlockingEngine::new(fake, Arc::clone(&entered), Arc::clone(&release));
    let service = Arc::new(AgentService::new(
        blocking,
        FakeRegistrar::new(),
        tmp.path().to_path_buf(),
    ));

    let handle = {
        let service = Arc::clone(&service);
        thread::spawn(move || service.run(request("proj-7")))
    };

    assert!(
        wait_send_entered(&entered, Duration::from_secs(2)),
        "run must reach send"
    );
    service.cancel("proj-7").expect("cancel");
    assert!(
        calls.calls().contains(&FakeCall::Cancel),
        "cancel must reach the engine for the active session"
    );

    signal(&release);
    handle.join().expect("join").expect("run completes");
}

#[test]
fn failure_preserves_workspace_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fake = FakeAgentEngine::new();
    fake.set_artifacts(vec![artifact("workspace/notes.txt", ArtifactKind::Text, 5)]);
    let file = write_artifact(tmp.path(), "proj-7", "notes.txt", b"hello");

    let registrar = FakeRegistrar::new();
    registrar.fail_next();
    let service = AgentService::new(fake, registrar, tmp.path().to_path_buf());

    let err = match service.run(request("proj-7")) {
        Err(err) => err,
        Ok(_) => panic!("expected registration failure"),
    };
    assert!(matches!(err, AgentError::RegistrationFailed(_)), "{err:?}");
    assert!(file.is_file(), "workspace artifact must remain");
    assert_eq!(fs::read(&file).expect("read"), b"hello");
}
