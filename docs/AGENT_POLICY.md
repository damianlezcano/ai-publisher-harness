# Agent Cost and Token Policy

This policy makes the harness resilient to a worker/model being unreliable.
Agent choice is an execution concern: replacing a worker must not change an
approved architecture, contract, or milestone. The Big Pickle experience is a
concrete reminder that the workflow must not be coupled to one model.

## Roles

| Agent | Primary role | Do not use by default for |
| --- | --- | --- |
| Codex Tierra | Tech lead: orchestration, architecture, planning, integration, conflict resolution, high-impact debugging, and final review of critical boundaries | Boilerplate, routine tests, simple specified modules, and trivial edits |
| Cursor Agent + Grok | Preferred builder for specified code, tests, bounded refactors, and medium-complexity modules | Redesigning approved architecture or contracts |
| Antigravity CLI / AGY Flash | Low-risk implementation, focused research, security tests, repetitive validation, and independent review | Broad, ambiguous ownership or unapproved architectural decisions |
| Other agents | Only when a specific capability provides clear value | Default use or duplicated work |

Cursor/Grok should use the least costly reasoning configuration that can meet a
closed contract. Antigravity Flash/free is preferred when its capability is
sufficient. Codex Tierra remains the project context holder and is not the
default implementation worker.

## Reasoning classification

| Level | Meaning | Default delegation |
| --- | --- | --- |
| LOW | Boilerplate, simple tests, small fully specified change | Cursor/Grok low reasoning or Antigravity Flash |
| MEDIUM | Bounded module, moderate logic, local integration, known security tests | Cursor/Grok or Antigravity; Codex supervises |
| HIGH | Architecture, critical security, cross-module integration, conflict, complex debugging | Codex Tierra; first split into LOW/MEDIUM tasks when safe |

Before Codex Tierra writes implementation code, it asks whether a clear
contract lets a lower-cost worker complete it safely. If yes, delegate.

## Delegation contract and handoff

Give a worker only: objective, allowed files/modules, relevant constraints and
contracts, Definition of Done, and exact verification commands. Do not flood a
worker with global project context. Every worker prompt includes:

```
DO NOT REDESIGN THE TASK.
DO NOT EXPLORE UNRELATED ALTERNATIVES.
IMPLEMENT THE CONTRACT PROVIDED.
IF THE CONTRACT IS AMBIGUOUS, STOP AND ASK THE ORCHESTRATOR.
```

The handoff is short: changed files, tests and result, blockers/risks, and
commit SHA. The integration checkout is lead-only; each author has one
exclusive worktree. Reviewers use a separate checkout or read-only diff.

## Failure policy

1. Assess whether the prompt or supplied contract was ambiguous; fix it first
   if necessary.
2. Permit at most one retry with the same agent type when the failure appears
   recoverable.
3. On the second failure, switch agent type.
4. Escalate implementation to Codex Tierra only when no reasonable delegated
   alternative remains.

**FAIL TWICE -> SWITCH AGENT.** Do not spend repeated iterations attempting to
make one unreliable worker succeed.

## Review and Herdr sequence

Author and reviewer are different agents whenever reasonable. Preferred
combinations are Cursor/Grok author plus Antigravity reviewer, or Antigravity
author plus Cursor/Grok reviewer. Codex Tierra reviews directly only for
architecture, critical security, cross-module integration, or unresolved
reviewer conflict.

When Herdr adds value: Codex Tierra defines the contract, creates the author
worktree, opens a non-focused pane, delegates, waits for the concise handoff,
runs task checks, delegates independent review, has the author resolve material
findings, integrates on the reserved checkout, and runs the milestone gate.
Never require every available agent for a task.
