# Harness Runtime

Roles are stable; providers, models, and launch mechanisms are operational
configuration. The current executable mapping is `config/agent-models.env` and
the governing routing/cost policy is `docs/AGENT_POLICY.md`.

## Routing

- Orchestrator: selects requirement, decomposes, integrates, runs gates, and
  manages handoff/checkpoint state.
- Worker: receives one bounded task contract through the approved launch and
  worktree process.
- Reviewer: is independent from the Worker and reviews the exact diff/evidence.

Use `scripts/agent-launch` where the policy requires it. Verify
`MODEL_REQUESTED == MODEL_ACTUAL` before sending a worker product work. Do not
make architecture or product behavior depend on a particular provider/model.

## Human gates

- `NO_HUMAN`: deterministic evidence and independent review are sufficient.
- `TARGETED_HUMAN`: a focused product, runtime, accessibility, or release
  scenario must be observed by a human.
- `RELEASE_HUMAN`: release-level distribution/product approval is required.

The applicable gate is declared by the requirement; it is additive to tests and
review. See `docs/TESTING.md`, `docs/VERIFY.md`, and the relevant release
documents.
