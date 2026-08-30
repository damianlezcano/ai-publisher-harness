use std::path::{Path, PathBuf};
use std::time::Duration;

use project_tunnel::log::extract_base_url;
use project_tunnel::model::{LocalOrigin, PublicBaseUrl, TunnelState};
use project_tunnel::{
    BinaryResolver, CloudflareQuickTunnel, FixedBinaryResolver, PathBinaryResolver, TunnelError,
    TunnelProvider,
};

fn fake_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-cloudflared"))
}

fn origin() -> LocalOrigin {
    LocalOrigin::from_port(8080).expect("valid origin")
}

fn tunnel_with_mode(mode: &str, startup: Duration, shutdown: Duration) -> CloudflareQuickTunnel {
    CloudflareQuickTunnel::with_timeouts(
        Box::new(FixedBinaryResolver::new(fake_binary())),
        startup,
        shutdown,
    )
    .with_env("FAKE_CLOUDFLARED_MODE".to_string(), mode.to_string())
}

fn expected_base() -> PublicBaseUrl {
    PublicBaseUrl::parse("https://fake-123.trycloudflare.com/").expect("valid base url")
}

fn process_exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[test]
fn extract_base_url_accepts_first_valid_https_trycloudflare_token() {
    let url = extract_base_url("2026-01-01T00:00:01Z INF https://fake-123.trycloudflare.com")
        .expect("valid url");
    assert_eq!(url.as_str(), "https://fake-123.trycloudflare.com/");
}

#[test]
fn extract_base_url_rejects_non_https_and_non_trycloudflare() {
    assert!(extract_base_url("http://evil.example.com").is_none());
    assert!(extract_base_url("https://not-trycloudflare.example.com/x").is_none());
    assert!(extract_base_url("https://user@fake-123.trycloudflare.com/").is_none());
}

#[test]
fn start_detects_url_from_stdout() {
    let mut tunnel = tunnel_with_mode("url", Duration::from_secs(2), Duration::from_secs(1));
    let session = tunnel.start(origin()).expect("start");
    assert_eq!(session.base_url(), &expected_base());
    assert!(tunnel.is_running());
    assert!(matches!(
        tunnel.state(),
        TunnelState::Running { base_url } if base_url == expected_base()
    ));
    tunnel.stop().expect("stop");
}

#[test]
fn start_detects_url_from_stderr() {
    let mut tunnel = tunnel_with_mode("url_stderr", Duration::from_secs(2), Duration::from_secs(1));
    let session = tunnel.start(origin()).expect("start");
    assert_eq!(
        session.base_url().as_str(),
        "https://fake-123.trycloudflare.com/"
    );
    tunnel.stop().expect("stop");
}

#[test]
fn garbage_urls_are_ignored_until_valid_url() {
    let mut tunnel = tunnel_with_mode(
        "garbage_then_url",
        Duration::from_secs(2),
        Duration::from_secs(1),
    );
    let session = tunnel.start(origin()).expect("start");
    assert_eq!(session.base_url(), &expected_base());
    tunnel.stop().expect("stop");
}

#[test]
fn garbage_only_is_not_accepted() {
    let mut tunnel = tunnel_with_mode(
        "garbage",
        Duration::from_millis(250),
        Duration::from_secs(1),
    );
    let err = tunnel.start(origin()).expect_err("must not become ready");
    assert_eq!(err, TunnelError::StartupTimeout);
    assert!(!tunnel.is_running());
    assert!(matches!(tunnel.state(), TunnelState::Failed { .. }));
}

#[test]
fn process_exit_before_url() {
    let mut tunnel = tunnel_with_mode(
        "exit_before_url",
        Duration::from_secs(2),
        Duration::from_secs(1),
    );
    let err = tunnel.start(origin()).expect_err("must fail");
    assert_eq!(err, TunnelError::ProcessExited { code: Some(1) });
    assert!(!tunnel.is_running());
}

#[test]
fn startup_timeout_on_silent_child() {
    let mut tunnel = tunnel_with_mode("silent", Duration::from_millis(150), Duration::from_secs(1));
    let err = tunnel.start(origin()).expect_err("must time out");
    assert_eq!(err, TunnelError::StartupTimeout);
    assert!(matches!(tunnel.state(), TunnelState::Failed { .. }));
}

#[test]
fn repeated_start_while_running_is_already_running() {
    let mut tunnel = tunnel_with_mode("url", Duration::from_secs(2), Duration::from_secs(1));
    tunnel.start(origin()).expect("start");
    let err = tunnel.start(origin()).expect_err("already running");
    assert_eq!(err, TunnelError::AlreadyRunning);
    tunnel.stop().expect("stop");
}

#[test]
fn stop_sets_stopped_and_repeated_stop_is_not_running() {
    let mut tunnel = tunnel_with_mode("url", Duration::from_secs(2), Duration::from_secs(1));
    tunnel.start(origin()).expect("start");
    tunnel.stop().expect("stop");
    assert_eq!(tunnel.state(), TunnelState::Stopped);
    assert!(tunnel.session().is_none());
    assert!(tunnel.child_pid().is_none());
    let err = tunnel.stop().expect_err("not running");
    assert_eq!(err, TunnelError::NotRunning);
}

#[test]
fn start_after_stop_reuses_adapter() {
    let mut tunnel = tunnel_with_mode("url", Duration::from_secs(2), Duration::from_secs(1));
    tunnel.start(origin()).expect("first start");
    tunnel.stop().expect("stop");
    let session = tunnel.start(origin()).expect("second start");
    assert_eq!(session.base_url(), &expected_base());
    tunnel.stop().expect("stop");
}

#[test]
fn stop_force_kills_when_graceful_stop_times_out() {
    let mut tunnel = tunnel_with_mode(
        "url_ignore_term",
        Duration::from_secs(2),
        Duration::from_millis(80),
    );
    tunnel.start(origin()).expect("start");
    let pid = tunnel.child_pid().expect("pid");
    let result = tunnel.stop();
    assert!(
        result.is_ok() || matches!(result, Err(TunnelError::StopFailed(_))),
        "{result:?}"
    );
    assert_eq!(tunnel.state(), TunnelState::Stopped);
    assert!(!process_exists(pid));
}

#[test]
fn drop_cleans_up_running_tunnel() {
    let pid;
    {
        let mut tunnel = tunnel_with_mode("url", Duration::from_secs(2), Duration::from_secs(1));
        tunnel.start(origin()).expect("start");
        pid = tunnel.child_pid().expect("pid");
        assert!(process_exists(pid));
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while process_exists(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!process_exists(pid), "drop should kill the child");
}

#[test]
fn path_resolver_finds_true_and_rejects_missing() {
    PathBinaryResolver::new("true")
        .resolve()
        .expect("true should be on PATH");
    let err = PathBinaryResolver::new("not-a-real-cloudflared-binary-xyz")
        .resolve()
        .expect_err("missing binary");
    assert!(matches!(err, TunnelError::BinaryNotFound(name) if name.contains("not-a-real")));
}
