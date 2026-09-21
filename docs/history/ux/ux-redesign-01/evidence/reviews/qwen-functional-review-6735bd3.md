# FUNCTIONAL REVIEW — REAL FREE-MODEL FIX (TASK A)

Commit reviewed: 6735bd3 (fix(agent,provider,fake): real /session/status polling + /session/{id}/message retrieval) on top of 2dcacc5, branch uxfix/functional.
Reviewer model: opencode-go/qwen3.8-flash (MODEL_REQUESTED==MODEL_ACTUAL). Read-only review; no edits, no commits.

VERDICT: REQUEST_CHANGES

FINDINGS:
- CODE_BLOCKER: none.
- CODE_IMPORTANT:
  1. The real completion signal — an EMPTY `/session/status` map (missing session key) — is never produced by the fake nor exercised by any test. The fake always emits `{"<own-id>":{"type":"<phase>"}}`, including `{"type":"idle"}`, a shape the real 1.18.25 sidecar never returns (real idle = key omitted; real busy = `{"type":"busy"}`, not `"working"`). Consequently the new `.unwrap_or_else(|| "idle")` branches in opencode.rs:96 and adapter.rs:417 have zero coverage, and the fake's default `messages_body` still uses the old invented top-level `role` shape (fake-opencode-server/src/lib.rs:202) instead of the real `{info:{role},parts:[...]}`.
  2. `send_status_endpoint_is_scoped_to_session_id` (tests/opencode_adapter.rs:277-289) does not actually test scoping: the fake keys the status map by `last_session_id`, which is always the engine's own session, so a buggy client that read ANY map entry (or a global busy flag) would pass identically. A true regression needs a status body where a foreign session is busy while the engine's own key is absent (assert: still completes) and/or own entry idle while another is busy.
  3. Pre-busy race on reused sessions: `poll_session` issues its first GET immediately after the 204 from `prompt_async` (no initial sleep, opencode.rs:89-90), and "missing entry" is indistinguishable from "not yet marked busy". Sessions are cached per project and reused across turns (opencode.rs:171-178), so a hit in that window silently returns the PREVIOUS turn's assistant text as the new completion (agent side) or a false ProviderUnavailable (provider side). Probability is low given the sidecar's single-threaded ordering (busy is set before the next request is processed), and the behavior matches the stated contract, so it is not a blocker — but a cheap watermark inside the existing functions (e.g. snapshot message count/last assistant id before `prompt_async`, require a NEW assistant message) would close it without redesign.
- CODE_POLISH:
  - Provider `poll_test_session` keeps an inline 20ms sleep while the agent got `STATUS_POLL_INTERVAL`; and `session_status_phase`/`last_assistant_text_from_messages`/`message_text` are now verbatim copies across the two crates — acceptable given the no-redesign constraint, but worth a follow-up note.
  - Test names say "busy" (`test_connection_busy_then_idle_connected`) while sequences use `"working"`; use the real `"busy"` string to mirror the sidecar.
  - `map_session_failure_text` dropped the old `value.get("error")` haystack; correct for the real session schema (no top-level `error`), just confirming it was intentional.

Per-focus review:

1. CORRECTNESS of polling — Semantics are right per the documented contract: `value.get(session_id)` reads only the engine's own key (no global busy read), missing entry defaults to `idle`, non-terminal phases loop, `failed/error` → TaskFailed (with assistant text if available), `aborted/cancelled` → Cancelled, deadline checked each iteration with 20ms sleeps against the 120s `DEFAULT_TASK` (project-agent/src/opencode.rs:20,86-123). Non-2xx → `AgentError::Http`, transport errors mapped via `map_backend_error`; no regression in error mapping. The only gap is the pre-busy race described in finding 3.

2. Message retrieval — `/session/{id}/message?limit=1000` parsing accepts both a bare array and the `{"data":[...]}` envelope, extracts `parts[].type==text` with `info.role` fallback (opencode.rs:125-142,308-349), takes the LAST assistant message, and idle-without-assistant-text → `TaskFailed("assistant completed without a response")`, surfaced to the user as the generic Spanish `AiTaskFailed` (project-app/src/error.rs:180-182) — no leak. Correct.

3. poll_test_session consistency — Uses the same `/session/status` + `/session/{id}/message` signal with identical phase tables and session-scoped lookup (project-provider/src/adapter.rs:405-461); idle-without-text → ProviderUnavailable, failed → text-based credential/model mapping preserved. Consistent with the agent engine.

4. Fake alignment — Partially aligned: routes, `{id,type}` map keying, prompt/abort/diff/message handlers, real session-object shape on `GET /session/{id}`, query-string stripping, and a stuck-busy timeout model (last phase repeats forever, exercised by `send_never_idle_times_out`/`test_connection_timeout_is_provider_unavailable`) all work. But it never emits the real terminal signal (empty map), never uses the real `"busy"` string, and its default message body is still the invented top-level-role shape — hence the fidelity finding.

5. Regression tests — Deterministic and offline (in-process fake, fixed ids, wall-clock margins of 5-25x), and they cover busy→idle completion, message retrieval via the new endpoint, no-assistant failure, failed-status mapping, and timeout. However, they do not assert the two most load-bearing real-contract behaviors: missing-entry-as-completed (uncovered) and true session scoping (falsely passing test) — findings 1 and 2.

6. Scope/hygiene — Exactly the 5 declared files; no model name in product source (`big-pickle` only in tests/fake); `log_event` emits fixed strings only (no prompts/credentials), and the provider keeps its `redact_credentials` second layer; OpenCodeBackend untouched. Re-ran `cargo test -p project-agent` (17 green in opencode_adapter) and `-p project-provider` (all suites green, 19 in provider_service) — no regressions.
