# TASK-D4: Root README repository map

- Requirement: `REQ-DOCS-001`
- Base SHA: `5664e44` (Orchestrator updates after D2/D3 integration)
- Status: `IN_REVIEW`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: Orchestrator fallback in worktree `../ai-publisher-req-docs-001-d1` / independent Reviewer `review`/`opencode`

## Scope

- Objective: add a compact “Repository structure” section to the root README and update its documentation links for the final taxonomy.
- Owned paths: `README.md`.
- Allowed paths: active requirement handoff files only.
- Prohibited paths: all other root entry points, docs content, `app/`, `crates/`, `opencode.json`, `a.zip`, and protected examples.
- Non-goals: no architectural narrative expansion, no product copy rewrite, no implementation change.
- Dependencies: D1 ledger and D2 taxonomy move must be integrated. D4 may run independently of D3 after D2.

## Architecture impact

- Affected contracts: `None`.
- Required ADR/change control: `None`.

## Acceptance and verification

- Acceptance criteria:
  - README maps `app/`, `crates/`, `docs/`, `tasks/`, `prompts/`, `skills/`, `scripts/`, `config/`, `packaging/`, `sidecars/`, `components/`, `examples/`, and relevant generated/local directories such as `target/`, `.pnpm-store/`, and local logs without treating them as source authority.
  - The map clearly labels PRODUCT, HARNESS, DOCUMENTATION, TASK LIFECYCLE, BUILD / DISTRIBUTION, and GENERATED / LOCAL ARTIFACTS.
  - It remains brief and links to canonical documents rather than duplicating them.
  - All moved documentation links resolve after D5.
- Focused commands:
  - `rg -n 'Repository structure|PRODUCT|HARNESS|DOCUMENTATION|TASK LIFECYCLE|BUILD / DISTRIBUTION|GENERATED / LOCAL ARTIFACTS' README.md`
  - `git diff --check`
- General gate: D6.

## Handoff

- Implementation result: `STATUS: IMPLEMENTATION_COMPLETE`. Root README has a Repository structure map with PRODUCT / HARNESS / DOCUMENTATION / TASK LIFECYCLE / BUILD / DISTRIBUTION / GENERATED / LOCAL ARTIFACTS and updated docs links.
- Verification evidence: `rg` headings in README.md; listed directories exist or are labeled generated/local.
- Reviewer verdict: not recorded (independent Reviewer pending).
- Rework history: none.
