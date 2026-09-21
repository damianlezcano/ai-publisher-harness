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

- Implementation result: merged `origin/main` (`0585c4e`) into the taxonomy branch; resolved path conflicts; gates re-run; independent Reviewer `REWORK` then re-review `PASS` at `d9603b3`.
- Verification evidence: see `evidence.md`.
- Reviewer verdict: `PASS` from independent `review`/`opencode` (`req-docs-001-reviewer`, Qwen3.8 Flash) at `d9603b3`. Human gate `TARGETED_HUMAN` still required.
- Rework history:
  - 2026-09-21: Reviewer `REWORK` — (1) extra blank line at EOF in `docs/checkpoints/README.md`; (2) document REQ-HARNESS-002/003 old-path literals as historical prose.
  - 2026-09-21: Author `d9603b3`; same-task re-review `PASS`.
