# TASK-D5: Repository reference-integrity repair

- Requirement: `REQ-DOCS-001`
- Base SHA: `5664e44` (Orchestrator updates after D2–D4 integration)
- Status: `IN_REVIEW`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: Orchestrator fallback in worktree `../ai-publisher-req-docs-001-d1` / independent Reviewer `review`/`opencode`

## Scope

- Objective: repair and prove all path/reference consumers of the approved taxonomy moves, using D1’s before inventory and a post-move report.
- Owned paths: textual references throughout tracked repository documentation, root entry points, `prompts/`, `skills/`, `tasks/`, and `scripts/` that refer to moved documentation paths; `tasks/active/REQ-DOCS-001/reference-integrity-report.md`.
- Allowed paths: `docs/architecture-contracts/README.md` and `manifest.tsv` only for path/reference repairs; no contract-semantic change.
- Prohibited paths: `app/`, `crates/`, runtime behavior, `config/agent-models.env`, `opencode.json`, `a.zip`, protected examples, binary historical evidence, and any content change unrelated to a moved/renamed path.
- Non-goals: no taxonomy redesign, no edits to historical findings, no deletion.
- Dependencies: D1 before-reference inventory plus integrated D2, D3, and D4 changes. This task precedes D6 and is the final path-repair authority.

## Architecture impact

- Affected contracts: documentation references to `AC-001` … `AC-014` only; their observables, selectors, and manifest meaning remain unchanged.
- Required ADR/change control: `None` unless a reference repair exposes a real invariant inconsistency.

## Acceptance and verification

- Acceptance criteria:
  - Before/after report accounts for all old paths from D1 and identifies intentional historical textual mentions separately from broken links.
  - Markdown links and relative assets resolve; script literal paths, `rg` references, README, AGENTS, CODEX_HANDOFF, START_CODEX, RUNTIME, tasks, prompts, skills, Architecture Contracts/manifest, and ADR references are repaired.
  - `scripts/verify` and `scripts/architecture-verify` use only final intended paths and continue to enforce the same semantic checks.
  - No old moved path remains in executable path checks; no unresolved Markdown target remains in tracked Markdown.
- Focused commands:
  - pre/post `rg -n` path inventories from D1
  - a deterministic local Markdown-link/path checker documented in the report
  - `bash -n scripts/verify scripts/architecture-verify`
  - `./scripts/architecture-verify`
  - `git diff --check`
- General gate: `CI=true ./scripts/verify` after all D2–D5 changes are integrated.

## Handoff

- Implementation result: `STATUS: IMPLEMENTATION_COMPLETE`. Repository-wide path repair plus comment-only `crates/project-app/tests/runtime_gate.rs` TESTING.md citation. Report: `reference-integrity-report.md` (0 unresolved Markdown targets).
- Verification evidence: see `reference-integrity-report.md`; `./scripts/architecture-verify` and `CI=true ./scripts/verify` passed in the worktree before the `main` merge.
- Reviewer verdict: not recorded (independent Reviewer pending).
- Rework history: none.
