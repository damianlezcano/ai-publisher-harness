# AC-012: Provider-call bound

- Status: `ACTIVE`
- Invariant: provider answer/worker calls are bounded by the resolved route and its serialized budget, never one call per corpus document by default.
- Rationale: bounded work preserves cost, latency and predictable behavior.
- Required observables: exhaustive negative work does not emit per-document remote calls.
- Prohibited observables: unbounded per-document provider calls for a single turn.
- Canonical test cases: `crates/project-app/tests/exhaustive_rag.rs::j_exhaustive_negative_does_not_issue_per_document_remote_calls`; focused command: `cargo test --locked -p project-app --test exhaustive_rag -- j_exhaustive_negative_does_not_issue_per_document_remote_calls`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: provider-call budget, aggregate/K6 planning, route dispatch.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-012`; update observables, test mapping, gate and ADR before approval.
