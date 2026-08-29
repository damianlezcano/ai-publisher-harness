# Multi-Agent Workflow

Use multiple agents only when tasks are independent enough to have separate
file ownership and checkout boundaries. The lead selects the cheapest reliable
pool using `docs/AGENT_POLICY.md`; no provider, model, or Antigravity is
mandatory for a task. OpenCode Go must use only its approved non-GPT/non-Grok
models.

## Roles

| Role | Best fit | Typical work |
| --- | --- | --- |
| Lead/integrator | Codex Tierra | Decomposition, boundary decisions, integration, final verification, high-impact debugging |
| Cheap implementation | Cursor Composer 2.5 / OpenCode Go MiMo | LOW code, tests, docs, boilerplate |
| Normal implementation/review | OpenCode Go DeepSeek V4 Flash | MEDIUM Rust, filesystem, adapters, tests, review |
| Complex coding | Cursor Grok 4.6 standard / OpenCode Go Kimi fallback | MEDIUM_HIGH/HIGH_CODING |
| Optional worker | Antigravity CLI / AGY | Only when available and clearly useful |
| Reviewer | Different agent than author | Architecture, security, tests, and acceptance-criteria review |

Assignments follow the task, not a rigid tool preference. The lead records the
reasoning class and selected pool, and ensures author and reviewer differ,
preferably across model/provider families, whenever practical.

## Herdr delegation

Before controlling Herdr, confirm `HERDR_ENV=1` and inspect the installed
commands. Resolve the worker through `scripts/agent-launch --dry-run` first;
only its `--launch` path may start a Cursor/OpenCode worker, because it injects
and verifies the exact model ID. Keep the lead in the current pane. For a bounded task, create a
sibling pane with `--current`, the current working directory, and `--no-focus`;
then start the selected recognized agent in that explicit pane. Use the agent's
unique name for prompts, waits, reads, and lifecycle checks.

Create the task worktree before prompting the author and direct that agent to
its assigned checkout. A prompt must include only the task-local context:
milestone, non-goals, exact file ownership, supplied contract, acceptance
criteria, verification commands, security invariants, and a request for a
brief commit-SHA handoff. It must also state:

```
DO NOT REDESIGN THE TASK.
DO NOT EXPLORE UNRELATED ALTERNATIVES.
IMPLEMENT THE CONTRACT PROVIDED.
IF THE CONTRACT IS AMBIGUOUS, STOP AND ASK THE ORCHESTRATOR.
```

Never rely on UI focus, never send an agent to the integration checkout to
edit, and inspect `blocked` state before responding to approvals or questions.

After the author commits, a different agent receives the commit/diff in its own
review checkout and returns only actionable findings and a decision. The lead
owns integration and runs `./scripts/verify` after every integration batch.

## Example task sequence

1. Lead classifies the task and creates `m2/publisher-route-guard` worktree,
   assigning only publisher files to the cheapest reliable author (for example
   Composer for LOW or DeepSeek for MEDIUM).
2. Lead assigns a separate worktree/diff review to a different pool,
   with `docs/SECURITY.md` invariants 1–7 and 9–11 as explicit review criteria.
3. Author resolves material findings in its worktree and reports the amended
   SHA. Reviewer rechecks the material changes.
4. Lead integrates the reviewed commit, runs the verification contract, and
   records the outcome.
