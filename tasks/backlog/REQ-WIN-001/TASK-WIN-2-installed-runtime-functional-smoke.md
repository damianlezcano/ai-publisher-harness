# TASK-WIN-2: Smoke funcional del instalador Windows y persistencia

- Requirement: `REQ-WIN-001`
- Base SHA: `4ba4ad3139cd61ac993dfc6fe7980651b95db165`
- Status: `PLANNED`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: assigned by the Orchestrator on activation / independent reviewer assigned on activation

## Scope

- Objective: on the designated real Windows PC, validate the installed NSIS
  artifact through launch, owned sidecars, project/conversation, attachments,
  Creation, preview, publication/public URL/stop, filesystem safety, and
  restart/update persistence.
- Owned paths: evidence under `tasks/active/REQ-WIN-001/` after selection.
- Allowed paths: installed test artifact and project data created specifically
  for this validation; safe read-only process/artifact inspection.
- Prohibited paths: arbitrary user data, credentials, unrelated projects,
  product code unless a reproduced failure gets a separate rework contract,
  and the assignment’s named local out-of-scope files.
- Non-goals: automatic test account provisioning or treating a failed public
  network as a code issue without diagnosis.

## Architecture impact

- Affected contracts: `AC-013` and publication security invariants.
- Required ADR/change control: `None` for validation; failure fixes follow
  normal change control.

## Acceptance and verification

- Acceptance criteria: satisfy REQ-WIN-001 criteria 3, 4, and 6 with a
  human-run, safe evidence checklist, including clean shutdown and restart.
- Focused commands: native process/lifecycle checks, relevant Rust/frontend
  tests, installer install/update commands, and inspection of test data only.
- General gate: `CI=true ./scripts/verify` in Linux after source changes.

## Handoff

- Implementation result: pending.
- Verification evidence: pending.
- Reviewer verdict: pending.
- Rework history: none.
