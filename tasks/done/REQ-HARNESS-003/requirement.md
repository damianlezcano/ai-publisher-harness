# REQ-HARNESS-003: Herdr worker launch despite unmeasurable orchestrator budget

- Status: `DONE`
- Baseline SHA: `3d85a3f` (`REQ-HARNESS-002` closed on `main`)
- Human gate: `NO_HUMAN` — owner approved closure on 2026-09-21 (no additional product/runtime observation required).
- Author / implementation role: Harness Architect/Implementer in the integration checkout
- Reviewer requirement: independent from this author. Independent Reviewer `PASS` was obtained; owner directed closure from that PASS. This Orchestrator did not self-review.

## Objective

Restore real Worker/Reviewer delegation through `scripts/agent-launch --launch`
inside Herdr when the Orchestrator session has no measurable OpenCode token
budget, without weakening fail-closed rotation or inventing a substitute
session's tokens.

## Observed failure

During the REQ-DOCS-001 smoke, Worker cheap resolved to `low` / `opencode` but:

```text
check-session-budget exit 4   (SESSION_BUDGET: UNKNOWN)
→ scripts/agent-launch --launch exit 12
→ DELEGATION_UNAVAILABLE
```

The Orchestrator was Cursor inside Herdr (`HERDR_ENV=1`). It is not an
identified OpenCode session. `check-session-budget` correctly refuses to pick a
latest/first OpenCode session. `agent-launch` then treated that valid UNKNOWN as
a launch prohibition.

## Non-goals

- Do not execute or modify `REQ-DOCS-001` or `tasks/active/REQ-DOCS-001/`.
- Do not alter the REQ-DOCS-001 author worktree.
- No product/runtime change under `app/` or `crates/`.
- Do not read or modify `opencode.json`, `a.zip`, or the two live A/B example files.
- No commit, push, reset, clean, or restore.
- Do not select a latest/first OpenCode session as the orchestrator budget.
- Do not bypass `scripts/agent-launch` with raw `herdr agent start`.

## Owned / allowed paths

- `scripts/agent-launch`, `scripts/check-session-budget`
- `scripts/test-session-budget`, `scripts/test-agent-launch`,
  `scripts/test-agent-launch-budget` (new)
- `scripts/verify`
- `docs/AGENT_POLICY.md`
- `docs/MULTI_AGENT_WORKFLOW.md` (pointer only if needed)
- `prompts/orchestrator.md` (pointer only if needed)
- `tasks/done/REQ-HARNESS-003/` (closed; was selected under `tasks/active/`)

## Prohibited paths

- `app/`, `crates/`, Architecture Contracts/ADRs, product authorities
- `tasks/active/REQ-DOCS-001/`, the REQ-DOCS-001 worktree
- `config/agent-models.env` model values
- `opencode.json` and the other untracked out-of-scope files named above

## Architecture Contracts affected

- `None` (Harness process only).

## Acceptance criteria

1. Root cause is recorded: UNKNOWN budget on a non-OpenCode Orchestrator is
   telemetry absence, not an over-budget OpenCode session.
2. Fail-closed remains for measured OpenCode orchestrators in the rotate bands
   (exit 2 / 3) and for an identified OpenCode session that cannot be read.
3. Cross-provider / latest-session token fallback remains forbidden.
4. Inside Herdr, `scripts/agent-launch --role low --provider opencode --launch`
   may start a Worker when the orchestrator budget is UNKNOWN for Codex, Cursor,
   Herdr, or other non-OpenCode identity.
5. Tests reproduce the previous exit-12 failure mode against the old decision
   and prove the new decision proceeds (without requiring a live model UI in
   the unit test). Rotate bands still refuse launch.
6. `CI=true ./scripts/verify` and `git diff --check` pass.
7. Independent Reviewer `PASS` and the declared human gate are recorded before Done.

## Verification commands

```bash
./scripts/test-session-budget
./scripts/test-agent-launch
./scripts/test-agent-launch-budget
./scripts/architecture-verify
CI=true ./scripts/verify
git diff --check
```
