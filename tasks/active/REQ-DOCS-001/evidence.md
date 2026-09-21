# REQ-DOCS-001 evidence

- Requirement status: `IN_REVIEW` (not Done). Human gate: `TARGETED_HUMAN`.
- Base SHA: `5664e4495891c495ef9e5b3f40bd7b828c40e312`
- Author worktree: `../ai-publisher-req-docs-001-d1` branch `req-docs-001/d1-inventory`
- This Orchestrator does **not** claim `PASS`, `DONE`, or architectural approval.

## Delegation

- `HERDR_ENV=1`. Worker routing: `./scripts/agent-class-resolve --class worker --tier cheap` → `low` / `opencode`.
- `./scripts/agent-launch --role low --provider opencode --launch` failed: `check-session-budget` exit 4 (`SESSION_BUDGET: UNKNOWN` on this Cursor session) → launcher exit 12.
- Recorded: `DELEGATION_UNAVAILABLE` for Worker and Reviewer panes. Direct `herdr agent start` was not used.
- Implementation therefore ran in the author worktree as Orchestrator fallback. Independent Reviewer must still be a different agent.

## Concurrent checkout

- Lead/integration checkout still holds uncommitted `REQ-HARNESS-002` work. REQ-DOCS-001 taxonomy changes live in the worktree and were **not** merged into the dirty integration tree.

## Commands

```text
git ls-files docs                     # 318 tracked docs files at D1
./scripts/architecture-verify         # PASS, 14 contracts
cargo fmt --all -- --check            # pass
git diff --check                      # pass
CI=true ./scripts/verify              # pass (exit 0) in worktree with components/ symlink for gitignored sidecars
```

## Artifacts

- `migration-ledger.md` — 343 rows; no DELETE; 0 uncovered docs paths
- `reference-inventory-before.md`
- `reference-integrity-report.md` — 0 unresolved Markdown targets after D5
- Taxonomy: `docs/product|architecture|engineering|distribution|history|checkpoints` plus kept `architecture-contracts/` and `decisions/`

## Reviewer package

Give an independent Reviewer (`scripts/agent-class-resolve --class reviewer` → `review` / `opencode`) this requirement, all TASK-D* contracts, the ledger, both inventories/reports, exact diff from `5664e44` on branch `req-docs-001/d1-inventory`, and the gate output above. Return only `PASS` or `REWORK`. Do not patch.
