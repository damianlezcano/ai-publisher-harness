//! Process-local diagnostics. This intentionally has no filesystem backend.
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

const CAPACITY: usize = 500;
static LOGS: OnceLock<Mutex<VecDeque<SessionLogEntry>>> = OnceLock::new();
static MIN_LEVEL: OnceLock<Mutex<LogLevel>> = OnceLock::new();

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
fn min_level() -> &'static Mutex<LogLevel> {
    MIN_LEVEL.get_or_init(|| Mutex::new(LogLevel::Info))
}
/// Metadata only: callers must never pass prompts, secrets, paths, or generated contents.
pub fn record(level: &str, message: impl Into<String>) {
    if LogLevel::from_entry(level) < *min_level().lock().unwrap_or_else(|e| e.into_inner()) {
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
        if arg == "--log-level"
            && let Some(value) = args.next()
        {
            level = LogLevel::parse(&value);
        }
    }
    *min_level().lock().unwrap_or_else(|e| e.into_inner()) = level;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn bounded_levels_and_clear_are_process_local() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        clear();
        configure_from_args(["--debug".to_owned()]);
        for index in 0..501 {
            record("INFO", format!("entry-{index}"));
        }
        let entries = list();
        assert_eq!(entries.len(), 500);
        assert_eq!(
            entries.first().map(|entry| entry.message.as_str()),
            Some("entry-1")
        );
        assert_eq!(
            LogLevel::Debug.cmp(&LogLevel::Info),
            std::cmp::Ordering::Less
        );
        clear();
        assert!(list().is_empty());
    }

    #[test]
    fn configure_from_args_sets_level_ordering() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        clear();
        configure_from_args(["--log-level".to_owned(), "warn".to_owned()]);
        record("INFO", "hidden");
        record("WARN", "shown");
        assert_eq!(
            list()
                .iter()
                .map(|entry| entry.message.as_str())
                .collect::<Vec<_>>(),
            ["shown"]
        );
        configure_from_args(["--log-level".to_owned(), "error".to_owned()]);
        record("WARN", "hidden-again");
        record("ERROR", "error");
        assert_eq!(
            list()
                .iter()
                .map(|entry| entry.message.as_str())
                .collect::<Vec<_>>(),
            ["shown", "error"]
        );
        clear();
    }
}
