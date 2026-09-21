# TASK-B2: Reproduction tests, live Herdr check, evidence

- Requirement: `REQ-HARNESS-003`
- Base SHA: `3d85a3f204af54e0169877f70f1ab855d6c11b26`
- Status: `PASS`
- Human gate: `NO_HUMAN`
- Author / reviewer: Harness Architect/Implementer / independent Reviewer (not this author)

## Scope

- Objective: add deterministic tests that encode the smoke-test failure and the
  correction; wire them into `scripts/verify`; record how to confirm a real
  cheap OpenCode worker starts in Herdr.
- Owned paths: `scripts/test-agent-launch-budget`, `scripts/test-session-budget`,
  `scripts/test-agent-launch`, `scripts/verify`,
  `tasks/done/REQ-HARNESS-003/evidence.md`
- Allowed paths: `docs/AGENT_POLICY.md`
- Prohibited paths: `app/`, `crates/`, `tasks/active/REQ-DOCS-001/`,
  `opencode.json`
- Non-goals: sending product work to the smoke worker; closing REQ-DOCS-001.
- Dependencies: TASK-B1

## Architecture impact

- Affected contracts: `None`
- Required ADR/change control: `None`

## Acceptance and verification

- Acceptance criteria: tests fail closed on rotate; tests allow UNKNOWN for
  non-OpenCode platforms; verify runs the new test; evidence records a Herdr
  confirmation procedure (and a live attempt if Herdr is available).
- Focused commands: `./scripts/test-agent-launch-budget`;
  `CI=true ./scripts/verify`; `git diff --check`
- General gate: `CI=true ./scripts/verify`

## Handoff

- Implementation result: `IMPLEMENTATION_COMPLETE`. Added
  `scripts/test-agent-launch-budget`; Herdr UNKNOWN coverage in
  `test-session-budget`; wired into `scripts/verify`. Live cheap OpenCode
  worker opened in Herdr (`harness003w`) with matching models, then closed
  without a product prompt.
- Verification evidence: `tasks/done/REQ-HARNESS-003/evidence.md`
- Reviewer verdict: `PASS` (independent; owner-confirmed for closure)
- Rework history: none
