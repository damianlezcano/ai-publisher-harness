# TASK-METRICS-3: Verificación transversal y gate humano de métricas

- Requirement: `REQ-METRICS-001`
- Base SHA: `4ba4ad3139cd61ac993dfc6fe7980651b95db165`
- Status: `PLANNED`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: assigned by the Orchestrator on activation / independent reviewer assigned on activation

## Scope

- Objective: collect reproducible cross-surface evidence and guide the targeted
  human UX review of representative metric states without recording sensitive
  user/provider content.
- Owned paths: verification/evidence material under
  `tasks/active/REQ-METRICS-001/` after selection.
- Allowed paths: read-only product UI/runtime operation and test artifacts;
  test-only changes only if the Orchestrator gives a bounded rework assignment.
- Prohibited paths: product behavior changes, protected contracts, packaging,
  and the assignment’s named local out-of-scope files.
- Non-goals: self-reviewing implementation or closing the requirement.

## Architecture impact

- Affected contracts: `AC-012`, `AC-013` (verification only).
- Required ADR/change control: `None`.

## Acceptance and verification

- Acceptance criteria: evidence covers provider actual/unavailable, local-only,
  retrieval, multi-call, failed/recovered/retry/reload binding, keyboard/popover
  access, responsive copy, privacy boundaries, and the human decision on
  visible-field usefulness.
- Focused commands: all focused commands from the implementation task;
  `./scripts/architecture-verify`; `CI=true ./scripts/verify`; `git diff --check`.
- General gate: `CI=true ./scripts/verify`.

## Handoff

- Implementation result: pending.
- Verification evidence: pending.
- Reviewer verdict: pending independent `PASS`.
- Rework history: none.
