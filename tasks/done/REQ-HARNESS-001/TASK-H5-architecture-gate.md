# TASK-H5: Architecture gate

- Requirement: `REQ-HARNESS-001`; Base SHA: `015d466`; Status: `PASS`; Human gate: `NO_HUMAN`.
- Author / reviewer: original author identity not evidenced / new independent reviewer required.
- Objective: execute focused architecture suites and prove AC-to-selector mapping without source hashes.
- Owned paths: `scripts/architecture-verify`, `docs/architecture-contracts/manifest.tsv`.
- Allowed paths: `docs/architecture-contracts/`; Prohibited paths: `app/`, `crates/`, `scripts/verify` except integration wiring.
- Affected contracts: `AC-001` through `AC-014`; ADR/change control: no invariant change authorized.
- Acceptance: manifest validates document structure and existing test functions, then runs deterministic focused suites.
- Commands: `./scripts/architecture-verify`; `bash -n scripts/architecture-verify`.
- Handoff: rework replaces registry-only ID checks with manifest/document/selector validation. Final independent review of REQ-HARNESS-001 returned `PASS`.
