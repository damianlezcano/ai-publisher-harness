# Agent Operating Rules

## Entry point and authority

Read `CODEX_HANDOFF.md`, then these sources in order:

1. `docs/product/PRODUCT.md`
2. `docs/architecture/ARCHITECTURE.md`
3. `docs/product/SECURITY.md`
4. `docs/product/UX.md`
5. ADRs in `docs/decisions/`

If code, a task, or a request conflicts with this order, stop and resolve the
conflict. `docs/engineering/HARNESS_ENGINEERING.md` is the canonical methodology; detailed operating
policy lives in `docs/engineering/AGENT_POLICY.md`, `docs/engineering/MULTI_AGENT_WORKFLOW.md`, and
`docs/engineering/WORKTREES.md`.

## Roles and context

| Role | Read | May decide | Must not do |
| --- | --- | --- | --- |
| Orchestrator | authority sources, `RUNTIME.md`, requirement/backlog, task and affected contracts | scope, task decomposition, routing, closure after gates | bypass a contract, reviewer, or required human gate |
| Worker | this file, `prompts/worker.md`, assigned task, requirement excerpt, affected contracts, owned code | implementation inside task scope | change architecture, claim PASS/DONE/approval, edit outside ownership |
| Reviewer | this file, `prompts/reviewer.md`, requirement/task, affected contracts, exact base/diff and evidence | PASS or REWORK | author the fix or review their own work |

Prompts define the persistent role contracts. The Orchestrator **execution**
procedure is only `prompts/orchestrator.md`. `RUNTIME.md` and
`config/agent-models.env` describe changeable execution routing; no permanent
prompt may hardcode a provider or model.

## Short-intent assignments

An utterance that names a requirement (for example `Implementar REQ-XXXX.`) is a
complete Orchestrator assignment. Do not wait for a restated playbook. Execute
`prompts/orchestrator.md`.

## Protected architecture

Protected paths include `docs/architecture/ARCHITECTURE.md`,
`docs/product/SECURITY.md`, ADRs in `docs/decisions/`,
`docs/architecture-contracts/`, and their gates/tests. Protected behavior
includes publication isolation and the conversation/Knowledge invariants named
in `CODEX_HANDOFF.md`.

If ordinary work needs to violate a protected contract, stop and report:

```text
ARCHITECTURE_CHANGE_REQUIRED: AC-XXX
```

It requires an ADR, affected IDs, updated observables/tests and architecture
gate, independent review, and explicit human approval before integration.

## Requirement and task lifecycle

Requirements enter `tasks/backlog/`. The Orchestrator moves a selected
requirement to `tasks/active/<REQ-ID>/`, creates bounded task contracts there,
and moves it to `tasks/done/<REQ-ID>/` only after verification, independent
review, and its declared human gate. Non-committed ideas stay in `tasks/future/`.
See `docs/engineering/REQUIREMENTS.md` and `tasks/TASK_CONTRACT_TEMPLATE.md`.
Locate state with `scripts/requirement-status` before acting.

Before editing, state milestone/requirement, exact owned paths, acceptance
criteria, verification commands, and planned author/reviewer. One implementation
task has one author checkout; reviewers inspect a separate read-only checkout
or exact diff and do not silently fix it.

## Required evidence

Run formatting, lint/type checks, relevant tests, applicable integration and
security checks, then `CI=true ./scripts/verify`. Record commands and results
in the task handoff. A Worker reports `IMPLEMENTATION_COMPLETE`; only the
Orchestrator may close a requirement after an independent `PASS` and any human
gate.

## Safety

Do not reset, clean, restore, rebase, delete, stage, or overwrite unrelated
work. Do not access credentials, tokens, account files, or private agent
configuration. Do not remove tests to make a migration pass. Product behavior
must not be redesigned to accommodate Harness structure.
