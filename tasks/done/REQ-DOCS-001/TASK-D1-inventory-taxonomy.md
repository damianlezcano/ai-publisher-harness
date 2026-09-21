# TASK-D1: Documentation inventory and migration ledger

- Requirement: `REQ-DOCS-001`
- Base SHA: `5664e4495891c495ef9e5b3f40bd7b828c40e312`
- Status: `DONE`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: Orchestrator fallback author in worktree `docs-d1-inventory` path (Worker `low`/`opencode` launch failed: `DELEGATION_UNAVAILABLE` / `agent-launch` exit 12, `check-session-budget` exit 4 on this Cursor session) / independent Reviewer `review`/`opencode` (must not be this Orchestrator)
- Author worktree: `../ai-publisher-req-docs-001-d1` on branch `req-docs-001/d1-inventory`

## Scope

- Objective: create a complete pre-move migration ledger from the requirement matrix, verify content classifications against the selected-base tree, and identify every inbound reference before any physical documentation move.
- Owned paths: `tasks/active/REQ-DOCS-001/migration-ledger.md`, `tasks/active/REQ-DOCS-001/reference-inventory-before.md`.
- Allowed paths: read-only inspection of tracked repository text except the prohibited paths; no production edit.
- Prohibited paths: all existing `docs/`, root entry points, `app/`, `crates/`, `opencode.json`, `a.zip`, and the two protected example files named in the requirement.
- Non-goals: do not move, rename, merge, delete, or update a link; do not classify M11 as committed work.
- Dependencies: none. This task must finish and be integrated before D2–D5 begin.

## Architecture impact

- Affected contracts: `None`; classification must nevertheless flag every protected architecture/security/ADR reference for D5.
- Required ADR/change control: `None`.

## Acceptance and verification

- Acceptance criteria:
  - Ledger covers every row of the requirement matrix and every file under the two UX evidence trees.
  - Each row records source, destination/action, category, preservation assertion, inbound references, owner task, and whether a script/gate consumes it.
  - The before inventory captures Markdown links, literal paths in scripts, entry points, tasks, prompts, skills, Architecture Contract documents/manifest, and ADR references.
  - Any classification contradiction is reported as `BLOCKED` for the Orchestrator; it is not resolved by an unapproved redesign.
- Focused commands:
  - `git ls-files docs prompts skills scripts tasks README.md AGENTS.md CODEX_HANDOFF.md START_CODEX.txt RUNTIME.md`
  - repository-wide `rg` inventories for every path scheduled to move
  - `git diff --check`
- General gate: not required for this no-product, active-task planning artifact; D6 runs the full gate after integration.

## Handoff

- Implementation result: `STATUS: IMPLEMENTATION_COMPLETE`. Created `migration-ledger.md` (343 rows; every matrix path + 184 ux-redesign-01 files + 64 ux-release-gate-01 files; 0 uncovered `docs/` paths; no DELETE) and `reference-inventory-before.md`. Zero documentation moves in D1.
- Orchestrator refinements (recorded, not a taxonomy redesign):
  - D5 may perform a comment-only path repair in `crates/project-app/tests/runtime_gate.rs` (`docs/TESTING.md` rustdoc citation).
  - `scripts/verify` must follow the HARNESS_REVIEW historical destination after D2.
- Verification evidence:
  - `git ls-files docs` — 318 tracked docs files
  - literal path scan of tracked text — see `reference-inventory-before.md` (0 unresolved relative Markdown targets)
  - `git diff --check` — clean for these artifacts
  - `git diff --name-status` vs HEAD — only untracked `tasks/active/REQ-DOCS-001/` (no `docs/` edits)
- Reviewer verdict: not recorded (Reviewer launch blocked by the same session-budget UNKNOWN gate).
- Rework history: none.
