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
                });
            }
            if Instant::now() >= deadline {
                return Err(SummaryFailure::ExecutionFailed);
            }
            thread::sleep(STATUS_POLL_INTERVAL);
        }
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
        // Ordinary informational questions must NOT route to summarization.
        assert!(!detect_summarize_intent("¿cómo está el proyecto?"));
        assert!(!detect_summarize_intent("qué decía la reunión 1"));
        assert!(!detect_summarize_intent(
            "resumime qué dijo Carla en la última reunión"
        ));
        assert!(!detect_summarize_intent(""));
    }
}
