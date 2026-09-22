# TASK-METRICS-1: Inventario y contrato semántico de métricas

- Requirement: `REQ-METRICS-001`
- Base SHA: `4ba4ad3139cd61ac993dfc6fe7980651b95db165`
- Status: `PLANNED`
- Human gate: `NO_HUMAN`
- Author / reviewer: assigned by the Orchestrator on activation / independent reviewer assigned on activation

## Scope

- Objective: produce the evidence-backed field matrix that decides which
  existing metrics are real provider telemetry, local structural facts,
  estimates, or internal-only, and specifies source, unit, scope,
  applicability, aggregation, unavailable state, and copy intent.
- Owned paths: `tasks/active/REQ-METRICS-001/` after selection, including the
  semantic decision record created there.
- Allowed paths: read-only inspection of the requirement, `app/src/`,
  `crates/project-app/`, relevant tests, architecture contracts, checkpoints,
  and current product/UX/security authorities.
- Prohibited paths: `app/`, `crates/`, protected authorities/contracts, and
  every out-of-scope local file named in `AGENTS.md` for this assignment.
- Non-goals: implementation, copy changes, new telemetry, or guessing missing
  provider semantics.

## Architecture impact

- Affected contracts: `AC-012`, `AC-013` (inspection only).
- Required ADR/change control: `None`; stop with
  `ARCHITECTURE_CHANGE_REQUIRED: AC-XXX` if a proposed semantic change would
  alter a protected invariant.

## Acceptance and verification

- Acceptance criteria: every currently exposed metric has a traceable decision;
  unknown values are explicitly unavailable/internal-only; the record lists
  the exact tests that will prove each planned change.
- Focused commands: `rg` evidence inventory; relevant existing metric test
  commands selected from the codebase; `git diff --check`.
- General gate: `CI=true ./scripts/verify` is required after implementation,
  not for this documentation-only planning task unless its own change warrants it.

## Handoff

- Implementation result: pending.
- Verification evidence: pending.
- Reviewer verdict: pending.
- Rework history: none.
