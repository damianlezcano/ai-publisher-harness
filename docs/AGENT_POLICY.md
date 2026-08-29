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
| Cursor Composer 2.5 | LOW/MEDIUM | Primary cheap implementation worker: specified code, structs, adapters, tests, refactors, docs, boilerplate |
| Cursor Grok 4.6 standard | MEDIUM_HIGH/HIGH_CODING | Complex coding, concurrency, difficult bugs, cross-module integration, security fixes; do not use Fast by default |
| OpenCode Go MiMo-V2.5 | LOW | Simple tests, boilerplate, docs, repetitive corrections |
| OpenCode Go DeepSeek V4 Flash | MEDIUM | Default OpenCode Go implementation/review worker for Rust, tests, filesystem, adapters, and moderate security |
| OpenCode Go Qwen3.8 Flash | MEDIUM fallback | Use after DeepSeek failure/degradation or for a deliberate second family opinion |
| OpenCode Go Kimi K2.7 Code | HIGH_CODING fallback | Difficult code after medium workers fail, or when Grok is unavailable/inappropriate |
| Antigravity CLI / AGY | Optional | Use only when available and clearly useful; never required by a workflow because of quota limits |

OpenCode Go must not use GPT or Grok models. Cursor Grok is reserved for
MEDIUM_HIGH/HIGH_CODING work; Composer and DeepSeek should absorb most normal
work. HIGH_ARCHITECTURE remains Codex Tierra.

## Reasoning matrix

| Classification | Preference order |
| --- | --- |
| LOW | Composer 2.5 → MiMo-V2.5 |
| MEDIUM | DeepSeek V4 Flash → Composer 2.5 → Qwen3.8 Flash |
| MEDIUM_HIGH | Grok 4.6 standard → DeepSeek V4 Flash → Kimi K2.7 Code |
| HIGH_CODING | Grok 4.6 standard → Kimi K2.7 Code → Codex only if delegation is unreasonable |
| HIGH_ARCHITECTURE | Codex Tierra |

These are preferences, not rigid dependencies. Availability and reliability
may justify switching workers.

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
