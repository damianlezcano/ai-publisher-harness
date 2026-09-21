# AC-001: Ordinary Chat isolation

- Status: `ACTIVE`
- Invariant: An `OrdinaryChat` turn performs no Knowledge preparation, retrieval, query embedding, evidence serialization, or fresh Knowledge session.
- Rationale: Availability of a project index must not silently change an ordinary conversation.
- Required observables: even with indexed material, a classifier-selected ordinary turn uses the conversational session, has no Knowledge context, and performs no retrieval or query embedding.
- Prohibited observables: Knowledge preparation, Knowledge route/retrieval mode, fresh Knowledge session, serialized Knowledge evidence, or inherited material body on an ordinary turn.
- Canonical test cases: `crates/project-app/src/app.rs::{knowledge_plus_ordinary_chat_classifier_can_return_ordinary_chat, ordinary_chat_uses_conversational_session_without_knowledge_context, post_attachment_knowledge_then_ordinary_chat_does_not_carry_materials}`; each runs with `cargo test --locked -p project-app --lib <selector>`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: turn routing, Knowledge preparation, session selection, `project-agent` and `project-app` routing tests.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-001`; update observables, test mapping, gate and ADR before approval.
