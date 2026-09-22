# TASK-WIN-3: Knowledge en Windows y evidencia de gate release

- Requirement: `REQ-WIN-001`
- Base SHA: `4eafcd7fdb90eed6d7ab44219189e80d82a4abe9`
- Status: `PLANNED`
- Human gate: `RELEASE_HUMAN`
- Author / reviewer: assigned after TASK-WIN-2 / independent reviewer assigned before review

## Scope

- Objective: prove or accurately block the current Windows Knowledge
  distribution contract: ONNX DLL payload/loading, first-use model path,
  material/import/index behavior, grounded Knowledge interaction or truthful
  degraded state, restart, and release-human evidence.
- Owned paths: evidence under `tasks/active/REQ-WIN-001/` after selection;
  a separate bounded fix contract if a reproducible payload/runtime defect is
  found.
- Allowed paths: designated Windows test artifact/data, relevant Knowledge
  distribution tests and safe diagnostics.
- Prohibited paths: raw material/prompt/provider content in evidence, model or
  component version substitution, architecture changes, and unrelated code.
- Non-goals: declaring prior non-Knowledge Windows HUMAN-PASS sufficient;
  bypassing local-only/offline degradation semantics.

## Architecture impact

- Affected contracts: `AC-013` (privacy) and current Knowledge invariants;
  preserve them.
- Required ADR/change control: ADR-0016 governs runtime/payload behavior. Any
  protected-contract change stops with `ARCHITECTURE_CHANGE_REQUIRED: AC-XXX`.

## Acceptance and verification

- Acceptance criteria: satisfy REQ-WIN-001 criteria 1, 2, 5, 7, and 8; retain
  only sanitized machine/artifact/operator evidence; obtain independent review
  and `RELEASE_HUMAN` before requirement closure.
- Focused commands: native payload/load and focused Knowledge tests, relevant
  installer/runtime observations, native format/lint/type checks; Linux
  architecture/general gate after integrated source changes.
- General gate: `CI=true ./scripts/verify` in its supported Linux environment.

## Handoff

- Implementation result: pending.
- Verification evidence: pending.
- Reviewer verdict: pending independent `PASS`.
- Rework history: none.
