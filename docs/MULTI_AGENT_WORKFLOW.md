# Multi-Agent Workflow

Use multiple agents only when tasks are independent enough to have separate
file ownership and checkout boundaries. The lead may choose any suitable
combination of Codex, Cursor Agent, OpenCode, and Antigravity CLI; none is
mandatory for a task. The role and retry policy is normative in
`docs/AGENT_POLICY.md`.

## Roles

| Role | Best fit | Typical work |
| --- | --- | --- |
| Lead/integrator | Codex Tierra | Decomposition, boundary decisions, integration, final verification, high-impact debugging |
| Preferred builder | Cursor Agent + Grok | Closed implementation tasks, bounded refactors, tests |
| Independent author, tester, or reviewer | Antigravity CLI / AGY Flash | Low-risk implementation, research, security tests, independent review |
| Other agents | As justified | Only when their capability is materially useful |
| Reviewer | Different agent than author | Architecture, security, tests, and acceptance-criteria review |

Assignments follow the task, not tool preference. The lead records why a role
was selected and ensures author and reviewer differ whenever practical.

## Herdr delegation

Before controlling Herdr, confirm `HERDR_ENV=1` and inspect the installed
commands. Keep the lead in the current pane. For a bounded task, create a
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

1. Lead creates `m2/publisher-route-guard` worktree and assigns only publisher
   files to a Cursor/Grok author.
2. Lead assigns a separate worktree/diff review to Antigravity CLI,
   with `docs/SECURITY.md` invariants 1–7 and 9–11 as explicit review criteria.
3. Author resolves material findings in its worktree and reports the amended
   SHA. Reviewer rechecks the material changes.
4. Lead integrates the reviewed commit, runs the verification contract, and
   records the outcome.
