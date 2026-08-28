# Multi-Agent Workflow

Use multiple agents only when tasks are independent enough to have separate
file ownership and checkout boundaries. The lead may choose any suitable
combination of Codex, Cursor Agent, OpenCode, and Antigravity CLI; none is
mandatory for a task.

## Roles

| Role | Best fit | Typical work |
| --- | --- | --- |
| Lead/integrator | Codex | Decomposition, boundary decisions, integration, final verification |
| UI author | Cursor Agent | Screen implementation, interaction and accessibility checks |
| Independent author or tester | OpenCode | Bounded adapter/core work, tests, focused review |
| Research/analysis | Antigravity CLI | Time-boxed alternatives, threat modeling, documentation analysis |
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
its assigned checkout. A prompt must include: milestone, non-goals, exact file
ownership, acceptance criteria, verification commands, security invariants,
and a request for a commit SHA/handoff. Never rely on UI focus, never send an
agent to the integration checkout to edit, and inspect `blocked` state before
responding to approvals or questions.

After the author commits, a different agent receives the commit/diff in its own
review checkout and returns only actionable findings and a decision. The lead
owns integration and runs `./scripts/verify` after every integration batch.

## Example task sequence

1. Lead creates `m2/publisher-route-guard` worktree and assigns only publisher
   files to an OpenCode author.
2. Lead assigns a separate worktree/diff review to Codex or Antigravity CLI,
   with `docs/SECURITY.md` invariants 1–7 and 9–11 as explicit review criteria.
3. Author resolves material findings in its worktree and reports the amended
   SHA. Reviewer rechecks the material changes.
4. Lead integrates the reviewed commit, runs the verification contract, and
   records the outcome.
