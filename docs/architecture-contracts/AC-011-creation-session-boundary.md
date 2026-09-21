# AC-011: Creation session boundary

- Status: `ACTIVE`
- Invariant: Creation that serializes Knowledge evidence invalidates the cached conversational session before a later ordinary turn.
- Rationale: creation tool work may use evidence, but that evidence cannot taint later chat continuity.
- Required observables: evidence-bearing creation rotates the later ordinary-chat session.
- Prohibited observables: reuse of a conversational session tainted by serialized Knowledge evidence.
- Canonical test cases: `crates/project-agent/tests/agent_service.rs::creation_serialized_evidence_rotates_cached_session_before_ordinary_chat`; focused command: `cargo test --locked -p project-agent --test agent_service -- creation_serialized_evidence_rotates_cached_session_before_ordinary_chat`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: Creation dispatch, conversational cache invalidation, session IDs.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-011`; update observables, test mapping, gate and ADR before approval.
