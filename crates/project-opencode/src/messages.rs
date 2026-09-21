//! Shared OpenCode session message-list parsing and terminal-completion
//! detection.
//!
//! The remote summarizers (compact per-item and K6) and the semantic intent
//! classifier read `GET /session/{id}/message` and must agree on what a terminal
//! assistant message looks like. OpenCode 1.18.25 returns a bare array of
//! `{info, parts}`; `finish` lives under `info` and is omitted while streaming,
//! `"tool-calls"` on intermediate tool turns, and a terminal reason (`"stop"`,
//! `"length"`, `"content-filter"`, `"error"`) when the turn is done. Text is
//! `parts[].type == "text"`. Some compatible serving layers wrap rows as
//! `{"data":[...]}`; that envelope is accepted too.
//!
//! This module is the shared compatibility parser for the summarizer and
//! classifier paths. The live agent chat engine (`project-agent`) still uses its
//! own stricter `finish == "stop"` completion predicate and is NOT yet migrated
//! to [`detect_terminal_assistant`]; this module must therefore not claim total
//! centralization.
//!
//! OpenCode 1.18.25 can also finish a scratch turn WITHOUT ever writing a
//! terminal `finish`: the assistant message stays `finish`-absent and
//! `time.completed`-absent while its text is complete and stable across polls.
//! That is exactly the human AppImage trace (correctly parent-correlated
//! assistant, `text_present`, `finish == None`, `status` absent). For that one
//! case the summarizer and classifier use [`ScratchCompletionTracker`] to accept
//! a finish-absent assistant ONLY after its structural fingerprint has been
//! QUIESCENT across polls for a conservative observation window. Text is never
//! accepted merely because it exists; it must have stopped changing.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde_json::Value;

/// How the terminal signal for an assistant message was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalSource {
    /// `info.finish == "stop"` — normal completion. The only success signal.
    FinishStop,
    /// `info.finish == "length"` — the turn completed but the output was
    /// truncated by the provider's length limit. Terminal but NOT success.
    FinishLength,
    /// `info.finish == "content-filter"` — the turn was filtered by the
    /// provider. Terminal failure; never success.
    FinishContentFilter,
    /// `info.finish == "error"` — the provider reported an error finish.
    FinishError,
    /// `info.error` was present on the newest relevant assistant message.
    ProviderError,
    /// `finish` was absent, but a correctly correlated scratch assistant's
    /// non-empty text remained structurally quiescent across consecutive polls
    /// for the minimum stability window. Scoped to the OpenCode 1.18.25 scratch
    /// completion behavior that omits terminal metadata; never used to accept
    /// still-streaming text.
    StableText,
    /// The newest relevant assistant reports `time.completed` while producing no
    /// usable content: no recognized `finish`, no error, no tool call, and no
    /// text/parts. OpenCode 1.18.25 marks the scratch turn DONE but the model
    /// emitted nothing. This is a terminal, non-success outcome that must fail
    /// immediately instead of waiting for a poll timeout.
    CompletedWithoutOutput,
}

impl TerminalSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TerminalSource::FinishStop => "finish_stop",
            TerminalSource::FinishLength => "finish_length",
            TerminalSource::FinishContentFilter => "finish_content_filter",
            TerminalSource::FinishError => "finish_error",
            TerminalSource::ProviderError => "provider_error",
            TerminalSource::StableText => "stable_text",
            TerminalSource::CompletedWithoutOutput => "completed_without_output",
        }
    }

    /// Whether this source is a normal, complete success (`finish == "stop"`).
    ///
    /// `StableText` is a successful completion but is intentionally NOT folded
    /// into this predicate: `is_success` is reserved for the explicit, always
    /// authoritative `finish == "stop"` signal, so callers can still tell the
    /// two completion kinds apart.
    pub fn is_success(self) -> bool {
        matches!(self, TerminalSource::FinishStop)
    }
}

/// The result of scanning a message list for a terminal assistant message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerminalDetection {
    /// The assistant text of the terminal message. `Some` only for a normal
    /// `finish == "stop"` success; never for length/content-filter/error and
    /// never for partial streaming text.
    pub text: Option<String>,
    /// The terminal signal that produced the decision, when one was found.
    pub terminal_source: Option<TerminalSource>,
    /// The raw `finish` value observed on the most recent assistant message
    /// (used for structural telemetry; never the assistant body).
    pub observed_finish: Option<String>,
    /// Whether at least one assistant message was present in the list.
    pub assistant_message_seen: bool,
    /// Whether a successful terminal assistant message was detected. True only
    /// for `finish == "stop"`; false for length/content-filter/error and for
    /// every non-terminal state.
    pub terminal_detected: bool,
    /// A provider error, populated when the newest relevant assistant message
    /// carries `info.error` or `finish == "error"`. A provider error must never
    /// become success; the summarizer/classifier treat this as a terminal
    /// failure rather than a timeout.
    pub provider_error: Option<String>,
    /// Whether the terminal signal was `finish == "length"` (truncated output).
    pub truncated: bool,
}

/// Parses a session message-list body into messages. Accepts a bare JSON array
/// (the OpenCode 1.18.25 `GET /session/{id}/message` contract) and a
/// `{"data":[...]}` envelope used by other list endpoints.
pub fn session_messages(body: &str) -> Option<Vec<Value>> {
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

/// The role of a message (`role` or `info.role`).
pub fn message_role(message: &Value) -> &str {
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

/// The id of a message (`id` or `info.id`).
pub fn message_id(message: &Value) -> Option<&str> {
    message.get("id").and_then(Value::as_str).or_else(|| {
        message
            .get("info")
            .and_then(|info| info.get("id"))
            .and_then(Value::as_str)
    })
}

/// The parent id of a message (`parentID`/`parentId`, top-level or under `info`).
pub fn parent_message_id(message: &Value) -> Option<&str> {
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

/// The finish reason of an assistant message (`info.finish` or `finish`).
pub fn assistant_finish(message: &Value) -> Option<&str> {
    message
        .get("info")
        .and_then(|info| info.get("finish"))
        .and_then(Value::as_str)
        .or_else(|| message.get("finish").and_then(Value::as_str))
}

/// Whether the message step has completed (`info.time.completed` is set). This
/// is a structural observation only: it is NOT treated as a terminal success
/// signal. OpenCode 1.18.25 semantics do not guarantee that a completed step
/// without a recognized `finish` reason is safe to present as complete, so
/// [`detect_terminal_assistant`] deliberately ignores this marker for success.
pub fn message_time_completed(message: &Value) -> bool {
    message
        .get("info")
        .and_then(|info| info.get("time"))
        .and_then(|time| time.get("completed"))
        .is_some_and(|completed| !completed.is_null())
        || message
            .get("time")
            .and_then(|time| time.get("completed"))
            .is_some_and(|completed| !completed.is_null())
}

/// A provider error attached to a message (`info.error.message` or `error`),
/// present when the model turn failed rather than completing.
pub fn message_error(message: &Value) -> Option<&str> {
    let error = message
        .get("info")
        .and_then(|info| info.get("error"))
        .or_else(|| message.get("error"));
    error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .or_else(|| error.and_then(Value::as_str))
}

/// The text of a message, assembled from `parts` of type `"text"`/`"json"`, with
/// a fallback to a top-level `content` string. Empty text yields `None`.
pub fn message_text(message: &Value) -> Option<String> {
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

/// Counts assistant messages in a message list.
pub fn assistant_message_count(messages: &[Value]) -> usize {
    messages
        .iter()
        .filter(|message| message_role(message) == "assistant")
        .count()
}

/// Privacy-safe, structural observation of one session-message row. This is
/// intended for temporary runtime diagnostics; it never contains message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchMessageMetadata {
    pub index: usize,
    pub id: Option<String>,
    pub role: String,
    pub parent_id: Option<String>,
    pub parent_match: &'static str,
    pub finish: Option<String>,
    pub error_present: bool,
    pub time_completed: bool,
    pub part_types: Vec<String>,
    pub text_present: bool,
    pub text_len: usize,
    pub in_before_ids: bool,
    pub relevant: bool,
    pub outcome: &'static str,
}

/// Returns structural metadata and a stable fingerprint for diagnostic log
/// de-duplication. It deliberately mirrors, but does not alter, the detector's
/// staleness and parent-correlation rules.
pub fn scratch_message_snapshot(
    messages: &[Value],
    before_ids: Option<&HashSet<String>>,
    originating_user_id: Option<&str>,
) -> (String, Vec<ScratchMessageMetadata>) {
    let rows: Vec<_> = messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let id = message_id(message).map(str::to_owned);
            let role = message_role(message).to_owned();
            let parent_id = parent_message_id(message).map(str::to_owned);
            let in_before_ids = id
                .as_deref()
                .is_some_and(|id| before_ids.is_some_and(|ids| ids.contains(id)));
            let parent_match = match (originating_user_id, parent_id.as_deref()) {
                (Some(expected), Some(actual)) if expected == actual => "true",
                (Some(_), Some(_)) => "false",
                (Some(_), None) => "unknown",
                (None, _) => "unknown",
            };
            let relevant = role == "assistant" && !in_before_ids && parent_match != "false";
            let text = message_text(message);
            let finish = assistant_finish(message).map(str::to_owned);
            let time_completed = message_time_completed(message);
            let outcome = if role != "assistant" {
                "wrong_role"
            } else if in_before_ids {
                "stale_before_id"
            } else if parent_match == "false" {
                "foreign_parent"
            } else if message_error(message).is_some() || finish.as_deref() == Some("error") {
                "provider_error"
            } else {
                match finish.as_deref() {
                    Some("stop") if text.is_none() => "empty_text",
                    Some("stop") => "terminal_stop",
                    Some("length") => "terminal_length",
                    Some("content-filter") => "terminal_content_filter",
                    Some("tool-calls") => "finish_tool_calls",
                    None if time_completed => "completed_without_output",
                    None => "finish_missing",
                    Some("unknown") => "finish_unknown",
                    Some(_) => "finish_streaming",
                }
            };
            ScratchMessageMetadata {
                index,
                id,
                role,
                parent_id,
                parent_match,
                finish,
                error_present: message_error(message).is_some(),
                time_completed,
                part_types: message_part_types(message),
                text_len: text.as_ref().map_or(0, String::len),
                text_present: text.is_some(),
                in_before_ids,
                relevant,
                outcome,
            }
        })
        .collect();
    (format!("{rows:?}"), rows)
}

/// Extracts this session's phase from the `/session/status` map. A missing key
/// is intentionally represented by `None`, as OpenCode 1.18.25 uses omission
/// for an idle session.
pub fn session_status_phase(value: &Value, session_id: &str) -> Option<String> {
    value
        .get(session_id)
        .and_then(|entry| entry.get("type"))
        .and_then(Value::as_str)
        .map(|phase| phase.to_ascii_lowercase())
}

/// Scans a message list (newest-first) for the terminal signal of the current
/// turn and returns a structural [`TerminalDetection`].
///
/// Precedence is absolute:
///
/// 1. **Error wins.** If the newest relevant assistant message (after `before_ids`
///    staleness filtering, `parentID`/`originating_user_id` filtering, and role
///    filtering) carries `info.error` or `finish == "error"`, this returns a
///    terminal failure immediately — never success, never partial text, and
///    never an older completed message.
/// 2. **`stop` is success.** Only `finish == "stop"` with non-empty text is a
///    normal complete result.
/// 3. **`length` is truncated.** `finish == "length"` is terminal but must not be
///    presented as complete (`truncated = true`, no text).
/// 4. **`content-filter` is failure.** Filtered content is never success.
/// 5. **`tool-calls` and `unknown` are non-terminal**, even with text or a
///    completed `time`.
/// 6. **`time.completed` never creates success.** A completed step without a
///    recognized terminal `finish` is not proven safe, so it keeps polling.
///
/// An assistant message with a missing `parentID` is accepted only when it is
/// not stale (`before_ids`) and no `originating_user_id` mismatch is proven; in
/// the dedicated scratch summarizer/classifier session this is the response to
/// the single submitted prompt.
pub fn detect_terminal_assistant(
    messages: &[Value],
    before_ids: Option<&HashSet<String>>,
    originating_user_id: Option<&str>,
) -> TerminalDetection {
    let mut assistant_seen = false;
    let mut observed_finish: Option<String> = None;
    let mut newest_relevant = true;
    for message in messages.iter().rev() {
        if message_role(message) != "assistant" {
            continue;
        }
        assistant_seen = true;
        let finish = assistant_finish(message);
        // Record the finish of the newest assistant message for telemetry; a
        // later terminal message will overwrite this via the return paths.
        if observed_finish.is_none() {
            observed_finish = finish.map(str::to_owned);
        }
        if let Some(id) = message_id(message)
            && before_ids.is_some_and(|ids| ids.contains(id))
        {
            continue;
        }
        if let Some(expected) = originating_user_id
            && let Some(actual) = parent_message_id(message)
            && actual != expected
        {
            continue;
        }

        // ERROR PRECEDENCE (absolute). This is checked on the newest relevant
        // assistant message BEFORE any text-success evaluation and BEFORE
        // walking older messages, so an older `stop` can never override a newer
        // provider error.
        if newest_relevant {
            newest_relevant = false;
            let error = message_error(message);
            if error.is_some() || finish == Some("error") {
                return TerminalDetection {
                    text: None,
                    terminal_source: Some(if error.is_some() {
                        TerminalSource::ProviderError
                    } else {
                        TerminalSource::FinishError
                    }),
                    observed_finish,
                    assistant_message_seen: assistant_seen,
                    terminal_detected: false,
                    provider_error: Some(
                        error
                            .map(str::to_owned)
                            .unwrap_or_else(|| "finish_error".to_owned()),
                    ),
                    truncated: false,
                };
            }
            // CompletedWithoutOutput: the newest relevant assistant is DONE
            // (`time.completed`) with no recognized `finish`, no error, no tool
            // call, and no usable text/parts. This is an empty-but-terminal
            // scratch turn (the human AppImage trace), NOT a still-streaming
            // one, so it must fail immediately rather than keep polling for the
            // 30s/120s timeout. A `finish` of any kind (`stop`, `length`,
            // `tool-calls`, `unknown`, ...) is handled below and never enters
            // this branch, so valid text and truncated/filtered/error turns are
            // unaffected.
            if message_time_completed(message)
                && finish.is_none()
                && !has_tool_part(message)
                && message_text(message).is_none()
            {
                return TerminalDetection {
                    text: None,
                    terminal_source: Some(TerminalSource::CompletedWithoutOutput),
                    observed_finish: finish.map(str::to_owned),
                    assistant_message_seen: assistant_seen,
                    terminal_detected: false,
                    provider_error: None,
                    truncated: false,
                };
            }
        }

        match finish {
            Some("tool-calls") => {
                // The turn is not done: the model wants tools. Keep looking at
                // older messages for a real terminal answer.
                continue;
            }
            Some("stop") => {
                if let Some(text) = message_text(message).filter(|text| !text.trim().is_empty()) {
                    return TerminalDetection {
                        text: Some(text),
                        terminal_source: Some(TerminalSource::FinishStop),
                        observed_finish: Some("stop".to_owned()),
                        assistant_message_seen: assistant_seen,
                        terminal_detected: true,
                        provider_error: None,
                        truncated: false,
                    };
                }
                // stop with empty text is not a usable completion; keep polling.
                continue;
            }
            Some("length") => {
                // Terminal but truncated. Never present as complete success.
                return TerminalDetection {
                    text: None,
                    terminal_source: Some(TerminalSource::FinishLength),
                    observed_finish: Some("length".to_owned()),
                    assistant_message_seen: assistant_seen,
                    terminal_detected: false,
                    provider_error: None,
                    truncated: true,
                };
            }
            Some("content-filter") => {
                // Filtered output is a terminal failure, never success.
                return TerminalDetection {
                    text: None,
                    terminal_source: Some(TerminalSource::FinishContentFilter),
                    observed_finish: Some("content-filter".to_owned()),
                    assistant_message_seen: assistant_seen,
                    terminal_detected: false,
                    provider_error: None,
                    truncated: false,
                };
            }
            None => {
                // Streaming (no recognized finish). `time.completed` is NOT a
                // safe success signal, so this stays non-terminal and the caller
                // keeps polling until a recognized terminal/error or a timeout.
                continue;
            }
            Some(_other) => {
                // e.g. "unknown" — not a proven terminal state; keep looking.
                continue;
            }
        }
    }
    TerminalDetection {
        text: None,
        terminal_source: None,
        observed_finish,
        assistant_message_seen: assistant_seen,
        terminal_detected: false,
        provider_error: None,
        truncated: false,
    }
}

/// Minimum number of consecutive structurally-identical scratch text
/// observations required before a finish-absent assistant can be accepted via
/// the quiescence fallback.
pub const SCRATCH_MIN_STABLE_POLLS: usize = 5;

/// Minimum wall-clock duration over which the scratch text must remain
/// structurally identical before a finish-absent assistant can be accepted.
///
/// The scratch poll interval is 20ms (both the summarizer and the classifier),
/// so this window comfortably spans `SCRATCH_MIN_STABLE_POLLS` polls while still
/// resolving a completed response in ~1s of provider latency — never the 30s
/// classifier or 120s summarizer timeout.
pub const SCRATCH_MIN_STABLE_DURATION: Duration = Duration::from_secs(1);

/// Structural fingerprint of a scratch assistant text candidate. It carries
/// identity and content-shape only — the message id, parent id, text length, an
/// in-memory-only content hash, the part-type structure, and the (always
/// absent) finish/error markers — never the text itself, so it can be compared
/// across polls and discarded without logging content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchTextFingerprint {
    pub id: Option<String>,
    pub parent_id: Option<String>,
    pub text_len: usize,
    pub text_hash: u64,
    pub part_types: Vec<String>,
    pub finish: Option<String>,
    pub error_present: bool,
}

/// Names the part types of a message (`parts[].type`), in order.
pub fn message_part_types(message: &Value) -> Vec<String> {
    message
        .get("parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("type").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// Whether a message carries a tool invocation part (`type == "tool"`). A tool
/// call requires further execution, so such a message can never be a stable-text
/// completion even when its `finish` is absent.
fn has_tool_part(message: &Value) -> bool {
    message
        .get("parts")
        .and_then(Value::as_array)
        .is_some_and(|parts| {
            parts
                .iter()
                .any(|part| part.get("type").and_then(Value::as_str) == Some("tool"))
        })
}

/// FNV-1a 64-bit content hash, deterministic and process-local. Used strictly
/// in memory to detect text change across polls; never logged or persisted.
fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Extracts the scratch stable-text candidate: the newest relevant assistant
/// message (after `before_ids` staleness filtering and originating-user/parent
/// correlation) that qualifies for the quiescence fallback.
///
/// A message qualifies only when ALL of the following hold:
///
/// - role is `assistant`;
/// - not stale (`before_ids`) and not proven foreign (`parentID`);
/// - no `info.error` and no `finish == "error"` (error precedence stays on
///   [`detect_terminal_assistant`], but a candidate must never carry an error);
/// - `finish` is ABSENT — any recognized finish (`stop`, `length`,
///   `content-filter`, `error`, `tool-calls`, `unknown`) is handled by
///   [`detect_terminal_assistant`] and never by the fallback;
/// - no tool invocation part (`tool-calls`-like state without metadata);
/// - non-empty assembled text.
///
/// This is deliberately scoped to the OpenCode 1.18.25 scratch behavior that
/// omits terminal metadata; it never accepts still-streaming text because the
/// caller must still pass the fingerprint through [`ScratchCompletionTracker`]
/// to prove quiescence.
pub fn scratch_text_candidate(
    messages: &[Value],
    before_ids: Option<&HashSet<String>>,
    originating_user_id: Option<&str>,
) -> Option<(ScratchTextFingerprint, String)> {
    for message in messages.iter().rev() {
        if message_role(message) != "assistant" {
            continue;
        }
        if let Some(id) = message_id(message)
            && before_ids.is_some_and(|ids| ids.contains(id))
        {
            continue;
        }
        if let Some(expected) = originating_user_id
            && let Some(actual) = parent_message_id(message)
            && actual != expected
        {
            continue;
        }
        // The newest relevant assistant governs the fallback. Only a
        // finish-absent, error-free, tool-free message with text qualifies; any
        // other state returns `None` so an older assistant can never be reused.
        if assistant_finish(message).is_some()
            || message_error(message).is_some()
            || has_tool_part(message)
        {
            return None;
        }
        let text = message_text(message)?;
        let fingerprint = ScratchTextFingerprint {
            id: message_id(message).map(str::to_owned),
            parent_id: parent_message_id(message).map(str::to_owned),
            text_len: text.len(),
            text_hash: fnv1a64(&text),
            part_types: message_part_types(message),
            finish: None,
            error_present: false,
        };
        return Some((fingerprint, text));
    }
    None
}

/// Stateful quiescence detector for finish-absent scratch assistants.
///
/// It observes the [`ScratchTextFingerprint`] of the correctly correlated
/// scratch assistant across consecutive polls and accepts the text only after
/// the fingerprint has been identical for [`SCRATCH_MIN_STABLE_POLLS`]
/// observations spanning at least [`SCRATCH_MIN_STABLE_DURATION`]. Any
/// output-relevant change resets the window, so a still-streaming message (text
/// growing, message id/parent changing, part types changing) can never finish
/// early.
#[derive(Debug, Default)]
pub struct ScratchCompletionTracker {
    last: Option<StableObservation>,
    stable_polls: usize,
    first_stable: Option<Instant>,
}

#[derive(Debug)]
struct StableObservation {
    fingerprint: ScratchTextFingerprint,
}

impl ScratchCompletionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets any accumulated stability, so a vanished or replaced candidate
    /// can never resume a prior window.
    pub fn reset(&mut self) {
        self.last = None;
        self.stable_polls = 0;
        self.first_stable = None;
    }

    /// Number of consecutive identical observations accumulated so far.
    pub fn stable_poll_count(&self) -> usize {
        self.stable_polls
    }

    /// Wall-clock time since the current stable observation first appeared.
    pub fn stable_elapsed(&self, now: Instant) -> Duration {
        self.first_stable
            .map(|first| now.saturating_duration_since(first))
            .unwrap_or(Duration::ZERO)
    }

    /// Observes one finish-absent candidate. Returns the accepted text once the
    /// candidate has been quiescent for the required window; `None` otherwise.
    pub fn observe(
        &mut self,
        fingerprint: ScratchTextFingerprint,
        text: String,
        now: Instant,
    ) -> Option<String> {
        let changed = self
            .last
            .as_ref()
            .is_none_or(|observation| observation.fingerprint != fingerprint);
        if changed {
            self.last = Some(StableObservation { fingerprint });
            self.stable_polls = 1;
            self.first_stable = Some(now);
            return None;
        }
        self.stable_polls += 1;
        let elapsed = self.stable_elapsed(now);
        if self.stable_polls >= SCRATCH_MIN_STABLE_POLLS && elapsed >= SCRATCH_MIN_STABLE_DURATION {
            return Some(text);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn detect(messages: &Value) -> TerminalDetection {
        detect_terminal_assistant(messages.as_array().unwrap(), None, None)
    }

    fn detect_with(
        messages: &Value,
        before_ids: &[&str],
        originating: Option<&str>,
    ) -> TerminalDetection {
        let set: HashSet<String> = before_ids.iter().map(|id| (*id).to_owned()).collect();
        detect_terminal_assistant(messages.as_array().unwrap(), Some(&set), originating)
    }

    // 1. stop + text -> normal success.
    #[test]
    fn stop_finish_is_terminal() {
        let messages = json!([
            {"info": {"id": "u1", "role": "user"}, "parts": [{"type": "text", "text": "x"}]},
            {"info": {"id": "a1", "role": "assistant", "finish": "stop"}, "parts": [{"type": "text", "text": "resumen"}]}
        ]);
        let detection = detect(&messages);
        assert!(detection.terminal_detected);
        assert!(detection.terminal_source.unwrap().is_success());
        assert_eq!(detection.text.as_deref(), Some("resumen"));
        assert_eq!(detection.terminal_source, Some(TerminalSource::FinishStop));
        assert!(!detection.truncated);
        assert_eq!(detection.provider_error, None);
    }

    // 2. length + text -> truncated, NOT normal success.
    #[test]
    fn length_finish_is_truncated_not_success() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "length"}, "parts": [{"type": "text", "text": "truncado"}]}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected, "length must not be success");
        assert_eq!(detection.text, None, "length must not yield text");
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::FinishLength)
        );
        assert!(detection.truncated);
        assert_eq!(detection.observed_finish.as_deref(), Some("length"));
    }

    // 3. content-filter + text -> failure, never success.
    #[test]
    fn content_filter_is_failure() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "content-filter"}, "parts": [{"type": "text", "text": "filtrado"}]}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(detection.text, None);
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::FinishContentFilter)
        );
    }

    // 4. tool-calls + text -> non-terminal.
    #[test]
    fn tool_calls_is_not_terminal() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "tool-calls"}, "parts": [{"type": "text", "text": "intermedio"}]}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(detection.terminal_source, None);
        assert_eq!(detection.text, None);
    }

    // 5. tool-calls + time.completed + text -> still non-terminal.
    #[test]
    fn tool_calls_with_time_completed_is_still_not_terminal() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "tool-calls", "time": {"completed": 2}}, "parts": [{"type": "text", "text": "intermedio"}]}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(detection.terminal_source, None);
    }

    // 6. unknown + time.completed + text -> non-terminal.
    #[test]
    fn unknown_finish_with_time_completed_is_not_terminal() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "unknown", "time": {"completed": 2}}, "parts": [{"type": "text", "text": "raro"}]}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(detection.terminal_source, None);
        assert_eq!(detection.observed_finish.as_deref(), Some("unknown"));
    }

    // 7. absent finish + time.completed + text -> non-terminal (no fallback).
    #[test]
    fn time_completed_without_finish_is_not_terminal() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "time": {"created": 1, "completed": 2}}, "parts": [{"type": "text", "text": "listo"}]}
        ]);
        let detection = detect(&messages);
        assert!(
            !detection.terminal_detected,
            "time.completed must not create a false success"
        );
        assert_eq!(detection.terminal_source, None);
        assert_eq!(detection.text, None);
    }

    // 8. info.error + no text -> failure.
    #[test]
    fn provider_error_is_reported_and_never_becomes_success() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "error": {"message": "context overflow"}}, "parts": []}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(detection.text, None);
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::ProviderError)
        );
        assert_eq!(
            detection.provider_error.as_deref(),
            Some("context overflow")
        );
    }

    #[test]
    fn provider_error_string_form_is_detected() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "error": "boom"}, "parts": []}
        ]);
        let detection = detect(&messages);
        assert_eq!(detection.provider_error.as_deref(), Some("boom"));
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::ProviderError)
        );
    }

    // 9. info.error + partial text -> failure (partial text must never win).
    #[test]
    fn provider_error_with_partial_text_is_failure() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "error": {"message": "boom"}}, "parts": [{"type": "text", "text": "partial"}]}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(detection.text, None);
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::ProviderError)
        );
    }

    // 10. info.error + finish=stop + text -> failure.
    #[test]
    fn same_row_provider_error_and_stop_and_text_is_failure() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "stop", "error": {"message": "boom"}}, "parts": [{"type": "text", "text": "success-looking"}]}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(
            detection.text, None,
            "same-row error + stop must not succeed"
        );
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::ProviderError)
        );
    }

    // 11. info.error + finish=length + text -> failure.
    #[test]
    fn same_row_provider_error_and_length_is_failure() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "length", "error": {"message": "boom"}}, "parts": [{"type": "text", "text": "x"}]}
        ]);
        let detection = detect(&messages);
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::ProviderError)
        );
        assert_eq!(detection.text, None);
        assert!(
            !detection.truncated,
            "error precedence beats length truncation"
        );
    }

    // 12. info.error + finish=content-filter + text -> failure.
    #[test]
    fn same_row_provider_error_and_content_filter_is_failure() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "content-filter", "error": {"message": "boom"}}, "parts": [{"type": "text", "text": "x"}]}
        ]);
        let detection = detect(&messages);
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::ProviderError)
        );
        assert_eq!(detection.text, None);
    }

    // 13. finish=error + text -> failure.
    #[test]
    fn finish_error_with_text_is_failure() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "error"}, "parts": [{"type": "text", "text": "partial"}]}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(detection.text, None);
        assert_eq!(detection.terminal_source, Some(TerminalSource::FinishError));
        assert!(detection.provider_error.is_some());
    }

    // 14. newest error + older stop -> failure (older success must not win).
    #[test]
    fn newest_provider_error_beats_older_stop() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "stop"}, "parts": [{"type": "text", "text": "old success"}]},
            {"info": {"id": "a2", "role": "assistant", "error": {"message": "boom"}}, "parts": [{"type": "text", "text": "partial"}]}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(detection.text, None, "must never return old success");
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::ProviderError)
        );
    }

    // 15. newest finish=error + older stop -> failure.
    #[test]
    fn newest_finish_error_beats_older_stop() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "stop"}, "parts": [{"type": "text", "text": "old success"}]},
            {"info": {"id": "a2", "role": "assistant", "finish": "error"}, "parts": [{"type": "text", "text": "partial"}]}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(detection.text, None);
        assert_eq!(detection.terminal_source, Some(TerminalSource::FinishError));
    }

    // 16. trailing open assistant after an earlier stop, same turn -> stop wins.
    #[test]
    fn newest_stop_is_chosen_over_trailing_open_assistant() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "stop"}, "parts": [{"type": "text", "text": "final"}]},
            {"info": {"id": "a2", "role": "assistant"}, "parts": []}
        ]);
        let detection = detect(&messages);
        assert!(detection.terminal_detected);
        assert_eq!(detection.text.as_deref(), Some("final"));
    }

    // 17. stale assistant in before_ids must be skipped.
    #[test]
    fn stale_assistant_in_before_ids_is_skipped() {
        let messages = json!([
            {"info": {"id": "a-old", "role": "assistant", "finish": "stop"}, "parts": [{"type": "text", "text": "stale"}]},
            {"info": {"id": "a-new", "role": "assistant", "finish": "stop"}, "parts": [{"type": "text", "text": "fresh"}]}
        ]);
        let detection = detect_with(&messages, &["a-old"], None);
        assert!(detection.terminal_detected);
        assert_eq!(detection.text.as_deref(), Some("fresh"));
    }

    // 18. foreign parentID must be skipped.
    #[test]
    fn foreign_parent_is_skipped() {
        let messages = json!([
            {"info": {"id": "a-foreign", "role": "assistant", "parentID": "other", "finish": "stop"}, "parts": [{"type": "text", "text": "foreign"}]},
            {"info": {"id": "a-mine", "role": "assistant", "parentID": "u1", "finish": "stop"}, "parts": [{"type": "text", "text": "mine"}]}
        ]);
        let detection = detect_with(&messages, &[], Some("u1"));
        assert!(detection.terminal_detected);
        assert_eq!(detection.text.as_deref(), Some("mine"));
    }

    // 19. missing parentID is accepted when not stale/foreign (documented).
    #[test]
    fn missing_parent_is_accepted_when_not_proven_foreign() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "stop"}, "parts": [{"type": "text", "text": "no parent"}]}
        ]);
        let detection = detect_with(&messages, &[], Some("u1"));
        assert!(detection.terminal_detected);
        assert_eq!(detection.text.as_deref(), Some("no parent"));
    }

    // 20. empty text is not terminal.
    #[test]
    fn empty_text_is_not_terminal() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "stop"}, "parts": []}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected);
        assert_eq!(detection.text, None);
    }

    // 21. malformed envelope yields no messages.
    #[test]
    fn malformed_envelope_yields_nothing() {
        assert_eq!(session_messages("not json"), None);
        assert_eq!(session_messages("42"), Some(vec![]));
    }

    // 22/23. bare array and {data:[...]} envelope both parse.
    #[test]
    fn session_messages_accepts_bare_array_and_data_envelope() {
        let bare = r#"[{"info":{"role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"x"}]}]"#;
        assert_eq!(session_messages(bare).unwrap().len(), 1);
        let enveloped = r#"{"location":{"directory":"/tmp"},"data":[{"info":{"role":"assistant","finish":"stop"},"parts":[{"type":"text","text":"x"}]}]}"#;
        assert_eq!(session_messages(enveloped).unwrap().len(), 1);
    }

    // 24. json parts contribute text.
    #[test]
    fn json_parts_contribute_text() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "stop"}, "parts": [{"type": "json", "value": {"summary": "ok"}}]}
        ]);
        let detection = detect(&messages);
        assert!(detection.terminal_detected);
        assert!(
            detection
                .text
                .as_deref()
                .unwrap()
                .contains("\"summary\":\"ok\"")
        );
    }

    // 25. content fallback contributes text.
    #[test]
    fn content_fallback_contributes_text() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "stop"}, "content": "desde content"}
        ]);
        let detection = detect(&messages);
        assert!(detection.terminal_detected);
        assert_eq!(detection.text.as_deref(), Some("desde content"));
    }

    // --- Scratch stable-text quiescence fallback ---

    fn candidate(messages: &Value) -> Option<(ScratchTextFingerprint, String)> {
        scratch_text_candidate(messages.as_array().unwrap(), None, None)
    }

    fn fingerprint(text: &str) -> ScratchTextFingerprint {
        ScratchTextFingerprint {
            id: Some("a1".to_owned()),
            parent_id: Some("u1".to_owned()),
            text_len: text.len(),
            text_hash: fnv1a64(text),
            part_types: vec!["text".to_owned()],
            finish: None,
            error_present: false,
        }
    }

    // 26. finish-absent + text is a candidate; finish-present is not.
    #[test]
    fn scratch_candidate_requires_absent_finish() {
        let absent = json!([
            {"info": {"id": "u1", "role": "user"}, "parts": [{"type": "text", "text": "x"}]},
            {"info": {"id": "a1", "role": "assistant", "parentID": "u1"}, "parts": [{"type": "text", "text": "resumen"}]}
        ]);
        let (fingerprint, text) = candidate(&absent).expect("finish-absent candidate");
        assert_eq!(text, "resumen");
        assert_eq!(fingerprint.finish, None);
        assert!(!fingerprint.error_present);

        let tool = json!([
            {"info": {"id": "u1", "role": "user"}, "parts": [{"type": "text", "text": "x"}]},
            {"info": {"id": "a1", "role": "assistant", "parentID": "u1", "finish": "tool-calls"}, "parts": [{"type": "text", "text": "resumen"}]}
        ]);
        assert!(
            candidate(&tool).is_none(),
            "tool-calls must not be a candidate"
        );

        let stop = json!([
            {"info": {"id": "u1", "role": "user"}, "parts": [{"type": "text", "text": "x"}]},
            {"info": {"id": "a1", "role": "assistant", "parentID": "u1", "finish": "stop"}, "parts": [{"type": "text", "text": "resumen"}]}
        ]);
        assert!(
            candidate(&stop).is_none(),
            "finish=stop is handled by the detector"
        );
    }

    // 27. a tool part (finish absent) is not a candidate.
    #[test]
    fn scratch_candidate_rejects_tool_part() {
        let messages = json!([
            {"info": {"id": "u1", "role": "user"}, "parts": [{"type": "text", "text": "x"}]},
            {"info": {"id": "a1", "role": "assistant", "parentID": "u1"}, "parts": [{"type": "tool", "tool": "read"}]}
        ]);
        assert!(candidate(&messages).is_none());
    }

    // 28. an error-bearing finish-absent assistant is not a candidate.
    #[test]
    fn scratch_candidate_rejects_provider_error() {
        let messages = json!([
            {"info": {"id": "u1", "role": "user"}, "parts": [{"type": "text", "text": "x"}]},
            {"info": {"id": "a1", "role": "assistant", "parentID": "u1", "error": {"message": "boom"}}, "parts": [{"type": "text", "text": "partial"}]}
        ]);
        assert!(candidate(&messages).is_none());
    }

    // 29. a foreign-parent assistant is not a candidate.
    #[test]
    fn scratch_candidate_requires_parent_correlation() {
        let messages = json!([
            {"info": {"id": "u1", "role": "user"}, "parts": [{"type": "text", "text": "x"}]},
            {"info": {"id": "a1", "role": "assistant", "parentID": "other"}, "parts": [{"type": "text", "text": "ajeno"}]}
        ]);
        assert!(scratch_text_candidate(messages.as_array().unwrap(), None, Some("u1")).is_none());
    }

    // 30. text appears once -> not terminal yet.
    #[test]
    fn quiescence_single_observation_is_not_terminal() {
        let mut tracker = ScratchCompletionTracker::new();
        let now = Instant::now();
        assert_eq!(
            tracker.observe(fingerprint("hola"), "hola".into(), now),
            None
        );
        assert_eq!(tracker.stable_poll_count(), 1);
    }

    // 31. growing text resets the window each time.
    #[test]
    fn quiescence_growing_text_resets_window() {
        let mut tracker = ScratchCompletionTracker::new();
        let now = Instant::now();
        tracker.observe(fingerprint("hola"), "hola".into(), now);
        tracker.observe(
            fingerprint("hola mundo"),
            "hola mundo".into(),
            now + Duration::from_millis(200),
        );
        assert_eq!(tracker.stable_poll_count(), 1, "growth must reset to one");
        tracker.observe(
            fingerprint("hola mundo ..."),
            "hola mundo ...".into(),
            now + Duration::from_millis(400),
        );
        assert_eq!(
            tracker.stable_poll_count(),
            1,
            "further growth resets again"
        );
    }

    // 32. same text for enough polls but below the minimum duration -> no success.
    #[test]
    fn quiescence_insufficient_duration_is_not_terminal() {
        let mut tracker = ScratchCompletionTracker::new();
        let now = Instant::now();
        let mut result = None;
        for i in 0..(SCRATCH_MIN_STABLE_POLLS + 5) {
            result = tracker.observe(
                fingerprint("hola"),
                "hola".into(),
                now + Duration::from_millis(i as u64 * 50),
            );
        }
        assert!(result.is_none(), "below the 1s duration must not succeed");
    }

    // 33. same text for enough polls + duration -> stable-text success.
    #[test]
    fn quiescence_accepts_after_polls_and_duration() {
        let mut tracker = ScratchCompletionTracker::new();
        let now = Instant::now();
        let mut result = None;
        for i in 0..SCRATCH_MIN_STABLE_POLLS {
            result = tracker.observe(
                fingerprint("hola"),
                "hola".into(),
                now + Duration::from_millis(i as u64 * 300),
            );
        }
        assert_eq!(
            result.as_deref(),
            Some("hola"),
            "enough polls spanning >1s must succeed"
        );
        assert!(tracker.stable_poll_count() >= SCRATCH_MIN_STABLE_POLLS);
        assert!(
            tracker.stable_elapsed(
                now + Duration::from_millis(300 * (SCRATCH_MIN_STABLE_POLLS as u64 - 1))
            ) >= SCRATCH_MIN_STABLE_DURATION
        );
    }

    // 34. a changed fingerprint resets stability (same length, different content).
    #[test]
    fn quiescence_detects_content_change_at_same_length() {
        let mut tracker = ScratchCompletionTracker::new();
        let now = Instant::now();
        tracker.observe(fingerprint("hola"), "hola".into(), now);
        // Same length, different content: "hola" -> "halo".
        let mut other = fingerprint("halo");
        other.text_hash = fnv1a64("halo");
        tracker.observe(
            other.clone(),
            "halo".into(),
            now + Duration::from_millis(300),
        );
        assert_eq!(tracker.stable_poll_count(), 1, "content change must reset");
    }

    // 35. reset() forgets a prior window so a vanished candidate cannot resume.
    #[test]
    fn quiescence_reset_forgets_prior_window() {
        let mut tracker = ScratchCompletionTracker::new();
        let now = Instant::now();
        tracker.observe(fingerprint("hola"), "hola".into(), now);
        tracker.observe(
            fingerprint("hola"),
            "hola".into(),
            now + Duration::from_millis(300),
        );
        tracker.reset();
        let mut result = None;
        for i in 0..SCRATCH_MIN_STABLE_POLLS {
            result = tracker.observe(
                fingerprint("hola"),
                "hola".into(),
                now + Duration::from_millis(400 + i as u64 * 300),
            );
        }
        assert_eq!(
            result.as_deref(),
            Some("hola"),
            "after reset a fresh full window must still be required"
        );
    }

    // --- CompletedWithoutOutput (scratch turn finished EMPTY) ---

    // 36. The exact human trace: correlated assistant, time.completed=true,
    // finish absent, no error, parts=[], text empty -> terminal empty failure.
    #[test]
    fn completed_without_output_is_terminal_empty_failure() {
        let messages = json!([
            {"info": {"id": "u1", "role": "user"}, "parts": [{"type": "text", "text": "x"}]},
            {"info": {"id": "a1", "role": "assistant", "parentID": "u1", "time": {"completed": 2}}, "parts": []}
        ]);
        let detection = detect(&messages);
        assert!(!detection.terminal_detected, "empty must never be success");
        assert_eq!(detection.text, None);
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::CompletedWithoutOutput)
        );
        assert_eq!(
            detection.terminal_source.unwrap().as_str(),
            "completed_without_output"
        );
        assert_eq!(detection.provider_error, None);
        assert!(!detection.truncated);
    }

    // 37. completed + text must NOT be classified as empty (valid text still
    // flows through the stable-text fallback, never a terminal empty).
    #[test]
    fn completed_with_text_is_not_completed_without_output() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "time": {"completed": 2}}, "parts": [{"type": "text", "text": "listo"}]}
        ]);
        let detection = detect(&messages);
        assert_ne!(
            detection.terminal_source,
            Some(TerminalSource::CompletedWithoutOutput),
            "non-empty text must never be classified as completed-without-output"
        );
        assert_eq!(detection.terminal_source, None);
    }

    // 38. Not-yet-completed and empty must NOT be classified prematurely.
    #[test]
    fn empty_assistant_without_time_completed_is_not_terminal() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant"}, "parts": []}
        ]);
        let detection = detect(&messages);
        assert_eq!(detection.terminal_source, None);
    }

    // 39. A tool part with time.completed is a tool-execution step, not an
    // empty completion.
    #[test]
    fn completed_with_tool_part_is_not_completed_without_output() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "time": {"completed": 2}}, "parts": [{"type": "tool", "tool": "read"}]}
        ]);
        let detection = detect(&messages);
        assert_eq!(detection.terminal_source, None);
    }

    // 40. A provider error still beats completed-without-output.
    #[test]
    fn error_beats_completed_without_output() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "time": {"completed": 2}, "error": {"message": "boom"}}, "parts": []}
        ]);
        let detection = detect(&messages);
        assert_eq!(
            detection.terminal_source,
            Some(TerminalSource::ProviderError)
        );
    }

    // 41. finish=tool-calls with time.completed and empty text is still
    // non-terminal (the model wants tools), never a terminal empty.
    #[test]
    fn tool_calls_completed_empty_is_not_completed_without_output() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "tool-calls", "time": {"completed": 2}}, "parts": []}
        ]);
        let detection = detect(&messages);
        assert_eq!(detection.terminal_source, None);
    }

    // 42. A trailing completed-empty assistant does not hide an older stop when
    // the trailing one has no time.completed (unchanged selection semantics).
    #[test]
    fn trailing_open_empty_assistant_does_not_override_older_stop() {
        let messages = json!([
            {"info": {"id": "a1", "role": "assistant", "finish": "stop"}, "parts": [{"type": "text", "text": "final"}]},
            {"info": {"id": "a2", "role": "assistant"}, "parts": []}
        ]);
        let detection = detect(&messages);
        assert!(detection.terminal_detected);
        assert_eq!(detection.text.as_deref(), Some("final"));
    }
}
