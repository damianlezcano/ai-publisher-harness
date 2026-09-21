# AC-003: Exclusive fallback

- Status: `ACTIVE`
- Invariant: Fallback is a single bounded replacement only after classifier error or low confidence.
- Rationale: combining a trusted semantic decision with fallback creates contradictory routes.
- Required observables: a classifier error or low-confidence result produces one deterministic fallback route and invokes no second classifier. `low_confidence_uses_exclusive_fallback_once` supplies a low-confidence semantic `CorpusThematic` result and proves that the resolved `OrdinaryChat` route has `SemanticFallback { LowConfidence }`, rather than retaining that semantic result.
- Prohibited observables: a semantic result mixed with fallback, a competing route, or repeated classifier invocation on the same turn.
- Canonical test cases: `crates/project-app/src/app.rs::classifier_failure_falls_back_to_deterministic_routing` (error path: one invocation and fallback provenance); `crates/project-app/src/intent.rs::low_confidence_uses_exclusive_fallback_once` (low-confidence path: `CorpusThematic` is replaced by the one `LowConfidence` fallback route). Focused commands: `cargo test --locked -p project-app --lib classifier_failure_falls_back_to_deterministic_routing` and `cargo test --locked -p project-app --lib low_confidence_uses_exclusive_fallback_once`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: classifier fallback, intent resolution, retrieval route tests.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-003`; update observables, test mapping, gate and ADR before approval.
