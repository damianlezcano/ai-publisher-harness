//! M5 agent security: loopback-only spawn argv, isolated XDG env, distinct
//! per-project working directories, and workspace-scoped artifact diff parsing.

use std::path::{Path, PathBuf};

use project_agent::model::{AgentProject, AgentPrompt};
use project_agent::{AgentEngine, OpenCodeAgentEngine};
use project_opencode::{build_argv, build_env};

#[path = "support/fake_server.rs"]
#[allow(dead_code)]
mod fake_server;

use fake_server::FakeServer;

fn engine_for(server: &FakeServer) -> OpenCodeAgentEngine {
    OpenCodeAgentEngine::new(PathBuf::from("/usr/bin/true"), unique_config_dir(), 0)
        .with_base_url(server.base_url())
}

fn unique_config_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!("project-agent-sec-{}-{n}", std::process::id()))
}

fn project(project_id: &str, workspace_dir: &str) -> AgentProject {
    AgentProject {
        project_id: project_id.into(),
        directory: workspace_dir.into(),
    }
}

fn prompt() -> AgentPrompt {
    AgentPrompt {
        text: "create an activity".into(),
        model: None,
    }
}

#[test]
fn build_argv_is_loopback_only_and_pure() {
    let argv = build_argv(4567);

    let host_index = argv
        .iter()
        .position(|arg| arg == "--hostname")
        .expect("--hostname flag");
    assert_eq!(argv[host_index + 1], "127.0.0.1");

    let port_index = argv
        .iter()
        .position(|arg| arg == "--port")
        .expect("--port flag");
    assert_eq!(argv[port_index + 1], "4567");

    assert!(argv.iter().any(|arg| arg == "serve"));
    assert!(argv.iter().any(|arg| arg == "--pure"));

    assert!(!argv.iter().any(|arg| arg == "--mdns"));
    assert!(!argv.iter().any(|arg| arg == "0.0.0.0"));

    for shell_token in ["-c", "sh", "bash", "&&", "|", ";"] {
        assert!(
            !argv.iter().any(|arg| arg == shell_token),
            "argv must not contain shell token {shell_token:?}"
        );
    }
}

#[test]
fn build_env_isolates_xdg_and_leaks_no_secrets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_dir = temp.path().join("opencode-config");
    let env = build_env(&config_dir);

    let value = |key: &str| -> Option<String> {
        env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };

    assert_eq!(
        value("XDG_CONFIG_HOME").as_deref(),
        Some(config_dir.display().to_string().as_str())
    );
    assert_eq!(
        value("XDG_DATA_HOME").as_deref(),
        Some(config_dir.join("data").display().to_string().as_str())
    );
    assert_eq!(
        value("XDG_CACHE_HOME").as_deref(),
        Some(config_dir.join("cache").display().to_string().as_str())
    );
    assert_eq!(
        value("XDG_STATE_HOME").as_deref(),
        Some(config_dir.join("state").display().to_string().as_str())
    );
    assert!(value("PATH").is_some(), "PATH must be present");
    assert!(value("HOME").is_some(), "HOME must be present");

    for secret_fragment in [
        "TOKEN",
        "API_KEY",
        "SECRET",
        "PASSWORD",
        "CREDENTIAL",
        "AWS",
    ] {
        assert!(
            !env.iter()
                .any(|(key, _)| { key.to_ascii_uppercase().contains(secret_fragment) }),
            "env must not leak a key containing {secret_fragment}"
        );
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let real_opencode = Path::new(&home).join(".config").join("opencode");
    for (key, value) in &env {
        assert_ne!(
            value,
            &real_opencode.display().to_string(),
            "{key} points at the user's real opencode config"
        );
        assert!(
            !Path::new(value).ends_with(".config/opencode"),
            "{key} has an opencode suffix outside the isolated config dir: {value}"
        );
    }
}

#[test]
fn project_a_b_use_distinct_working_directories() {
    let server = FakeServer::start();
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");

    engine
        .open_session(&project("proj-a", "/tmp/proj-a/workspace"))
        .expect("session a");
    let dir_a = server.last_directory().expect("directory a");

    engine
        .open_session(&project("proj-b", "/tmp/proj-b/workspace"))
        .expect("session b");
    let dir_b = server.last_directory().expect("directory b");

    assert_eq!(dir_a, "/tmp/proj-a/workspace");
    assert_eq!(dir_b, "/tmp/proj-b/workspace");
    assert_ne!(dir_a, dir_b);
}

#[test]
fn malicious_diff_paths_are_rejected() {
    let server = FakeServer::start();
    server.set_diff_body(
        r#"[
            {"path":"workspace/../../etc/passwd","byte_size":1},
            {"path":"workspace/../secret.txt","byte_size":1},
            {"path":"/etc/passwd","byte_size":1},
            {"path":"workspace/../inputs/x","byte_size":1},
            {"path":"workspace/index.html","byte_size":12,"sha256":"abc"}
        ]"#,
    );
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine
        .open_session(&project("proj-7", "/tmp/proj-7/workspace"))
        .expect("session");
    let task = engine.send(&session, &prompt()).expect("send");

    assert_eq!(task.artifacts.len(), 1);
    let artifact = &task.artifacts[0];
    assert_eq!(artifact.path, "workspace/index.html");
    assert_eq!(artifact.byte_size, 12);
    assert_eq!(artifact.sha256.as_deref(), Some("abc"));
}

#[test]
fn output_outside_workspace_ignored() {
    let server = FakeServer::start();
    server.set_diff_body(
        r#"[
            {"path":"outputs/index.html","byte_size":1},
            {"path":"publish/index.html","byte_size":1},
            {"path":"project.json","byte_size":1}
        ]"#,
    );
    let engine = engine_for(&server);
    engine.ensure_ready().expect("ready");
    let session = engine
        .open_session(&project("proj-7", "/tmp/proj-7/workspace"))
        .expect("session");
    let task = engine.send(&session, &prompt()).expect("send");

    assert!(task.artifacts.is_empty());
}
