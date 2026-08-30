use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use project_tunnel::TunnelError;
use project_tunnel::supervisor::ChildGuard;

const URL_NEEDLE: &str = "https://fake-123.trycloudflare.com";

fn fake_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-cloudflared"))
}

fn spawn_mode(mode: &str) -> ChildGuard {
    let binary = fake_binary();
    let argv = vec![binary.to_string_lossy().into_owned()];
    let envs = vec![("FAKE_CLOUDFLARED_MODE".to_string(), mode.to_string())];
    ChildGuard::spawn(&binary, &argv, &envs).expect("spawn fake-cloudflared")
}

fn recv_line_containing(
    rx: &std::sync::mpsc::Receiver<String>,
    needle: &str,
    timeout: Duration,
) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            panic!("timed out waiting for line containing {needle:?}");
        }
        match rx.recv_timeout(remaining) {
            Ok(line) if line.contains(needle) => return line,
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => {
                panic!("timed out waiting for line containing {needle:?}");
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("line channel disconnected before {needle:?}");
            }
        }
    }
}

fn process_exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[test]
fn spawn_captures_url_line_from_stdout() {
    let guard = spawn_mode("url");
    let rx = guard.lines();
    let line = recv_line_containing(&rx, URL_NEEDLE, Duration::from_secs(2));
    assert!(line.contains(URL_NEEDLE), "{line}");
}

#[test]
fn spawn_captures_url_line_from_stderr() {
    let guard = spawn_mode("url_stderr");
    let rx = guard.lines();
    let line = recv_line_containing(&rx, URL_NEEDLE, Duration::from_secs(2));
    assert!(line.contains(URL_NEEDLE), "{line}");
}

#[test]
fn try_wait_detects_early_exit() {
    let mut guard = spawn_mode("exit_before_url");
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = guard.try_wait() {
            break status;
        }
        if Instant::now() >= deadline {
            panic!("child did not exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(!status.success());
}

#[test]
fn request_stop_stops_running_child() {
    let mut guard = spawn_mode("url");
    let _ = guard.lines();
    guard.request_stop();
    let status = guard
        .wait(Duration::from_secs(2))
        .expect("child should exit after request_stop");
    assert!(!process_exists(guard.pid()));
    let _ = status;
}

#[test]
fn force_kill_stops_silent_child() {
    let mut guard = spawn_mode("silent");
    let pid = guard.pid();
    assert!(process_exists(pid));
    guard.force_kill();
    assert!(guard.try_wait().is_some() || !process_exists(pid));
    assert!(!process_exists(pid));
}

#[test]
fn wait_times_out_if_child_stays_alive() {
    let mut guard = spawn_mode("silent");
    let err = guard
        .wait(Duration::from_millis(80))
        .expect_err("silent child must not exit");
    assert_eq!(err, TunnelError::StartupTimeout);
    guard.force_kill();
}

#[test]
fn flood_does_not_deadlock_and_stays_bounded() {
    let mut guard = spawn_mode("flood");
    let rx = guard.lines();
    let mut seen = 0usize;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && seen < 32 {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(_) => seen += 1,
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    assert!(guard.try_wait().is_none(), "flood child should stay alive");
    guard.force_kill();
    let _ = guard.wait(Duration::from_secs(2));
}

#[test]
fn malformed_utf8_is_lossy_decoded() {
    let guard = spawn_mode("malformed");
    let rx = guard.lines();
    let url = recv_line_containing(&rx, URL_NEEDLE, Duration::from_secs(2));
    assert!(url.contains(URL_NEEDLE), "{url}");
}

#[test]
fn drop_kills_still_running_child() {
    let pid;
    {
        let guard = spawn_mode("silent");
        pid = guard.pid();
        assert!(process_exists(pid));
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while process_exists(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!process_exists(pid), "drop should kill and reap the child");
}
