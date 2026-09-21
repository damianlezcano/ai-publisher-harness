# TASK-D6: Orchestrator integration, verification, and review handoff

- Requirement: `REQ-DOCS-001`
- Base SHA: updated by the Orchestrator to the integrated D1–D5 base
- Status: `IN_REVIEW`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: Orchestrator/integrator on this continuation / independent Reviewer `review`/`opencode` (must not be this Orchestrator)

## Scope

- Objective: integrate only reviewed D1–D5 work, run the declared full gates, assemble factual evidence, request independent review, and present the documentation map for targeted human approval.
- Owned paths: `tasks/active/REQ-DOCS-001/evidence.md`, `tasks/active/REQ-DOCS-001/review-handoff.md`; integration of reviewed commits only.
- Allowed paths: only conflict-free integration/rework paths authorized by the reviewed task contracts.
- Prohibited paths: new functional edits, unreviewed cleanup, product/runtime code, `opencode.json`, `a.zip`, and protected examples.
- Non-goals: the Orchestrator does not author a silent fix for a Worker; rework returns to the bounded author task and receives re-review.
- Dependencies: reviewed/integrated D1, D2, D3, D4, and D5 handoffs; then an independent review and targeted human approval.

## Architecture impact

- Affected contracts: path/reference traceability only for `AC-001` … `AC-014`.
- Required ADR/change control: `None`; stop and escalate if a protected semantic change is discovered.

## Acceptance and verification

- Acceptance criteria:
  - Evidence records actual commands, exit status, exact base/diff, changed paths, migration-ledger reconciliation, reference report, task handoffs, and reviewer identity/class without invented PASS/DONE claims.
  - The independent Reviewer receives requirement, all task contracts, exact diff/base, gate output, migration ledger, and integrity report in a separate review checkout/diff view.
  - Requirement remains ACTIVE through any REWORK and until independent PASS plus targeted human approval. Only then may it move to Done.
- Focused commands:
  - `git status --short`
  - `git diff --check`
  - `cargo fmt --all -- --check`
  - `./scripts/architecture-verify`
  - `CI=true ./scripts/verify`
  - D5 documented Markdown/reference-integrity checker
- General gate: `CI=true ./scripts/verify` is mandatory and must pass.

## Handoff

- Implementation result: integration/evidence/review package only.
- Verification evidence: real terminal results, not projected outcomes.
- Reviewer verdict: exactly `PASS` or `REWORK`, supplied by an independent Reviewer.
- Rework history: chronological factual entries retained until closure.
