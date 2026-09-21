# TASK-H1: Entry-point authority reconciliation

- Requirement: `REQ-HARNESS-001`; Base SHA: `015d466`; Status: `PASS`; Human gate: `NO_HUMAN`.
- Author / reviewer: original author identity not evidenced / new independent reviewer required.
- Objective: align entry points on the canonical Harness methodology.
- Owned paths: `README.md`, `START_CODEX.txt`, `CODEX_HANDOFF.md`, `AGENTS.md`, `examples/README.md`.
- Allowed paths: `docs/HARNESS_ENGINEERING.md`; Prohibited paths: product code and authorities.
- Affected contracts: `None` (governance only); ADR/change control: `None`.
- Acceptance: no transitional authority; examples are historical only.
- Commands: `rg -n -i 'until then|when exista|durante su migración|M0-only' README.md START_CODEX.txt CODEX_HANDOFF.md AGENTS.md examples/README.md`; `git diff --check`.
- Handoff: repository evidence shows these entry files were part of the migration; rework corrected the review finding. Final independent review of REQ-HARNESS-001 returned `PASS`.
