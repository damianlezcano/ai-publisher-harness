# TASK-D2: Canonical and historical documentation reorganization

- Requirement: `REQ-DOCS-001`
- Base SHA: `5664e44` (Orchestrator updates to the integrated D1 base when activated)
- Status: `DONE`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: Orchestrator fallback in worktree `../ai-publisher-req-docs-001-d1` (`DELEGATION_UNAVAILABLE` on Worker launch) / independent Reviewer `review`/`opencode`

## Scope

- Objective: apply the approved migration ledger to establish the target `docs/` taxonomy, preserving content and adding only the required documentation indexes/pointers.
- Owned paths: `docs/` excluding `docs/architecture-contracts/` and `docs/decisions/` content; specifically the current files/directories listed in the requirement matrix and new `docs/README.md`, `docs/history/README.md`, `docs/checkpoints/README.md`.
- Allowed paths: `scripts/verify` only when its required-document/evidence paths must follow an approved move; active requirement handoff files.
- Prohibited paths: `app/`, `crates/`, `config/`, `packaging/`, `sidecars/`, root entry points, `prompts/`, `skills/`, `opencode.json`, `a.zip`, protected examples, and semantic edits to Architecture Contracts/ADRs/security/product policy.
- Non-goals: no content deletion, no redesign, no task-lifecycle relocation, no link repair outside owned docs/scripts (D5 owns repository-wide repair).
- Dependencies: D1 migration ledger must be integrated and accepted by the Orchestrator. D2 precedes D3–D5 because it establishes final documentation paths.

## Architecture impact

- Affected contracts: governance paths for `AC-001` … `AC-014`; invariant text and manifest semantics must not change.
- Required ADR/change control: `None` for path-only/document-index changes. Stop if a protected invariant requires wording/semantic change.

## Acceptance and verification

- Acceptance criteria:
  - The target taxonomy and every destination exactly follow D1’s approved ledger.
  - `docs/history/milestones/` retains M1–M10 and an index; no milestone design enters `tasks/done/`.
  - UX report/evidence trees and historical reviews/audits move intact, including binary evidence and harness scripts.
  - `CURRENT_CHECKPOINT.md` is concise/current and historical dated material is preserved in `docs/checkpoints/` with an index; no checkpoint content is silently lost.
  - No source is deleted except through a content-preserving Git move recorded in the ledger.
  - Indexes clearly distinguish canonical/current, history/evidence, and checkpoints.
- Focused commands:
  - `git diff --name-status`
  - `git diff --check`
  - `git diff --find-renames --summary`
  - relevant `scripts/verify` document-path assertions, if changed
- General gate: D6 runs `CI=true ./scripts/verify` after D3–D5 integration.

## Handoff

- Implementation result: `STATUS: IMPLEMENTATION_COMPLETE`. Applied D1 ledger: current docs under `docs/product|architecture|engineering|distribution`; history under `docs/history/{milestones,reviews,ux}`; checkpoints split into `docs/checkpoints/`; added `docs/README.md`, `docs/history/README.md`, `docs/checkpoints/README.md`. No DELETE.
- Verification evidence: `git diff --find-renames --summary` in the author worktree; `git diff --check` pass.
- Reviewer verdict: not recorded (independent Reviewer pending).
- Rework history: none.
