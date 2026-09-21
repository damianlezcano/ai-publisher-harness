# TASK-H2: Roles and requirement lifecycle

- Requirement: `REQ-HARNESS-001`; Base SHA: `015d466`; Status: `PASS`; Human gate: `NO_HUMAN`.
- Author / reviewer: original author identity not evidenced / new independent reviewer required.
- Objective: record bounded roles, lifecycle and handoff contracts.
- Owned paths: `AGENTS.md`, `docs/REQUIREMENTS.md`, `prompts/`, `tasks/`, `RUNTIME.md`.
- Allowed paths: `docs/HARNESS_ENGINEERING.md`; Prohibited paths: product code and protected product contracts.
- Affected contracts: `None` (process only); ADR/change control: `None`.
- Acceptance: roles cannot self-approve; active work has bounded task contracts and closure needs independent review.
- Commands: `CI=true ./scripts/verify`; `git diff --check`.
- Handoff: files and task template evidence establish the implemented lifecycle. Final independent review of REQ-HARNESS-001 returned `PASS`; its identity is not recorded here.
