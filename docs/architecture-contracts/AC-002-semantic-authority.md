# AC-002: Semantic authority

- Status: `ACTIVE`
- Invariant: When structural routing cannot resolve intent, one semantic classifier decision is authoritative for that turn.
- Rationale: downstream engines must consume a stable route rather than reinterpret user wording.
- Required observables: an eligible ambiguous turn invokes the semantic classifier once; its valid decision determines the bound intent and downstream retrieval mode.
- Prohibited observables: keyword override, fallback mixed with a successful semantic decision, or downstream reclassification of that decision.
- Canonical test cases: `crates/project-app/src/app.rs::{knowledge_turn_invokes_the_semantic_classifier_once_and_it_controls_intent, trusted_normal_semantic_is_not_reinterpreted_as_exhaustive_or_k6}`; each runs with `cargo test --locked -p project-app --lib <selector>`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: intent classifier, route binding, exhaustive/thematic routing tests.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-002`; update observables, test mapping, gate and ADR before approval.
