# AC-006: Bounded continuity

- Status: `ACTIVE`
- Invariant: visible conversation continuity is durable, same-conversation, bounded, and excludes evidence and internal worker bodies.
- Rationale: continuity is useful without turning a conversation into an unbounded hidden context.
- Required observables: visible continuity contains at most four ordered messages, each UTF-8-safely truncated to 480 characters, with at most 1,400 characters overall; it excludes the current prompt.
- Prohibited observables: Knowledge evidence/raw chunks, classifier/scratch/K6/PerItem bodies, or another conversation's history in visible context.
- Canonical test cases: `crates/project-app/src/conversation_context.rs::{omits_the_current_user_turn_and_keeps_the_prior_pair, keeps_at_most_four_visible_messages_in_order, utf8_truncation_does_not_split_scalar_values, omits_internal_prompts_chunks_and_scratch_bodies}`; each runs with `cargo test --locked -p project-app --lib <selector>`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: message persistence, context assembly, session frontier.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-006`; update observables, test mapping, gate and ADR before approval.
