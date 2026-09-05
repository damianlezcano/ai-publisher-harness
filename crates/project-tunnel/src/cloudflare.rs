use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::{Duration, Instant};

use project_process::{ChildGuard, ProcessError};

use crate::log::{self, extract_base_url};
use crate::model::{LocalOrigin, TunnelSession, TunnelState};
use crate::port::TunnelProvider;
use crate::resolver::BinaryResolver;
use crate::{TunnelError, TunnelResult};

const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Cloudflare Quick Tunnel adapter over `ChildGuard`.
///
/// Extra process environment for tests is injected with [`Self::with_env`]
/// (for example `FAKE_PROCESS_MODE`). Production callers leave this empty.
pub struct CloudflareQuickTunnel {
    resolver: Box<dyn BinaryResolver>,
    guard: Option<ChildGuard>,
    state: TunnelState,
    startup_timeout: Duration,
    shutdown_timeout: Duration,
    extra_env: Vec<(String, String)>,
}

impl CloudflareQuickTunnel {
    pub fn new(resolver: Box<dyn BinaryResolver>) -> Self {
        Self::with_timeouts(resolver, DEFAULT_STARTUP_TIMEOUT, DEFAULT_SHUTDOWN_TIMEOUT)
    }

    pub fn with_timeouts(
        resolver: Box<dyn BinaryResolver>,
        startup: Duration,
        shutdown: Duration,
    ) -> Self {
        Self {
            resolver,
            guard: None,
            state: TunnelState::Stopped,
            startup_timeout: startup,
            shutdown_timeout: shutdown,
            extra_env: Vec::new(),
        }
    }

    /// Append an environment variable used at spawn (after the managed child
    /// env: PATH/HOME plus the Windows SYSTEMROOT).
    pub fn with_env(mut self, key: String, value: String) -> Self {
        self.extra_env.push((key, value));
        self
    }

    /// Child pid when a process is supervised (tests asserting cleanup).
    pub fn child_pid(&self) -> Option<u32> {
        self.guard.as_ref().map(ChildGuard::pid)
    }

    fn abort_guard(&mut self) {
        if let Some(mut guard) = self.guard.take() {
            guard.force_kill();
        }
    }

    fn fail(&mut self, stage: &str, err: TunnelError) -> TunnelError {
        self.abort_guard();
        self.state = TunnelState::Failed {
            reason: fail_reason(&err).into(),
        };
        log::emit(&format!("failed stage={stage} error={}", fail_detail(&err)));
        err
    }

    fn start_inner(&mut self, origin: LocalOrigin) -> TunnelResult<TunnelSession> {
        let started = Instant::now();
        let binary = self
            .resolver
            .resolve()
            .map_err(|err| self.fail("binary_resolve", err))?;
        let argv = vec![
            "tunnel".to_string(),
            "--url".to_string(),
            origin.as_str().to_string(),
            "--no-autoupdate".to_string(),
            "--loglevel".to_string(),
            "info".to_string(),
        ];
        let envs = build_child_env(&self.extra_env);

        let guard = ChildGuard::spawn(&binary, &argv, &envs)
            .map_err(map_process_error)
            .map_err(|err| self.fail("spawn", err))?;
        log::emit(&format!("process pid={}", guard.pid()));
        let lines = guard.lines();
        self.guard = Some(guard);

        let deadline = Instant::now() + self.startup_timeout;
        loop {
            match lines.try_recv() {
                Ok(line) => {
                    if let Some(base_url) = extract_base_url(&line) {
                        self.state = TunnelState::Running {
                            base_url: base_url.clone(),
                        };
                        log::emit(&format!(
                            "public_url={} elapsed_ms={}",
                            base_url.as_str(),
                            started.elapsed().as_millis()
                        ));
                        return Ok(TunnelSession::new(base_url));
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {}
            }

            if let Some(status) = self.guard.as_mut().and_then(ChildGuard::try_wait) {
                log::emit(&format!("exited status={:?}", status.code()));
                let err = TunnelError::ProcessExited {
                    code: status.code(),
                };
                return Err(self.fail("process_exited", err));
            }

            if Instant::now() >= deadline {
                return Err(self.fail("url_acquisition", TunnelError::StartupTimeout));
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Child environment for `cloudflared`. The supervisor clears the inherited
/// environment; only PATH/HOME and (on Windows) the parent `SYSTEMROOT` value
/// are provided, plus any test-only `extra_env`.
///
/// Windows DNS/getaddrinfo requires `SYSTEMROOT` after `env_clear`: without it
/// `cloudflared` cannot resolve `api.trycloudflare.com`, the Quick Tunnel
/// request fails, and the process exits with code 1. Only that single variable
/// is forwarded; no other parent variable is inherited. Non-Windows targets
/// forward nothing extra.
pub fn build_child_env(extra_env: &[(String, String)]) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        ),
        (
            "HOME".to_string(),
            std::env::var("HOME").unwrap_or_default(),
        ),
    ];
    env.extend(windows_systemroot_env());
    env.extend(extra_env.iter().cloned());
    env
}

/// On Windows the reconstructed child environment must carry the parent
/// `SYSTEMROOT` value verbatim, mirroring the proven OpenCode child policy.
/// When the parent value is absent the existing minimal convention applies
/// (empty value, no hardcoded fallback); the launch keeps the existing
/// process-exit surfacing.
#[cfg(windows)]
fn windows_systemroot_env() -> Vec<(String, String)> {
    vec![(
        "SYSTEMROOT".to_string(),
        std::env::var("SYSTEMROOT").unwrap_or_default(),
    )]
}

#[cfg(not(windows))]
fn windows_systemroot_env() -> Vec<(String, String)> {
    Vec::new()
}

fn map_process_error(err: ProcessError) -> TunnelError {
    match err {
        ProcessError::BinaryNotFound(name) => TunnelError::BinaryNotFound(name),
        ProcessError::StartFailed(reason) => TunnelError::StartFailed(reason),
        ProcessError::Timeout => TunnelError::StartFailed("timeout".into()),
        ProcessError::StopFailed(reason) => TunnelError::StopFailed(reason),
    }
}

fn fail_reason(err: &TunnelError) -> &'static str {
    match err {
        TunnelError::StartupTimeout => "startup timeout",
        TunnelError::ProcessExited { .. } => "process exited",
        TunnelError::BinaryNotFound(_) => "binary not found",
        TunnelError::StartFailed(_) => "start failed",
        TunnelError::AlreadyRunning => "already running",
        _ => "failed",
    }
}

/// Diagnostic detail for the error stage log. Safe: only process-level
/// metadata (never credentials, prompts, or artifact contents). Binary paths
/// are trimmed to the bare name to honor the crate's no-paths log contract.
fn fail_detail(err: &TunnelError) -> String {
    match err {
        TunnelError::StartFailed(reason) => format!("start failed: {reason}"),
        TunnelError::StopFailed(reason) => format!("stop failed: {reason}"),
        TunnelError::BinaryNotFound(name) => {
            let name = std::path::Path::new(name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("cloudflared");
            format!("binary not found: {name}")
        }
        other => other.to_string(),
    }
}

impl TunnelProvider for CloudflareQuickTunnel {
    fn start(&mut self, origin: LocalOrigin) -> TunnelResult<TunnelSession> {
        if self.is_running() {
            return Err(TunnelError::AlreadyRunning);
        }
        self.state = TunnelState::Starting;
        log::emit(&format!("starting origin={}", origin.as_str()));
        self.start_inner(origin)
    }

    fn session(&self) -> Option<TunnelSession> {
        match &self.state {
            TunnelState::Running { base_url } => Some(TunnelSession::new(base_url.clone())),
            _ => None,
        }
    }

    fn state(&self) -> TunnelState {
        self.state.clone()
    }

    fn stop(&mut self) -> TunnelResult<()> {
        if !self.is_running() {
            return Err(TunnelError::NotRunning);
        }
        self.state = TunnelState::Stopping;
        let pid = self.guard.as_ref().map(ChildGuard::pid);
        log::emit(&format!(
            "stopping pid={}",
            pid.map(|pid| pid.to_string())
                .unwrap_or_else(|| "none".into())
        ));
        let mut result = Ok(());
        if let Some(mut guard) = self.guard.take() {
            guard.request_stop();
            if guard.wait(self.shutdown_timeout).is_err() {
                guard.force_kill();
                result = Err(TunnelError::StopFailed("shutdown timeout".into()));
            }
        }
        self.state = TunnelState::Stopped;
        log::emit(&format!(
            "stopped pid={}",
            pid.map(|pid| pid.to_string())
                .unwrap_or_else(|| "none".into())
        ));
        result
    }

    fn is_running(&self) -> bool {
        matches!(self.state, TunnelState::Running { .. })
    }
}

impl Drop for CloudflareQuickTunnel {
    fn drop(&mut self) {
        if let Some(mut guard) = self.guard.take() {
            guard.request_stop();
            if guard.wait(self.shutdown_timeout).is_err() {
                guard.force_kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_child_env;
    use std::collections::BTreeSet;

    #[test]
    fn child_env_keeps_managed_keys_and_forwards_windows_systemroot() {
        let env = build_child_env(&[]);
        let keys: BTreeSet<&str> = env.iter().map(|(k, _)| k.as_str()).collect();

        let mut expected: BTreeSet<&str> = ["PATH", "HOME"].into_iter().collect();

        #[cfg(windows)]
        {
            expected.insert("SYSTEMROOT");
            let parent = std::env::var("SYSTEMROOT").unwrap_or_default();
            let value = env
                .iter()
                .find(|(k, _)| k == "SYSTEMROOT")
                .map(|(_, v)| v.as_str());
            assert_eq!(
                value,
                Some(parent.as_str()),
                "SYSTEMROOT must be forwarded verbatim from the parent env"
            );
        }

        #[cfg(not(windows))]
        {
            assert!(
                !keys.contains("SYSTEMROOT"),
                "SYSTEMROOT must not be forwarded on non-Windows targets"
            );
        }

        assert_eq!(
            keys, expected,
            "child env must contain exactly the managed keys, never arbitrary parent variables"
        );
    }

    #[test]
    fn child_env_preserves_path_and_home_verbatim() {
        let env = build_child_env(&[]);
        let parent_path = std::env::var("PATH").unwrap_or_default();
        let parent_home = std::env::var("HOME").unwrap_or_default();

        let path = env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.as_str());
        let home = env
            .iter()
            .find(|(k, _)| k == "HOME")
            .map(|(_, v)| v.as_str());
        assert_eq!(path, Some(parent_path.as_str()), "PATH preserved");
        assert_eq!(home, Some(parent_home.as_str()), "HOME preserved");
    }

    #[test]
    fn child_env_appends_extra_env_after_managed_keys() {
        let env = build_child_env(&[("FAKE_PROCESS_MODE".into(), "print".into())]);
        let value = env
            .iter()
            .find(|(k, _)| k == "FAKE_PROCESS_MODE")
            .map(|(_, v)| v.as_str());
        assert_eq!(value, Some("print"));
    }
}
