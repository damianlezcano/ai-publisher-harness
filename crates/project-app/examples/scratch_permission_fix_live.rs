//! Mini live A/B of scratch permission profiles (tool-safe vs text-producing).
//!
//! Not product code. Prompt/assistant bodies are never logged.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use project_app::session_contract::{self, SessionContract, SessionSpec};
use project_opencode::OpenCodeBackend;
use serde_json::{Value, json};

const MODEL: (&str, &str) = ("opencode", "big-pickle");
const PROMPT: &str = "hola";
const MAX_POLLS: usize = 300;
const POLL_INTERVAL: Duration = Duration::from_millis(100);

fn deny(permission: &str, pattern: &str) -> Value {
    json!({
        "permission": permission,
        "pattern": pattern,
        "action": "deny",
    })
}

const CODING_TOOLS: &[&str] = &[
    "bash",
    "read",
    "edit",
    "glob",
    "grep",
    "task",
    "skill",
    "lsp",
    "webfetch",
    "websearch",
    "todowrite",
    "question",
];

/// Candidate B: explicit coding-tool denies with pattern `*` (hides tools via
/// OpenCode `Permission.disabled`).
fn explicit_star_denies() -> Value {
    let mut rules: Vec<Value> = CODING_TOOLS.iter().map(|tool| deny(tool, "*")).collect();
    rules.push(deny("external_directory", "*"));
    Value::Array(rules)
}

/// Candidate C: same tools denied at execution time with pattern `**`, which
/// OpenCode 1.18.25 still wildcard-matches, but `Permission.disabled` only hides
/// tools when the last matching rule's pattern is exactly `"*"`.
fn explicit_execution_denies() -> Value {
    let mut rules: Vec<Value> = CODING_TOOLS.iter().map(|tool| deny(tool, "**")).collect();
    rules.push(deny("external_directory", "*"));
    Value::Array(rules)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("scratch_permission_fix_live: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let started = Instant::now();
    let scratch = tempfile::tempdir().map_err(|err| err.to_string())?;
    let scratch_dir = scratch
        .path()
        .join("scratch")
        .to_string_lossy()
        .replace('\\', "/");
    std::fs::create_dir_all(&scratch_dir).map_err(|err| err.to_string())?;

    let backend = OpenCodeBackend::new(sidecar_binary(), opencode_config_dir(), free_port()?);
    backend
        .ensure_ready()
        .map_err(|err| format!("backend not ready: {err}"))?;

    let model = Some((MODEL.0.to_owned(), MODEL.1.to_owned()));
    let control = SessionSpec::baseline()
        .with_label("A")
        .with_fresh_session(true)
        .with_directory(Some(scratch_dir.clone()))
        .with_directory_kind("scratch")
        .with_permission(Some(project_opencode::external_directory_deny_permission()))
        .with_permission_profile("ordinary_external_directory_deny")
        .with_agent(None)
        .with_prompt(Some(PROMPT.to_owned()))
        .with_model(model.clone());
    let star_denies = control
        .clone()
        .with_label("B")
        .with_permission(Some(explicit_star_denies()))
        .with_permission_profile("explicit_star_denies");
    let execution_denies = control
        .clone()
        .with_label("C")
        .with_permission(Some(explicit_execution_denies()))
        .with_permission_profile("explicit_execution_denies");

    assert_one_dim(&control, &star_denies, "permission");
    assert_one_dim(&control, &execution_denies, "permission");

    let only = std::env::var("SCRATCH_PERMISSION_LIVE_ONLY").unwrap_or_default();
    let mut calls = 0usize;
    if only == "confirm" {
        let helper = control
            .clone()
            .with_label("classifier")
            .with_permission(Some(project_opencode::scratch_tool_free_permission()))
            .with_permission_profile("scratch_tool_free");
        let classifier = capture(&backend, &helper, &mut calls);
        emit(
            "classifier",
            "POST-IMPLEMENT helper (classifier-like)",
            &helper,
            &classifier,
        );
        let summarizer = capture(
            &backend,
            &helper.clone().with_label("summarizer"),
            &mut calls,
        );
        emit(
            "summarizer",
            "POST-IMPLEMENT helper (summarizer-like)",
            &helper,
            &summarizer,
        );
        eprintln!(
            "scratch_permission_fix_live: classifier_ok={} summarizer_ok={} classifier_text_len={} summarizer_text_len={}",
            produced_text(&classifier) && !classifier.error_present,
            produced_text(&summarizer) && !summarizer.error_present,
            classifier.text_len,
            summarizer.text_len
        );
        shutdown(&backend, started, calls);
        return Ok(());
    }
    if only != "C" {
        let a = capture(&backend, &control, &mut calls);
        emit(
            "A",
            "CONTROL ordinary external_directory deny",
            &control,
            &a,
        );
        if !produced_text(&a) {
            eprintln!(
                "scratch_permission_fix_live: STOP control A empty terminal={} elapsed_ms={}",
                a.terminal_classification, a.elapsed_ms
            );
            shutdown(&backend, started, calls);
            return Ok(());
        }

        if only != "skip-B" {
            let b = capture(&backend, &star_denies, &mut calls);
            emit(
                "B",
                "explicit coding-tool * denies, no global *",
                &star_denies,
                &b,
            );
            eprintln!(
                "scratch_permission_fix_live: B_ok={} text_len={} finish={} error={} terminal={}",
                produced_text(&b) && !b.error_present,
                b.text_len,
                b.finish.as_deref().unwrap_or("None"),
                b.error_present,
                b.terminal_classification
            );
        }
    }

    let c = capture(&backend, &execution_denies, &mut calls);
    emit(
        "C",
        "explicit coding-tool ** execution denies + external_directory *",
        &execution_denies,
        &c,
    );
    eprintln!(
        "scratch_permission_fix_live: C_ok={} text_len={} finish={} error={} terminal={}",
        produced_text(&c) && !c.error_present,
        c.text_len,
        c.finish.as_deref().unwrap_or("None"),
        c.error_present,
        c.terminal_classification
    );

    shutdown(&backend, started, calls);
    Ok(())
}

fn capture(backend: &OpenCodeBackend, spec: &SessionSpec, calls: &mut usize) -> SessionContract {
    *calls += 1;
    session_contract::capture(backend, spec, MAX_POLLS, POLL_INTERVAL)
}

fn emit(step: &str, changed: &str, spec: &SessionSpec, contract: &SessionContract) {
    eprintln!(
        "step={step} changed={changed} directory_kind={} permission_profile={} http_status={} assistant_seen={} parent_match={} part_types={:?} text_present={} text_len={} finish={} time_completed={} error_present={} elapsed_ms={} terminal={} prompt_async_status={} create_session_status={}",
        spec.directory_kind,
        spec.permission_profile,
        contract.prompt_async_status,
        contract.assistant_seen,
        contract.parent_match.unwrap_or("none"),
        contract.assistant_part_types,
        contract.text_present,
        contract.text_len,
        contract.finish.as_deref().unwrap_or("None"),
        contract.time_completed,
        contract.error_present,
        contract.elapsed_ms,
        contract.terminal_classification,
        contract.prompt_async_status,
        contract.create_session_status,
    );
}

fn produced_text(contract: &SessionContract) -> bool {
    contract.text_present && contract.text_len > 0
}

fn assert_one_dim(a: &SessionSpec, b: &SessionSpec, expected: &str) {
    let dims = session_contract::differing_dimensions(a, b);
    if dims != [expected] {
        panic!(
            "ladder {} → {} must change only {expected}, got {dims:?}",
            a.label, b.label
        );
    }
}

fn sidecar_binary() -> PathBuf {
    let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../sidecars/opencode-x86_64-unknown-linux-gnu");
    if bundled.is_file() {
        bundled
    } else {
        PathBuf::from("opencode")
    }
}

fn opencode_config_dir() -> PathBuf {
    if let Some(override_dir) = std::env::var_os("EDUCAI_OPENCODE_CONFIG_DIR") {
        return PathBuf::from(override_dir);
    }
    dirs_data_home().join("com.educai.publisher/opencode")
}

fn dirs_data_home() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg);
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
}

fn free_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|err| format!("bind ephemeral: {err}"))?;
    Ok(listener
        .local_addr()
        .map_err(|err| format!("local_addr: {err}"))?
        .port())
}

fn shutdown(backend: &OpenCodeBackend, started: Instant, calls: usize) {
    let _ = backend.shutdown();
    eprintln!(
        "scratch_permission_fix_live: provider_calls={calls} wall_ms={} model={}/{}",
        started.elapsed().as_millis(),
        MODEL.0,
        MODEL.1
    );
}
