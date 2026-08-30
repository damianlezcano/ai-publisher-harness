use project_agent::model::{
    AgentProject, AgentSession, AgentStatus, Artifact, ArtifactKind, TaskStatus,
};
use project_agent::{AgentEngine, AgentError, FakeAgentEngine, FakeCall};

fn artifact(path: &str, kind: ArtifactKind, byte_size: u64) -> Artifact {
    Artifact {
        path: path.to_owned(),
        kind,
        byte_size,
        sha256: Some("0123456789abcdef".repeat(4)),
    }
}

#[test]
fn task_status_construction_and_equality() {
    let statuses = [
        TaskStatus::Queued,
        TaskStatus::Running,
        TaskStatus::Completed,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ];
    assert_eq!(statuses[0], TaskStatus::Queued);
    assert_eq!(statuses[1], TaskStatus::Running);
    assert_eq!(statuses[2], TaskStatus::Completed);
    assert_eq!(statuses[3], TaskStatus::Failed);
    assert_eq!(statuses[4], TaskStatus::Cancelled);
    assert_ne!(statuses[0], statuses[1]);
    assert_ne!(statuses[2], statuses[4]);
}

#[test]
fn agent_status_construction_and_equality() {
    let statuses = [
        AgentStatus::Stopped,
        AgentStatus::Starting,
        AgentStatus::Ready,
        AgentStatus::Failed,
    ];
    assert_eq!(statuses[0], AgentStatus::Stopped);
    assert_eq!(statuses[1], AgentStatus::Starting);
    assert_eq!(statuses[2], AgentStatus::Ready);
    assert_eq!(statuses[3], AgentStatus::Failed);
    assert_ne!(statuses[0], statuses[2]);
    assert_ne!(statuses[2], statuses[3]);
}

#[test]
fn artifact_construction_and_equality() {
    let a = artifact("outputs/actividad/index.html", ArtifactKind::Web, 2048);
    assert_eq!(a.path, "outputs/actividad/index.html");
    assert_eq!(a.kind, ArtifactKind::Web);
    assert_eq!(a.byte_size, 2048);
    assert_eq!(
        a.sha256.as_deref(),
        Some("0123456789abcdef".repeat(4).as_str())
    );

    let same = artifact("outputs/actividad/index.html", ArtifactKind::Web, 2048);
    assert_eq!(a, same);

    let different_kind = artifact("outputs/actividad/index.html", ArtifactKind::Pdf, 2048);
    assert_ne!(a, different_kind);

    let no_sha = Artifact {
        sha256: None,
        ..artifact("outputs/notes.pdf", ArtifactKind::Pdf, 42)
    };
    assert_eq!(no_sha.sha256, None);
    assert_ne!(a, no_sha);
}

#[test]
fn fake_engine_lifecycle_ready_then_repeat_errors() {
    let engine = FakeAgentEngine::new();
    assert_eq!(engine.status(), AgentStatus::Stopped);

    let info = engine.ensure_ready().unwrap();
    assert_eq!(info.version, "fake");
    assert_eq!(engine.status(), AgentStatus::Ready);

    assert!(matches!(
        engine.ensure_ready(),
        Err(AgentError::BackendAlreadyReady)
    ));
}

#[test]
fn fake_engine_open_session_uses_project_id() {
    let engine = FakeAgentEngine::new();
    engine.ensure_ready().unwrap();

    let project = AgentProject {
        project_id: "proj-7".into(),
        directory: "/tmp/proj-7".into(),
    };
    let session = engine.open_session(&project).unwrap();
    assert_eq!(session.id, "session-proj-7");
    assert_eq!(session.project_id, "proj-7");
}

#[test]
fn fake_engine_send_returns_scripted_artifacts_and_message() {
    let engine = FakeAgentEngine::new();
    engine.ensure_ready().unwrap();

    let artifacts = vec![
        artifact("outputs/actividad/index.html", ArtifactKind::Web, 2048),
        artifact("outputs/actividad/guia.pdf", ArtifactKind::Pdf, 1024),
    ];
    engine.set_artifacts(artifacts.clone());
    engine.set_message("done".into());

    let session = AgentSession {
        id: "session-proj-7".into(),
        project_id: "proj-7".into(),
    };
    let prompt = project_agent::AgentPrompt {
        text: "create an activity".into(),
        model: None,
    };

    let task = engine.send(&session, &prompt).unwrap();
    assert_eq!(task.id, "task-1");
    assert_eq!(task.status, TaskStatus::Completed);
    assert_eq!(task.artifacts, artifacts);
    assert_eq!(task.message.as_deref(), Some("done"));

    let second = engine.send(&session, &prompt).unwrap();
    assert_eq!(second.id, "task-2");
}

#[test]
fn fake_engine_fault_injection() {
    let engine = FakeAgentEngine::new();

    engine.fail_ready();
    assert!(matches!(
        engine.ensure_ready(),
        Err(AgentError::BackendStartFailed(_))
    ));
    assert_eq!(engine.status(), AgentStatus::Stopped);
    let info = engine.ensure_ready().unwrap();
    assert_eq!(info.version, "fake");
    assert_eq!(engine.status(), AgentStatus::Ready);

    engine.fail_session();
    let project = AgentProject {
        project_id: "proj-7".into(),
        directory: "/tmp/proj-7".into(),
    };
    assert!(matches!(
        engine.open_session(&project),
        Err(AgentError::SessionCreationFailed(_))
    ));

    let session = engine.open_session(&project).unwrap();
    assert_eq!(session.id, "session-proj-7");

    engine.fail_send();
    let prompt = project_agent::AgentPrompt {
        text: "create an activity".into(),
        model: None,
    };
    assert!(matches!(
        engine.send(&session, &prompt),
        Err(AgentError::TaskFailed(_))
    ));
}

#[test]
fn fake_engine_send_and_cancel_before_ready_error() {
    let engine = FakeAgentEngine::new();
    let session = AgentSession {
        id: "session-proj-7".into(),
        project_id: "proj-7".into(),
    };
    let prompt = project_agent::AgentPrompt {
        text: "create an activity".into(),
        model: None,
    };

    assert!(matches!(
        engine.send(&session, &prompt),
        Err(AgentError::BackendNotReady)
    ));
    assert_eq!(engine.cancel(&session), Err(AgentError::BackendNotReady));
}

#[test]
fn fake_engine_shutdown_returns_to_stopped() {
    let engine = FakeAgentEngine::new();
    engine.ensure_ready().unwrap();
    assert_eq!(engine.status(), AgentStatus::Ready);

    engine.shutdown().unwrap();
    assert_eq!(engine.status(), AgentStatus::Stopped);

    let session = AgentSession {
        id: "session-proj-7".into(),
        project_id: "proj-7".into(),
    };
    let prompt = project_agent::AgentPrompt {
        text: "create an activity".into(),
        model: None,
    };
    assert!(matches!(
        engine.send(&session, &prompt),
        Err(AgentError::BackendNotReady)
    ));
}

#[test]
fn fake_engine_records_calls_in_order() {
    let engine = FakeAgentEngine::new();
    assert_eq!(engine.calls(), Vec::<FakeCall>::new());

    engine.ensure_ready().unwrap();
    let project = AgentProject {
        project_id: "proj-7".into(),
        directory: "/tmp/proj-7".into(),
    };
    let session = engine.open_session(&project).unwrap();
    let prompt = project_agent::AgentPrompt {
        text: "create an activity".into(),
        model: None,
    };
    engine.send(&session, &prompt).unwrap();
    engine.cancel(&session).unwrap();
    engine.shutdown().unwrap();

    assert_eq!(
        engine.calls(),
        vec![
            FakeCall::Ready,
            FakeCall::OpenSession,
            FakeCall::Send,
            FakeCall::Cancel,
            FakeCall::Shutdown,
        ]
    );
}

#[test]
fn agent_error_display_is_short() {
    assert_eq!(
        AgentError::BackendStartFailed("raw backend output".into()).to_string(),
        "failed to start agent backend"
    );
    assert_eq!(
        AgentError::BinaryNotFound("/opt/opencode/bin/opencode".into()).to_string(),
        "agent backend binary not found"
    );
    assert_eq!(
        AgentError::IncompatibleVersion {
            found: "9.9.9".into(),
            expected: "1.0.0".into()
        }
        .to_string(),
        "incompatible agent backend version (found 9.9.9, expected 1.0.0)"
    );
    assert_eq!(
        AgentError::TaskFailed("raw task failure".into()).to_string(),
        "agent task failed"
    );
    assert_eq!(
        AgentError::Cancelled.to_string(),
        "agent task was cancelled"
    );
    assert_eq!(AgentError::Timeout.to_string(), "agent operation timed out");
    assert_eq!(
        AgentError::ShutdownFailed("raw shutdown output".into()).to_string(),
        "failed to shut down agent backend"
    );
}
