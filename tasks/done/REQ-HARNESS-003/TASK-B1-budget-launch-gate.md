# TASK-B1: Session-budget launch decision

- Requirement: `REQ-HARNESS-003`
- Base SHA: `3d85a3f204af54e0169877f70f1ab855d6c11b26`
- Status: `PASS`
- Human gate: `NO_HUMAN`
- Author / reviewer: Harness Architect/Implementer / independent Reviewer (not this author)

## Scope

- Objective: make `check-session-budget` label non-OpenCode orchestrators
  distinctly, and make `agent-launch --launch` warn-and-proceed on expected
  UNKNOWN while still refusing rotate bands and unreadable identified OpenCode
  sessions.
- Owned paths: `scripts/check-session-budget`, `scripts/agent-launch`,
  `docs/AGENT_POLICY.md`
- Allowed paths: `docs/MULTI_AGENT_WORKFLOW.md`, `prompts/orchestrator.md`,
  `scripts/verify`, this requirement directory
- Prohibited paths: `app/`, `crates/`, `tasks/active/REQ-DOCS-001/`,
  `opencode.json`, product authorities
- Non-goals: changing token thresholds; measuring some other OpenCode session.

## Architecture impact

- Affected contracts: `None`
- Required ADR/change control: `None`

## Acceptance and verification

- Acceptance criteria: Cursor/Herdr/Codex UNKNOWN does not yield launcher
  exit 12; OpenCode rotate still exit 10/11; identified OpenCode unreadable
  still fail-closed.
- Focused commands: `./scripts/test-session-budget`;
  `./scripts/test-agent-launch-budget`
- General gate: `CI=true ./scripts/verify`

## Handoff

- Implementation result: `IMPLEMENTATION_COMPLETE`. `check-session-budget` labels
  Cursor/Herdr/Codex; `agent-launch` warns-and-proceeds on those UNKNOWN
  platforms; OpenCode unreadable and rotate bands remain fail-closed.
- Verification evidence: `tasks/done/REQ-HARNESS-003/evidence.md`
- Reviewer verdict: `PASS` (independent; owner-confirmed for closure)
- Rework history: none
