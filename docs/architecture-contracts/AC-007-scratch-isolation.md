# AC-007: Scratch isolation

- Status: `ACTIVE`
- Invariant: classifier, PerItem, and K6 worker activity uses disposable scratch responsibility, never the visible conversational session.
- Rationale: internal planning must not become user-visible continuity.
- Required observables: classifier work opens one separate scratch session, while the final answer remains on its own responsibility; the scratch prompt contains no document body. PerItem and K6 production seams each open their own bounded OpenCode session, issue no conversational provider call, and keep the staged document body out of persisted visible messages.
- Prohibited observables: scratch session reuse as the visible conversational session, scratch transcript contamination of visible messages, document bodies exposed to the classifier, or PerItem/K6 internal evidence forwarded into the visible conversation.
- Canonical test cases: `crates/project-app/src/app.rs::classifier_session_is_separate_and_receives_no_document_bodies`; `crates/project-app/src/app.rs::compact_per_item_production_seam_one_ready_material`; `crates/project-app/src/app.rs::k6_selected_per_source_production_seam_one_ready_material`. Focused commands: `cargo test --locked -p project-app --lib classifier_session_is_separate_and_receives_no_document_bodies`, `cargo test --locked -p project-app --lib compact_per_item_production_seam_one_ready_material`, and `cargo test --locked -p project-app --lib k6_selected_per_source_production_seam_one_ready_material`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: scratch workers, K6/PerItem routing, session responsibility.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-007`; update observables, test mapping, gate and ADR before approval.
