use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fake_opencode_server::FakeServer;
use project_agent::model::{
    AgentProject, AgentPrompt, AgentStatus, ArtifactKind, ModelRef, TaskStatus,
};
use project_agent::{AgentEngine, AgentError, OpenCodeAgentEngine, UsageSource};
use serde_json::json;

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
        knowledge: None,
        conversation_context: None,
    }
}

fn web_doc_diff() -> &'static str {
    r#"[{"path":"workspace/index.html","byte_size":12,"sha256":"abc"},{"path":"workspace/guide.docx","byte_size":24},{"path":"inputs/secret.txt","byte_size":1}]"#
}

#[test]
fn completed_turn_surfaces_actual_session_usage_delta() {
    let server = FakeServer::start();
    server.set_session_details_sequence(&[
        r#"{"id":"ses-1","cost":0.01,"tokens":{"input":100,"output":20,"cache":{"read":5,"write":2}}}"#,
        r#"{"id":"ses-1","cost":0.018,"tokens":{"input":1834,"output":446,"cache":{"read":5,"write":7}}}"#,
    ]);
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let task = engine.send(&session, &prompt()).expect("completed task");

    assert_eq!(task.usage.source, UsageSource::ProviderActual);
    assert_eq!(task.usage.input_tokens, Some(1734));
    assert_eq!(task.usage.output_tokens, Some(426));
    assert_eq!(task.usage.cache_read_tokens, Some(0));
    assert_eq!(task.usage.cache_write_tokens, Some(5));
    assert_eq!(task.usage.total_tokens, Some(2165));
    assert!(
        (task.usage.cost_usd.expect("reported cost") - 0.008).abs() < f64::EPSILON,
        "cost delta must preserve provider-reported arithmetic"
    );
}

#[test]
fn missing_session_usage_stays_unavailable_not_zero() {
    let server = FakeServer::start();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let task = engine.send(&session, &prompt()).expect("completed task");
    assert_eq!(task.usage.source, UsageSource::Unavailable);
    assert_eq!(task.usage.input_tokens, None);
    assert_eq!(task.usage.total_tokens, None);
    assert_eq!(task.usage.cost_usd, None);
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
    assert_eq!(
        server.last_permission(),
        Some(json!([{
            "permission": "external_directory",
            "pattern": "*",
            "action": "deny",
        }]))
    );
}

#[test]
fn send_uses_ordinary_session_contract_without_directory_query() {
    let server = FakeServer::start();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    engine.send(&session, &prompt()).expect("completed task");

    assert_eq!(
        server.prompt_async_paths().len(),
        1,
        "exactly one prompt_async per ordinary chat turn"
    );
    let prompt_path = server.prompt_async_paths().first().unwrap().clone();
    assert!(prompt_path.contains("/prompt_async"));
    assert!(
        !prompt_path.contains("directory="),
        "ordinary chat prompt_async must not carry ?directory=: {prompt_path}"
    );
    for path in server
        .message_paths()
        .iter()
        .chain(server.session_get_paths().iter())
    {
        assert!(
            !path.contains("directory="),
            "ordinary chat endpoint must not carry ?directory=: {path}"
        );
    }
    assert!(!server.message_paths().is_empty());
}

#[test]
fn fresh_sessions_never_reuse_the_project_conversation_cache() {
    let server = FakeServer::start();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");

    let cached = engine.open_session(&project()).expect("cached session");
    assert_eq!(
        engine.open_session(&project()).expect("cached reuse").id,
        cached.id,
        "ordinary chat retains its established project session"
    );

    let first = engine
        .open_fresh_session(&project())
        .expect("fresh session");
    let second = engine
        .open_fresh_session(&project())
        .expect("another fresh session");
    assert_ne!(first.id, cached.id);
    assert_ne!(second.id, first.id);
    assert_eq!(server.created_session_ids().len(), 3);
    assert_eq!(
        engine
            .open_session(&project())
            .expect("cached session survives")
            .id,
        cached.id,
        "a Knowledge QA scratch session must not replace ordinary-chat state"
    );
}

#[test]
fn ephemeral_send_failure_does_not_drop_the_conversation_cache() {
    let server = FakeServer::start();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let cached = engine.open_session(&project()).expect("cached");
    let fresh = engine.open_fresh_session(&project()).expect("fresh");
    server.set_status_sequence(&["failed"]);
    assert!(engine.send(&fresh, &prompt()).is_err());
    assert_eq!(
        engine.open_session(&project()).expect("still cached").id,
        cached.id
    );
}

#[test]
fn conversational_send_failure_invalidates_the_cached_session() {
    let server = FakeServer::start();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let cached = engine.open_session(&project()).expect("cached");
    server.set_status_sequence(&["failed"]);
    assert!(engine.send(&cached, &prompt()).is_err());
    let next = engine
        .open_session(&project())
        .expect("replacement session");
    assert_ne!(next.id, cached.id);
}

#[test]
fn invalidate_cached_session_drops_reusable_conversational_id() {
    let server = FakeServer::start();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let cached = engine.open_session(&project()).expect("cached");
    engine.invalidate_cached_session(&project().project_id);
    let next = engine.open_session(&project()).expect("rotated");
    assert_ne!(next.id, cached.id);
    assert_eq!(
        engine
            .open_session(&project())
            .expect("reuse after rotate")
            .id,
        next.id
    );
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
fn send_preserves_quoted_and_special_text_exactly_in_request_body() {
    // User text is DATA, not syntax: every one of these must reach the OpenCode
    // `parts[0].text` field byte-for-byte, with no shell quoting, no JSON
    // re-encoding of the string, and no punctuation stripping.
    let cases = [
        "\"hola\"",
        "'hola'",
        "hola \"mundo\"",
        "¿qué significa \"test\"?",
        "{\"a\":\"b\"}",
        "$HOME",
        "$(echo hola)",
        "`echo hola`",
        "hola; echo mundo",
        r"C:\Users\test\archivo.txt",
        r"C:\\Users\\test\\archivo.txt",
        "texto con \\ backslash",
        "\"emoji 😀\"",
        "línea uno\nlínea \"dos\"\nlínea tres",
    ];
    for text in cases {
        let server = FakeServer::start();
        let engine = engine_for(&server);
        engine.ensure_ready().expect("ready");
        let session = engine.open_session(&project()).expect("session");
        let req = AgentPrompt {
            text: text.to_owned(),
            model: None,
            knowledge: None,
            conversation_context: None,
        };
        let task = engine.send(&session, &req).expect("send");
        assert_eq!(task.status, TaskStatus::Completed);
        assert_eq!(
            server.last_prompt_text().as_deref(),
            Some(text),
            "prompt {text:?} must reach the request body unchanged"
        );
    }
}

#[test]
fn send_completes_with_web_and_document_artifacts() {
    let server = FakeServer::start();
    server.set_status_sequence(&["busy", "idle"]);
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
    assert_eq!(task.artifacts[0].path, "workspace/index.html");
    assert_eq!(task.artifacts[0].kind, ArtifactKind::Web);
    assert_eq!(task.artifacts[0].byte_size, 12);
    assert_eq!(task.artifacts[0].sha256.as_deref(), Some("abc"));
    assert_eq!(task.artifacts[1].path, "workspace/guide.docx");
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
    server.set_status_sequence(&["busy"]);
    server.set_status_delay(Duration::from_millis(50));
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let err = match engine.send(&session, &prompt()) {
        Err(err) => err,
        Ok(_) => panic!("timeout"),
    };
    // A bounded execution timeout now surfaces as `Timeout` so the terminal
    // failure classifier can report `timeout` distinctly from a task failure.
    assert!(
        matches!(err, AgentError::Timeout | AgentError::TaskFailed(_)),
        "{err:?}"
    );
}

#[test]
fn failed_send_evicts_cached_session_for_next_turn() {
    let server = FakeServer::start();
    server.set_status_sequence(&["busy"]);
    server.set_status_delay(Duration::from_millis(50));
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let first = engine.open_session(&project()).expect("first session");
    let _ = engine.send(&first, &prompt());
    server.set_session_id("ses-fresh");
    let second = engine.open_session(&project()).expect("fresh session");
    assert_eq!(
        second.id, "ses-fresh",
        "failed turn must not reuse its session"
    );
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
            {"path":"workspace/index.html","byte_size":1},
            {"path":"workspace/a.docx","byte_size":2},
            {"path":"workspace/a.xlsx","byte_size":3},
            {"path":"workspace/a.pptx","byte_size":4},
            {"path":"workspace/a.pdf","byte_size":5},
            {"path":"workspace/a.png","byte_size":6},
            {"path":"workspace/a.jpg","byte_size":7},
            {"path":"workspace/a.gif","byte_size":8},
            {"path":"workspace/a.svg","byte_size":9},
            {"path":"workspace/a.webp","byte_size":10},
            {"path":"workspace/a.ico","byte_size":11},
            {"path":"workspace/a.md","byte_size":12},
            {"path":"workspace/a.txt","byte_size":13},
            {"path":"workspace/a.bin","byte_size":14},
            {"path":"inputs/skip.txt","byte_size":99},
            {"path":"outputs/skip.txt","byte_size":99}
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
            ("workspace/index.html", ArtifactKind::Web),
            ("workspace/a.docx", ArtifactKind::Document),
            ("workspace/a.xlsx", ArtifactKind::Spreadsheet),
            ("workspace/a.pptx", ArtifactKind::Presentation),
            ("workspace/a.pdf", ArtifactKind::Pdf),
            ("workspace/a.png", ArtifactKind::Image),
            ("workspace/a.jpg", ArtifactKind::Image),
            ("workspace/a.gif", ArtifactKind::Image),
            ("workspace/a.svg", ArtifactKind::Image),
            ("workspace/a.webp", ArtifactKind::Image),
            ("workspace/a.ico", ArtifactKind::Image),
            ("workspace/a.md", ArtifactKind::Text),
            ("workspace/a.txt", ArtifactKind::Text),
            ("workspace/a.bin", ArtifactKind::Other),
        ]
    );
}

#[test]
fn session_relative_html_paths_are_workspace_web_artifacts() {
    let server = FakeServer::start();
    server.set_status_sequence(&["busy", "idle"]);
    server
        .set_diff_body(r#"[{"path":"rosco.html","byte_size":24},{"path":"app.js","byte_size":8}]"#);
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let task = engine.send(&session, &prompt()).expect("send");
    assert_eq!(task.artifacts.len(), 2);
    assert_eq!(task.artifacts[0].path, "workspace/rosco.html");
    assert_eq!(task.artifacts[0].kind, ArtifactKind::Web);
    assert_eq!(task.artifacts[1].path, "workspace/app.js");
    assert_eq!(task.artifacts[1].kind, ArtifactKind::Other);
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
fn send_fetches_assistant_text_from_message_endpoint() {
    let server = FakeServer::start();
    server.set_status_sequence(&["busy", "idle"]);
    server.set_messages_body(
        r#"[{"info":{"id":"old","role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"viejo"}]}]"#,
    );
    server.set_prompt_response_text("hola desde el endpoint");
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let task = engine.send(&session, &prompt()).expect("send");
    assert_eq!(task.status, TaskStatus::Completed);
    assert_eq!(task.message.as_deref(), Some("hola desde el endpoint"));
}

#[test]
fn send_selects_only_new_turn_terminal_text_and_excludes_reasoning() {
    let server = FakeServer::start();
    server.set_status_sequence(&["idle"]);
    server.set_messages_body(
        r#"[
            {"info":{"id":"user-1","role":"user"},"parts":[{"type":"text","text":"pregunta"}]},
            {"info":{"id":"old","role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"respuesta vieja"}]}
        ]"#,
    );
    server.set_prompt_response_text("respuesta final");
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let task = engine.send(&session, &prompt()).expect("send");
    assert_eq!(task.message.as_deref(), Some("respuesta final"));
}

#[test]
fn sequential_sends_select_each_current_turn_response() {
    let server = FakeServer::start();
    server.set_status_sequence(&["idle"]);
    server.set_messages_body("[]");
    server.set_prompt_response_text("primera respuesta");
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");

    let first = engine.send(&session, &prompt()).expect("first send");
    assert_eq!(first.message.as_deref(), Some("primera respuesta"));

    server.set_prompt_response_text("segunda respuesta");
    let second = engine.send(&session, &prompt()).expect("second send");
    assert_eq!(second.message.as_deref(), Some("segunda respuesta"));
}

#[test]
fn growing_assistant_message_resets_grace_until_stop() {
    let server = FakeServer::start();
    server.set_status_sequence(&["idle"]);
    server.set_prompt_appends_response(false);
    server.set_messages_sequence(&[
        "[]",
        r#"[{"info":{"id":"user-1","role":"user"},"parts":[{"type":"text","text":"hola"}]},{"info":{"id":"assistant-1","role":"assistant","parentID":"user-1"},"parts":[]}]"#,
        r#"[{"info":{"id":"user-1","role":"user"},"parts":[{"type":"text","text":"hola"}]},{"info":{"id":"assistant-1","role":"assistant","parentID":"user-1"},"parts":[{"type":"step-start"},{"type":"reasoning","text":"pensando"}]}]"#,
        r#"[{"info":{"id":"user-1","role":"user"},"parts":[{"type":"text","text":"hola"}]},{"info":{"id":"assistant-1","role":"assistant","parentID":"user-1","finish":"stop"},"parts":[{"type":"step-start"},{"type":"reasoning","text":"pensando"},{"type":"text","text":"¡Hola!"},{"type":"step-finish"}]}]"#,
    ]);
    let engine = OpenCodeAgentEngine::new(PathBuf::from("/usr/bin/true"), unique_config_dir(), 0)
        .with_base_url(server.base_url())
        .with_timeouts(Duration::from_secs(2), Duration::from_millis(500));
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let task = engine.send(&session, &prompt()).expect("send");
    assert_eq!(task.message.as_deref(), Some("¡Hola!"));
}

#[test]
fn send_idle_without_new_assistant_message_times_out() {
    // Watermark check: a pre-existing assistant message without a new one after
    // prompt_async must not be mistaken for this turn's completion.
    let server = FakeServer::start();
    server.set_status_sequence(&["idle"]);
    server.set_messages_body(
        r#"[{"info":{"id":"msg-old","role":"assistant"},"parts":[{"type":"text","text":"old"}]}]"#,
    );
    server.set_prompt_appends_response(false);
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let err = engine.send(&session, &prompt()).expect_err("timeout");
    assert!(
        matches!(err, AgentError::TaskFailed(ref reason) if reason == "timed out" || reason == "timed out waiting for turn identity"),
        "{err:?}"
    );
}

#[test]
fn send_completes_when_status_map_omits_session_key() {
    // The real 1.18.25 sidecar signals completion with an empty /session/status
    // map (the session key disappears). The fake default idle already emits that.
    let server = FakeServer::start();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let task = engine.send(&session, &prompt()).expect("send");
    assert_eq!(task.message.as_deref(), Some("done"));
}

#[test]
fn send_empty_assistant_without_files_completes_on_explicit_stop() {
    let server = FakeServer::start();
    server.set_status_sequence(&["idle"]);
    server.set_messages_body(
        r#"[{"info":{"id":"msg-1","role":"assistant"},"parts":[{"type":"text","text":""}]}]"#,
    );
    server.set_diff_body("[]");
    let engine = OpenCodeAgentEngine::new(PathBuf::from("/usr/bin/true"), unique_config_dir(), 0)
        .with_base_url(server.base_url())
        .with_timeouts(Duration::from_secs(2), Duration::from_secs(5));
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let started = std::time::Instant::now();
    let task = engine.send(&session, &prompt()).expect("send");
    assert_eq!(task.status, TaskStatus::Completed);
    assert!(task.message.is_none());
    assert!(task.artifacts.is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "empty idle reply must not spin until the task timeout"
    );
}

#[test]
fn send_does_not_treat_brief_listo_as_complete_before_artifacts() {
    let server = FakeServer::start();
    server.set_status_sequence(&["idle"]);
    server.set_messages_body("[]");
    server.set_prompt_response_text("Listo.");
    server.set_diff_body("[]");
    server.set_prompt_response_finish("tool-calls");
    let engine = OpenCodeAgentEngine::new(PathBuf::from("/usr/bin/true"), unique_config_dir(), 0)
        .with_base_url(server.base_url())
        .with_timeouts(Duration::from_secs(2), Duration::from_secs(5));
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let started = std::time::Instant::now();
    let task = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(120));
            server.set_diff_body(r#"[{"path":"index.html","byte_size":12}]"#);
            server.set_messages_body(
                r#"[{"info":{"id":"user-appended-1","role":"user"},"parts":[{"type":"text","text":"prompt"}]},{"info":{"id":"msg-final","role":"assistant","parentID":"user-appended-1","finish":"stop"},"parts":[{"type":"text","text":"Listo."}]}]"#,
            );
        });
        engine.send(&session, &prompt()).expect("send")
    });
    assert_eq!(task.status, TaskStatus::Completed);
    assert_eq!(task.message.as_deref(), Some("Listo."));
    assert_eq!(task.artifacts.len(), 1);
    assert_eq!(task.artifacts[0].path, "workspace/index.html");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn send_does_not_treat_intermediate_text_as_terminal_before_artifacts() {
    let server = FakeServer::start();
    server.set_status_sequence(&["idle"]);
    server.set_messages_body("[]");
    server.set_prompt_response_text("Voy a preparar la actividad.");
    server.set_prompt_response_finish("tool-calls");
    server.set_diff_body(r#"[{"path":"index.html","byte_size":12}]"#);
    let engine = OpenCodeAgentEngine::new(PathBuf::from("/usr/bin/true"), unique_config_dir(), 0)
        .with_base_url(server.base_url())
        .with_timeouts(Duration::from_secs(2), Duration::from_secs(5));
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let started = std::time::Instant::now();
    let task = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(120));
            server.set_messages_body(
                r#"[{"info":{"id":"user-appended-1","role":"user"},"parts":[{"type":"text","text":"prompt"}]},{"info":{"id":"msg-final","role":"assistant","parentID":"user-appended-1","finish":"stop"},"parts":[{"type":"text","text":"Actividad creada."}]}]"#,
            );
        });
        engine.send(&session, &prompt()).expect("send")
    });
    assert!(started.elapsed() >= Duration::from_millis(120));
    assert_eq!(task.message.as_deref(), Some("Actividad creada."));
    assert_eq!(task.artifacts.len(), 1);
}

#[test]
fn send_tolerates_transient_diff_errors_during_ack_wait() {
    let server = FakeServer::start();
    server.set_status_sequence(&["idle"]);
    server.set_messages_body("[]");
    server.set_prompt_response_text("Listo.");
    server.set_diff_status(503);
    server.set_diff_body("[]");
    server.set_prompt_response_finish("tool-calls");
    let engine = OpenCodeAgentEngine::new(PathBuf::from("/usr/bin/true"), unique_config_dir(), 0)
        .with_base_url(server.base_url())
        .with_timeouts(Duration::from_secs(2), Duration::from_secs(5));
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let task = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(120));
            server.set_diff_status(200);
            server.set_diff_body(r#"[{"path":"index.html","byte_size":12}]"#);
            server.set_messages_body(
                r#"[{"info":{"id":"user-appended-1","role":"user"},"parts":[{"type":"text","text":"prompt"}]},{"info":{"id":"msg-final","role":"assistant","parentID":"user-appended-1","finish":"stop"},"parts":[{"type":"text","text":"Listo."}]}]"#,
            );
        });
        engine.send(&session, &prompt()).expect("send")
    });
    assert_eq!(task.status, TaskStatus::Completed);
    assert_eq!(task.artifacts.len(), 1);
    assert_eq!(task.artifacts[0].path, "workspace/index.html");
}

#[test]
fn send_completes_on_explicit_stop_without_files() {
    let server = FakeServer::start();
    server.set_status_sequence(&["idle"]);
    server.set_messages_body(
        r#"[{"info":{"id":"msg-1","role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"Listo."}]}]"#,
    );
    server.set_diff_body("[]");
    let engine = OpenCodeAgentEngine::new(PathBuf::from("/usr/bin/true"), unique_config_dir(), 0)
        .with_base_url(server.base_url())
        .with_timeouts(Duration::from_secs(2), Duration::from_secs(5));
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    let task = engine.send(&session, &prompt()).expect("send");
    assert_eq!(task.status, TaskStatus::Completed);
    assert_eq!(task.message.as_deref(), Some("Listo."));
    assert!(task.artifacts.is_empty());
}

#[test]
fn send_ignores_foreign_session_in_status_map() {
    let server = FakeServer::start();
    server.set_session_id("own-session");
    // The engine's own session key is absent; a foreign session is busy.
    server.set_session_poll_body(r#"{"foreign-session":{"type":"busy"}}"#);
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine.open_session(&project()).expect("session");
    assert_eq!(session.id, "own-session");
    let task = engine.send(&session, &prompt()).expect("send");
    assert_eq!(task.message.as_deref(), Some("done"));
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

/// Regression (M10 packaging): concurrent `ensure_ready` callers at app startup
/// must serialize on the backend so a caller that probes the booting child
/// cannot force-kill it. `fake-process` in `serve_http` mode boots with a
/// 600 ms delay (simulating the slow AppImage-FUSE sidecar start); all callers
/// must still converge on a single healthy backend.
#[test]
fn concurrent_ensure_ready_serializes_spawn_and_all_callers_succeed() {
    let port = free_port();
    let engine = Arc::new(
        OpenCodeAgentEngine::new(fake_process_bin(), unique_config_dir(), port)
            .with_timeouts(Duration::from_secs(15), Duration::from_secs(2))
            .with_env("FAKE_PROCESS_MODE".into(), "serve_http".into())
            .with_env("FAKE_PROCESS_DELAY_MS".into(), "600".into()),
    );

    const THREADS: usize = 8;
    let barrier = Arc::new(std::sync::Barrier::new(THREADS));
    let mut handles = Vec::with_capacity(THREADS);
    for _ in 0..THREADS {
        let engine = Arc::clone(&engine);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            engine.ensure_ready().map(|info| info.version)
        }));
    }
    for handle in handles {
        let result = handle.join().expect("ensure_ready thread");
        assert_eq!(
            result.as_deref(),
            Ok("1.18.25"),
            "concurrent ensure_ready caller must succeed, got {result:?}"
        );
    }
    assert_eq!(engine.status(), AgentStatus::Ready);
    engine.shutdown().expect("shutdown");
    assert_eq!(engine.status(), AgentStatus::Stopped);
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr().map(|addr| addr.port()))
        .expect("free loopback port")
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
