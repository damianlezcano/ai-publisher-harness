# Orchestrator Contract

This file is the **single execution authority** for the Orchestrator role.
`AGENTS.md` remains the superior operating-rule table (roles, protected
architecture, safety). Do not copy this procedure into `START_CODEX.txt`,
`CODEX_HANDOFF.md`, user prompts, or requirements.

The user prompt expresses intent. This contract supplies process, limits, and
evidence. Runtime model IDs are never chosen here; they come from `RUNTIME.md`
and `config/agent-models.env` through `scripts/agent-class-resolve` and
`scripts/agent-launch`.

## Short intent

Treat any of the following as a **complete** assignment:

- `Implementar REQ-XXXX.`
- `Implement REQ-XXXX.`
- `Execute REQ-XXXX.`

Do not ask the user to restate lifecycle, Herdr, Worker cost, gates, review, or
routing. If the utterance names a `REQ-ID`, run this procedure immediately.

## Load

1. Authority order in `AGENTS.md` / `CODEX_HANDOFF.md` (product first).
2. This contract.
3. `docs/REQUIREMENTS.md` and the selected requirement after it is located.
4. `RUNTIME.md` only as routing input.

Keep global context here. Workers receive task-local context only.

## Procedure

1. **Resolve REQ-ID** from the utterance. If missing or malformed, stop.
2. **Locate lifecycle:** `./scripts/requirement-status <REQ-ID>`. Honor `ACTION`:
   - `REJECT` / `FUTURE` / `DONE` / `UNKNOWN` / `AMBIGUOUS`: stop; do not execute.
   - `MOVE_TO_ACTIVE`: move `tasks/backlog/<REQ-ID>/` to `tasks/active/<REQ-ID>/`
     (create the directory form if the backlog item is a single file). Then
     create or confirm bounded task contracts from
     `tasks/TASK_CONTRACT_TEMPLATE.md`.
   - `CONTINUE`: use the existing active contracts; do not restart a closed
     requirement.
3. **Load** `requirement.md` and every `TASK-*.md`. Apply declared task
   dependencies; do not start a task whose dependencies are unmet.
4. **Declare** milestone/requirement, owned paths, acceptance criteria,
   verification commands, human gate, author, and independent reviewer.
5. **Route** each ready task with `scripts/agent-class-resolve` (see Routing).
   Default Worker tier is `cheap`. Escalate only as `docs/AGENT_POLICY.md`
   requires. Do not hardcode a model ID.
6. **Delegate** bounded implementation (see Delegation). The Orchestrator does
   not implement a Worker-owned task in the lead pane when a Worker can be
   launched.
7. **Integrate** Worker handoffs. Run the task-local commands, then
   `./scripts/architecture-verify` when architecture/docs contracts require it,
   then `CI=true ./scripts/verify`. Failure is `REWORK`, not closure.
8. **Independent review:** launch a Reviewer that is not the author and not this
   Orchestrator. The Orchestrator **must not self-review**. Give the Reviewer
   `prompts/reviewer.md`, the requirement, task contracts, affected ACs, exact
   base SHA/diff, and verification evidence.
9. **REWORK:** keep the requirement Active; return the same task to a fresh or
   same-task author session; re-verify; re-review.
10. **Stop at the human gate.** After implementation and the required
    independent Reviewer pass path, set task/requirement status to `IN_REVIEW`
    until that Reviewer returns `PASS` and the declared human gate is satisfied.
    Do not move the requirement to `tasks/done/` and do not declare `DONE`,
    `PASS`, or architectural approval from this role alone.
11. **Pane hygiene (when Herdr allows it):** close completed secondary panes;
    keep the Orchestrator pane.

Never bypass protected architecture. Report `ARCHITECTURE_CHANGE_REQUIRED:
AC-XXX` and stop when ordinary work needs an architectural exception.

## Routing

Harness classes (conceptual): Orchestrator → strong; Worker → cheap; Reviewer →
strong independent. Concrete `--role` / `--provider` values come from
`scripts/agent-class-resolve` (Worker `cheap` prefers OpenCode Go; Cursor is
`FALLBACK_LAUNCH_*` last resort only):

```bash
./scripts/agent-class-resolve --class worker --tier cheap
./scripts/agent-launch --role "$LAUNCH_ROLE" --provider "$LAUNCH_PROVIDER" --check-config
```

`--dry-run` is the live pre-launch availability check; `--launch` is the only
approved start path. Require `MODEL_REQUESTED == MODEL_ACTUAL` before sending
product/task work. `high-review` / `high-architecture` need an explicit
escalation reason per `docs/AGENT_POLICY.md`.

## Delegation

Mechanism: `docs/MULTI_AGENT_WORKFLOW.md` + `docs/WORKTREES.md` +
`scripts/agent-launch`. Direct `herdr agent start` without this launcher is
forbidden.

When `HERDR_ENV=1`:

1. Inspect installed `herdr` help; do not guess subcommands.
2. Create the author worktree; keep this checkout lead-owned.
3. Create a sibling pane (current tab, `--no-focus`, do not steal UI focus).
4. `./scripts/agent-launch --role … --provider … --worktree … --pane … --name … --dry-run`
5. The same command with `--launch`. Proceed only when it prints matching
   `MODEL_REQUESTED` and `MODEL_ACTUAL`.
6. Send **minimal** Worker context: `AGENTS.md`, `prompts/worker.md`, the one
   task contract, requirement excerpt, affected contracts, owned/prohibited
   paths, acceptance criteria, verification commands. Do not send this
   Orchestrator procedure or global backlog.
7. After `IMPLEMENTATION_COMPLETE`, integrate, verify, then launch a **different**
   Reviewer the same way (`--class reviewer`).
8. Close completed secondary panes.

When `HERDR_ENV` is not `1`: do not invent another launcher. Record
`DELEGATION_UNAVAILABLE`. Independent review and human-gate stop still apply;
missing Herdr is not permission to self-review or to close the requirement.

To confirm later that secondary Workers actually opened, see **Confirming
delegated workers** in `docs/MULTI_AGENT_WORKFLOW.md`.
