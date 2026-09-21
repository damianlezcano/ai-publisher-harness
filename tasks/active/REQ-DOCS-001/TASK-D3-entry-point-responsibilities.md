# TASK-D3: Entry-point and Harness responsibility cleanup

- Requirement: `REQ-DOCS-001`
- Base SHA: `5664e44` (Orchestrator updates after D1/D2 integration)
- Status: `IN_REVIEW`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: Orchestrator fallback in worktree `../ai-publisher-req-docs-001-d1` / independent Reviewer `review`/`opencode`

## Scope

- Objective: make bootstrap, durable handoff, repository rules, runtime routing, methodology, policy, workflow, and prompts point to one another without repeating authority content.
- Owned paths: `START_CODEX.txt`, `CODEX_HANDOFF.md`, `AGENTS.md`, `RUNTIME.md`, `prompts/orchestrator.md`, `prompts/worker.md`, `prompts/reviewer.md`, and moved `docs/engineering/{HARNESS_ENGINEERING,AGENT_POLICY,MULTI_AGENT_WORKFLOW,WORKTREES,REQUIREMENTS,TESTING,VERIFY,DEFINITION_OF_DONE}.md`.
- Allowed paths: `docs/README.md` and active requirement handoff files when a pointer is needed.
- Prohibited paths: root `README.md` (D4), product/runtime code, ADRs, Architecture Contract content/manifest, `opencode.json`, `a.zip`, and protected examples.
- Non-goals: do not change role authority, human-gate semantics, model routing policy, protected product boundaries, or current checkpoint facts beyond path/reference corrections.
- Dependencies: D1 ledger and D2 taxonomy move must be integrated; D3 may run alongside D4 only after D2.

## Architecture impact

- Affected contracts: `None`; operational documents must continue to require `ARCHITECTURE_CHANGE_REQUIRED: AC-XXX` where applicable.
- Required ADR/change control: `None`.

## Acceptance and verification

- Acceptance criteria:
  - START_CODEX is a short bootstrap pointer, not a duplicate handoff/policy document.
  - CODEX_HANDOFF remains durable continuation context and links to current checkpoint; AGENTS remains concise mandatory operating rules; RUNTIME remains execution routing/human gates.
  - HARNESS_ENGINEERING is methodology; AGENT_POLICY is cost/reliability/session policy; MULTI_AGENT_WORKFLOW is delegation procedure; prompts are role-local contracts.
  - TESTING, VERIFY, and DEFINITION OF DONE explicitly cross-reference their separate purposes; CURRENT_CHECKPOINT/checkpoints boundary matches the requirement.
  - No permanent prompt or requirement hardcodes a provider/model.
- Focused commands:
  - `rg -n 'START_CODEX|CODEX_HANDOFF|CURRENT_CHECKPOINT|HARNESS_ENGINEERING|AGENT_POLICY|MULTI_AGENT_WORKFLOW' README.md AGENTS.md CODEX_HANDOFF.md START_CODEX.txt RUNTIME.md prompts docs/engineering`
  - `git diff --check`
- General gate: D6.

## Handoff

- Implementation result: `STATUS: IMPLEMENTATION_COMPLETE`. START_CODEX reduced to bootstrap pointer; CODEX_HANDOFF/AGENTS/RUNTIME/prompts and `docs/engineering/*` retargeted to the new taxonomy without merging those surfaces.
- Verification evidence: path retargets in owned files; `git diff --check` pass. Merge with later `main` harness short-intent work is D6 integration.
- Reviewer verdict: not recorded (independent Reviewer pending).
- Rework history: none.
