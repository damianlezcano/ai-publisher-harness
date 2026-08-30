//! Shared `opencode serve` process ownership and loopback HTTP client.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use project_process::{ChildGuard, ProcessError};
use serde_json::Value;

use crate::error::{BackendError, BackendResult};
use crate::status::BackendStatus;
use crate::{Semver, build_argv, build_env};

const DEFAULT_STARTUP: Duration = Duration::from_secs(30);
const DEFAULT_SHUTDOWN: Duration = Duration::from_secs(5);
const HTTP_ATTEMPT: Duration = Duration::from_secs(2);

pub struct OpenCodeBackend {
    binary: PathBuf,
    config_dir: PathBuf,
    port: u16,
    min: Semver,
    max_exclusive: Semver,
    version_range: String,
    extra_env: Vec<(String, String)>,
    startup_timeout: Duration,
    shutdown_timeout: Duration,
    client: reqwest::blocking::Client,
    inner: Mutex<Inner>,
}

struct Inner {
    base_url: Option<String>,
    guard: Option<ChildGuard>,
    status: BackendStatus,
    version: Option<String>,
}

impl OpenCodeBackend {
    /// Production: resolve/spawn `opencode serve` on 127.0.0.1:<port> with an
    /// isolated XDG config, lazily. Version range default ">=1.18 <2".
    pub fn new(binary: PathBuf, config_dir: PathBuf, port: u16) -> Self {
        Self {
            binary,
            config_dir,
            port,
            min: Semver::parse("1.18").unwrap_or(Semver(1, 18, 0)),
            max_exclusive: Semver::parse("2").unwrap_or(Semver(2, 0, 0)),
            version_range: ">=1.18 <2".into(),
            extra_env: Vec::new(),
            startup_timeout: DEFAULT_STARTUP,
            shutdown_timeout: DEFAULT_SHUTDOWN,
            client: reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(HTTP_ATTEMPT)
                .build()
                .expect("reqwest client"),
            inner: Mutex::new(Inner {
                base_url: None,
                guard: None,
                status: BackendStatus::Stopped,
                version: None,
            }),
        }
    }

    pub fn set_version_range(&mut self, min: &str, max_exclusive: &str) {
        self.min = Semver::parse(min).unwrap_or(Semver(0, 0, 0));
        self.max_exclusive = Semver::parse(max_exclusive).unwrap_or(Semver(u64::MAX, 0, 0));
        self.version_range = format!(">={min} <{max_exclusive}");
    }

    /// Test seam: use an already-known base URL (no spawn). `ensure_ready` still
    /// probes `/global/health` and checks version.
    pub fn set_base_url(&self, base_url: String) {
        let mut inner = lock(&self.inner);
        inner.base_url = Some(trim_slash(&base_url));
    }

    pub fn set_startup_timeout(&mut self, startup: Duration) {
        self.startup_timeout = startup;
    }

    /// Extra child environment after the isolated XDG/PATH/HOME set (tests).
    pub fn push_env(&mut self, key: String, value: String) {
        self.extra_env.push((key, value));
    }

    pub fn status(&self) -> BackendStatus {
        lock(&self.inner).status
    }

    pub fn base_url(&self) -> Option<String> {
        lock(&self.inner).base_url.clone()
    }

    pub fn require_ready(&self) -> BackendResult<String> {
        let inner = lock(&self.inner);
        if inner.status != BackendStatus::Ready {
            return Err(BackendError::NotReady);
        }
        inner.base_url.clone().ok_or(BackendError::NotReady)
    }

    pub fn get(&self, path: &str) -> BackendResult<(u16, String)> {
        let url = self.url_for(path)?;
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|err| BackendError::Http(err.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|err| BackendError::Http(err.to_string()))?;
        Ok((status, text))
    }

    pub fn post(&self, path: &str, body: &Value) -> BackendResult<(u16, String)> {
        let url = self.url_for(path)?;
        let response = self
            .client
            .post(&url)
            .json(body)
            .send()
            .map_err(|err| BackendError::Http(err.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|err| BackendError::Http(err.to_string()))?;
        Ok((status, text))
    }

    pub fn delete(&self, path: &str) -> BackendResult<(u16, String)> {
        let url = self.url_for(path)?;
        let response = self
            .client
            .delete(&url)
            .send()
            .map_err(|err| BackendError::Http(err.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|err| BackendError::Http(err.to_string()))?;
        Ok((status, text))
    }

    pub fn ensure_ready(&self) -> BackendResult<String> {
        let existing = {
            let inner = lock(&self.inner);
            inner.base_url.clone()
        };
        if let Some(base) = existing {
            match self.probe_health(&base) {
                Ok((true, version)) => {
                    self.check_version(&version).map_err(|err| self.fail(err))?;
                    let mut inner = lock(&self.inner);
                    inner.status = BackendStatus::Ready;
                    inner.version = Some(version.clone());
                    log_event("backend ready");
                    return Ok(version);
                }
                Ok((false, version)) => {
                    return Err(
                        self.fail(BackendError::StartFailed(format!("unhealthy ({version})")))
                    );
                }
                Err(err) => return Err(self.fail(err)),
            }
        }

        {
            let mut inner = lock(&self.inner);
            inner.status = BackendStatus::Starting;
        }
        log_event("backend starting");
        {
            let mut inner = lock(&self.inner);
            if let Err(err) = self.spawn_backend(&mut inner) {
                inner.status = BackendStatus::Failed;
                return Err(err);
            }
        }

        match self.wait_until_healthy() {
            Ok(version) => {
                if let Err(err) = self.check_version(&version) {
                    return Err(self.fail(err));
                }
                let mut inner = lock(&self.inner);
                inner.status = BackendStatus::Ready;
                inner.version = Some(version.clone());
                log_event("backend ready");
                Ok(version)
            }
            Err(err) => Err(self.fail(err)),
        }
    }

    pub fn shutdown(&self) -> BackendResult<()> {
        let mut inner = lock(&self.inner);
        if inner.status == BackendStatus::Stopped && inner.guard.is_none() {
            return Ok(());
        }
        let had_process = inner.guard.is_some();
        if let Some(mut guard) = inner.guard.take() {
            guard.request_stop();
            if guard.wait(self.shutdown_timeout).is_err() {
                guard.force_kill();
            }
        }
        if had_process {
            inner.base_url = None;
        }
        inner.status = BackendStatus::Stopped;
        log_event("backend stopped");
        Ok(())
    }

    fn url_for(&self, path: &str) -> BackendResult<String> {
        let base = self.base_url().ok_or(BackendError::NotReady)?;
        let path = path.trim_start_matches('/');
        Ok(format!("{base}/{path}"))
    }

    fn probe_health(&self, base: &str) -> BackendResult<(bool, String)> {
        let url = format!("{base}/global/health");
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|err| BackendError::Http(err.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|err| BackendError::Http(err.to_string()))?;
        if !status.is_success() {
            return Err(BackendError::Http(format!("health status {status}")));
        }
        let value: Value = serde_json::from_str(&body)
            .map_err(|err| BackendError::Http(format!("malformed health JSON: {err}")))?;
        let healthy = value
            .get("healthy")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let version = value
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        Ok((healthy, version))
    }

    fn check_version(&self, found: &str) -> BackendResult<()> {
        let Some(parsed) = Semver::parse(found) else {
            return Err(BackendError::IncompatibleVersion {
                found: found.to_owned(),
                expected: self.version_range.clone(),
            });
        };
        if parsed < self.min || parsed >= self.max_exclusive {
            return Err(BackendError::IncompatibleVersion {
                found: found.to_owned(),
                expected: self.version_range.clone(),
            });
        }
        Ok(())
    }

    fn fail(&self, err: BackendError) -> BackendError {
        let mut inner = lock(&self.inner);
        if let Some(mut guard) = inner.guard.take() {
            guard.force_kill();
        }
        inner.status = BackendStatus::Failed;
        err
    }

    fn spawn_backend(&self, inner: &mut Inner) -> BackendResult<()> {
        fs::create_dir_all(&self.config_dir)
            .map_err(|err| BackendError::StartFailed(err.to_string()))?;
        fs::create_dir_all(self.config_dir.join("data"))
            .map_err(|err| BackendError::StartFailed(err.to_string()))?;
        fs::create_dir_all(self.config_dir.join("cache"))
            .map_err(|err| BackendError::StartFailed(err.to_string()))?;
        fs::create_dir_all(self.config_dir.join("state"))
            .map_err(|err| BackendError::StartFailed(err.to_string()))?;

        let argv = build_argv(self.port);
        let mut envs = build_env(&self.config_dir);
        envs.extend(self.extra_env.iter().cloned());

        let guard = ChildGuard::spawn(&self.binary, &argv, &envs).map_err(map_process_error)?;
        inner.guard = Some(guard);
        inner.base_url = Some(format!("http://127.0.0.1:{}", self.port));
        Ok(())
    }

    fn wait_until_healthy(&self) -> BackendResult<String> {
        let deadline = Instant::now() + self.startup_timeout;
        loop {
            let base = lock(&self.inner).base_url.clone();
            let Some(base) = base else {
                return Err(BackendError::StartFailed("missing base url".into()));
            };

            {
                let mut inner = lock(&self.inner);
                if let Some(guard) = inner.guard.as_mut()
                    && let Some(status) = guard.try_wait()
                {
                    return Err(BackendError::StartFailed(format!(
                        "process exited ({:?})",
                        status.code()
                    )));
                }
            }

            match self.probe_health(&base) {
                Ok((true, version)) => return Ok(version),
                Ok((false, _)) | Err(BackendError::Http(_)) => {}
                Err(err) => return Err(err),
            }

            if Instant::now() >= deadline {
                return Err(BackendError::Timeout);
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for OpenCodeBackend {
    fn drop(&mut self) {
        let mut inner = lock(&self.inner);
        if let Some(mut guard) = inner.guard.take() {
            guard.request_stop();
            if guard.wait(self.shutdown_timeout).is_err() {
                guard.force_kill();
            }
        }
        inner.status = BackendStatus::Stopped;
    }
}

fn lock(mutex: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn map_process_error(err: ProcessError) -> BackendError {
    match err {
        ProcessError::BinaryNotFound(name) => BackendError::BinaryNotFound(name),
        ProcessError::StartFailed(reason) => BackendError::StartFailed(reason),
        ProcessError::Timeout => BackendError::Timeout,
        ProcessError::StopFailed(reason) => BackendError::ShutdownFailed(reason),
    }
}

fn log_event(event: &str) {
    eprintln!("[agent] {event}");
}

fn trim_slash(url: &str) -> String {
    url.trim_end_matches('/').to_owned()
}
