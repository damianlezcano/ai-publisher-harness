# Active Agent Cost, Reliability, and Token Policy

This is the active harness policy. Agent/model selection is an execution
decision, never an architectural or milestone dependency. A worker may be
replaced without changing approved contracts. Big Pickle is retained only as a
historical failure example; it is not a current dependency.

## Principle

Use the cheapest reliable model that can complete the task. Codex Tierra is a
scarce orchestration resource and is not the default builder.

## Agent pools and roles

| Pool/model | Level | Preferred use |
| --- | --- | --- |
| Codex Tierra | HIGH_ARCHITECTURE | Orchestration, architecture, decomposition, contracts, integration, critical security decisions, conflicts, final gate |
| Cursor Composer 2.5 | LOW; MEDIUM fallback | Primary LOW worker and MEDIUM fallback for specified code, structs, adapters, tests, refactors, docs, boilerplate |
| Cursor Grok 4.6 standard | MEDIUM_HIGH/HIGH_CODING | Complex coding, concurrency, difficult bugs, cross-module integration, security fixes; do not use Fast by default |
| OpenCode Go MiMo-V2.5 | LOW | Simple tests, boilerplate, docs, repetitive corrections |
| OpenCode Go DeepSeek V4 Flash | MEDIUM | Default OpenCode Go implementation/review worker for Rust, tests, filesystem, adapters, and moderate security |
| OpenCode Go Qwen3.8 Max | MEDIUM fallback | Use after DeepSeek failure/degradation or for a deliberate second family opinion |
| OpenCode Go Kimi K2.7 Code | HIGH_CODING fallback | Difficult code after medium workers fail, or when Grok is unavailable/inappropriate |
| Antigravity CLI / AGY | Optional | Use only when available and clearly useful; never required by a workflow because of quota limits |

OpenCode Go must not use GPT or Grok models. Cursor Grok is reserved for
MEDIUM_HIGH/HIGH_CODING work; Composer and DeepSeek should absorb most normal
work. HIGH_ARCHITECTURE remains Codex Tierra.

## Reasoning matrix

| Classification | Preference order |
| --- | --- |
| LOW | Composer 2.5 → MiMo-V2.5 |
| MEDIUM | DeepSeek V4 Flash → Composer 2.5 → Qwen3.8 Max |
| MEDIUM_HIGH | Grok 4.6 standard → DeepSeek V4 Flash → Kimi K2.7 Code |
| HIGH_CODING | Grok 4.6 standard/medium → Kimi K2.7 Code |
| HIGH_ARCHITECTURE | Codex Tierra |

These are preferences, not rigid dependencies. Availability and reliability
may justify switching workers.

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
`opencode-go/deepseek-v4-flash`, `opencode-go/qwen3.8-max`, and
`opencode-go/kimi-k2.7-code`. `opencode/mimo-v2.5-free` is not equivalent to
the OpenCode Go MiMo ID and is never an automatic substitute. Big Pickle, GPT,
Grok, provider defaults, and last-used OpenCode models are prohibited.

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

## Review, sessions, and failure

Author and reviewer must differ whenever practical, preferably across model or
provider families. Codex reviews directly only for architecture, critical
security, integration, cross-module changes, or reviewer conflicts.

Reuse a healthy session for a small finding. Stop and switch when a worker
deviates, repeats an error, or produces unrelated changes. Keep **FAIL TWICE →
SWITCH AGENT**: assess prompt ambiguity, allow one retry, then switch model;
escalate to Codex only when appropriate workers cannot solve the task.

## Token efficiency

1. Small context over full-repository context.
2. Contract-first delegation over exploratory workers.
3. Cheap model first for clear tasks.
4. Escalate capability only when necessary.
5. Do not use multiple models for one task unless review, failure, or uncertainty justifies it.
6. Reuse healthy sessions for small fixes.
7. Switch quickly from unreliable workers.
8. Treat Codex Tierra as a scarce orchestration resource.

## Platform

The active platform policy remains Fedora 44 x86_64 for development and the
initial Linux MVP. Preserve portability; do not add Windows-specific work yet.
