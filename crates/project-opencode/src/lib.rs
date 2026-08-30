//! Shared OpenCode serve process, loopback HTTP client, and isolated XDG env.

#![forbid(unsafe_code)]

mod backend;
mod error;
mod status;

use std::path::Path;

pub use backend::OpenCodeBackend;
pub use error::{BackendError, BackendResult};
pub use status::BackendStatus;

/// Loopback-only argv for `opencode serve`. No shell involved: every element
/// is a literal token passed via `Command::args`.
pub fn build_argv(port: u16) -> Vec<String> {
    vec![
        "serve".into(),
        "--hostname".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--pure".into(),
    ]
}

/// Child environment with the user's environment cleared (by the supervisor)
/// and replaced by PATH/HOME plus isolated XDG dirs under `config_dir`.
pub fn build_env(config_dir: &Path) -> Vec<(String, String)> {
    vec![
        ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
        ("HOME".into(), std::env::var("HOME").unwrap_or_default()),
        ("XDG_CONFIG_HOME".into(), config_dir.display().to_string()),
        (
            "XDG_DATA_HOME".into(),
            config_dir.join("data").display().to_string(),
        ),
        (
            "XDG_CACHE_HOME".into(),
            config_dir.join("cache").display().to_string(),
        ),
        (
            "XDG_STATE_HOME".into(),
            config_dir.join("state").display().to_string(),
        ),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Semver(u64, u64, u64);

impl Semver {
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim().trim_start_matches(['v', 'V', '>', '=', '<']);
        let core = trimmed.split(['-', '+', ' ']).next().unwrap_or("");
        if core.is_empty() {
            return None;
        }
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let patch = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Some(Semver(major, minor, patch))
    }
}
