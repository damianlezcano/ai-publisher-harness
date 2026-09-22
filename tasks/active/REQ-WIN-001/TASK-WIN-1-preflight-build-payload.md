# TASK-WIN-1: Preflight Windows, build nativo e inventario de payload

- Requirement: `REQ-WIN-001`
- Base SHA: `4eafcd7fdb90eed6d7ab44219189e80d82a4abe9`
- Status: `IMPLEMENTING`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: `win1-author` / `win1-reviewer` (independent)

## Scope

- Objective: establish reproducible Windows machine/source/toolchain evidence;
  build the NSIS installer natively; inspect its checksum-pinned sidecar and
  ONNX Runtime payload state; classify failures without changing product code.
- Owned paths: evidence under `tasks/active/REQ-WIN-001/` after selection;
  temporary native build artifacts on the designated Windows machine.
- Allowed paths: `packaging/windows/`, `config/components.json`, Tauri bundle
  configuration, distribution docs, relevant scripts/tests, strictly for
  inspection or a separately assigned fix.
- Prohibited paths: product/runtime code absent a reproduced failure and new
  bounded rework contract; protected authorities; unrelated local files.
- Non-goals: Linux cross-build, silent payload substitution, or a claim of
  runtime success based solely on build success.

## Architecture impact

- Affected contracts: `AC-013` (inspection only).
- Required ADR/change control: ADR-0013/ADR-0016 constraints apply; no change
  without a separately approved failure fix.

## Acceptance and verification

- Acceptance criteria: record native environment/version/source/provenance;
  execute `packaging/windows/build.ps1`; capture artifact hash/size; compare
  installed/bundle payload to manifest; state exactly whether Windows ONNX
  runtime is present and load-testable.
- Focused commands: native PowerShell build; relevant manifest/distribution
  contract tests; selected native Rust/frontend checks; `git diff --check`.
- General gate: `CI=true ./scripts/verify` in its supported Linux environment
  after any integrated source change.

## Handoff

- Implementation result: pending.
- Verification evidence: pending.
- Reviewer verdict: pending.
- Rework history: none.
