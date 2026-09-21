# AC-009: Exhaustive negative truth

- Status: `ACTIVE`
- Invariant: a global negative claim is local and truthful only when coverage is complete.
- Rationale: incomplete material must never be represented as absence from the whole corpus.
- Required observables: incomplete coverage produces no global-absence claim; complete negatives remain local.
- Prohibited observables: exhaustive negative assertion with incomplete coverage.
- Canonical test cases: `crates/project-app/tests/exhaustive_rag.rs::e_exhaustive_incomplete_must_not_claim_global_absence`; focused command: `cargo test --locked -p project-app --test exhaustive_rag -- e_exhaustive_incomplete_must_not_claim_global_absence`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: exhaustive route, coverage accounting, answer composition.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-009`; update observables, test mapping, gate and ADR before approval.
