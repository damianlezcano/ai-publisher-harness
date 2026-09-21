# TASK-O2: Requirement locate and role-class mapping

- Requirement: `REQ-HARNESS-002`
- Base SHA: `5664e44`
- Status: `PASS`
- Human gate: `NO_HUMAN`
- Author / reviewer: Harness Architect/Implementer / independent Reviewer required

## Scope

- Objective: add the smallest executable helpers so an Orchestrator can locate a
  REQ-ID lifecycle and map Harness classes onto `scripts/agent-launch` without
  embedding model IDs.
- Owned paths: `scripts/requirement-status`, `scripts/test-requirement-status`,
  `scripts/agent-class-resolve`, `scripts/test-agent-class-resolve`,
  `RUNTIME.md`, `scripts/verify`
- Allowed paths: `docs/AGENT_POLICY.md` (pointer only if needed)
- Prohibited paths: `app/`, `crates/`, `config/agent-models.env` value rewrites,
  `tasks/backlog/REQ-DOCS-001/`
- Non-goals: a new launcher that bypasses Herdr/`agent-launch --launch`.

## Architecture impact

- Affected contracts: `None`
- Required ADR/change control: `None`

## Acceptance and verification

- Acceptance criteria: `requirement-status` locates lifecycle from `tasks/`;
  class resolve emits launch role/provider (and cheap last-resort fallback);
  Worker cheap prefers OpenCode Go; verify runs the new tests.
- Focused commands: `./scripts/test-requirement-status`;
  `./scripts/test-agent-class-resolve`
- General gate: `CI=true ./scripts/verify`

## Handoff

- Implementation result: helpers plus tests wired into `scripts/verify`.
- Verification evidence: recorded in `evidence.md` after gates.
- Reviewer verdict: `PASS` (independent; owner-confirmed for closure after routing rework).
- Rework history: Worker cheap primary was `low`/`cursor`; corrected to
  `low`/`opencode` with `low`/`cursor` last-resort fallback.
