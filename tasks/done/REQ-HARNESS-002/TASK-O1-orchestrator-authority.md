# TASK-O1: Orchestrator workflow authority and short intent

- Requirement: `REQ-HARNESS-002`
- Base SHA: `5664e44`
- Status: `PASS`
- Human gate: `NO_HUMAN`
- Author / reviewer: Harness Architect/Implementer / independent Reviewer required

## Scope

- Objective: make `prompts/orchestrator.md` the single Orchestrator execution
  contract, including short-intent bootstrap, lifecycle, delegation, gates,
  independent review, and human-gate stop. Point other Harness docs at it.
- Owned paths: `prompts/orchestrator.md`, `prompts/worker.md`,
  `prompts/reviewer.md`, `AGENTS.md`, `docs/HARNESS_ENGINEERING.md`,
  `docs/REQUIREMENTS.md`, `docs/MULTI_AGENT_WORKFLOW.md`
- Allowed paths: `README.md`, `RUNTIME.md`
- Prohibited paths: `app/`, `crates/`, `tasks/backlog/REQ-DOCS-001/`,
  Architecture Contracts, product authorities
- Non-goals: executing REQ-DOCS-001; duplicating the full workflow in
  START_CODEX or CODEX_HANDOFF.

## Architecture impact

- Affected contracts: `None`
- Required ADR/change control: `None`

## Acceptance and verification

- Acceptance criteria: short intent is defined as a complete assignment;
  remaining docs reference the Orchestrator contract instead of restating it.
- Focused commands: `./scripts/test-short-intent-orchestration`
- General gate: `CI=true ./scripts/verify`

## Handoff

- Implementation result: see repository diff for owned paths.
- Verification evidence: recorded in `evidence.md` after gates.
- Reviewer verdict: `PASS` (independent; owner-confirmed for closure).
- Rework history: none for this task's owned paths; see TASK-O2 routing rework.
