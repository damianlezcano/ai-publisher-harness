//! Provider-independent remote summarization execution over the shared
//! OpenCode backend (K6).
//!
//! A summarization request is a bounded, self-contained prompt that runs in a
//! **dedicated scratch session**, never the project's chat session, so
//! background summary generation never creates a user-visible chat turn and
//! never reuses conversation history. This module depends only on the generic
//! `project_opencode::OpenCodeBackend` HTTP surface, not on any provider-
//! specific API or the agent chat adapter.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use project_knowledge::{RemoteSummarizer, SummaryFailure, SummaryOutput, SummaryRequest};
use project_opencode::{OpenCodeBackend, with_directory_query};
use serde_json::{Value, json};

const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(20);
const SUMMARY_TASK_TIMEOUT: Duration = Duration::from_secs(120);
const MESSAGE_LIMIT: &str = "1000";

/// The bounded summary operation requested by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryIntent {
    /// Ordinary question answering: K3/K4 retrieval remains appropriate.
    None,
    /// A request to summarize the complete existing project corpus.
    Project,
    /// An explicit request to summarize each source selected in this turn.
    SelectedPerSource,
}

/// Provider- and language-independent summarization intent.
///
/// This is deliberately not a single hardcoded Spanish string trigger. It
/// detects a *summary verb* plus an *all/corpus scope marker* across a small
/// multilingual set, so "resumime todos los archivos", "summarize all files",
/// "resumí el proyecto", etc. resolve to the same internal operation. Ordinary
/// informational questions (which carry no corpus-scope marker) continue through
/// K3/K4 chat.
pub fn detect_summary_intent(text: &str, selected_attachment_count: usize) -> SummaryIntent {
    let normalized = text.to_lowercase();
    let has_summary_verb = SUMMARY_VERBS.iter().any(|verb| normalized.contains(verb));
    if !has_summary_verb {
        return SummaryIntent::None;
    }
    if PROJECT_SCOPE_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return SummaryIntent::Project;
    }
    let explicit_per_source = PER_SOURCE_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker));
    if explicit_per_source && selected_attachment_count > 0 {
        return SummaryIntent::SelectedPerSource;
    }
    // Corpus-wide wording without a current composer selection keeps the
    // historical whole-project K6 route. Attachments are what convert the
    // same wording into an exact selected-set inventory.
    if explicit_per_source {
        return SummaryIntent::Project;
    }
    SummaryIntent::None
}

/// Compatibility predicate for callers that only need to distinguish summary
/// work from normal semantic chat. New routing must use [`detect_summary_intent`]
/// so selected source identity is retained.
pub fn detect_summarize_intent(text: &str) -> bool {
    detect_summary_intent(text, 1) != SummaryIntent::None
}

const SUMMARY_VERBS: &[&str] = &[
    "resum",
    "resumen",
    "summar",
    "síntesis",
    "sintetiz",
    "sintesis",
];
const PROJECT_SCOPE_MARKERS: &[&str] = &[
    "el proyecto",
    "the project",
    "carpeta entera",
    "whole project",
    "entire project",
    "proyecto entero",
];
const PER_SOURCE_MARKERS: &[&str] = &[
    "todos",
    "todas",
    "cada archivo",
    "cada documento",
    "each attached file",
    "each file",
    "each document",
    "todo el",
    "toda la",
    "los archivos",
    "los documentos",
    "las notas",
    "all files",
    "all documents",
    // Explicit single-source summarization: "resumime el archivo", "resumí
    // este documento", "sintetizá el material". These route to K6 so a single
    // supported indexed document is summarized through bounded evidence instead
    // of being raw-forwarded AND then also summarized by a normal chat run.
    "el archivo",
    "este archivo",
    "el documento",
    "este documento",
    "el material",
    "este material",
    "el adjunto",
    "el texto",
    "este texto",
];

/// Executes one bounded summarization request against the shared OpenCode
/// backend in an isolated scratch session.
pub struct OpenCodeRemoteSummarizer {
    backend: Arc<OpenCodeBackend>,
    scratch_dir: PathBuf,
    task_timeout: Duration,
    model: Option<(String, String)>,
}

impl OpenCodeRemoteSummarizer {
    pub fn new(backend: Arc<OpenCodeBackend>, scratch_dir: PathBuf) -> Self {
        Self {
            backend,
            scratch_dir,
            task_timeout: SUMMARY_TASK_TIMEOUT,
            model: None,
        }
    }

    /// Pins the same provider/model the conversation already selected so the
    /// scratch session does not depend on OpenCode's default-model resolution.
    pub fn with_model(mut self, provider_id: String, model_id: String) -> Self {
        self.model = Some((provider_id, model_id));
        self
    }

    /// Test-only bound so a broken poll cannot stall the suite for 120s per node.
    #[cfg(test)]
    pub fn with_task_timeout(mut self, task_timeout: Duration) -> Self {
        self.task_timeout = task_timeout;
        self
    }

    /// Builds one self-contained prompt from a request (evidence-labelled, with
    /// the structured JSON contract instruction). Evidence text only crosses the
    /// boundary; no corpus, paths, or model internals are ever sent.
    fn build_prompt(request: &SummaryRequest) -> String {
        let mut body = String::new();
        body.push_str(&request.instruction);
        body.push('\n');
        body.push_str("\nEvidencia (ÚNICAMENTE esta evidencia):\n");
        for (evidence_ref, text) in request.labels.iter().zip(&request.evidence_texts) {
            body.push_str(&format!(
                "\n[{}] (fuente: {}):\n{}\n",
                evidence_ref.label, evidence_ref.source_name, text
            ));
        }
        body
    }
}

impl RemoteSummarizer for OpenCodeRemoteSummarizer {
    fn summarize(
        &self,
        request: &SummaryRequest,
    ) -> std::result::Result<SummaryOutput, SummaryFailure> {
        let prompt = Self::build_prompt(request);

        if let Err(_err) = self.backend.ensure_ready() {
            return Err(SummaryFailure::ProviderUnavailable);
        }

        let directory = self.scratch_dir.to_string_lossy().replace('\\', "/");
        let path = with_directory_query("/session", &directory);
        // Scratch synthesis is not a coding-agent turn. An empty ruleset lets
        // OpenCode 1.18.25 request tools; a blocked permission never writes
        // `info.finish: "stop"` and the poll hits SUMMARY_TASK_TIMEOUT (120s).
        let (status, text) = self
            .backend
            .post(
                &path,
                &json!({
                    "permission": [
                        {
                            "permission": "external_directory",
                            "pattern": "*",
                            "action": "deny"
                        },
                        {
                            "permission": "*",
                            "pattern": "*",
                            "action": "deny"
                        }
                    ]
                }),
            )
            .map_err(|_| SummaryFailure::ExecutionFailed)?;
        if !(200..300).contains(&status) {
            return Err(SummaryFailure::ExecutionFailed);
        }
        let value: Value =
            serde_json::from_str(&text).map_err(|_| SummaryFailure::ExecutionFailed)?;
        let session_id = session_id_from_value(&value).ok_or(SummaryFailure::ExecutionFailed)?;
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge][summary] node_kind={} session_id={} call=start",
                summary_level_name(request.level),
                session_id
            ),
        );
        let abort_path = with_directory_query(&format!("/session/{session_id}/abort"), &directory);
        let usage_before = session_usage(&self.backend, &session_id, &directory);

        let message_path = with_directory_query(
            &format!("/session/{session_id}/message?limit={MESSAGE_LIMIT}"),
            &directory,
        );
        let before_ids = match self.backend.get(&message_path) {
            Ok((status, body)) if (200..300).contains(&status) => message_ids_from_body(&body),
            _ => std::collections::HashSet::new(),
        };

        let mut body = json!({ "parts": [{ "type": "text", "text": prompt }] });
        if let Some((provider_id, model_id)) = &self.model {
            body["model"] = json!({
                "providerID": provider_id,
                "modelID": model_id,
            });
        }
        let prompt_path =
            with_directory_query(&format!("/session/{session_id}/prompt_async"), &directory);
        let (status, _) = self
            .backend
            .post(&prompt_path, &body)
            .map_err(|_| SummaryFailure::ExecutionFailed)?;
        if !(200..300).contains(&status) && status != 204 {
            let _ = self.backend.post(&abort_path, &json!({}));
            crate::session_log::record(
                "WARN",
                format!(
                    "[knowledge][summary] node_kind={} session_id={} error_class=execution_failed timeout_reason=prompt_http",
                    summary_level_name(request.level),
                    session_id
                ),
            );
            return Err(SummaryFailure::ExecutionFailed);
        }

        // Poll this scratch session for the assistant stop that belongs to the
        // prompt just submitted. OpenCode 1.18.25 `GET /session/{id}/message`
        // returns a bare array of `{info, parts}`. A reused session can still
        // contain an earlier `finish:"stop"`; that stale row must not complete
        // the current request.
        let started = Instant::now();
        let deadline = started + self.task_timeout;
        let mut poll_count = 0usize;
        let mut originating_user_id: Option<String> = None;
        loop {
            poll_count += 1;
            let (status, body) = match self.backend.get(&message_path) {
                Ok(response) => response,
                Err(_) => {
                    if Instant::now() >= deadline {
                        let _ = self.backend.post(&abort_path, &json!({}));
                        crate::session_log::record(
                            "WARN",
                            format!(
                                "[knowledge][summary] node_kind={} session_id={} poll_count={} error_class=execution_failed timeout_reason=poll_transport elapsed_ms={}",
                                summary_level_name(request.level),
                                session_id,
                                poll_count,
                                started.elapsed().as_millis()
                            ),
                        );
                        return Err(SummaryFailure::ExecutionFailed);
                    }
                    thread::sleep(STATUS_POLL_INTERVAL);
                    continue;
                }
            };
            if (200..300).contains(&status)
                && let Some(messages) = session_messages_from_body(&body)
            {
                if originating_user_id.is_none() {
                    originating_user_id = newest_user_id_not_in(&messages, &before_ids);
                }
                let message_count = messages.len();
                if let Some(text) =
                    terminal_assistant_text(&messages, originating_user_id.as_deref(), &before_ids)
                {
                    crate::session_log::record(
                        "INFO",
                        format!(
                            "[knowledge][summary] node_kind={} session_id={} poll_count={} message_count={} finish=stop elapsed_ms={} error_class=none",
                            summary_level_name(request.level),
                            session_id,
                            poll_count,
                            message_count,
                            started.elapsed().as_millis()
                        ),
                    );
                    let usage = session_usage(&self.backend, &session_id, &directory)
                        .map(|after| usage_delta(usage_before.as_ref(), &after))
                        .unwrap_or_default();
                    let _ = self.backend.post(&abort_path, &json!({}));
                    return Ok(SummaryOutput {
                        text,
                        model_id: self.model.as_ref().map(|(_, model)| model.clone()),
                        provider_id: self.model.as_ref().map(|(provider, _)| provider.clone()),
                        usage,
                    });
                }
            }
            if Instant::now() >= deadline {
                let _ = self.backend.post(&abort_path, &json!({}));
                crate::session_log::record(
                    "WARN",
                    format!(
                        "[knowledge][summary] node_kind={} session_id={} poll_count={} error_class=execution_failed timeout_reason=finish_stop_missing elapsed_ms={}",
                        summary_level_name(request.level),
                        session_id,
                        poll_count,
                        started.elapsed().as_millis()
                    ),
                );
                return Err(SummaryFailure::ExecutionFailed);
            }
            thread::sleep(STATUS_POLL_INTERVAL);
        }
    }
}

fn summary_level_name(level: project_knowledge::SummaryLevel) -> &'static str {
    match level {
        project_knowledge::SummaryLevel::Document => "document",
        project_knowledge::SummaryLevel::Batch => "batch",
        project_knowledge::SummaryLevel::Global => "global",
    }
}

fn session_id_from_value(value: &Value) -> Option<String> {
    value
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("data")
                .and_then(|data| data.get("id"))
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
}

fn session_usage(
    backend: &OpenCodeBackend,
    session_id: &str,
    directory: &str,
) -> Option<project_knowledge::SummaryUsage> {
    let path = with_directory_query(&format!("/session/{session_id}"), directory);
    let (status, body) = backend.get(&path).ok()?;
    if !(200..300).contains(&status) {
        return None;
    }
    let value: Value = serde_json::from_str(&body).ok()?;
    let tokens = value.get("tokens");
    let input_tokens = tokens.and_then(|v| v.get("input")).and_then(Value::as_u64);
    let output_tokens = tokens.and_then(|v| v.get("output")).and_then(Value::as_u64);
    let cache_read_tokens = tokens
        .and_then(|v| v.get("cache"))
        .and_then(|v| v.get("read"))
        .and_then(Value::as_u64);
    let cache_write_tokens = tokens
        .and_then(|v| v.get("cache"))
        .and_then(|v| v.get("write"))
        .and_then(Value::as_u64);
    let cost_usd = value.get("cost").and_then(Value::as_f64);
    let provider_actual = input_tokens.is_some()
        || output_tokens.is_some()
        || cache_read_tokens.is_some()
        || cache_write_tokens.is_some()
        || cost_usd.is_some();
    Some(project_knowledge::SummaryUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        cost_usd,
        provider_actual,
    })
}

fn usage_delta(
    before: Option<&project_knowledge::SummaryUsage>,
    after: &project_knowledge::SummaryUsage,
) -> project_knowledge::SummaryUsage {
    let delta = |after: Option<u64>, before: Option<u64>| match (after, before) {
        (Some(after), Some(before)) if after >= before => Some(after - before),
        (Some(after), None) => Some(after),
        _ => None,
    };
    let cost = match (after.cost_usd, before.and_then(|value| value.cost_usd)) {
        (Some(after), Some(before)) if after >= before => Some(after - before),
        (Some(after), None) => Some(after),
        _ => None,
    };
    let input_tokens = delta(
        after.input_tokens,
        before.and_then(|value| value.input_tokens),
    );
    let output_tokens = delta(
        after.output_tokens,
        before.and_then(|value| value.output_tokens),
    );
    let cache_read_tokens = delta(
        after.cache_read_tokens,
        before.and_then(|value| value.cache_read_tokens),
    );
    let cache_write_tokens = delta(
        after.cache_write_tokens,
        before.and_then(|value| value.cache_write_tokens),
    );
    project_knowledge::SummaryUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        cost_usd: cost,
        provider_actual: input_tokens.is_some()
            || output_tokens.is_some()
            || cache_read_tokens.is_some()
            || cache_write_tokens.is_some()
            || cost.is_some(),
    }
}

/// OpenCode 1.18.25 `GET /session/{id}/message` returns a bare array of
/// `{info, parts}`. Other list endpoints (integrations/models, and some
/// `/api/` routes) wrap rows as `{"data":[...]}`. Accept both; never treat a
/// non-array `data` object as a completed message list.
fn session_messages_from_body(body: &str) -> Option<Vec<Value>> {
    let value: Value = serde_json::from_str(body).ok()?;
    Some(match value {
        Value::Array(items) => items,
        Value::Object(mut map) => map
            .remove("data")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    })
}

fn message_ids_from_body(body: &str) -> HashSet<String> {
    session_messages_from_body(body)
        .unwrap_or_default()
        .iter()
        .filter_map(message_id)
        .map(str::to_owned)
        .collect()
}

fn newest_user_id_not_in(messages: &[Value], before_ids: &HashSet<String>) -> Option<String> {
    messages.iter().rev().find_map(|message| {
        if message_role(message) != "user" {
            return None;
        }
        let id = message_id(message)?;
        if before_ids.contains(id) {
            return None;
        }
        Some(id.to_owned())
    })
}

fn terminal_assistant_text(
    messages: &[Value],
    originating_user_id: Option<&str>,
    before_ids: &HashSet<String>,
) -> Option<String> {
    messages.iter().rev().find_map(|message| {
        if message_role(message) != "assistant" {
            return None;
        }
        if assistant_finish(message) != Some("stop") {
            return None;
        }
        if let Some(id) = message_id(message)
            && before_ids.contains(id)
        {
            return None;
        }
        if let Some(expected) = originating_user_id
            && let Some(actual) = parent_message_id(message)
            && actual != expected
        {
            return None;
        }
        message_text(message).filter(|text| !text.trim().is_empty())
    })
}

fn message_id(message: &Value) -> Option<&str> {
    message.get("id").and_then(Value::as_str).or_else(|| {
        message
            .get("info")
            .and_then(|info| info.get("id"))
            .and_then(Value::as_str)
    })
}

fn parent_message_id(message: &Value) -> Option<&str> {
    message
        .get("parentID")
        .and_then(Value::as_str)
        .or_else(|| message.get("parentId").and_then(Value::as_str))
        .or_else(|| {
            message
                .get("info")
                .and_then(|info| info.get("parentID"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            message
                .get("info")
                .and_then(|info| info.get("parentId"))
                .and_then(Value::as_str)
        })
}

fn message_role(message: &Value) -> &str {
    message
        .get("role")
        .and_then(Value::as_str)
        .or_else(|| {
            message
                .get("info")
                .and_then(|info| info.get("role"))
                .and_then(Value::as_str)
        })
        .unwrap_or("")
}

fn assistant_finish(message: &Value) -> Option<&str> {
    message
        .get("info")
        .and_then(|info| info.get("finish"))
        .and_then(Value::as_str)
        .or_else(|| message.get("finish").and_then(Value::as_str))
}

fn message_text(message: &Value) -> Option<String> {
    let parts = message.get("parts").and_then(Value::as_array);
    let mut chunks = Vec::new();
    for part in parts.into_iter().flatten() {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = part.get("text").and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    chunks.push(text.to_owned());
                }
            }
            Some("json") => {
                if let Some(text) = part.get("text").and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    chunks.push(text.to_owned());
                } else if let Some(value) = part.get("value") {
                    chunks.push(value.to_string());
                }
            }
            _ => {}
        }
    }
    if !chunks.is_empty() {
        return Some(chunks.join(""));
    }
    message
        .get("content")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_contains_instruction_and_only_bounded_evidence() {
        let request = SummaryRequest {
            level: project_knowledge::SummaryLevel::Document,
            labels: vec![project_knowledge::SummaryEvidenceRef {
                label: "E1".to_owned(),
                source_label: "reunion.txt".to_owned(),
                source_name: "reunion.txt".to_owned(),
                chunk_label: "C1".to_owned(),
            }],
            evidence_texts: vec!["Se definió el presupuesto.".to_owned()],
            instruction: "Resumí.".to_owned(),
            estimated_input_units: 10,
        };
        let prompt = OpenCodeRemoteSummarizer::build_prompt(&request);
        assert!(prompt.contains("Resumí."));
        assert!(prompt.contains("[E1]"));
        assert!(prompt.contains("Se definió el presupuesto."));
    }

    fn completed_text(body: &str) -> Option<String> {
        terminal_assistant_text(
            &session_messages_from_body(body).unwrap(),
            None,
            &HashSet::new(),
        )
    }

    #[test]
    fn session_messages_unwrap_opencode_1_18_25_list_envelope() {
        let body = r#"{"location":{"directory":"/tmp/scratch"},"data":[{"info":{"id":"a1","role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"{\"summary\":\"ok\",\"topics\":[],\"decisions\":[],\"action_items\":[],\"questions\":[]}"}]}]}"#;
        let messages = session_messages_from_body(body).expect("enveloped list");
        let text =
            terminal_assistant_text(&messages, None, &HashSet::new()).expect("completed assistant");
        assert!(text.contains("\"summary\":\"ok\""));
        let bare = r#"[{"info":{"role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"bare"}]}]"#;
        assert_eq!(completed_text(bare).as_deref(), Some("bare"));
    }

    #[test]
    fn terminal_text_uses_newest_stop_even_if_a_later_assistant_is_still_open() {
        let body = r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","finish":"tool-calls"},"parts":[{"type":"tool","tool":"read"}]},{"info":{"id":"a2","role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"{\"summary\":\"ok\",\"topics\":[],\"decisions\":[],\"action_items\":[],\"questions\":[]}"}]},{"info":{"id":"a3","role":"assistant"},"parts":[]}]"#;
        let text = completed_text(body)
            .expect("stop must not be hidden by a trailing in-progress assistant");
        assert!(text.contains("\"summary\":\"ok\""));
        let streaming_only = r#"[{"info":{"id":"a1","role":"assistant"},"parts":[]}]"#;
        assert_eq!(completed_text(streaming_only), None);
        let tool_calls_only = r#"[{"info":{"id":"a1","role":"assistant","finish":"tool-calls"},"parts":[{"type":"text","text":"not yet"}]}]"#;
        assert_eq!(completed_text(tool_calls_only), None);
    }

    #[test]
    fn terminal_text_ignores_stale_stop_from_a_previous_user_turn() {
        let body = r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"old"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1","finish":"stop"},"parts":[{"type":"text","text":"{\"summary\":\"stale\"}"}]},{"info":{"id":"u2","role":"user"},"parts":[{"type":"text","text":"new"}]},{"info":{"id":"a2","role":"assistant","parentID":"u2"},"parts":[]}]"#;
        let messages = session_messages_from_body(body).unwrap();
        let mut before = HashSet::new();
        before.insert("u1".to_owned());
        before.insert("a1".to_owned());
        assert_eq!(
            terminal_assistant_text(&messages, Some("u2"), &before),
            None,
            "an earlier stop must not complete the current prompt"
        );
        let done = r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"old"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1","finish":"stop"},"parts":[{"type":"text","text":"{\"summary\":\"stale\"}"}]},{"info":{"id":"u2","role":"user"},"parts":[{"type":"text","text":"new"}]},{"info":{"id":"a2","role":"assistant","parentID":"u2","finish":"stop"},"parts":[{"type":"json","value":{"summary":"fresh","topics":[],"decisions":[],"action_items":[],"questions":[]}}]}]"#;
        let messages = session_messages_from_body(done).unwrap();
        let text = terminal_assistant_text(&messages, Some("u2"), &before).expect("current stop");
        assert!(text.contains("fresh"));
        assert!(!text.contains("stale"));
    }

    #[test]
    fn summarize_observes_real_1_18_25_bare_array_stop_without_120s_timeout() {
        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        server.set_messages_sequence(&[
            "[]",
            r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant"},"parts":[]}]"#,
            r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","finish":"tool-calls"},"parts":[{"type":"tool","tool":"read"}]},{"info":{"id":"a2","role":"assistant"},"parts":[]}]"#,
            r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","finish":"tool-calls"},"parts":[{"type":"tool","tool":"read"}]},{"info":{"id":"a2","role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"{\"summary\":\"ok\",\"topics\":[],\"decisions\":[],\"action_items\":[],\"questions\":[]}"}]},{"info":{"id":"a3","role":"assistant"},"parts":[]}]"#,
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), tmp.path().join("cfg"), 0);
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("ready");
        let summarizer = OpenCodeRemoteSummarizer::new(Arc::new(backend), tmp.path().to_path_buf())
            .with_task_timeout(Duration::from_millis(400));
        let request = SummaryRequest {
            level: project_knowledge::SummaryLevel::Document,
            labels: vec![project_knowledge::SummaryEvidenceRef {
                label: "E1".to_owned(),
                source_label: "sample.md".to_owned(),
                source_name: "sample.md".to_owned(),
                chunk_label: "C1".to_owned(),
            }],
            evidence_texts: vec!["Se definió el presupuesto.".to_owned()],
            instruction: "Resumí.".to_owned(),
            estimated_input_units: 10,
        };
        let started = Instant::now();
        let output = summarizer.summarize(&request).expect("must observe stop");
        assert!(output.text.contains("\"summary\":\"ok\""));
        assert!(
            started.elapsed() < Duration::from_millis(400),
            "poll must not fall through to the 120s timeout: {:?}",
            started.elapsed()
        );
        assert!(
            server.abort_called(),
            "completed scratch session must be aborted so it cannot leak into the next node"
        );
    }

    #[test]
    fn summarize_times_out_when_1_18_25_never_writes_finish_stop() {
        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        server.set_messages_sequence(&[
            r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","finish":"tool-calls"},"parts":[{"type":"tool","tool":"read"}]},{"info":{"id":"a2","role":"assistant"},"parts":[]}]"#,
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), tmp.path().join("cfg"), 0);
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("ready");
        let summarizer = OpenCodeRemoteSummarizer::new(Arc::new(backend), tmp.path().to_path_buf())
            .with_task_timeout(Duration::from_millis(80));
        let request = SummaryRequest {
            level: project_knowledge::SummaryLevel::Document,
            labels: vec![project_knowledge::SummaryEvidenceRef {
                label: "E1".to_owned(),
                source_label: "sample.md".to_owned(),
                source_name: "sample.md".to_owned(),
                chunk_label: "C1".to_owned(),
            }],
            evidence_texts: vec!["Se definió el presupuesto.".to_owned()],
            instruction: "Resumí.".to_owned(),
            estimated_input_units: 10,
        };
        let started = Instant::now();
        match summarizer.summarize(&request) {
            Err(SummaryFailure::ExecutionFailed) => {}
            Err(_) => panic!("expected execution timeout"),
            Ok(_) => panic!("expected execution timeout"),
        }
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(
            server.abort_called(),
            "timed-out scratch session must abort"
        );
    }

    #[test]
    fn summarize_does_not_complete_from_a_stale_stop_when_the_session_is_reused() {
        let server = fake_opencode_server::FakeServer::start();
        server.set_session_id("ses-reused");
        server.set_prompt_appends_response(true);
        server.set_k6_echo_from_prompt(true);
        let tmp = tempfile::tempdir().unwrap();
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), tmp.path().join("cfg"), 0);
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("ready");
        let summarizer = OpenCodeRemoteSummarizer::new(Arc::new(backend), tmp.path().to_path_buf())
            .with_task_timeout(Duration::from_millis(400));
        let first = SummaryRequest {
            level: project_knowledge::SummaryLevel::Document,
            labels: vec![project_knowledge::SummaryEvidenceRef {
                label: "E1".to_owned(),
                source_label: "a.md".to_owned(),
                source_name: "a.md".to_owned(),
                chunk_label: "C1".to_owned(),
            }],
            evidence_texts: vec!["SELECTED_0_BODY_SENTINEL first".to_owned()],
            instruction: "Resumí el documento.\nDocumento: a.md.".to_owned(),
            estimated_input_units: 10,
        };
        let second = SummaryRequest {
            level: project_knowledge::SummaryLevel::Document,
            labels: vec![project_knowledge::SummaryEvidenceRef {
                label: "E1".to_owned(),
                source_label: "b.md".to_owned(),
                source_name: "b.md".to_owned(),
                chunk_label: "C1".to_owned(),
            }],
            evidence_texts: vec!["SELECTED_1_BODY_SENTINEL second".to_owned()],
            instruction: "Resumí el documento.\nDocumento: b.md.".to_owned(),
            estimated_input_units: 10,
        };
        let one = summarizer.summarize(&first).expect("first node");
        let two = summarizer.summarize(&second).expect("second node");
        assert!(one.text.contains("Resumen de a.md"));
        assert!(two.text.contains("Resumen de b.md"));
        assert!(!two.text.contains("Resumen de a.md"));
    }

    #[test]
    fn summarize_intent_is_provider_and_language_independent() {
        assert!(detect_summarize_intent("resumime todos los archivos"));
        assert!(detect_summarize_intent("Resumí todo el proyecto"));
        assert!(detect_summarize_intent("summarize all files"));
        assert!(detect_summarize_intent("sintetizá todas las notas"));
        assert!(detect_summarize_intent(
            "hacé una síntesis de toda la reunión"
        ));
        assert!(detect_summarize_intent("resumime el archivo"));
        assert!(detect_summarize_intent("resumí este documento"));
        assert!(detect_summarize_intent("sintetizá el material adjunto"));
        // Ordinary informational questions must NOT route to summarization.
        assert!(!detect_summarize_intent("¿cómo está el proyecto?"));
        assert!(!detect_summarize_intent("qué decía la reunión 1"));
        assert!(!detect_summarize_intent(
            "resumime qué dijo Carla en la última reunión"
        ));
        assert!(!detect_summarize_intent(""));
    }

    #[test]
    fn selected_per_source_intent_does_not_turn_semantic_chat_into_summary() {
        assert_eq!(
            detect_summary_intent("haceme un resumen de cada archivo ordenado por fecha", 5),
            SummaryIntent::SelectedPerSource
        );
        assert_eq!(
            detect_summary_intent("summarize each attached file", 5),
            SummaryIntent::SelectedPerSource
        );
        assert_eq!(
            detect_summary_intent("resumí todo el proyecto", 5),
            SummaryIntent::Project
        );
        assert_eq!(
            detect_summary_intent("¿en cuál archivo hablan de vacaciones?", 5),
            SummaryIntent::None
        );
        assert_eq!(
            detect_summary_intent("compará lo que dicen sobre X", 5),
            SummaryIntent::None
        );
        assert_eq!(
            detect_summary_intent("resumime todos los archivos", 0),
            SummaryIntent::Project
        );
        assert_eq!(
            detect_summary_intent("resumime todos los archivos", 5),
            SummaryIntent::SelectedPerSource
        );
        assert_eq!(
            detect_summary_intent("resumime el archivo", 0),
            SummaryIntent::Project
        );
        assert_eq!(
            detect_summary_intent("resumime el archivo", 1),
            SummaryIntent::SelectedPerSource
        );
        assert_eq!(
            detect_summary_intent("¿qué dicen sobre gramática?", 5),
            SummaryIntent::None
        );
    }

    #[test]
    fn usage_delta_keeps_provider_telemetry_separate_from_estimated_units() {
        let before = project_knowledge::SummaryUsage {
            input_tokens: Some(100),
            output_tokens: Some(20),
            cache_read_tokens: Some(5),
            cache_write_tokens: Some(1),
            cost_usd: Some(0.01),
            provider_actual: true,
        };
        let after = project_knowledge::SummaryUsage {
            input_tokens: Some(1834),
            output_tokens: Some(446),
            cache_read_tokens: Some(5),
            cache_write_tokens: Some(7),
            cost_usd: Some(0.018),
            provider_actual: true,
        };
        let delta = usage_delta(Some(&before), &after);
        assert_eq!(delta.input_tokens, Some(1734));
        assert_eq!(delta.output_tokens, Some(426));
        assert_eq!(delta.cache_read_tokens, Some(0));
        assert_eq!(delta.cache_write_tokens, Some(6));
        assert!(delta.provider_actual);
        assert!((delta.cost_usd.expect("cost") - 0.008).abs() < f64::EPSILON);
    }
}
