# AC-004: Single route binding

- Status: `ACTIVE`
- Invariant: Each accepted turn binds one route and one corresponding provider-work plan.
- Rationale: a stable binding prevents duplicated sends and divergent handling.
- Required observables: one normalized route is resolved/applied before dispatch; follow-up binding has that route as its sole owner; preparation follows the bound intent.
- Prohibited observables: an unbound dispatch, duplicate or competing follow-up binding, prompt re-interpretation, or late route replacement.
- Canonical test cases: `crates/project-app/src/app.rs::{dispatch_message_run_binds_and_applies_a_missing_route_before_dispatch, followup_binding_has_a_single_authoritative_owner_on_the_route, retrieval_preparation_follows_the_normalized_route_not_prompt_wording}`; each runs with `cargo test --locked -p project-app --lib <selector>`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: bound route handoff, agent service orchestration, provider-call accounting.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-004`; update observables, test mapping, gate and ADR before approval.
