# REQ-DOCS-001 reference integrity report (after moves)

- Base SHA: `5664e4495891c495ef9e5b3f40bd7b828c40e312`
- Worktree: `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-req-docs-001-d1` branch `req-docs-001/d1-inventory`
- Checker: local relative Markdown link resolution (`Path.resolve` from the linking file) + literal old-path substring scan of tracked/untracked text
- Markdown/text relative links checked: 136
- Unresolved relative targets: 0
- Remaining old moved-path literals outside the requirement contract/ledger/inventory: 0

## Unresolved Markdown targets

_none_

## Remaining old moved-path literals (excluding REQ-DOCS-001 contract/ledger/inventory)

_none_

## Intentional old-path prose

- `tasks/active/REQ-DOCS-001/requirement.md`, `migration-ledger.md`, `reference-inventory-before.md`, and `TASK-D*.md` record pre-move sources and destinations.
- `scripts/verify` and `scripts/architecture-verify` use only final paths (`docs/product/`, `docs/architecture/`, `docs/engineering/`, `docs/history/...`).
- Comment-only crate repair: `crates/project-app/tests/runtime_gate.rs` now cites `docs/engineering/TESTING.md`.

## Gate evidence (worktree)

- `git diff --check` — pass
- `cargo fmt --all -- --check` — pass
- `./scripts/architecture-verify` — PASS (14 contracts; mapped tests executed)
- `CI=true ./scripts/verify` — pass (exit 0), after a local `components/` symlink to the integration checkout’s gitignored sidecar blobs (not part of the documentation change)
