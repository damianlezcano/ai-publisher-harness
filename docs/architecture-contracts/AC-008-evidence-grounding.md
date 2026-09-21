# AC-008: Evidence grounding

- Status: `ACTIVE`
- Invariant: Knowledge answers identify supporting selected sources and do not claim support from unselected material.
- Rationale: grounding is the observable trust boundary for Knowledge answers.
- Required observables: multi-source semantic answers preserve identifiable source provenance.
- Prohibited observables: unsupported citations, absent sources, or forwarding the raw corpus as evidence.
- Canonical test cases: `crates/project-app/tests/exhaustive_rag.rs::b_multi_source_semantic_keeps_identifiable_sources`; focused command: `cargo test --locked -p project-app --test exhaustive_rag -- b_multi_source_semantic_keeps_identifiable_sources`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: evidence package, citation/source rendering, retrieval selection.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-008`; update observables, test mapping, gate and ADR before approval.
