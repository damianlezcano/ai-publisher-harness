# Definition of Done

Copy this checklist into each implementation task, issue, or pull request.
A task is complete only when every applicable item is checked, with command
output or a short rationale linked in the handoff.

## Scope

- [ ] The milestone and task acceptance criteria are written and satisfied.
- [ ] The changed files belong only to the task's dedicated worktree.
- [ ] No unrelated refactor, dependency upgrade, or product behavior is mixed in.

## Product and architecture

- [ ] The change agrees with `CODEX_HANDOFF.md` and the ordered source of truth.
- [ ] Dependency direction remains UI -> Core -> Ports -> Adapters.
- [ ] User-facing text uses the vocabulary in `docs/UX.md`; internal mechanics
      remain hidden by default.

## Quality and security

- [ ] Relevant unit, integration, and end-to-end checks were added or updated.
- [ ] Formatting, lint, type, build, and relevant tests pass.
- [ ] `./scripts/verify` passes.
- [ ] Each affected invariant in `docs/SECURITY.md` is either unaffected or has
      a named test. Security-sensitive changes have an independent review.

## Review and handoff

- [ ] Documentation and ADRs are updated when behavior, architecture, or a
      durable tradeoff changes.
- [ ] The author and reviewer are different agents whenever practical.
- [ ] Review findings are resolved or explicitly accepted by the human owner.
- [ ] The handoff records changed files, verification commands/results,
      reviewer, risks, and follow-up work.
