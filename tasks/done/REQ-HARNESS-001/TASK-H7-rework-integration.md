# TASK-H7: Rework integration and evidence

- Requirement: `REQ-HARNESS-001`; Base SHA: `015d466`; Status: `PASS`; Human gate: `NO_HUMAN`.
- Author / reviewer: current Orchestrator rework / new independent reviewer required.
- Objective: correct review findings, run required gates and record actual evidence for requirement closure.
- Owned paths: all Harness paths listed by the requirement plus the narrowly allowed test-only assertions, fakes, and helpers in `crates/project-app/src/app.rs` and `crates/project-app/src/intent.rs`; excluding product paths and pre-existing local untracked files.
- Allowed paths: task evidence; Architecture Contract manifest/documents and gate only as needed for selector mapping; minimal deterministic tests/assertions or test fakes/helpers under `crates/` only when necessary to verify an Architecture Contract. Prohibited paths: `app/`, runtime/product behavior under `crates/`, `opencode.json`, `a.zip`, and the two pre-existing example files.
- Affected contracts: traceability only for `AC-001` through `AC-014`; ADR/change control: no invariant change authorized.
- Acceptance: all review findings addressed; the only authorized `crates/` changes are test fakes/helpers/assertions in `crates/project-app/src/app.rs` for Architecture Contract verification and the test-only AC-003 low-confidence exclusive-fallback counter/assertion in `crates/project-app/src/intent.rs`; no runtime/productive behavior or functional architecture change is authorized, and commands pass.
- Commands: `./scripts/architecture-verify`; `CI=true ./scripts/verify`; `git diff --check`; required Git visibility commands.
- Handoff: final command outputs are recorded in `evidence.md`; final independent review of REQ-HARNESS-001 returned `PASS` and human approval for closure was granted.
