//! M4 tunnel security invariants, offline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use project_process::ChildGuard;
use project_tunnel::TunnelError;
use project_tunnel::model::{LocalOrigin, PublicBaseUrl};

fn fake_binary() -> PathBuf {
    let exe = std::env::current_exe().expect("current test executable");
    let mut dir = exe.parent().expect("exe parent").to_path_buf();
    if dir.file_name().is_some_and(|name| name == "deps") {
        dir.pop();
    }
    let bin = dir.join(format!("fake-process{}", std::env::consts::EXE_SUFFIX));
    assert!(
        bin.is_file(),
        "fake-process binary not found at {}",
        bin.display()
    );
    bin
}

fn spawn_child(mode: &str, envs: &[(&str, String)]) -> ChildGuard {
    let binary = fake_binary();
    let mut vars: Vec<(String, String)> = vec![("FAKE_PROCESS_MODE".to_string(), mode.into())];
    vars.extend(envs.iter().map(|(k, v)| (k.to_string(), v.clone())));
    ChildGuard::spawn(&binary, &[], &vars).expect("spawn fake-process")
}

fn process_exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn collect_lines(rx: &std::sync::mpsc::Receiver<String>, timeout: Duration) -> Vec<String> {
    let deadline = Instant::now() + timeout;
    let mut lines = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(line) => lines.push(line),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    lines
}

fn sensitive_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    [
        "CLOUDFLARE",
        "TOKEN",
        "API_KEY",
        "SECRET",
        "AWS",
        "CREDENTIAL",
    ]
    .iter()
    .any(|needle| upper.contains(*needle))
}

#[test]
fn origin_is_loopback_only() {
    for input in [
        "http://0.0.0.0:8080/",
        "http://localhost:8080/",
        "http://192.168.1.5:8080/",
        "http://[::1]:8080/",
        "https://127.0.0.1:8080/",
    ] {
        assert!(
            matches!(
                LocalOrigin::parse(input),
                Err(TunnelError::InvalidOrigin(_))
            ),
            "must reject {input:?}"
        );
    }
    let origin = LocalOrigin::parse("http://127.0.0.1:8080/").expect("loopback origin");
    assert_eq!(origin.port(), 8080);
    assert_eq!(origin.as_str(), "http://127.0.0.1:8080/");
}

#[test]
fn origin_cannot_carry_shell_metacharacters() {
    for input in [
        "http://127.0.0.1:8080/;rm -rf /",
        "http://127.0.0.1:80 80/",
        "http://127.0.0.1:8080/ \"x\"",
        "http://127.0.0.1:8080/'x'",
        "http://127.0.0.1:8080/;echo pwned",
    ] {
        assert!(
            matches!(
                LocalOrigin::parse(input),
                Err(TunnelError::InvalidOrigin(_))
            ),
            "must reject {input:?}"
        );
    }
    let origin = LocalOrigin::from_port(8080).expect("loopback port");
    let forbidden = [' ', '\'', '"', ';', '$', '`', '&', '|', '<', '>'];
    assert!(
        !origin.as_str().chars().any(|c| forbidden.contains(&c)),
        "stored origin must carry no shell metacharacters: {}",
        origin.as_str()
    );
}

#[test]
fn public_base_url_rejects_non_https_and_foreign_hosts() {
    for input in [
        "http://abc.trycloudflare.com/",
        "https://abc.example.com/",
        "https://trycloudflare.com/",
        "https://user@abc.trycloudflare.com/",
        "https://abc.trycloudflare.com/path",
    ] {
        assert!(
            matches!(
                PublicBaseUrl::parse(input),
                Err(TunnelError::InvalidBaseUrl(_))
            ),
            "must reject {input:?}"
        );
    }
    let url = PublicBaseUrl::parse("https://abc.trycloudflare.com/").expect("valid base url");
    assert_eq!(url.host(), "abc.trycloudflare.com");
}

#[test]
fn no_env_leakage_to_child() {
    let mut guard = spawn_child(
        "env",
        &[
            ("PATH", std::env::var("PATH").unwrap_or_default()),
            ("HOME", std::env::var("HOME").unwrap_or_default()),
        ],
    );
    let rx = guard.lines();
    let lines = collect_lines(&rx, Duration::from_secs(2));

    let mut printed: BTreeMap<String, String> = BTreeMap::new();
    for line in &lines {
        if let Some((key, value)) = line.split_once('=') {
            printed.insert(key.to_string(), value.to_string());
        }
    }

    assert!(
        printed.contains_key("PATH"),
        "child must receive PATH, printed: {lines:?}"
    );
    assert!(
        printed.contains_key("HOME"),
        "child must receive HOME, printed: {lines:?}"
    );
    assert_eq!(
        printed.get("FAKE_PROCESS_MODE").map(String::as_str),
        Some("env")
    );

    for key in printed.keys() {
        if key == "FAKE_PROCESS_MODE" {
            continue;
        }
        assert!(
            !sensitive_key(key),
            "child env leaked secret-like variable {key}"
        );
    }

    let parent_has_sensitive = std::env::vars().any(|(key, _)| sensitive_key(&key));
    if !parent_has_sensitive {
        assert_eq!(
            printed.len(),
            3,
            "child env should be exactly PATH, HOME, FAKE_PROCESS_MODE, printed: {printed:?}"
        );
    }

    guard.request_stop();
    let _ = guard.wait(Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn child_cleanup_on_drop_no_orphan() {
    let pid;
    {
        let guard = spawn_child("silent", &[]);
        pid = guard.pid();
        assert!(process_exists(pid), "child should be alive before drop");
        assert_ne!(std::process::id(), pid);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while process_exists(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !process_exists(pid),
        "dropping the guard must kill and reap the child"
    );
    assert!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_err(),
        "signal 0 to a reaped pid must fail"
    );
}
