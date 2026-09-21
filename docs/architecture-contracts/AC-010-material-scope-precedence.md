# AC-010: Material scope precedence

- Status: `ACTIVE`
- Invariant: current-turn explicit material, compatible referent, and active scope resolve in that precedence; unrelated historical material is excluded.
- Rationale: a user-selected scope must not grow silently.
- Required observables: current-turn attachments win over an older MaterialSet; absent attachments, a compatible MaterialSet wins; absent both, the conversation-active material set is used.
- Prohibited observables: implicit inclusion of unrelated historical material or a lower-precedence scope replacing a higher-precedence one.
- Canonical test cases: `crates/project-app/src/app.rs::{per_source_fresh_attachments_win_over_prior_material_set, per_source_reuses_compatible_prior_material_set, per_source_spanish_and_french_bind_the_active_import_set_identically}`; each runs with `cargo test --locked -p project-app --lib <selector>`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: active materials, follow-up referents, retrieval scope resolution.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-010`; update observables, test mapping, gate and ADR before approval.
