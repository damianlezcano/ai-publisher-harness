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

/// Append `directory` as a query parameter, matching OpenCode 1.18.25
/// (`POST /session?directory=`). A JSON body field named `directory` is not
/// in the create-session schema and is ignored, so the sidecar would otherwise
/// bind the session to its process cwd (the AppImage mount).
pub fn with_directory_query(path: &str, directory: &str) -> String {
    let encoded = encode_query_component(directory);
    if path.ends_with('?') {
        format!("{path}directory={encoded}")
    } else if path.contains('?') {
        format!("{path}&directory={encoded}")
    } else {
        format!("{path}?directory={encoded}")
    }
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::new();
    for &byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
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

#[cfg(test)]
mod tests {
    use super::with_directory_query;

    #[test]
    fn directory_query_percent_encodes_path_separators() {
        let path = with_directory_query("/session", "/tmp/proj-7/workspace");
        assert_eq!(path, "/session?directory=%2Ftmp%2Fproj-7%2Fworkspace");
    }

    #[test]
    fn directory_query_appends_to_existing_query() {
        let path = with_directory_query("/session?limit=1", "/home/a");
        assert_eq!(path, "/session?limit=1&directory=%2Fhome%2Fa");
    }

    #[test]
    fn directory_query_does_not_insert_ampersand_after_trailing_question_mark() {
        let path = with_directory_query("/session?", "/tmp/a");
        assert_eq!(path, "/session?directory=%2Ftmp%2Fa");
    }

    #[test]
    fn directory_query_percent_encodes_ampersand_equals_plus_space_and_non_ascii() {
        let path = with_directory_query("/session", "/tmp/a&b=c+d e/café");
        assert_eq!(
            path,
            "/session?directory=%2Ftmp%2Fa%26b%3Dc%2Bd%20e%2Fcaf%C3%A9"
        );
    }
}
