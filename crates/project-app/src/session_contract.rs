//! Controlled A/B session-contract harness (offline).
//!
//! The normal project-agent chat path and the scratch classifier/summarizer
//! paths share one wire contract (create session -> `prompt_async` -> poll
//! `GET /session/{id}/message`) but differ in a few structural knobs:
//! directory, permission ruleset, agent, prompt content, and model. This module
//! models those knobs as a single [`SessionSpec`], runs one session against any
//! [`OpenCodeBackend`] (the in-process fake server offline, or the real sidecar
//! later) and captures a privacy-safe structural [`SessionContract`].
//!
//! It is the substrate for the one-variable-at-a-time A/B ladder that will later
//! locate the first transition from `text != empty` (normal path) to
//! `text == empty` (scratch path). It performs no live provider calls here and
//! captures nothing sensitive: prompt and assistant bodies are never stored, and
//! raw directory paths are reduced to a present/absent flag plus a sanitized
//! role label supplied by the caller.

use std::collections::HashSet;
use std::time::Duration;

use project_opencode::messages::{
    self, ScratchMessageMetadata, TerminalDetection, detect_terminal_assistant,
    scratch_message_snapshot,
};
use project_opencode::{OpenCodeBackend, with_directory_query};
use serde_json::{Value, json};

/// One mutable knob of the create-session/`prompt_async` contract, toggled
/// independently so an A/B ladder changes exactly one variable per step.
///
/// `worktree` and `cwd` are intentionally NOT modeled here: neither the normal
/// nor the scratch path sets them explicitly today (OpenCode derives both from
/// the `?directory=` value), so they cannot be a causal difference until a
/// later ladder step proves otherwise.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionSpec {
    /// Human label for this ladder step (telemetry/tests only, never content).
    pub label: &'static str,
    /// Whether this run uses a fresh (stateless) session. Scratch is always
    /// fresh; normal chat reuses the project session across turns.
    pub fresh_session: bool,
    /// Working directory for `POST /session?directory=` (`None` => no query).
    pub directory: Option<String>,
    /// Permission ruleset body of `POST /session` (`None` => no permission).
    pub permission: Option<Value>,
    /// Optional agent override (`None` => OpenCode default agent).
    pub agent: Option<String>,
    /// Prompt text sent to `prompt_async`. The content itself is never captured
    /// or logged; only its presence/count enters [`SessionContract`].
    pub prompt: Option<String>,
    /// Pinned provider/model for `prompt_async` (`None` => default resolution).
    pub model: Option<(String, String)>,
    /// Caller-supplied directory role for telemetry (`workspace` / `scratch`).
    /// Not a wire field; excluded from [`differing_dimensions`].
    pub directory_kind: &'static str,
    /// Caller-supplied permission role for telemetry
    /// (`ordinary_external_directory_deny` / `scratch_tool_free` / `none`).
    /// Not a wire field; excluded from [`differing_dimensions`].
    pub permission_profile: &'static str,
}

impl SessionSpec {
    /// A deliberately minimal baseline: an ordinary-chat turn with a trivial
    /// prompt, the ordinary-chat permission ruleset, a workspace directory, and
    /// no model pin. Each A/B ladder step overrides exactly one field.
    pub fn baseline() -> Self {
        Self {
            label: "baseline",
            fresh_session: false,
            directory: Some("/tmp/educai-ab/workspace".to_owned()),
            permission: Some(project_opencode::external_directory_deny_permission()),
            agent: None,
            prompt: Some("hola".to_owned()),
            model: None,
            directory_kind: "workspace",
            permission_profile: "ordinary_external_directory_deny",
        }
    }

    pub fn with_label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }

    pub fn with_fresh_session(mut self, fresh: bool) -> Self {
        self.fresh_session = fresh;
        self
    }

    pub fn with_directory(mut self, directory: Option<String>) -> Self {
        self.directory = directory;
        self
    }

    pub fn with_permission(mut self, permission: Option<Value>) -> Self {
        self.permission = permission;
        self
    }

    pub fn with_agent(mut self, agent: Option<String>) -> Self {
        self.agent = agent;
        self
    }

    pub fn with_prompt(mut self, prompt: Option<String>) -> Self {
        self.prompt = prompt;
        self
    }

    pub fn with_model(mut self, model: Option<(String, String)>) -> Self {
        self.model = model;
        self
    }

    pub fn with_directory_kind(mut self, directory_kind: &'static str) -> Self {
        self.directory_kind = directory_kind;
        self
    }

    pub fn with_permission_profile(mut self, permission_profile: &'static str) -> Self {
        self.permission_profile = permission_profile;
        self
    }
}

/// Lists which dimensions differ between two specs, in a stable order. The A/B
/// ladder asserts this returns exactly one name per consecutive step.
pub fn differing_dimensions(a: &SessionSpec, b: &SessionSpec) -> Vec<&'static str> {
    let mut dims = Vec::new();
    if a.fresh_session != b.fresh_session {
        dims.push("fresh_session");
    }
    if a.directory != b.directory {
        dims.push("directory");
    }
    if a.permission != b.permission {
        dims.push("permission");
    }
    if a.agent != b.agent {
        dims.push("agent");
    }
    if a.prompt != b.prompt {
        dims.push("prompt");
    }
    if a.model != b.model {
        dims.push("model");
    }
    dims
}

/// Privacy-safe structural record of one executed session contract. Never
/// contains prompt text, assistant text, or raw directory paths.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionContract {
    /// Whether the create-session request carried a `?directory=` query.
    pub create_session_directory_present: bool,
    /// Whether the create-session body carried a non-empty `permission` array.
    pub create_session_permission_present: bool,
    /// Whether the create-session body carried an `agent` field.
    pub create_session_agent_present: bool,
    /// HTTP status of the create-session request.
    pub create_session_status: u16,
    /// HTTP status of the `prompt_async` request (204 expected).
    pub prompt_async_status: u16,
    /// `prompt_async` part types, in order (structural only).
    pub prompt_part_types: Vec<String>,
    /// `prompt_async` part count.
    pub prompt_part_count: usize,
    /// `prompt_async` pinned model, when set.
    pub prompt_model: Option<(String, String)>,
    /// Structural message-list rows observed on the final poll (never text).
    pub messages: Vec<ScratchMessageMetadata>,
    /// Terminal detection on the final poll.
    pub terminal: TerminalDetection,
    /// Stable classification of the terminal outcome
    /// (`finish_stop`, `stable_text`, `completed_without_output`, `pending`,
    /// ...).
    pub terminal_classification: &'static str,
    /// Whether `prompt_async` returned a non-empty HTTP body (`204` is empty).
    pub prompt_async_response_present: bool,
    /// Whether any assistant message was observed on the final poll.
    pub assistant_seen: bool,
    /// Parent correlation of the newest relevant assistant (`true`/`false`/
    /// `unknown`), when an assistant row exists.
    pub parent_match: Option<&'static str>,
    /// Part types of the newest relevant assistant (structural only).
    pub assistant_part_types: Vec<String>,
    /// Whether the newest relevant assistant had extractable text.
    pub text_present: bool,
    /// Byte length of that text (`0` when absent). Never the text itself.
    pub text_len: usize,
    /// Raw `finish` on the newest relevant assistant.
    pub finish: Option<String>,
    /// Whether `time.completed` was set on the newest relevant assistant.
    pub time_completed: bool,
    /// Whether `error` was present on the newest relevant assistant.
    pub error_present: bool,
    /// Wall time from create-session through terminal detection or poll budget.
    pub elapsed_ms: u128,
}

/// Runs one session contract against `backend` and returns the structural
/// capture. `backend` must already be ready. `max_polls` bounds the poll loop so
/// a stuck backend can never hang the harness; `poll_interval` is the sleep
/// between message-list reads.
///
/// This issues exactly one create-session, one `prompt_async`, and one cleanup
/// `abort` (control plane only, not a provider call), matching the production
/// scratch adapters, so it never inflates a provider-call count.
pub fn capture(
    backend: &OpenCodeBackend,
    spec: &SessionSpec,
    max_polls: usize,
    poll_interval: Duration,
) -> SessionContract {
    let started = std::time::Instant::now();
    let create_path = match &spec.directory {
        Some(directory) => with_directory_query("/session", directory),
        None => "/session".to_owned(),
    };
    let mut create_body = json!({});
    if let Some(permission) = &spec.permission {
        create_body["permission"] = permission.clone();
    }
    if let Some(agent) = &spec.agent {
        create_body["agent"] = json!(agent);
    }
    let (create_session_status, create_body_text) = backend
        .post(&create_path, &create_body)
        .unwrap_or((0, String::new()));
    let create_session_directory_present = spec.directory.is_some();
    let create_session_permission_present = spec
        .permission
        .as_ref()
        .is_some_and(|permission| permission.as_array().is_some_and(|rules| !rules.is_empty()));
    let create_session_agent_present = spec.agent.is_some();

    let session_id = serde_json::from_str::<Value>(&create_body_text)
        .ok()
        .and_then(session_id_from_value)
        .unwrap_or_default();

    // Snapshot the message ids that exist BEFORE the prompt, so a stale row
    // cannot be misattributed to this turn (mirrors the production adapters).
    let message_path = format!("/session/{session_id}/message?limit=1000");
    let before_ids: HashSet<String> = backend
        .get(&message_path)
        .ok()
        .filter(|(status, _)| (200..300).contains(status))
        .map(|(_, body)| {
            messages::session_messages(&body)
                .unwrap_or_default()
                .iter()
                .filter_map(messages::message_id)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let prompt_path = format!("/session/{session_id}/prompt_async");
    let mut prompt_body = json!({ "parts": [] });
    let (prompt_part_types, prompt_part_count) = match &spec.prompt {
        Some(prompt) => {
            prompt_body["parts"] = json!([{ "type": "text", "text": prompt }]);
            (vec!["text".to_owned()], 1usize)
        }
        None => (Vec::new(), 0usize),
    };
    if let Some((provider_id, model_id)) = &spec.model {
        prompt_body["model"] = json!({ "providerID": provider_id, "modelID": model_id });
    }
    let (prompt_async_status, prompt_response) = backend
        .post(&prompt_path, &prompt_body)
        .unwrap_or((0, String::new()));
    let prompt_async_response_present = !prompt_response.trim().is_empty();

    let mut last_messages: Vec<Value> = Vec::new();
    let mut terminal = TerminalDetection::default();
    let mut originating_user_id: Option<String> = None;
    for _ in 0..max_polls {
        if let Ok((status, body)) = backend.get(&message_path)
            && (200..300).contains(&status)
            && let Some(messages) = messages::session_messages(&body)
        {
            if originating_user_id.is_none() {
                originating_user_id = newest_user_id_not_in(&messages, &before_ids);
            }
            last_messages = messages.clone();
            terminal = detect_terminal_assistant(
                &messages,
                Some(&before_ids),
                originating_user_id.as_deref(),
            );
            if terminal.terminal_source.is_some() {
                break;
            }
        }
        std::thread::sleep(poll_interval);
    }

    // Cleanup abort mirrors production (control plane only).
    let _ = backend.post(&format!("/session/{session_id}/abort"), &json!({}));

    let (_, message_rows) = scratch_message_snapshot(
        &last_messages,
        Some(&before_ids),
        originating_user_id.as_deref(),
    );
    let terminal_classification = terminal
        .terminal_source
        .map(|source| source.as_str())
        .unwrap_or("pending");
    let assistant = message_rows
        .iter()
        .rev()
        .find(|row| row.role == "assistant" && row.relevant);
    let assistant_seen = assistant.is_some() || terminal.assistant_message_seen;
    let parent_match = assistant.map(|row| row.parent_match);
    let assistant_part_types = assistant
        .map(|row| row.part_types.clone())
        .unwrap_or_default();
    let text_present = assistant.is_some_and(|row| row.text_present);
    let text_len = assistant.map(|row| row.text_len).unwrap_or(0);
    let finish = assistant.and_then(|row| row.finish.clone());
    let time_completed = assistant.is_some_and(|row| row.time_completed);
    let error_present = assistant.is_some_and(|row| row.error_present);
    let elapsed_ms = started.elapsed().as_millis();

    SessionContract {
        create_session_directory_present,
        create_session_permission_present,
        create_session_agent_present,
        create_session_status,
        prompt_async_status,
        prompt_part_types,
        prompt_part_count,
        prompt_model: spec.model.clone(),
        messages: message_rows,
        terminal,
        terminal_classification,
        prompt_async_response_present,
        assistant_seen,
        parent_match,
        assistant_part_types,
        text_present,
        text_len,
        finish,
        time_completed,
        error_present,
        elapsed_ms,
    }
}

fn session_id_from_value(value: Value) -> Option<String> {
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

fn newest_user_id_not_in(messages: &[Value], before_ids: &HashSet<String>) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| {
            messages::message_role(message) == "user"
                && messages::message_id(message).is_some_and(|id| !before_ids.contains(id))
        })
        .and_then(messages::message_id)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn backend() -> (OpenCodeBackend, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), tmp.path().join("cfg"), 0);
        (backend, tmp)
    }

    fn summarizer_prompt() -> String {
        "Resumí el documento.\nDocumento: sample.md.\n[E1]\nSe definió el presupuesto.".to_owned()
    }

    /// The A/B ladder changes exactly one dimension per step:
    ///   A baseline normal -> B fresh session -> C summarizer prompt ->
    ///   D scratch directory -> E scratch permission -> F scratch contract
    ///   (+ pinned model). Each consecutive pair differs by exactly one name.
    #[test]
    fn ab_ladder_changes_exactly_one_dimension_per_step() {
        let baseline = SessionSpec::baseline().with_label("A normal");
        let fresh = baseline
            .clone()
            .with_label("B fresh")
            .with_fresh_session(true);
        let summarizer_prompt = fresh
            .clone()
            .with_label("C summarizer prompt")
            .with_prompt(Some(summarizer_prompt()));
        let scratch_dir = summarizer_prompt
            .clone()
            .with_label("D scratch directory")
            .with_directory(Some("/tmp/educai-ab/scratch".to_owned()));
        let scratch_permission = scratch_dir
            .clone()
            .with_label("E scratch permission")
            .with_permission(Some(project_opencode::scratch_tool_free_permission()));
        let scratch_contract = scratch_permission
            .clone()
            .with_label("F scratch actual")
            .with_model(Some(("opencode".to_owned(), "big-pickle".to_owned())));

        let steps = [
            &baseline,
            &fresh,
            &summarizer_prompt,
            &scratch_dir,
            &scratch_permission,
            &scratch_contract,
        ];
        for pair in steps.windows(2) {
            let diff = differing_dimensions(pair[0], pair[1]);
            assert_eq!(
                diff.len(),
                1,
                "{} -> {} must change exactly one dimension, got {diff:?}",
                pair[0].label,
                pair[1].label
            );
        }
        assert_eq!(differing_dimensions(&baseline, &fresh), ["fresh_session"]);
        assert_eq!(differing_dimensions(&fresh, &summarizer_prompt), ["prompt"]);
        assert_eq!(
            differing_dimensions(&summarizer_prompt, &scratch_dir),
            ["directory"]
        );
        assert_eq!(
            differing_dimensions(&scratch_dir, &scratch_permission),
            ["permission"]
        );
        assert_eq!(
            differing_dimensions(&scratch_permission, &scratch_contract),
            ["model"]
        );
    }

    /// Capturing the normal baseline and the scratch contract against the fake
    /// server must observe the structural wire differences (directory, global
    /// permission deny) without leaking prompt/directory content.
    #[test]
    fn capture_observes_structural_contract_differences_offline() {
        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_response_finish("stop");
        server.set_prompt_response_text(
            r#"{"summary":"ok","topics":[],"decisions":[],"action_items":[],"questions":[]}"#,
        );
        let (backend, _tmp) = backend();
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("ready");

        let normal = SessionSpec::baseline().with_label("normal");
        let scratch = SessionSpec::baseline()
            .with_label("scratch")
            .with_fresh_session(true)
            .with_directory(Some("/tmp/educai-ab/scratch".to_owned()))
            .with_permission(Some(project_opencode::scratch_tool_free_permission()))
            .with_prompt(Some(summarizer_prompt()))
            .with_model(Some(("opencode".to_owned(), "big-pickle".to_owned())));

        let normal_contract = capture(&backend, &normal, 40, Duration::from_millis(20));
        let scratch_contract = capture(&backend, &scratch, 40, Duration::from_millis(20));

        // Both paths hit the same create-session/prompt_async/message endpoints.
        assert!(normal_contract.create_session_directory_present);
        assert!(scratch_contract.create_session_directory_present);
        assert!((200..300).contains(&normal_contract.create_session_status));
        assert!((200..300).contains(&scratch_contract.create_session_status));
        assert_eq!(normal_contract.prompt_part_count, 1);
        assert_eq!(scratch_contract.prompt_part_count, 1);
        assert_eq!(normal_contract.prompt_part_types, ["text"]);
        assert_eq!(scratch_contract.prompt_part_types, ["text"]);

        // The scratch contract pins the model; the normal baseline does not.
        assert_eq!(normal_contract.prompt_model, None);
        assert_eq!(
            scratch_contract.prompt_model,
            Some(("opencode".to_owned(), "big-pickle".to_owned()))
        );

        // Neither path sends an explicit `agent` field today (the default agent
        // governs both) — captured structurally, not asserted as a root cause.
        assert!(!normal_contract.create_session_agent_present);
        assert!(!scratch_contract.create_session_agent_present);

        // The fake server returns a `finish=stop` assistant, so both complete.
        assert_eq!(normal_contract.terminal_classification, "finish_stop");
        assert_eq!(scratch_contract.terminal_classification, "finish_stop");

        // Privacy: a completed empty turn must classify as empty (not success).
        let empty = serde_json::to_string(&serde_json::json!([
            {"info": {"id": "u1", "role": "user"}, "parts": [{"type": "text", "text": "x"}]},
            {"info": {"id": "a1", "role": "assistant", "parentID": "u1", "time": {"completed": 2}}, "parts": []}
        ]))
        .unwrap();
        server.set_messages_sequence(&["[]", empty.as_str()]);
        let empty_contract = capture(&backend, &scratch, 40, Duration::from_millis(20));
        assert_eq!(
            empty_contract.terminal_classification,
            "completed_without_output"
        );
    }
}
