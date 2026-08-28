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
- Apply the role, cost, retry, and Herdr delegation policy in
  `docs/AGENT_POLICY.md` and `docs/MULTI_AGENT_WORKFLOW.md`.
- Apply the primary-platform and portability policy in `docs/PLATFORM_POLICY.md`.
- Follow `docs/WORKTREES.md`, `docs/MULTI_AGENT_WORKFLOW.md`, and
  `docs/TESTING.md` for execution details.
- Before editing, state the milestone, exact file ownership, acceptance
  criteria, and the planned author/reviewer. An agent owns a checkout for the
  duration of its task; reviewers inspect a committed diff from another
  checkout and do not edit the author's checkout.
- Treat a security-invariant change as a security-review task, not a routine
  implementation task. Resolve any conflict with the source-of-truth order
  before writing code.

## Mandatory completion checks
Before claiming a task is complete:
- run formatting
- run lint/type checks
- run relevant tests
- run integration checks where applicable
- run `./scripts/verify` once implemented
- ensure no security invariant regressed
- record the commands and their result in the task handoff or pull request

## User-facing philosophy
The user is non-technical. Internal technical power must not leak into default UX.
