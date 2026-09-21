# Harness Runtime

Roles are stable; providers, models, and launch mechanisms are operational
configuration. The current executable mapping is `config/agent-models.env` and
the governing routing/cost policy is `docs/engineering/AGENT_POLICY.md`.

## Routing

- Orchestrator: selects requirement, decomposes, integrates, runs gates, and
  manages handoff/checkpoint state.
- Worker: receives one bounded task contract through the approved launch and
  worktree process.
- Reviewer: is independent from the Worker and reviews the exact diff/evidence.

Harness role classes are resolved by `scripts/agent-class-resolve`. That helper
prints `LAUNCH_ROLE` and `LAUNCH_PROVIDER`, and for Worker `cheap` also prints
`FALLBACK_LAUNCH_ROLE` and `FALLBACK_LAUNCH_PROVIDER`. Concrete CLI model IDs
live in `config/agent-models.env` and are applied solely by
`scripts/agent-launch`.

| Class | Default tier | Primary `agent-launch` role / provider | Last-resort fallback |
| --- | --- | --- | --- |
| Orchestrator | `strong` | `medium` / `opencode` | — |
| Worker | `cheap` | `low` / `opencode` | `low` / `cursor` |
| Reviewer | `independent` | `review` / `opencode` | — |

Worker `cheap` must resolve OpenCode Go first. Cursor is only the last-resort
fallback for that tier (`FALLBACK_LAUNCH_*`), never the primary cheap provider.
Other worker tiers (`coding`, `fallback`, `visual`) and reviewer tiers
(`escalation`, `architecture`) follow `docs/engineering/AGENT_POLICY.md`. Never put a model
ID in a user prompt, requirement, or role prompt.

Use `scripts/agent-launch` where the policy requires it. Verify
`MODEL_REQUESTED == MODEL_ACTUAL` before sending a worker product work. Do not
make architecture or product behavior depend on a particular provider/model.

## Human gates

- `NO_HUMAN`: deterministic evidence and independent review are sufficient.
- `TARGETED_HUMAN`: a focused product, runtime, accessibility, or release
  scenario must be observed by a human.
- `RELEASE_HUMAN`: release-level distribution/product approval is required.

The applicable gate is declared by the requirement; it is additive to tests and
review. See `docs/engineering/TESTING.md`, `docs/engineering/VERIFY.md`, and the relevant release
documents.
