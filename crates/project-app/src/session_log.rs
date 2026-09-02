//! Process-local diagnostics. This intentionally has no filesystem backend.
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

const CAPACITY: usize = 500;
static LOGS: OnceLock<Mutex<VecDeque<SessionLogEntry>>> = OnceLock::new();

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
