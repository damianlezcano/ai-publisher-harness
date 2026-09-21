# REQ-DOCS-001 evidence

- Requirement status: `IN_REVIEW` (not Done). Human gate: `TARGETED_HUMAN`.
- Planning baseline SHA: `5664e4495891c495ef9e5b3f40bd7b828c40e312`
- Integrated review SHA: `d9603b399ab8471a274c2bf5da2feb3e88dc58b9` on `req-docs-001/d1-inventory`
- Independent Reviewer: `req-docs-001-reviewer` (`review` / `opencode`). First verdict `REWORK` at `c50e189`; same-task re-review `PASS` at `d9603b3`.
- This Orchestrator does **not** claim `DONE` or architectural approval. Requirement stays Active until targeted human approval.

## Delegation

- `HERDR_ENV=1`.
- Worker `cheap`: implementation of D1–D5 ran as Orchestrator fallback in the author worktree after an earlier `agent-launch` failure. Direct `herdr agent start` was not used.
- Reviewer: `./scripts/agent-class-resolve --class reviewer` → `review` / `opencode`. Launch is D6 of this continuation; the Reviewer must not be this Orchestrator.

## Concurrent checkout

- Lead `main` is `0585c4e` (`origin/main`). REQ-HARNESS-002/003 are already on `main`. The docs branch merged that history and retargeted paths. Taxonomy changes remain on `req-docs-001/d1-inventory` until Reviewer `PASS` plus targeted human approval.

## Commands (author worktree after merge)

```text
git diff --check                      # pass
cargo fmt --all -- --check            # pass
./scripts/architecture-verify         # PASS, 14 contracts
CI=true ./scripts/verify              # pass (exit 0); local components/ symlink to lead gitignored sidecars
```

Integrated commits: `ebc2079` (taxonomy) then `4f53811` (merge `origin/main`).

## Artifacts

- `migration-ledger.md` — 343 rows; no DELETE; 0 uncovered docs paths
- `reference-inventory-before.md`
- `reference-integrity-report.md` — 0 unresolved Markdown targets after D5 (pre-merge); merge repaired remaining harness-script/prompt paths
- Taxonomy: `docs/product|architecture|engineering|distribution|history|checkpoints` plus kept `architecture-contracts/` and `decisions/`

## Reviewer package

Give an independent Reviewer `prompts/reviewer.md`, this requirement, all TASK-D* contracts, the ledger, both inventories/reports, exact diff `0585c4e...4f53811` (or `git log --oneline 0585c4e..HEAD` plus `git diff --find-renames 0585c4e`), and the gate output above. Return only `PASS` or `REWORK`. Do not patch.
