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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use project_knowledge::{RemoteSummarizer, SummaryFailure, SummaryOutput, SummaryRequest};
use project_opencode::messages::{
    self, ScratchCompletionTracker, TerminalSource, detect_terminal_assistant, message_id,
    message_role, scratch_message_snapshot, session_status_phase,
};
use project_opencode::{OpenCodeBackend, scratch_tool_free_permission, with_directory_query};
use serde_json::{Value, json};

const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(20);
const SUMMARY_TASK_TIMEOUT: Duration = Duration::from_secs(120);
const MESSAGE_LIMIT: &str = "1000";

/// Process-local handle for a durable K6 operation. The durable ledger is
/// written by AppState; this handle only lets Cancel stop this process's active
/// scratch session and prevents scheduling further nodes.
#[derive(Default)]
pub struct SummaryCancellation {
    cancelled: AtomicBool,
    active_session: Mutex<Option<String>>,
}

impl project_knowledge::SummaryExecutionControl for SummaryCancellation {
    fn is_cancelled(&self) -> bool {
        self.is_cancelled()
    }
}

impl SummaryCancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    pub fn set_active_session(&self, session_id: Option<String>) {
        *self
            .active_session
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = session_id;
    }
    pub fn active_session(&self) -> Option<String> {
        self.active_session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// The bounded summary operation requested by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryIntent {
    /// Ordinary question answering: K3/K4 retrieval remains appropriate.
    None,
    /// A request to summarize the complete existing project corpus.
    Project,
    /// A generic summary over the exact current-turn selected set. This uses
    /// the bounded per-item aggregate path, not K6 and not top-K retrieval.
    SelectedBatchAggregate,
    /// A COMPACT per-source summary: one brief identifiable result per selected
    /// source, produced through a bounded number of aggregate remote calls
    /// (never one call per document and never the K6 document-node pipeline).
    PerItemBatchAggregate,
    /// A DEEP per-source summary/analysis: the durable K6 hierarchical
    /// document/batch/global summarization path.
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
    let has_analysis_verb = ANALYSIS_VERBS.iter().any(|verb| normalized.contains(verb));
    if !has_summary_verb && !has_analysis_verb {
        return SummaryIntent::None;
    }
    // A summary verb scoped to a specific content question ("resumime qué dijo
    // Carla", "resumime cuánto se decidió") is an ordinary informational
    // question, not a whole-material summarization. It must continue through
    // K3/K4 chat even when the same turn carries attachments.
    if SPECIFIC_QUESTION_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
    {
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
    if explicit_per_source {
        // A per-source request is DEEP only when it carries an explicit
        // detail/exhaustiveness cue or an analysis verb ("analizá", "análisis",
        // "analyse", "analysis"). Otherwise it is COMPACT: one brief result per
        // source through a bounded number of aggregate remote calls.
        //
        // Per-source wording NEVER degrades to the whole-project K6 route, even
        // with zero current-turn attachments: the exact material scope is
        // resolved deterministically downstream (current-turn attachments, then
        // the newest compatible prior MaterialSet, then the conversation-active
        // set, else a truthful no-selection). This is what keeps "Resumime cada
        // archivo por separado." / "Résume chaque fichier séparément." out of
        // `WholeCorpusSummary`.
        if has_analysis_verb || has_deep_summary_cue(&normalized) {
            return SummaryIntent::SelectedPerSource;
        }
        return SummaryIntent::PerItemBatchAggregate;
    }
    // With no current selection, explicit corpus-wide wording retains the
    // historical project K6 behavior. Bare summary language remains chat.
    if selected_attachment_count == 0
        && CORPUS_SCOPE_MARKERS
            .iter()
            .any(|marker| normalized.contains(marker))
    {
        return SummaryIntent::Project;
    }
    // A bare summary request in the same turn as accepted current-turn
    // materials ("me podrás hacer un resumen") targets exactly those materials.
    // Without a current-turn selection the phrase stays ordinary chat so it
    // never magically binds arbitrary corpus material.
    if selected_attachment_count > 0 {
        return SummaryIntent::SelectedBatchAggregate;
    }
    SummaryIntent::None
}

/// Detects an explicit deep/detail cue in a summary/analysis request, across a
/// small multilingual set. This is the deterministic fallback for the semantic
/// compact-vs-deep distinction; the semantic classifier is the authoritative
/// multilingual source.
pub fn has_deep_summary_cue(normalized: &str) -> bool {
    DEEP_SUMMARY_CUES.iter().any(|cue| normalized.contains(cue))
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
    "résum",
    "synthes",
    "synthétis",
];

/// Analysis verbs ("analizá", "análisis", "analyse", "analysis", ...). An
/// analysis request is deep by nature: it is never treated as a compact
/// per-item summary.
const ANALYSIS_VERBS: &[&str] = &["analiz", "análisis", "analisis", "analy", "analys"];
const PROJECT_SCOPE_MARKERS: &[&str] = &[
    "el proyecto",
    "the project",
    "carpeta entera",
    "whole project",
    "entire project",
    "proyecto entero",
];
const PER_SOURCE_MARKERS: &[&str] = &[
    "cada archivo",
    "cada documento",
    "uno por uno",
    "por separado",
    "individualmente",
    "archivo por archivo",
    "documento por documento",
    "resumen detallado de cada",
    "each attached file",
    "each file",
    "each document",
    "one by one",
    "separately",
    "individually",
    // French
    "chaque fichier",
    "chaque document",
    "séparément",
    "separément",
    "individuellement",
    "fichier par fichier",
    // Portuguese
    "cada arquivo",
    "separadamente",
    // German
    "jede datei",
    "jedes dokument",
    "einzeln",
    // Italian
    "ogni file",
    "ogni documento",
    "separatamente",
];

/// Explicit deep/detail cues. These distinguish a deep per-source analysis from
/// a compact per-source summary when a summary verb is present. Deliberately
/// semantic (detail/exhaustiveness/thoroughness) rather than a hardcoded phrase.
const DEEP_SUMMARY_CUES: &[&str] = &[
    // Spanish
    "detallad",
    "exhaustiv",
    "profund",
    "en detalle",
    "a fondo",
    "minucios",
    // English
    "detailed",
    "in depth",
    "in-depth",
    "exhaustive",
    "thorough",
    "deep",
    "comprehensive",
    // French
    "détaillé",
    "détaill",
    "detaill",
    "exhaustif",
    "approfondi",
    "en détail",
    "en detail",
    "profond",
    // Portuguese
    "detalhad",
    "exaustiv",
    "em detalhe",
    // German
    "detailliert",
    "ausführlich",
    "ausfuehrlich",
    "eingehend",
    // Italian
    "dettagliat",
    "approfondit",
    "esaustiv",
];
const CORPUS_SCOPE_MARKERS: &[&str] = &[
    "todos los archivos",
    "todas las notas",
    "todos los documentos",
    "all files",
    "all documents",
    "estos archivos",
    "estas notas",
    "el archivo",
    "el documento",
];

/// Interrogative/content-query markers that scope a summary verb to a specific
/// question ("resumime qué dijo Carla") rather than a whole-material
/// summarization. Presence suppresses the bare-summary + current-turn-material
/// binding so such phrasing continues through ordinary semantic chat.
const SPECIFIC_QUESTION_MARKERS: &[&str] = &[
    "qué", "cuánto", "cuántos", "cuántas", "cómo", "cuándo", "dónde", "quién", "quienes",
    "por qué", "what", "how many", "how much", "when", "where", "who", "which",
];

/// Executes one bounded summarization request against the shared OpenCode
/// backend in an isolated scratch session.
pub struct OpenCodeRemoteSummarizer {
    backend: Arc<OpenCodeBackend>,
    scratch_dir: PathBuf,
    task_timeout: Duration,
    model: Option<(String, String)>,
    cancellation: Option<Arc<SummaryCancellation>>,
}

impl OpenCodeRemoteSummarizer {
    pub fn new(backend: Arc<OpenCodeBackend>, scratch_dir: PathBuf) -> Self {
        Self {
            backend,
            scratch_dir,
            task_timeout: SUMMARY_TASK_TIMEOUT,
            model: None,
            cancellation: None,
        }
    }

    /// Pins the same provider/model the conversation already selected so the
    /// scratch session does not depend on OpenCode's default-model resolution.
    pub fn with_model(mut self, provider_id: String, model_id: String) -> Self {
        self.model = Some((provider_id, model_id));
        self
    }

    pub fn with_cancellation(mut self, cancellation: Arc<SummaryCancellation>) -> Self {
        self.cancellation = Some(cancellation);
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
    ///
    /// The exact serialization is shared with the compact per-item packer via
    /// [`crate::per_item::serialize_summary_prompt`], so the compact path's
    /// budget always measures the exact bytes this function will emit.
    fn build_prompt(request: &SummaryRequest) -> String {
        crate::per_item::serialize_summary_prompt(
            &request.instruction,
            &request.labels,
            &request.evidence_texts,
        )
    }
}

impl RemoteSummarizer for OpenCodeRemoteSummarizer {
    fn summarize(
        &self,
        request: &SummaryRequest,
    ) -> std::result::Result<SummaryOutput, SummaryFailure> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(|control| control.is_cancelled())
        {
            return Err(SummaryFailure::Cancelled);
        }
        let prompt = Self::build_prompt(request);

        if let Err(_err) = self.backend.ensure_ready() {
            return Err(SummaryFailure::ProviderUnavailable);
        }

        let directory = self.scratch_dir.to_string_lossy().replace('\\', "/");
        let path = with_directory_query("/session", &directory);
        // Scratch synthesis is not a coding-agent turn and must never execute
        // coding tools. Use the shared scratch ruleset (explicit execution
        // denies + external_directory deny, ADR-0006). Do not send a global
        // `*` deny: OpenCode 1.18.25 then completes an empty assistant.
        let (status, text) = self
            .backend
            .post(
                &path,
                &json!({ "permission": scratch_tool_free_permission() }),
            )
            .map_err(|_| SummaryFailure::ExecutionFailed)?;
        if !(200..300).contains(&status) {
            return Err(SummaryFailure::ExecutionFailed);
        }
        let value: Value =
            serde_json::from_str(&text).map_err(|_| SummaryFailure::ExecutionFailed)?;
        let session_id = session_id_from_value(&value).ok_or(SummaryFailure::ExecutionFailed)?;
        if let Some(control) = &self.cancellation {
            control.set_active_session(Some(session_id.clone()));
        }
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge][summary] node_kind={} session_id={} call=start",
                summary_level_name(request.level),
                session_id
            ),
        );
        let abort_path = format!("/session/{session_id}/abort");
        let usage_before = session_usage(&self.backend, &session_id);

        let message_path = format!("/session/{session_id}/message?limit={MESSAGE_LIMIT}");
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
        let prompt_path = format!("/session/{session_id}/prompt_async");
        let (status, response_body) = self
            .backend
            .post(&prompt_path, &body)
            .map_err(|_| SummaryFailure::ExecutionFailed)?;
        crate::session_log::record(
            "INFO",
            format!(
                "[opencode][scratch] node_kind={} session_id={} phase=prompt_async provider_id={} model_id={} http_status={} response_present={}",
                summary_level_name(request.level),
                session_id,
                self.model
                    .as_ref()
                    .map(|(p, _)| p.as_str())
                    .unwrap_or("default"),
                self.model
                    .as_ref()
                    .map(|(_, m)| m.as_str())
                    .unwrap_or("default"),
                status,
                !response_body.trim().is_empty(),
            ),
        );
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
        let mut last_status: Option<String> = None;
        let mut last_snapshot: Option<String> = None;
        let mut tracker = ScratchCompletionTracker::new();
        loop {
            if self
                .cancellation
                .as_ref()
                .is_some_and(|control| control.is_cancelled())
            {
                let _ = self.backend.post(&abort_path, &json!({}));
                return Err(SummaryFailure::Cancelled);
            }
            poll_count += 1;
            // Observability only: this is the same status endpoint used by the
            // proven chat path. Its result never changes scratch completion.
            let mut status_present = false;
            if let Ok((status, body)) = self.backend.get("/session/status")
                && (200..300).contains(&status)
                && let Ok(value) = serde_json::from_str::<Value>(&body)
            {
                let phase = session_status_phase(&value, &session_id);
                status_present = phase.is_some();
                let fingerprint = phase.clone().unwrap_or_else(|| "absent".into());
                if last_status.as_deref() != Some(fingerprint.as_str()) {
                    crate::session_log::record(
                        "INFO",
                        format!(
                            "[opencode][scratch] session={} phase=status status_present={} status={} elapsed_ms={}",
                            session_id,
                            phase.is_some(),
                            fingerprint,
                            started.elapsed().as_millis()
                        ),
                    );
                    last_status = Some(fingerprint);
                }
            }
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
                && let Some(messages) = messages::session_messages(&body)
            {
                if originating_user_id.is_none() {
                    originating_user_id = newest_user_id_not_in(&messages, &before_ids);
                }
                let (fingerprint, snapshot) = scratch_message_snapshot(
                    &messages,
                    Some(&before_ids),
                    originating_user_id.as_deref(),
                );
                if last_snapshot.as_deref() != Some(fingerprint.as_str()) {
                    crate::session_log::record(
                        "INFO",
                        format!(
                            "[opencode][scratch] session={} phase=messages before_ids_count={} originating_user_id={} entries={:?} elapsed_ms={}",
                            session_id,
                            before_ids.len(),
                            originating_user_id.as_deref().unwrap_or("none"),
                            snapshot,
                            started.elapsed().as_millis()
                        ),
                    );
                    last_snapshot = Some(fingerprint);
                }
                let message_count = messages.len();
                let detection = detect_terminal_assistant(
                    &messages,
                    Some(&before_ids),
                    originating_user_id.as_deref(),
                );
                if let Some(text) = detection.text {
                    crate::session_log::record(
                        "INFO",
                        format!(
                            "[knowledge][summary] node_kind={} session_id={} poll_count={} message_count={} terminal_source={} observed_finish={} assistant_message_seen={} terminal_detected={} finish_present={} status_present={} finish=stop elapsed_ms={} error_class=none",
                            summary_level_name(request.level),
                            session_id,
                            poll_count,
                            message_count,
                            detection
                                .terminal_source
                                .map(TerminalSource::as_str)
                                .unwrap_or("none"),
                            detection.observed_finish.as_deref().unwrap_or("none"),
                            detection.assistant_message_seen,
                            detection.terminal_detected,
                            detection.observed_finish.is_some(),
                            status_present,
                            started.elapsed().as_millis()
                        ),
                    );
                    let usage = session_usage(&self.backend, &session_id)
                        .map(|after| usage_delta(usage_before.as_ref(), &after))
                        .unwrap_or_default();
                    let _ = self.backend.post(&abort_path, &json!({}));
                    if let Some(control) = &self.cancellation {
                        control.set_active_session(None);
                    }
                    return Ok(SummaryOutput {
                        text,
                        model_id: self.model.as_ref().map(|(_, model)| model.clone()),
                        provider_id: self.model.as_ref().map(|(provider, _)| provider.clone()),
                        usage,
                    });
                }
                // A terminal non-success (length / content-filter / provider
                // error / finish=error) is a real failure, never a 120s timeout
                // and never success. Log only the structural class, not the body.
                if let Some(source) = detection.terminal_source {
                    let reason = match source {
                        TerminalSource::FinishLength => "output_truncated",
                        TerminalSource::FinishContentFilter => "content_filter",
                        TerminalSource::FinishError | TerminalSource::ProviderError => {
                            "provider_error"
                        }
                        TerminalSource::CompletedWithoutOutput => "completed_without_output",
                        TerminalSource::FinishStop | TerminalSource::StableText => {
                            unreachable!("stop/stable-text yield text")
                        }
                    };
                    let _ = self.backend.post(&abort_path, &json!({}));
                    if let Some(control) = &self.cancellation {
                        control.set_active_session(None);
                    }
                    crate::session_log::record(
                        "WARN",
                        format!(
                            "[knowledge][summary] node_kind={} session_id={} poll_count={} error_class=execution_failed timeout_reason={} terminal_source={} observed_finish={} assistant_message_seen={} terminal_detected={} provider_error={} truncated={} elapsed_ms={}",
                            summary_level_name(request.level),
                            session_id,
                            poll_count,
                            reason,
                            source.as_str(),
                            detection.observed_finish.as_deref().unwrap_or("none"),
                            detection.assistant_message_seen,
                            detection.terminal_detected,
                            detection.provider_error.is_some(),
                            detection.truncated,
                            started.elapsed().as_millis()
                        ),
                    );
                    return Err(SummaryFailure::ExecutionFailed);
                }
                // OpenCode 1.18.25 scratch sessions can complete WITHOUT ever
                // writing `finish` (or `time.completed`): the correlated
                // assistant text is complete and stable while every explicit
                // terminal marker is absent. Accept that text only once it has
                // been QUIESCENT — structurally identical across polls for the
                // minimum stability window. Text that is still streaming keeps
                // resetting the window and can never finish early; a subsequent
                // provider error or `finish` still wins on the next poll.
                let candidate = messages::scratch_text_candidate(
                    &messages,
                    Some(&before_ids),
                    originating_user_id.as_deref(),
                );
                if let Some((fingerprint, candidate_text)) = candidate {
                    if let Some(text) = tracker.observe(fingerprint, candidate_text, Instant::now())
                    {
                        crate::session_log::record(
                            "INFO",
                            format!(
                                "[knowledge][summary] node_kind={} session_id={} poll_count={} message_count={} terminal_source=stable_text observed_finish=none assistant_message_seen={} terminal_detected=true finish_present=false status_present={} stable_poll_count={} stable_elapsed_ms={} elapsed_ms={} error_class=none",
                                summary_level_name(request.level),
                                session_id,
                                poll_count,
                                message_count,
                                detection.assistant_message_seen,
                                status_present,
                                tracker.stable_poll_count(),
                                tracker.stable_elapsed(Instant::now()).as_millis(),
                                started.elapsed().as_millis()
                            ),
                        );
                        let usage = session_usage(&self.backend, &session_id)
                            .map(|after| usage_delta(usage_before.as_ref(), &after))
                            .unwrap_or_default();
                        let _ = self.backend.post(&abort_path, &json!({}));
                        if let Some(control) = &self.cancellation {
                            control.set_active_session(None);
                        }
                        return Ok(SummaryOutput {
                            text,
                            model_id: self.model.as_ref().map(|(_, model)| model.clone()),
                            provider_id: self.model.as_ref().map(|(provider, _)| provider.clone()),
                            usage,
                        });
                    }
                } else {
                    // No qualifying candidate this poll: forget any partial
                    // stability so a replaced/vanished assistant cannot resume
                    // an older window.
                    tracker.reset();
                }
            }
            if Instant::now() >= deadline {
                let _ = self.backend.post(&abort_path, &json!({}));
                if let Some(control) = &self.cancellation {
                    control.set_active_session(None);
                }
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

    fn summarize_controlled(
        &self,
        request: &SummaryRequest,
        control: &dyn project_knowledge::SummaryExecutionControl,
    ) -> std::result::Result<SummaryOutput, SummaryFailure> {
        if control.is_cancelled() {
            return Err(SummaryFailure::Cancelled);
        }
        // The production cancellation instance is also installed on this
        // adapter, so its existing poll loop owns OpenCode session aborts.
        self.summarize(request)
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
) -> Option<project_knowledge::SummaryUsage> {
    let path = format!("/session/{session_id}");
    let (status, body) = backend.get(&path).ok()?;
    if !(200..300).contains(&status) {
        return None;
    }
    let value: Value = serde_json::from_str(&body).ok()?;
    Some(usage_from_session_value(&value))
}

fn usage_from_session_value(value: &Value) -> project_knowledge::SummaryUsage {
    // OpenCode currently returns the session object directly. Some compatible
    // serving layers use the same `{data: ...}` envelope as other endpoints;
    // accept that transport wrapper without treating absent fields as zero.
    let value = value.get("data").unwrap_or(value);
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
    project_knowledge::SummaryUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        cost_usd,
        provider_actual,
    }
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

/// Ids of every message already in a session message list. The summarizer
/// snapshots these before submitting the prompt so a stale terminal row (from a
/// prior turn on a reused session) can never complete the current request.
fn message_ids_from_body(body: &str) -> HashSet<String> {
    messages::session_messages(body)
        .unwrap_or_default()
        .iter()
        .filter_map(message_id)
        .map(str::to_owned)
        .collect()
}

/// The newest user-message id that is not already in `before_ids` — the user
/// turn that owns the assistant response we are waiting for.
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
        assert!(
            !prompt.contains(project_agent::knowledge_answer_grounding_instruction()),
            "summarizer scratch must not use the Knowledge-answer grounding contract"
        );
    }

    fn completed_text(body: &str) -> Option<String> {
        detect_terminal_assistant(&messages::session_messages(body).unwrap(), None, None).text
    }

    #[test]
    fn session_messages_unwrap_opencode_1_18_25_list_envelope() {
        let body = r#"{"location":{"directory":"/tmp/scratch"},"data":[{"info":{"id":"a1","role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"{\"summary\":\"ok\",\"topics\":[],\"decisions\":[],\"action_items\":[],\"questions\":[]}"}]}]}"#;
        let messages = messages::session_messages(body).expect("enveloped list");
        let text = detect_terminal_assistant(&messages, None, None)
            .text
            .expect("completed assistant");
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
        let messages = messages::session_messages(body).unwrap();
        let mut before = HashSet::new();
        before.insert("u1".to_owned());
        before.insert("a1".to_owned());
        assert_eq!(
            detect_terminal_assistant(&messages, Some(&before), Some("u2")).text,
            None,
            "an earlier stop must not complete the current prompt"
        );
        let done = r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"old"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1","finish":"stop"},"parts":[{"type":"text","text":"{\"summary\":\"stale\"}"}]},{"info":{"id":"u2","role":"user"},"parts":[{"type":"text","text":"new"}]},{"info":{"id":"a2","role":"assistant","parentID":"u2","finish":"stop"},"parts":[{"type":"json","value":{"summary":"fresh","topics":[],"decisions":[],"action_items":[],"questions":[]}}]}]"#;
        let messages = messages::session_messages(done).unwrap();
        let text = detect_terminal_assistant(&messages, Some(&before), Some("u2"))
            .text
            .expect("current stop");
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

    /// Contract: the summarizer scratch session must use the shared tool-safe
    /// scratch permission helper (`external_directory` deny, no global `*` deny)
    /// while still invoking the model through the same endpoint contract: no
    /// `?directory=` on prompt_async/message/abort/session (`directory` is read
    /// only by `POST /session`).
    #[test]
    fn scratch_session_matches_ordinary_chat_request_contract() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_response_finish("stop");
        server.set_prompt_response_text(
            r#"{"summary":"ok","topics":[],"decisions":[],"action_items":[],"questions":[]}"#,
        );
        let tmp = tempfile::tempdir().unwrap();
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), tmp.path().join("cfg"), 0);
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("ready");
        let summarizer = OpenCodeRemoteSummarizer::new(Arc::new(backend), tmp.path().to_path_buf())
            .with_model("opencode".to_owned(), "big-pickle".to_owned())
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
            instruction: "Resumí el archivo.".to_owned(),
            estimated_input_units: 10,
        };
        summarizer.summarize(&request).expect("must summarize");

        // Session create payload: shared scratch helper (explicit tool
        // execution denies + external_directory deny, never global * deny).
        assert_eq!(
            server.last_permission(),
            Some(scratch_tool_free_permission()),
            "scratch must send the shared tool-safe permission body"
        );

        // Exactly one fresh session and one prompt_async (no retry/resend).
        assert_eq!(server.created_session_ids().len(), 1);
        assert_eq!(server.prompt_async_paths().len(), 1);

        // prompt_async URL carries no ?directory and pins the conversation model.
        let prompt_path = server.prompt_async_paths().first().unwrap().clone();
        assert!(
            prompt_path.contains("/prompt_async"),
            "unexpected prompt_async path: {prompt_path}"
        );
        assert!(
            !prompt_path.contains("directory="),
            "prompt_async must not carry ?directory=: {prompt_path}"
        );
        assert_eq!(
            server.prompt_async_models().first().cloned().flatten(),
            Some(json!({ "providerID": "opencode", "modelID": "big-pickle" }))
        );

        // message/abort/session-metadata endpoints must not carry ?directory=.
        for path in server
            .message_paths()
            .iter()
            .chain(server.abort_paths().iter())
            .chain(server.session_get_paths().iter())
        {
            assert!(
                !path.contains("directory="),
                "scratch endpoint must not carry ?directory=: {path}"
            );
        }
        assert!(!server.message_paths().is_empty());
        assert!(server.abort_called());
        assert_eq!(
            server.abort_paths().len(),
            1,
            "scratch must abort exactly once on completion"
        );
    }

    /// EXACT HUMAN-TRACE REGRESSION: OpenCode 1.18.25 completes a scratch turn
    /// with a correctly parent-correlated assistant whose text is complete and
    /// stable, but `finish` is absent, `time.completed` is absent/false, and
    /// `/session/status` is absent. The same structural snapshot repeats every
    /// poll. The summarizer must accept it via the quiescence fallback
    /// (`terminal_source=stable_text`), NOT wait 120s for `finish_stop_missing`.
    #[test]
    fn summarize_accepts_quiescent_finish_absent_assistant_without_120s_timeout() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        // The exact production shape: user row + assistant row with parent match,
        // finish=None, error absent, time.completed absent, part_types=["text"],
        // non-empty text. The same snapshot repeats on every subsequent poll.
        let stable = r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1"},"parts":[{"type":"text","text":"{\"summary\":\"ok\",\"topics\":[],\"decisions\":[],\"action_items\":[],\"questions\":[]}"}]}]"#;
        server.set_messages_sequence(&["[]", stable]);
        let tmp = tempfile::tempdir().unwrap();
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), tmp.path().join("cfg"), 0);
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("ready");
        let summarizer = OpenCodeRemoteSummarizer::new(Arc::new(backend), tmp.path().to_path_buf())
            .with_task_timeout(Duration::from_secs(10));
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
        let output = summarizer
            .summarize(&request)
            .expect("quiescent text must complete");
        assert!(output.text.contains("\"summary\":\"ok\""));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "must resolve in provider latency + quiescence window, not the 120s timeout: {:?}",
            started.elapsed()
        );
        assert!(
            started.elapsed() >= Duration::from_millis(900),
            "must actually observe the quiescence window: {:?}",
            started.elapsed()
        );
        let logs = crate::session_log::list();
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("terminal_source=stable_text")),
            "must record the stable-text completion: {logs:?}"
        );
        assert!(
            !logs
                .iter()
                .any(|entry| entry.message.contains("timeout_reason=finish_stop_missing")),
            "a valid quiescent completion must never emit finish_stop_missing"
        );
        assert!(
            server.abort_called(),
            "completed scratch session must abort"
        );
    }

    #[test]
    fn summarize_times_out_when_1_18_25_never_writes_finish_stop() {
        // This test emits a `finish_stop_missing` line into the shared session
        // log; serialize with the other log-asserting scratch tests and clear
        // the buffer so it cannot pollute a concurrent quiescent-completion test.
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
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

    /// OpenCode 1.18.25 scratch turn that completes EMPTY (`time.completed`, no
    /// `finish`, no error, no text). The summarizer must fail the slot
    /// IMMEDIATELY (`ExecutionFailed`) — never the 120s `finish_stop_missing`
    /// timeout, never a success, and never a retry/resend (exactly one session,
    /// one prompt_async, one abort).
    #[test]
    fn summarize_completed_without_output_fails_immediately() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        let empty = serde_json::to_string(&json!([
            {"info": {"id": "u1", "role": "user"}, "parts": [{"type": "text", "text": "x"}]},
            {"info": {"id": "a1", "role": "assistant", "parentID": "u1", "time": {"completed": 2}}, "parts": []}
        ]))
        .unwrap();
        server.set_messages_sequence(&["[]", empty.as_str()]);
        let tmp = tempfile::tempdir().unwrap();
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), tmp.path().join("cfg"), 0);
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("ready");
        let summarizer = OpenCodeRemoteSummarizer::new(Arc::new(backend), tmp.path().to_path_buf())
            .with_task_timeout(Duration::from_secs(10));
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
            Err(_) => panic!("expected execution failure for an empty completion"),
            Ok(_) => panic!("must never produce a successful summary from empty output"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "must fail immediately, not after the 120s timeout: {:?}",
            started.elapsed()
        );
        assert!(server.abort_called());
        assert_eq!(server.created_session_ids().len(), 1);
        assert_eq!(server.prompt_async_paths().len(), 1);
        let logs = crate::session_log::list();
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("completed_without_output")),
            "must record the completed-without-output classification: {logs:?}"
        );
        assert!(
            !logs
                .iter()
                .any(|entry| entry.message.contains("timeout_reason=finish_stop_missing")),
            "an empty completed turn must never wait for finish_stop_missing"
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
            SummaryIntent::PerItemBatchAggregate
        );
        assert_eq!(
            detect_summary_intent("summarize each attached file", 5),
            SummaryIntent::PerItemBatchAggregate
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
            SummaryIntent::SelectedBatchAggregate
        );
        assert_eq!(
            detect_summary_intent("me podras hacer un resumen de estos archivos?", 50),
            SummaryIntent::SelectedBatchAggregate
        );
        assert_eq!(
            detect_summary_intent("haceme un resumen de estos archivos", 50),
            SummaryIntent::SelectedBatchAggregate
        );
        assert_eq!(
            detect_summary_intent("resumime estos archivos", 3),
            SummaryIntent::SelectedBatchAggregate
        );
        assert_eq!(
            detect_summary_intent("resumí estas notas", 3),
            SummaryIntent::SelectedBatchAggregate
        );
        assert_eq!(
            detect_summary_intent("resumime estos archivos", 0),
            SummaryIntent::Project
        );
        assert_eq!(
            detect_summary_intent("resumime el archivo", 0),
            SummaryIntent::Project
        );
        assert_eq!(
            detect_summary_intent("resumime el archivo", 1),
            SummaryIntent::SelectedBatchAggregate
        );
        assert_eq!(
            detect_summary_intent("¿qué dicen sobre gramática?", 5),
            SummaryIntent::None
        );
    }

    #[test]
    fn bare_summary_verb_with_current_turn_materials_targets_them() {
        // CASE A/B: 50 attachments + a bare summary request must select the
        // exact current-turn set, never fall through to normal top-K chat.
        assert_eq!(
            detect_summary_intent("me podrás hacer un resumen", 50),
            SummaryIntent::SelectedBatchAggregate
        );
        assert_eq!(
            detect_summary_intent("me podras hacer un resumen", 50),
            SummaryIntent::SelectedBatchAggregate
        );
        assert_eq!(
            detect_summary_intent("haceme un resumen", 50),
            SummaryIntent::SelectedBatchAggregate
        );
        // CASE C: single attachment, deictic-free summary verb.
        assert_eq!(
            detect_summary_intent("resumime esto", 1),
            SummaryIntent::SelectedBatchAggregate
        );
        assert_eq!(
            detect_summary_intent("resumí esto", 1),
            SummaryIntent::SelectedBatchAggregate
        );
    }

    #[test]
    fn per_source_markers_split_compact_vs_deep_by_depth_cue() {
        // COMPACT: explicit per-source without a deep/detail cue -> bounded
        // per-item aggregate (never K6).
        for prompt in [
            "resumime cada archivo por separado",
            "resumí uno por uno todos los archivos",
            "resumime cada documento",
            "dame un resumen breve de cada documento",
            "résume chaque fichier séparément",
            "give me a short summary of each file",
        ] {
            assert_eq!(
                detect_summary_intent(prompt, 50),
                SummaryIntent::PerItemBatchAggregate,
                "{prompt}"
            );
        }
        // DEEP: explicit per-source WITH a deep/detail cue -> K6.
        for prompt in [
            "haceme un resumen detallado de cada documento",
            "hacé un resumen exhaustivo y profundo de cada archivo",
            "resumime cada archivo por separado en detalle",
        ] {
            assert_eq!(
                detect_summary_intent(prompt, 50),
                SummaryIntent::SelectedPerSource,
                "{prompt}"
            );
        }
    }

    #[test]
    fn compact_per_source_wording_stays_compact_with_zero_attachments() {
        // A later turn with zero current-turn attachments must still resolve
        // compact per-source wording to the bounded per-item aggregate route,
        // never to the whole-project K6 route. The material scope is resolved
        // deterministically downstream, not by this detector.
        for prompt in [
            "Resumime cada archivo por separado.",
            "resumime cada archivo por separado",
            "resumí cada documento",
            "resumí uno por uno todos los archivos",
            "Résume chaque fichier séparément.",
            "résume chaque fichier séparément",
            "Give me a short summary of each file.",
            "give me a short summary of each file",
        ] {
            assert_eq!(
                detect_summary_intent(prompt, 0),
                SummaryIntent::PerItemBatchAggregate,
                "{prompt}"
            );
        }
        // Deep per-source wording with zero attachments stays K6 (selected
        // per-source), never whole-corpus.
        for prompt in [
            "haceme un resumen detallado de cada documento",
            "hacé un resumen exhaustivo y profundo de cada archivo",
            "analizá detalladamente cada documento por separado",
        ] {
            assert_eq!(
                detect_summary_intent(prompt, 0),
                SummaryIntent::SelectedPerSource,
                "{prompt}"
            );
        }
    }

    #[test]
    fn analysis_verbs_are_deep_by_nature() {
        for prompt in [
            "analizá detalladamente cada documento por separado",
            "hacé un análisis profundo de cada archivo",
            "analyse chaque document en détail",
            "give me a detailed in-depth analysis of every file individually",
        ] {
            assert_eq!(
                detect_summary_intent(prompt, 50),
                SummaryIntent::SelectedPerSource,
                "{prompt}"
            );
        }
    }

    #[test]
    fn bare_summary_verb_without_attachments_stays_ordinary_chat() {
        // CASE D: no current-turn material must not bind arbitrary corpus.
        assert_eq!(
            detect_summary_intent("me podrás hacer un resumen", 0),
            SummaryIntent::None
        );
        assert_eq!(
            detect_summary_intent("haceme un resumen", 0),
            SummaryIntent::None
        );
        assert_eq!(
            detect_summary_intent("resumime esto", 0),
            SummaryIntent::None
        );
    }

    #[test]
    fn summary_verb_scoped_to_a_specific_question_never_binds_materials() {
        // "resumime qué dijo Carla" is a content question, not a whole-material
        // summarization, even when the same turn carries attachments.
        assert_eq!(
            detect_summary_intent("resumime qué dijo Carla en la última reunión", 5),
            SummaryIntent::None
        );
        assert_eq!(
            detect_summary_intent("resumime qué se habló en estos archivos", 50),
            SummaryIntent::None
        );
        assert_eq!(
            detect_summary_intent("resumime cuánto se decidió", 50),
            SummaryIntent::None
        );
        assert_eq!(
            detect_summary_intent("summarize what Carla said", 5),
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

    #[test]
    fn session_usage_accepts_direct_and_enveloped_provider_metadata() {
        let direct: Value = serde_json::from_str(
            r#"{"tokens":{"input":100,"output":20,"cache":{"read":5,"write":2}},"cost":0.01}"#,
        )
        .unwrap();
        let enveloped: Value = serde_json::from_str(
            r#"{"data":{"tokens":{"input":100,"output":20,"cache":{"read":5,"write":2}},"cost":0.01}}"#,
        )
        .unwrap();
        assert_eq!(
            usage_from_session_value(&direct),
            usage_from_session_value(&enveloped)
        );
    }
}
