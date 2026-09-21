# REQ-HARNESS-003 evidence

## Formal closure

- Requirement status: `DONE`. Baseline: `3d85a3f`.
- Independent review: `PASS` — obtained before closure; owner directed this
  lifecycle close from that PASS. This Orchestrator did not self-review.
- Human gate: `NO_HUMAN` — approved for closure by the owner on 2026-09-21.
- No commit, push, reset, or restore.
- `REQ-DOCS-001` was not executed or modified. `tasks/active/REQ-DOCS-001/` and
  worktree `ai-publisher-req-docs-001-d1` were left intact.

## Root cause

`scripts/check-session-budget` correctly reports `SESSION_BUDGET: UNKNOWN`
(exit 4) when the Orchestrator is not an identified OpenCode session (this
smoke: Cursor inside Herdr, `HERDR_ENV=1`, no `OPENCODE_SESSION_ID`). It must
not measure a latest/first OpenCode session.

`scripts/agent-launch --launch` previously mapped every budget exit other than
0/1/2/3 — including that valid UNKNOWN — to launcher exit 12. The REQ-DOCS-001
Worker-cheap smoke therefore failed closed as `DELEGATION_UNAVAILABLE` even
though the Orchestrator was not over budget.

UNKNOWN on Codex/Cursor/Herdr is telemetry absence, not a measured rotate band.

## Correction (fail-closed preserved)

`--launch` now:

- warns and proceeds on budget exit 4 when `PLATFORM` is `CODEX`, `CURSOR`,
  `HERDR`, or `UNKNOWN`;
- still exits 10 / 11 on measured OpenCode rotate / hard-rotate;
- still exits 12 when an *identified* OpenCode session is unreadable
  (`PLATFORM: OPENCODE` + exit 4).

No latest-session token fallback. No raw `herdr agent start` bypass.

## Tests

- `scripts/test-session-budget`: Herdr without OpenCode identity →
  `PLATFORM: HERDR`, `SESSION_BUDGET: UNKNOWN`, no session id from the list.
- `scripts/test-agent-launch-budget` (new): encodes the legacy table
  (budget 4 → launcher 12); asserts HERDR/CURSOR/CODEX/UNKNOWN proceed;
  OpenCode unreadable still 12; rotate 2/3 still 10/11; `herdr` is not
  invoked on refuse.

## Live Herdr confirmation (cheap OpenCode Worker)

Inside the implementation session (`HERDR_ENV=1`, lead checkout, not the docs
worktree):

1. `./scripts/check-session-budget` → exit 4, `PLATFORM: HERDR`
2. Sibling pane `herdr pane split --current --direction right --cwd "$PWD" --no-focus` → `w25:p2`
3. `./scripts/agent-launch --role low --provider opencode --worktree <lead> --pane w25:p2 --name harness003w --launch`
   - warning: `UNKNOWN (PLATFORM: HERDR); launching worker`
   - `herdr agent start` argv: `opencode --model opencode-go/mimo-v2.5`
   - `MODEL_REQUESTED: opencode-go/mimo-v2.5`
   - `MODEL_ACTUAL: opencode-go/mimo-v2.5`
   - launcher exit 0
4. No Worker prompt was sent (smoke only). Pane `w25:p2` closed.

## How to re-verify in Herdr

From a Herdr pane (`HERDR_ENV=1`) in the lead checkout:

```bash
test "$HERDR_ENV" = 1
./scripts/check-session-budget; echo exit:$?
# expect UNKNOWN exit 4 for Cursor/Herdr without OPENCODE_SESSION_ID

pane_json=$(herdr pane split --current --direction right --cwd "$PWD" --no-focus)
pane_id=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["pane"]["pane_id"])' <<<"$pane_json")

./scripts/agent-launch --role low --provider opencode \
  --worktree "$PWD" --pane "$pane_id" --name harness003-verify --launch
# expect MODEL_REQUESTED == MODEL_ACTUAL == opencode-go/mimo-v2.5

herdr agent list
herdr pane close "$pane_id"   # after smoke; do not send product work
```

Do not use the REQ-DOCS-001 worktree for this smoke.

## Gate results (implementation, 2026-09-21)

- `./scripts/test-session-budget` — PASS
- `./scripts/test-agent-launch` — PASS
- `./scripts/test-agent-launch-budget` — PASS
- `./scripts/architecture-verify` — PASS (14 contracts; mapped tests executed)
- `CI=true ./scripts/verify` — PASS (`verify: M10 contract passed`)
- `git diff --check` — PASS (included in verify)

## Gate results (closure, 2026-09-21)

- `./scripts/test-session-budget` — PASS
- `./scripts/test-agent-launch` — PASS (via `CI=true ./scripts/verify`)
- `./scripts/test-agent-launch-budget` — PASS
- `./scripts/architecture-verify` — PASS (`14 contracts; mapped tests executed`)
- `CI=true ./scripts/verify` — PASS (exit 0; `verify: M10 contract passed`)
- `git diff --check` — PASS (exit 0)
- `./scripts/requirement-status REQ-HARNESS-003` — `STATE: DONE` / `ACTION: REJECT`
- `./scripts/requirement-status REQ-DOCS-001` — `STATE: ACTIVE` (untouched by this closure; worktree `ai-publisher-req-docs-001-d1` preserved)
