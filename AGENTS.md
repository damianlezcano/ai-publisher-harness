# Agent Operating Rules

## Source of truth order
1. `CODEX_HANDOFF.md`
2. `docs/PRODUCT.md`
3. `docs/ARCHITECTURE.md`
4. `docs/SECURITY.md`
5. `docs/UX.md`
6. ADRs under `docs/decisions/`

If implementation conflicts with these documents, stop and resolve the discrepancy instead of silently changing product behavior.

## Working model
- Prefer scoped tasks with explicit acceptance criteria.
- Prefer one worktree per independent implementation task.
- Do not have multiple agents edit the same files concurrently.
- Author and reviewer should differ when practical.
- Avoid speculative abstractions beyond the next milestone.
- Update docs when architectural or product behavior changes.

## Mandatory completion checks
Before claiming a task is complete:
- run formatting
- run lint/type checks
- run relevant tests
- run integration checks where applicable
- run `./scripts/verify` once implemented
- ensure no security invariant regressed

## User-facing philosophy
The user is non-technical. Internal technical power must not leak into default UX.
