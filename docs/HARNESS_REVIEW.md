# M0 Harness Review

This review records the initial harness gaps and the M0 resolution. It does not
authorize product implementation.

## Resolved gaps

- `scripts/verify` was a message-only placeholder; it now enforces the M0
  repository contract and is the extension point for later milestone gates.
- The Definition of Done lacked ownership, evidence, handoff, and explicit
  security-review requirements; these are now mandatory checklist items.
- Test expectations were implicit; `docs/TESTING.md` defines levels, fixtures,
  deterministic execution, and the publication security regression matrix.
- Worktree and author/reviewer boundaries were principles rather than an
  operational workflow; `docs/WORKTREES.md` and
  `docs/MULTI_AGENT_WORKFLOW.md` make them executable.
- Project-local agent workflows were placeholders; the four minimal skills now
  describe repeatable implementation, review, architecture review, and UI
  verification handoffs.
- ADR naming existed but lifecycle and trigger criteria did not; the ADR
  convention now specifies both.

## Deliberately deferred

No product language, framework, package manager, test runner, desktop shell,
or external sidecar has been selected or installed in M0. Those choices belong
to a later ADR or the relevant milestone and must update `docs/VERIFY.md` when
they introduce a toolchain.

## M0 acceptance evidence

Run `./scripts/bootstrap` and `./scripts/verify`. Both must pass without
network access or product dependencies.
