//! Provider-independent remote summarization execution over the shared
//! OpenCode backend (K6).
//!
//! A summarization request is a bounded, self-contained prompt that runs in a
//! **dedicated scratch session**, never the project's chat session, so
//! background summary generation never creates a user-visible chat turn and
//! never reuses conversation history. This module depends only on the generic
//! `project_opencode::OpenCodeBackend` HTTP surface, not on any provider-
//! specific API or the agent chat adapter.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use project_knowledge::{RemoteSummarizer, SummaryFailure, SummaryOutput, SummaryRequest};
use project_opencode::{OpenCodeBackend, with_directory_query};
use serde_json::{Value, json};

const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(20);
const SUMMARY_TASK_TIMEOUT: Duration = Duration::from_secs(120);

/// Provider- and language-independent whole-project summarization intent.
///
/// This is deliberately not a single hardcoded Spanish string trigger. It
/// detects a *summary verb* plus an *all/corpus scope marker* across a small
/// multilingual set, so "resumime todos los archivos", "summarize all files",
/// "resumí el proyecto", etc. resolve to the same internal operation. Ordinary
/// informational questions (which carry no corpus-scope marker) continue through
/// K3/K4 chat.
pub fn detect_summarize_intent(text: &str) -> bool {
    let normalized = text.to_lowercase();
    let has_summary_verb = SUMMARY_VERBS.iter().any(|verb| normalized.contains(verb));
    if !has_summary_verb {
        return false;
    }
    SCOPE_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
}

const SUMMARY_VERBS: &[&str] = &[
    "resum",
    "resumen",
    "summar",
    "síntesis",
    "sintetiz",
    "sintesis",
];
const SCOPE_MARKERS: &[&str] = &[
    "todos",
    "todas",
    "todo el",
    "toda la",
    "los archivos",
    "las notas",
    "all files",
    "all documents",
    "el proyecto",
    "the project",
    "la reunión",
    "carpeta entera",
    "whole project",
    "entire project",
    "proyecto entero",
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
}

impl OpenCodeRemoteSummarizer {
    pub fn new(backend: Arc<OpenCodeBackend>, scratch_dir: PathBuf) -> Self {
        Self {
            backend,
            scratch_dir,
        }
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
        let (status, text) = self
            .backend
            .post(&path, &json!({ "permission": [] }))
            .map_err(|_| SummaryFailure::ExecutionFailed)?;
        if !(200..300).contains(&status) {
            return Err(SummaryFailure::ExecutionFailed);
        }
        let value: Value =
            serde_json::from_str(&text).map_err(|_| SummaryFailure::ExecutionFailed)?;
        let session_id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or(SummaryFailure::ExecutionFailed)?
            .to_owned();
        // OpenCode session metadata is local bookkeeping for this exact
        // execution, not an additional provider/model request. The session may
        // be reused by neither K6 nor any chat turn, but use a delta anyway to
        // preserve the accounting contract.
        let usage_before = session_usage(&self.backend, &session_id);

        let body = json!({ "parts": [{ "type": "text", "text": prompt }] });
        let prompt_path = format!("/session/{session_id}/prompt_async");
        let (status, _) = self
            .backend
            .post(&prompt_path, &body)
            .map_err(|_| SummaryFailure::ExecutionFailed)?;
        if !(200..300).contains(&status) && status != 204 {
            return Err(SummaryFailure::ExecutionFailed);
        }

        // Poll the dedicated session for the single terminal assistant text.
        let deadline = Instant::now() + SUMMARY_TASK_TIMEOUT;
        let message_path = format!("/session/{session_id}/message?limit=16");
        loop {
            let (status, body) = self
                .backend
                .get(&message_path)
                .map_err(|_| SummaryFailure::ExecutionFailed)?;
            if (200..300).contains(&status)
                && let Ok(value) = serde_json::from_str::<Value>(&body)
                && let Some(messages) = value.as_array()
                && let Some(text) = terminal_assistant_text(messages)
            {
                return Ok(SummaryOutput {
                    text,
                    model_id: None,
                    provider_id: None,
                    usage: session_usage(&self.backend, &session_id)
                        .map(|after| usage_delta(usage_before.as_ref(), &after))
                        .unwrap_or_default(),
                });
            }
            if Instant::now() >= deadline {
                return Err(SummaryFailure::ExecutionFailed);
            }
            thread::sleep(STATUS_POLL_INTERVAL);
        }
    }
}

fn session_usage(
    backend: &OpenCodeBackend,
    session_id: &str,
) -> Option<project_knowledge::SummaryUsage> {
    let (status, body) = backend.get(&format!("/session/{session_id}")).ok()?;
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

fn terminal_assistant_text(messages: &[Value]) -> Option<String> {
    messages
        .iter()
        .rfind(|message| message_role(message) == "assistant")
        .filter(|message| assistant_finish(message) == Some("stop"))
        .and_then(message_text)
        .filter(|text| !text.trim().is_empty())
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
        if part.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = part.get("text").and_then(Value::as_str)
        {
            chunks.push(text);
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
