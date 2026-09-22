# TASK-METRICS-2: Contrato visible, binding y regresiones de métricas

- Requirement: `REQ-METRICS-001`
- Base SHA: `4ba4ad3139cd61ac993dfc6fe7980651b95db165`
- Status: `PLANNED`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: assigned by the Orchestrator on activation / independent reviewer assigned on activation

## Scope

- Objective: implement only the semantic decisions approved in
  `TASK-METRICS-1`, across the existing per-turn and Conversation Details
  surfaces, plus their backend/frontend regression coverage.
- Owned paths: exact paths assigned by the Orchestrator after the inventory;
  expected bounded candidates are `app/src/types.ts`, `app/src/messages.ts`,
  `app/src/turnMetricsBinding.ts`, `app/src/components/AssistantMetrics.tsx`,
  `app/src/components/ConversationMetrics.tsx`, their tests, and necessary
  `crates/project-app/` metric DTO/application tests.
- Allowed paths: task evidence under `tasks/active/REQ-METRICS-001/` and
  focused test/configuration files required by the approved change.
- Prohibited paths: routing/session architecture, provider credential behavior,
  publication, packaging, sidecars, unrelated UI, protected authorities, and
  the assignment’s named local out-of-scope files.
- Non-goals: fabricate telemetry; expose sensitive/internal data; redesign
  Knowledge or provider behavior.

## Architecture impact

- Affected contracts: `AC-012`, `AC-013`; preserve, do not edit.
- Required ADR/change control: `None`, unless a needed change alters an AC.

## Acceptance and verification

- Acceptance criteria: satisfy REQ-METRICS-001 criteria 2–7; tests cover
  binding ambiguity/recovery/reload plus provider-actual, unavailable,
  local-only, retrieval, and multi-call semantics as applicable to the change.
- Focused commands: selected `pnpm` frontend tests/type/lint/format/build and
  focused `cargo test -p project-app` commands, then
  `./scripts/architecture-verify` and `git diff --check`.
- General gate: `CI=true ./scripts/verify`.

## Handoff

- Implementation result: pending.
- Verification evidence: pending.
- Reviewer verdict: pending.
- Rework history: none.
