# TASK-H3: Historical state and backlog classification

- Requirement: `REQ-HARNESS-001`; Base SHA: `015d466`; Status: `PASS`; Human gate: `NO_HUMAN`.
- Author / reviewer: original author identity not evidenced / new independent reviewer required.
- Objective: distinguish evidenced historical baseline work from uncommitted future ideas.
- Owned paths: `tasks/done/`, `tasks/backlog/`, `tasks/future/`, `docs/engineering/REQUIREMENTS.md`.
- Allowed paths: `tasks/active/REQ-HARNESS-001/`; Prohibited paths: product code and M11 implementation.
- Affected contracts: `None`; ADR/change control: `None`.
- Acceptance: M11 is future until separately approved; no historical approval is invented.
- Commands: `find tasks -maxdepth 3 -type f | sort`; `git status --short`.
- Handoff: existing M11 text explicitly required separate approval; rework moved it to `future/`. Final independent review of REQ-HARNESS-001 returned `PASS`.
