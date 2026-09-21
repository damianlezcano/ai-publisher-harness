# AC-005: Ephemeral Knowledge evidence

- Status: `ACTIVE`
- Invariant: Serialized Knowledge evidence uses a fresh ephemeral session and never replaces the conversational cache.
- Rationale: retrieved corpus evidence must not leak into later ordinary chat.
- Required observables: Knowledge synthesis opens a fresh session while ordinary chat may reuse its conversational session.
- Prohibited observables: reuse of a Knowledge-evidence session as the conversational session.
- Canonical test cases: `crates/project-agent/tests/agent_service.rs::knowledge_synthesis_opens_ephemeral_sessions_ordinary_chat_reuses_conversation`; focused command: `cargo test --locked -p project-agent --test agent_service -- knowledge_synthesis_opens_ephemeral_sessions_ordinary_chat_reuses_conversation`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: session frontier, evidence serialization, cancellation/session restoration.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-005`; update observables, test mapping, gate and ADR before approval.
