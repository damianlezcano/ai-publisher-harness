use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use project_agent::model::{
    AgentProject, AgentPrompt, AgentStatus, ArtifactKind, ModelRef, TaskStatus,
};
use project_agent::{AgentEngine, AgentError, OpenCodeAgentEngine};

#[path = "support/fake_server.rs"]
mod fake_server;

use fake_server::FakeServer;

fn engine_for(server: &FakeServer) -> OpenCodeAgentEngine {
    OpenCodeAgentEngine::new(PathBuf::from("/usr/bin/true"), unique_config_dir(), 0)
        .with_base_url(server.base_url())
        .with_timeouts(Duration::from_secs(2), Duration::from_millis(400))
}

fn unique_config_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("project-agent-oc-{}-{n}", std::process::id()))
}

fn project() -> AgentProject {
    AgentProject {
        project_id: "proj-7".into(),
        directory: PathBuf::from("/tmp/proj-7"),
    }
}

fn prompt() -> AgentPrompt {
    AgentPrompt {
        text: "create an activity".into(),
        model: None,
    }
}

fn web_doc_diff() -> &'static str {
    r#"[{"path":"outputs/index.html","byte_size":12,"sha256":"abc"},{"path":"outputs/guide.docx","byte_size":24},{"path":"inputs/secret.txt","byte_size":1}]"#
}

#[test]
fn readiness_healthy_version_in_range_is_ready() {
    let server = FakeServer::start();
    server.set_health_version("1.18.25");
    let engine = engine_for(&server);
    assert_eq!(engine.status(), AgentStatus::Stopped);
    let info = engine.ensure_ready().expect("ready");
    assert_eq!(info.version, "1.18.25");
    assert_eq!(engine.status(), AgentStatus::Ready);
}

#[test]
fn readiness_version_out_of_range_is_incompatible() {
    let server = FakeServer::start();
    server.set_health_version("9.0.0");
    let engine = engine_for(&server);
    let err = match engine.ensure_ready() {
        Err(err) => err,
        Ok(_) => panic!("incompatible"),
    };
    assert!(
        matches!(
            err,
            AgentError::IncompatibleVersion { ref found, .. } if found == "9.0.0"
        ),
        "{err:?}"
    );
    assert_eq!(engine.status(), AgentStatus::Failed);
}

#[test]
fn open_session_posts_directory_and_returns_id() {
    let server = FakeServer::start();
    server.set_session_id("ses-42");
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    assert_eq!(session.id, "ses-42");
    assert_eq!(session.project_id, "proj-7");
    assert_eq!(server.last_directory().as_deref(), Some("/tmp/proj-7"));
}

#[test]
fn open_session_error_is_session_creation_failed() {
    let server = FakeServer::start();
    server.fail_session();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let err = match engine.open_session(&project()) {
        Err(err) => err,
        Ok(_) => panic!("session fail"),
    };
    assert!(
        matches!(err, AgentError::SessionCreationFailed(_)),
        "{err:?}"
    );
}

#[test]
fn send_completes_with_web_and_document_artifacts() {
    let server = FakeServer::start();
    server.set_status_sequence(&["working", "idle"]);
    server.set_diff_body(web_doc_diff());
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let mut req = prompt();
    req.model = Some(ModelRef {
        provider_id: "opencode".into(),
        model_id: "local".into(),
    });
    let task = engine.send(&session, &req).expect("send");
    assert!(server.prompt_called());
    assert_eq!(task.id, "ses-1-task");
    assert_eq!(task.status, TaskStatus::Completed);
    assert_eq!(task.message.as_deref(), Some("done"));
    assert_eq!(task.artifacts.len(), 2);
    assert_eq!(task.artifacts[0].path, "outputs/index.html");
    assert_eq!(task.artifacts[0].kind, ArtifactKind::Web);
    assert_eq!(task.artifacts[0].byte_size, 12);
    assert_eq!(task.artifacts[0].sha256.as_deref(), Some("abc"));
    assert_eq!(task.artifacts[1].path, "outputs/guide.docx");
    assert_eq!(task.artifacts[1].kind, ArtifactKind::Document);
}

#[test]
fn send_malformed_json_is_http_or_task_failed() {
    let server = FakeServer::start();
    server.set_malformed_session();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let err = match engine.send(&session, &prompt()) {
        Err(err) => err,
        Ok(_) => panic!("malformed"),
    };
    assert!(
        matches!(err, AgentError::Http(_) | AgentError::TaskFailed(_)),
        "{err:?}"
    );
}

#[test]
fn send_failed_status_is_task_failed() {
    let server = FakeServer::start();
    server.set_status_sequence(&["failed"]);
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let err = match engine.send(&session, &prompt()) {
        Err(err) => err,
        Ok(_) => panic!("failed"),
    };
    assert!(matches!(err, AgentError::TaskFailed(_)), "{err:?}");
}

#[test]
fn send_never_idle_times_out() {
    let server = FakeServer::start();
    server.set_status_sequence(&["working"]);
    server.set_status_delay(Duration::from_millis(50));
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let err = match engine.send(&session, &prompt()) {
        Err(err) => err,
        Ok(_) => panic!("timeout"),
    };
    assert_eq!(err, AgentError::Timeout);
}

#[test]
fn cancel_calls_abort() {
    let server = FakeServer::start();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    engine.cancel(&session).expect("abort");
    assert!(server.abort_called());
}

#[test]
fn artifact_kind_mapping_and_outputs_only() {
    let server = FakeServer::start();
    server.set_diff_body(
        r#"[
            {"path":"outputs/index.html","byte_size":1},
            {"path":"outputs/a.docx","byte_size":2},
            {"path":"outputs/a.xlsx","byte_size":3},
            {"path":"outputs/a.pptx","byte_size":4},
            {"path":"outputs/a.pdf","byte_size":5},
            {"path":"outputs/a.png","byte_size":6},
            {"path":"outputs/a.jpg","byte_size":7},
            {"path":"outputs/a.gif","byte_size":8},
            {"path":"outputs/a.svg","byte_size":9},
            {"path":"outputs/a.webp","byte_size":10},
            {"path":"outputs/a.ico","byte_size":11},
            {"path":"outputs/a.md","byte_size":12},
            {"path":"outputs/a.txt","byte_size":13},
            {"path":"outputs/a.bin","byte_size":14},
            {"path":"inputs/skip.txt","byte_size":99},
            {"path":"workspace/skip.txt","byte_size":99}
        ]"#,
    );
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let task = engine.send(&session, &prompt()).expect("send");
    let kinds: Vec<_> = task
        .artifacts
        .iter()
        .map(|a| (a.path.as_str(), a.kind))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("outputs/index.html", ArtifactKind::Web),
            ("outputs/a.docx", ArtifactKind::Document),
            ("outputs/a.xlsx", ArtifactKind::Spreadsheet),
            ("outputs/a.pptx", ArtifactKind::Presentation),
            ("outputs/a.pdf", ArtifactKind::Pdf),
            ("outputs/a.png", ArtifactKind::Image),
            ("outputs/a.jpg", ArtifactKind::Image),
            ("outputs/a.gif", ArtifactKind::Image),
            ("outputs/a.svg", ArtifactKind::Image),
            ("outputs/a.webp", ArtifactKind::Image),
            ("outputs/a.ico", ArtifactKind::Image),
            ("outputs/a.md", ArtifactKind::Text),
            ("outputs/a.txt", ArtifactKind::Text),
            ("outputs/a.bin", ArtifactKind::Other),
        ]
    );
}

#[test]
fn status_stopped_ready_stopped() {
    let server = FakeServer::start();
    let engine = engine_for(&server);
    assert_eq!(engine.status(), AgentStatus::Stopped);
    engine.ensure_ready().expect("ready");
    assert_eq!(engine.status(), AgentStatus::Ready);
    engine.shutdown().expect("shutdown");
    assert_eq!(engine.status(), AgentStatus::Stopped);
}

#[test]
fn shutdown_is_idempotent() {
    let server = FakeServer::start();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    engine.shutdown().expect("first");
    engine.shutdown().expect("second");
    assert_eq!(engine.status(), AgentStatus::Stopped);
}

#[test]
fn spawn_maps_process_failures() {
    let missing = OpenCodeAgentEngine::new(
        PathBuf::from("/no/such/opencode-binary-xyz"),
        unique_config_dir(),
        1,
    )
    .with_timeouts(Duration::from_millis(80), Duration::from_millis(80));
    let err = match missing.ensure_ready() {
        Err(err) => err,
        Ok(_) => panic!("missing binary"),
    };
    assert!(
        matches!(err, AgentError::BinaryNotFound(ref name) if name.contains("opencode-binary")),
        "{err:?}"
    );

    let fake = fake_process_bin();
    let exiting = OpenCodeAgentEngine::new(fake.clone(), unique_config_dir(), 1)
        .with_timeouts(Duration::from_millis(200), Duration::from_millis(80))
        .with_env("FAKE_PROCESS_MODE".into(), "exit".into());
    let err = match exiting.ensure_ready() {
        Err(err) => err,
        Ok(_) => panic!("process exit"),
    };
    assert!(matches!(err, AgentError::BackendStartFailed(_)), "{err:?}");

    let lingering = OpenCodeAgentEngine::new(fake, unique_config_dir(), 1)
        .with_timeouts(Duration::from_millis(80), Duration::from_millis(80))
        .with_env("FAKE_PROCESS_MODE".into(), "print".into());
    let err = match lingering.ensure_ready() {
        Err(err) => err,
        Ok(_) => panic!("timeout without http"),
    };
    assert_eq!(err, AgentError::Timeout);
}

fn fake_process_bin() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_fake-process") {
        return PathBuf::from(path);
    }
    let exe = std::env::current_exe().expect("current_exe");
    let mut dir = exe.parent().expect("parent").to_path_buf();
    if dir.file_name().is_some_and(|name| name == "deps") {
        dir.pop();
    }
    let candidate = dir.join(format!("fake-process{}", std::env::consts::EXE_SUFFIX));
    if candidate.is_file() {
        return candidate;
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent().and_then(Path::parent).expect("workspace");
    let status = Command::new(cargo)
        .current_dir(workspace)
        .args([
            "build",
            "--offline",
            "--locked",
            "-p",
            "project-process",
            "--bin",
            "fake-process",
        ])
        .status()
        .expect("build fake-process");
    assert!(status.success(), "failed to build fake-process");
    let built = workspace
        .join("target")
        .join("debug")
        .join(format!("fake-process{}", std::env::consts::EXE_SUFFIX));
    assert!(
        built.is_file(),
        "fake-process missing at {}",
        built.display()
    );
    built
}
