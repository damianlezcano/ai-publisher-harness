# REQ-DOCS-001 evidence

- Requirement status: `DONE`.
- Planning baseline SHA: `5664e4495891c495ef9e5b3f40bd7b828c40e312`
- Integrated review SHA: `d9603b399ab8471a274c2bf5da2feb3e88dc58b9` on `req-docs-001/d1-inventory`
- Independent Reviewer: `req-docs-001-reviewer` (`review` / `opencode`). First verdict `REWORK` at `c50e189`; same-task re-review `PASS` at `d9603b3`.
- Human gate: `TARGETED_HUMAN` — owner approved the documentation map and entry-point wording on 2026-09-21.
- Lifecycle close: Orchestrator moved `tasks/active/REQ-DOCS-001/` to `tasks/done/REQ-DOCS-001/` after Reviewer `PASS` and that human approval. No commit or push.

## Delegation

- `HERDR_ENV=1`.
- Worker `cheap`: implementation of D1–D5 ran as Orchestrator fallback in the author worktree after an earlier `agent-launch` failure. Direct `herdr agent start` was not used.
- Reviewer: `./scripts/agent-class-resolve --class reviewer` → `review` / `opencode`. Independent from this Orchestrator.

## Concurrent checkout

- Lead `main` remains `0585c4e` (`origin/main`). Taxonomy implementation lives on `req-docs-001/d1-inventory` and is not merged in this close.

## Commands (author worktree after merge)

```text
git diff --check                      # pass
cargo fmt --all -- --check            # pass
./scripts/architecture-verify         # PASS, 14 contracts
CI=true ./scripts/verify              # pass (exit 0); local components/ symlink to lead gitignored sidecars
```

Integrated commits: `ebc2079` (taxonomy) then `4f53811` (merge `origin/main`). Post-review fix `d9603b3`. Uncommitted at close: `CODEX_HANDOFF.md` HEAD wording (015d466 is protected baseline, not current `main` HEAD) plus this lifecycle move.

## Artifacts

- `migration-ledger.md` — 343 rows; no DELETE; 0 uncovered docs paths
- `reference-inventory-before.md`
- `reference-integrity-report.md`
- Taxonomy: `docs/product|architecture|engineering|distribution|history|checkpoints` plus kept `architecture-contracts/` and `decisions/`
