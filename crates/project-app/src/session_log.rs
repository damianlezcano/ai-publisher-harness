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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<SessionUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<SessionKnowledgeMetrics>,
}

/// Sanitized, per-turn remote-provider accounting for the session viewer.
///
/// One logical user turn may legitimately cause several remote calls (K6
/// document/batch/global synthesis). This record is the AGGREGATE of every
/// remote call that turn caused, correlated to the durable user turn id and a
/// reason code. Missing fields mean the backend did not report that value; they
/// are never converted to zero.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsage {
    pub conversation_id: String,
    pub turn_id: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    /// Elapsed wall-clock time for the completed turn. This is local
    /// structural timing, not provider billing telemetry.
    pub turn_duration_ms: Option<u128>,
    pub source: String,
    /// Number of distinct remote provider calls this logical turn caused,
    /// including K6 hierarchical/retry synthesis where applicable.
    pub remote_calls: Option<usize>,
    /// Why the remote call(s) were made: `normal_chat`, `summary_global`,
    /// `recovery`, `retry`, or `other`. Structural only, never content.
    pub reason: String,
    /// True when the remote request boundary also carried content through the
    /// raw attachment route in addition to (or instead of) bounded Knowledge
    /// evidence. Never asserted when raw forwarding was correctly suppressed.
    pub additional_attachment_route: bool,
}

/// Local architectural estimates. These are deliberately separate from remote
/// provider telemetry and contain no material names or content.
///
/// Corpus/index counts (`material_count`, `corpus_bytes`, `corpus_utf8_chars`,
/// `corpus_est_tokens`) are always known once the local store opens. Per-turn
/// retrieval/evidence/preparation facts only exist for a K3/K4 chat turn; a K6
/// summary turn has no K4 candidate/evidence set, so those fields are `None`
/// (rendered "No disponible") rather than an invented zero.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionKnowledgeMetrics {
    pub conversation_id: String,
    pub material_count: usize,
    pub corpus_bytes: u64,
    pub corpus_utf8_chars: usize,
    pub corpus_est_tokens: usize,
    pub retrieval_candidate_count: Option<usize>,
    pub selected_evidence_count: Option<usize>,
    pub selected_evidence_bytes: Option<usize>,
    pub selected_evidence_utf8_chars: Option<usize>,
    pub evidence_est_tokens: Option<usize>,
    pub context_reduction_pct: Option<usize>,
    pub semantic_provider_state: String,
    pub request_preparation_ms: Option<u128>,
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
        usage: None,
        knowledge: None,
    });
}

pub fn record_usage(usage: SessionUsage) {
    let message = format!(
        "[usage] conversation_id={} turn_id={} provider={} model={} input_tokens={} output_tokens={} cache_read_tokens={} cache_write_tokens={} total_tokens={} cost_usd={} usage_source={} remote_calls={} reason={} additional_attachment_route={}",
        usage.conversation_id,
        usage.turn_id,
        usage.provider,
        usage.model,
        optional_u64(usage.input_tokens),
        optional_u64(usage.output_tokens),
        optional_u64(usage.cache_read_tokens),
        optional_u64(usage.cache_write_tokens),
        optional_u64(usage.total_tokens),
        optional_f64(usage.cost_usd),
        usage.source,
        usage
            .remote_calls
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unavailable".to_owned()),
        usage.reason,
        usage.additional_attachment_route,
    );
    record_structured("INFO", message, Some(usage), None);
}

pub fn record_knowledge(metrics: SessionKnowledgeMetrics, message: String) {
    record_structured("INFO", message, None, Some(metrics));
}

fn record_structured(
    level: &str,
    message: String,
    usage: Option<SessionUsage>,
    knowledge: Option<SessionKnowledgeMetrics>,
) {
    if LogLevel::from_entry(level) < *min_level().lock().unwrap_or_else(|e| e.into_inner()) {
        return;
    }
    eprintln!("[EducAI][{level}] {message}");
    let mut logs = buffer().lock().unwrap_or_else(|e| e.into_inner());
    if logs.len() == CAPACITY {
        logs.pop_front();
    }
    logs.push_back(SessionLogEntry {
        level: level.to_owned(),
        message,
        usage,
        knowledge,
    });
}

fn optional_u64(value: Option<u64>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unavailable".to_owned())
}

fn optional_f64(value: Option<f64>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unavailable".to_owned())
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

    #[test]
    fn knowledge_metrics_are_structural_and_do_not_need_content_fields() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        clear();
        configure_from_args(["--log-level".to_owned(), "info".to_owned()]);
        record_knowledge(
            SessionKnowledgeMetrics {
                conversation_id: "conversation-a".to_owned(),
                material_count: 2,
                corpus_bytes: 2048,
                corpus_utf8_chars: 2000,
                corpus_est_tokens: 683,
                retrieval_candidate_count: Some(9),
                selected_evidence_count: Some(3),
                selected_evidence_bytes: Some(600),
                selected_evidence_utf8_chars: Some(580),
                evidence_est_tokens: Some(200),
                context_reduction_pct: Some(70),
                semantic_provider_state: "available".to_owned(),
                request_preparation_ms: Some(12),
            },
            "[knowledge] structural counts only".to_owned(),
        );
        let value = serde_json::to_value(list().pop().expect("knowledge entry")).unwrap();
        assert_eq!(value["knowledge"]["corpusBytes"], 2048);
        assert_eq!(value["knowledge"]["selectedEvidenceCount"], 3);
        assert!(value["knowledge"].get("prompt").is_none());
        assert!(value["knowledge"].get("content").is_none());
        assert!(value["knowledge"].get("path").is_none());
        clear();
    }

    #[test]
    fn summary_knowledge_metrics_keep_unknown_fields_null_never_zero() {
        // A K6 summary turn has no K4 retrieval/evidence set. Those per-turn
        // fields must serialize as `null` (rendered "No disponible"), never an
        // invented zero, while corpus/index facts remain concrete.
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        clear();
        configure_from_args(["--log-level".to_owned(), "info".to_owned()]);
        record_knowledge(
            SessionKnowledgeMetrics {
                conversation_id: "conversation-b".to_owned(),
                material_count: 1,
                corpus_bytes: 400,
                corpus_utf8_chars: 380,
                corpus_est_tokens: 134,
                retrieval_candidate_count: None,
                selected_evidence_count: None,
                selected_evidence_bytes: None,
                selected_evidence_utf8_chars: None,
                evidence_est_tokens: None,
                context_reduction_pct: None,
                semantic_provider_state: "available".to_owned(),
                request_preparation_ms: None,
            },
            "[knowledge] summary_corpus only".to_owned(),
        );
        let value = serde_json::to_value(list().pop().expect("knowledge entry")).unwrap();
        assert_eq!(value["knowledge"]["materialCount"], 1);
        assert_eq!(value["knowledge"]["corpusBytes"], 400);
        assert!(value["knowledge"]["retrievalCandidateCount"].is_null());
        assert!(value["knowledge"]["selectedEvidenceCount"].is_null());
        assert!(value["knowledge"]["contextReductionPct"].is_null());
        assert!(value["knowledge"]["requestPreparationMs"].is_null());
        clear();
    }
}
