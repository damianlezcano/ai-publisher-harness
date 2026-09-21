# TASK-O3: Minimal session bootstrap and gate evidence

- Requirement: `REQ-HARNESS-002`
- Base SHA: `5664e44`
- Status: `PASS`
- Human gate: `NO_HUMAN`
- Author / reviewer: Harness Architect/Implementer / independent Reviewer required

## Scope

- Objective: reduce `START_CODEX.txt` and trim Orchestrator-procedure copies
  from `CODEX_HANDOFF.md`; do not execute REQ-DOCS-001; run required gates.
- Owned paths: `START_CODEX.txt`, `CODEX_HANDOFF.md`,
  `scripts/test-short-intent-orchestration`,
  `tasks/active/REQ-HARNESS-002/`
- Allowed paths: `README.md`
- Prohibited paths: `app/`, `crates/`, `tasks/backlog/REQ-DOCS-001/`
- Non-goals: committing, pushing, or closing without independent review.

## Architecture impact

- Affected contracts: `None`
- Required ADR/change control: `None`

## Acceptance and verification

- Acceptance criteria: bootstrap is a pointer; gates pass; this requirement
  does not close or execute REQ-DOCS-001.
- Focused commands: `./scripts/test-short-intent-orchestration`;
  `./scripts/architecture-verify`; `git diff --check`
- General gate: `CI=true ./scripts/verify`

## Handoff

- Implementation result: bootstrap slimmed; evidence file after gates.
- Verification evidence: recorded in `evidence.md`.
- Reviewer verdict: `PASS` (independent; owner-confirmed for closure).
- Rework history: Worker cheap routing corrected (OpenCode Go primary,
  Cursor last-resort). See `evidence.md`.
