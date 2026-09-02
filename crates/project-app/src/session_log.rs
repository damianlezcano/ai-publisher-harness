//! Process-local diagnostics. This intentionally has no filesystem backend.
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

const CAPACITY: usize = 500;
static LOGS: OnceLock<Mutex<VecDeque<SessionLogEntry>>> = OnceLock::new();
static MIN_LEVEL: OnceLock<LogLevel> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}
impl LogLevel {
    fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "debug" => Self::Debug,
            "warn" | "warning" => Self::Warn,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
    fn from_entry(value: &str) -> Self {
        Self::parse(value)
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLogEntry {
    pub level: String,
    pub message: String,
}

fn buffer() -> &'static Mutex<VecDeque<SessionLogEntry>> {
    LOGS.get_or_init(|| Mutex::new(VecDeque::new()))
}
/// Metadata only: callers must never pass prompts, secrets, paths, or generated contents.
pub fn record(level: &str, message: impl Into<String>) {
    if LogLevel::from_entry(level) < *MIN_LEVEL.get_or_init(|| LogLevel::Info) {
        return;
    }
    let message = message.into();
    eprintln!("[EducAI][{level}] {message}");
    let mut logs = buffer().lock().unwrap_or_else(|e| e.into_inner());
    if logs.len() == CAPACITY {
        logs.pop_front();
    }
    logs.push_back(SessionLogEntry {
        level: level.to_owned(),
        message,
    });
}
/// Minimal launch parser: `--debug` or `--log-level debug|info|warn|error`.
pub fn configure_from_args(args: impl IntoIterator<Item = String>) {
    let mut args = args.into_iter();
    let mut level = LogLevel::Info;
    while let Some(arg) = args.next() {
        if arg == "--debug" {
            level = LogLevel::Debug;
        }
        if arg == "--log-level" {
            if let Some(value) = args.next() {
                level = LogLevel::parse(&value);
            }
        }
    }
    let _ = MIN_LEVEL.set(level);
}
pub fn list() -> Vec<SessionLogEntry> {
    buffer()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}
pub fn clear() {
    buffer().lock().unwrap_or_else(|e| e.into_inner()).clear();
}
