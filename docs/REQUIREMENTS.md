# Requirement Lifecycle

## States

| State | Location | Meaning |
| --- | --- | --- |
| Backlog | `tasks/backlog/` | Approved to consider, not started |
| Active | `tasks/active/<REQ-ID>/` | A selected requirement with bounded task contracts |
| Done | `tasks/done/<REQ-ID>/` | Closed with outcome and evidence |
| Future | `tasks/future/` | Idea or dependency, not a commitment |

## Requirement contract

Every new requirement states its ID, objective, expected observable behavior,
non-goals, affected architecture contracts, acceptance criteria, verification,
and human gate. It describes *what* is needed, not implementation file lists.

The Orchestrator alone selects it, moves it to Active, and creates task
contracts. No requirement becomes Done from an agent narrative: it needs the
declared gates, independent review PASS, and any required human result.

## Task contract

Use `tasks/TASK_CONTRACT_TEMPLATE.md`. Each task records requirement ID, base
SHA, bounded scope, owned/allowed/prohibited paths, contract impact, acceptance
criteria, commands, human gate, and handoff state. Workers receive only this
task-local context plus the relevant requirement excerpt and contracts.

## Closure and rework

The required loop is:

```text
IMPLEMENT → VERIFY → REVIEW → REWORK → VERIFY → REVIEW → PASS
```

`IMPLEMENTATION_COMPLETE` is a Worker handoff, not closure. A reviewer returns
only `PASS` or `REWORK`; it does not silently patch author work. Historical
requirements are summarized only when Git, docs, tests, or checkpoints provide
the evidence; missing evidence is recorded as unknown rather than invented.
