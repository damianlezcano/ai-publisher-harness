# FUNCTIONAL RE-REVIEW — TASK A (uxfix/functional)

Commit reviewed: 030d788 (fix(agent,provider,fake): real terminal /session/status signal + watermark + scoping tests) on top of 6735bd3.
Reviewer model: opencode-go/qwen3.8-flash. Read-only review; no edits, no commits.

VERDICT: APPROVE

FINDINGS:
- CODE_BLOCKER: none.
- CODE_IMPORTANT: none. All three prior importants are properly resolved:
  1. Terminal-signal fidelity: the fake now emits the real contract — empty `{}` map on idle, `{"<id>":{"type":"busy"}}` while working (incl. `working`→`busy` normalization) — and the default message body uses the real `{info:{role},parts:[...]}` shape. The `.unwrap_or_else(|| "idle")` branch (opencode.rs:96, adapter.rs:417) is now covered by `send_completes_when_status_map_omits_session_key` and by the foreign-busy override tests.
  2. Real scoping regression tests: `send_ignores_foreign_session_in_status_map` and `test_connection_ignores_foreign_busy_session` serve a status map where a foreign session is busy while the engine's own key is absent; an implementation that read any map value (or a global busy flag) would loop to timeout, so these tests genuinely fail on the old bug shape.
  3. Pre-busy/stale-text race fixed by the assistant-message-count watermark: agent counts assistant messages before `prompt_async` (opencode.rs:238) and completes only when the count strictly exceeds it after idle, otherwise keeps polling until the deadline; `send_idle_without_new_assistant_message_times_out` proves the previous turn's text can no longer be silently returned on reused per-project sessions. Provider applies the same watermark (adapter.rs:331). Provider interval constant aligned (`STATUS_POLL_INTERVAL`).
- CODE_POLISH (non-blocking, optional follow-up):
  - A transient non-2xx from `/session/{id}/message` inside the poll loop now aborts an otherwise-healthy task via `?` (AgentError::Http) instead of retrying until deadline.
  - `assistant_message_count` requires `role=="assistant"` while `last_assistant_text_from_messages` also accepts role-less messages (inconsistent; safe direction — only over-timeout, never stale text).
  - Provider `before_assistant_count` uses `.unwrap_or(0)`; harmless only because the connector always creates a fresh scratch session per `test_connection`.
  - Fake still emits `{"type":"failed"}`, a value the real 1.18.25 sidecar does not produce (failures surface via message error objects); kept for legacy defensive mapping.
  - A legitimately completed turn with no new assistant message now burns the full task timeout instead of failing fast — intentional trade-off, tested.

Per-focus re-verification:
1. Real polling correctness — session-scoped `value.get(session_id)`, missing entry = idle, watermark gates completion; failed/aborted/timeout paths and HTTP error mapping preserved (project-agent/src/opencode.rs:86-133, project-provider/src/adapter.rs:406-452).
2. Message retrieval — bare-array and `{"data":[...]}` envelope accepted; last assistant text extracted from `info.role`/`parts`; completion requires a NEW assistant message; failure path keeps generic "task failed" fallback (no leak — project-app maps to generic Spanish).
3. poll_test_session — same real signal and watermark as the agent; foreign-busy scoping and no-response → ProviderUnavailable both tested (provider timeout uses the 2s test seam, deterministic).
4. Fake fidelity — empty-map terminal, busy string, prompt_async appends an assistant response (`set_prompt_appends_response`), real default message shape.
5. Tests — deterministic, offline, fixed ids, ample timeout margins; new tests assert empty-map completion, real scoping, and watermark timeout in both crates.
6. Scope/hygiene — same 5 files only; no model name in product source (`big-pickle` only in tests/fake); no prompt/secret logging; OpenCodeBackend untouched.

Checks run: `cargo test -p project-agent` (18 green in opencode_adapter; all suites green), `cargo test -p project-provider` (24 green in provider_adapter; all suites green incl. 19 provider_service, 12 provider_security), `cargo fmt --check` clean, working tree clean at 030d788.
