# Active Agent Cost, Reliability, and Token Policy

This is the active harness policy. Agent/model selection is an execution
decision, never an architectural or milestone dependency. A worker may be
replaced without changing approved contracts. Big Pickle is retained only as a
historical failure example; it is not a current dependency.

## Principle

Use the cheapest reliable model that can satisfy the task. Prefer
specialization: Kimi K2.7 Code writes code, Qwen3.8 Flash reviews, DeepSeek V4
Flash orchestrates, and Composer/MiMo handle simple visual/mechanical work. A
MAX / PRO model is never used when a FLASH / CODE / LOW model can satisfy the
task. Codex Tierra is a scarce orchestration resource and is not the default
builder.

## Model matrix (owner directive 2026-08-31, supersedes earlier matrices)

| Role | Model (CLI id) | Notes |
| --- | --- | --- |
| Orchestrator / integration / checkpoints | OpenCode Go DeepSeek V4 Flash (`opencode-go/deepseek-v4-flash`) | Decomposes, coordinates, handoffs, integrates, manages checkpoints. Does NOT implement normal coding tasks itself. |
| Coding — normal / complex | OpenCode Go Kimi K2.7 Code (`opencode-go/kimi-k2.7-code`) | Primary coding worker. Used aggressively for Rust, TypeScript/React, state management, migrations, adapters, significant tests, refactors. |
| Independent review — default | OpenCode Go Qwen3.8 Flash (`opencode-go/qwen3.8-flash`) | Code/contract/regression/security/implementation/frontend review. The default Qwen reviewer. |
| Independent review — escalation | OpenCode Go Qwen3.8 Max (`opencode-go/qwen3.8-max`) | ESCALATION-ONLY. Launched only when the orchestrator records an explicit `ESCALATION_REASON` (security-critical cross-module finding, subtle concurrency issue, schema/data-loss risk, repeated author/reviewer disagreement, or a difficult architectural invariant). |
| LOW / visual / CSS / copy | Cursor Composer 2.5 (`composer-2.5`); fallback OpenCode Go MiMo V2.5 (`opencode-go/mimo-v2.5`) | Simple test, boilerplate, docs, repetitive corrections, CSS/copy. |
| HIGH_CODING fallback | Cursor Grok 4.6 medium (`cursor-grok-4.6-medium`) | Only after Kimi K2.7 Code fails twice on the SAME bounded task. Never used through OpenCode Go. |
| HIGH_ARCHITECTURE | fresh OpenCode Go DeepSeek V4 Pro (`opencode-go/deepseek-v4-pro`) | ESCALATION-ONLY. Fresh session for genuine architecture/security decisions; closed after the design is persisted. Never used for orchestration, routine review, coding, tests, integration, or housekeeping. |

## Review policy

- AUTHOR != REVIEWER always. Prefer cross-family review: Kimi author →
  Qwen3.8 Flash reviewer; Composer author → Qwen3.8 Flash or DeepSeek Flash
  reviewer; DeepSeek Flash author (only when unavoidable) → Qwen3.8 Flash
  reviewer.
- Qwen3.8 Max is escalation-only. If no explicit escalation reason exists, do
  NOT launch Qwen3.8 Max; prefer Qwen3.8 Flash.

## Worker session lifecycle (disposable execution contexts)

Repository + commits + `docs/CURRENT_CHECKPOINT.md` are the durable memory;
agent sessions are disposable and must not carry state between tasks.

- Every bounded implementation task: ONE TASK → ONE FRESH AUTHOR SESSION.
- Every independent review: ONE REVIEW → ONE FRESH REVIEWER SESSION.
- A worker/reviewer session MUST NOT be reused for a different task.
- An author session may remain alive ONLY for: the same bounded task, immediate
  test/fix iteration, or REQUEST_CHANGES from the reviewer for that same task.
- A reviewer session may remain alive ONLY for: the initial review, or re-review
  of fixes for that SAME task.
- After PASS + APPROVE + handoff captured: close the author session and the
  reviewer session immediately. A done/idle pane with no pending same-task
  re-review is CONTEXT_LEAK and must be closed.
- Per task, before closing any worker/reviewer session: capture task ID,
  requested/actual model, owned scope, commit SHA, tests/checks, findings, and
  reviewer verdict in the handoff/PR. Do not retain conversational context for
  audit; evidence lives in Git/docs.

## Model verification

Every worker must verify before receiving the full task that
`MODEL_REQUESTED == MODEL_ACTUAL`. On mismatch, stop that worker and do not give
it the task.

## Failure policy

For a recoverable failure on the SAME task, one retry in the same author session
is allowed. Second failure: close that author session and switch model/provider
per the matrix (e.g., Kimi → Cursor Grok 4.6 medium). Do not carry a failed
session into another task.

## Orchestrator context budget

- Around ~80K: avoid unnecessary repository exploration; summarize active state
  into `CURRENT_CHECKPOINT.md`.
- Around ~100K: launch no new tasks; finish active bounded work; complete
  reviews; integrate; close all completed panes; update `CURRENT_CHECKPOINT.md`;
  rotate the orchestrator.
- At >= 130K: ROTATE_SESSION as soon as the active task reaches a safe
  checkpoint. Never let an orchestration session reach 200K-400K.

## Executable model enforcement

`config/agent-models.env` is the canonical mapping from reasoning role and
provider to the exact CLI model ID. `scripts/agent-launch` is mandatory for
every future Cursor or OpenCode worker; direct generic `herdr agent start` is
forbidden for harness work. It discovers the configured ID from the provider
CLI before launch, prints TASK/REASONING_LEVEL/AGENT_PROVIDER/MODEL_REQUESTED/
MODEL_ACTUAL/WORKTREE, and fails closed if a mapping is empty or unavailable.

For a Herdr worker it passes the native `--model <id>` during `agent start`.
The worker prompt is sent only after this pre-launch verification. The launcher
does not rely on provider defaults, prior sessions, prompt prose, or interactive
selection. Cursor's canonical standard Grok ID is `cursor-grok-4.6-medium`;
`cursor-grok-4.6-high` is not a default or an automatic fallback.

`--check-config` is offline and is the deterministic `scripts/verify` gate.
`--dry-run` performs live CLI availability discovery and is required as the
pre-launch check; `--launch` repeats it immediately before Herdr starts a pane.
Its pre-launch `MODEL_ACTUAL` is intentionally `UNVERIFIED`: an available ID
does not prove the running agent accepted it. For `--launch`, the launcher waits
for the provider UI to report the exact configured active-model display name;
only then does it print `MODEL_ACTUAL` equal to `MODEL_REQUESTED` and return
success. The orchestrator may send a product task only after that success. A
mismatch or an unavailable ID fails closed; the worker is terminated or
reconfigured and only the documented fallback may be attempted.

The approved OpenCode Go IDs are `opencode-go/mimo-v2.5`,
`opencode-go/deepseek-v4-flash`, `opencode-go/qwen3.8-flash`,
`opencode-go/qwen3.8-max`, `opencode-go/kimi-k2.7-code`, and
`opencode-go/deepseek-v4-pro`. The V4 Pro ID is used only for fresh
HIGH_ARCHITECTURE escalation sessions and must be pinned in
`config/agent-models.env` before its first `--launch`. The high-review role
(`opencode-go/qwen3.8-max`) is escalation-only. `opencode/mimo-v2.5-free` is not
equivalent to the OpenCode Go MiMo ID and is never an automatic substitute. Big
Pickle, GPT, and Grok via OpenCode Go, provider defaults, and last-used
OpenCode models are prohibited.

## Task contracts and worker behavior

Each worker receives only the objective, allowed files/modules, forbidden scope,
relevant contract and invariants, Definition of Done, and verification
commands. The lead retains global project context.

Every implementation prompt includes:

```
DO NOT REDESIGN THE TASK.
DO NOT EXPLORE UNRELATED ALTERNATIVES.
IMPLEMENT THE PROVIDED CONTRACT.
DO NOT MODIFY FILES OUTSIDE YOUR OWNERSHIP.
IF THE CONTRACT IS AMBIGUOUS: STOP AND ASK THE ORCHESTRATOR.
RUN THE REQUIRED CHECKS BEFORE HANDOFF.
RETURN A SHORT REPORT.
```

Required handoff format:

```
STATUS: PASS / BLOCKED / FAIL
CHANGES:
- ...
TESTS:
- command — result
FINDINGS:
- ... (if any)
COMMIT:
- SHA
```

## Token efficiency

1. Small context over full-repository context.
2. Contract-first delegation over exploratory workers.
3. Cheap model first for clear tasks.
4. Escalate capability only when necessary.
5. Do not use multiple models for one task unless review, failure, or uncertainty justifies it.
6. Reuse a healthy session for small fixes within the SAME task.
7. Switch quickly from unreliable workers (FAIL TWICE → SWITCH AGENT).
8. Treat Codex Tierra as a scarce orchestration resource.

## Session, checkpoint, and rotation policy

- The repository is the durable memory and source of truth; conversation history is not carried forward between sessions.
- `docs/CURRENT_CHECKPOINT.md` is the current handoff for orchestration sessions and is rewritten per phase, never accumulated as historical documentation.
- Implementation/maintenance orchestration runs on OpenCode Go DeepSeek V4 Flash (`opencode-go/deepseek-v4-flash`).
- A genuine HIGH_ARCHITECTURE decision is escalated to a fresh DeepSeek V4 Pro session; the Pro session is closed after the architecture decision/design is persisted to the repository.
- Do not keep historical milestone chats or prior sessions alive for the next milestone; start from repository state.
- Rotate orchestration sessions around 100K-150K context: prepare checkpoint/rotation above ~100K, rotate above ~150K.
- Workers receive task-local context only (see "Task contracts and worker behavior"); the orchestration lead retains global project context.

## Platform

The active platform policy remains Fedora 44 x86_64 for development and the
initial Linux MVP. Preserve portability; do not add Windows-specific work yet.