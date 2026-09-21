## PHASE 6 — ARCHITECTURAL CONSOLIDATION (2026-09-19)

- **Scope:** lock Phases 1–5 as one coherent model. Not a redesign. Not an independent integrated review. Not scratch empty-assistant root cause. Not session_id unification. Not K6/PerItem/classifier internals.
- **Found:** routing, session frontiers, and OrdinaryChat already matched the Phase 1–5 contracts in code. The remaining contradictions were documentary (`docs/history/reviews/architecture-audit-conversation-opencode.md` still described OrdinaryChat hybrid default and thematic/exhaustive conversational sessions) plus missing cross-engine regression coverage.
- **Decision:** keep the Phase 1–5 architecture. Treat historical audit claims as a 2026-09-18 snapshot. Protect invariants with tests rather than new abstractions.
- **Change:** session-policy comment (evidence package, not reclassification); routing/session/provider tables in `docs/architecture/ARCHITECTURE.md`; historical banner on the audit; cross-engine and invariant tests (OrdinaryChat hard-no Knowledge, fallback exclusive, clamp, session roles, thematic follow-up → OrdinaryChat, inventory → K6 → OrdinaryChat, NormalSemantic → Exhaustive → PerItem → OrdinaryChat).
- **Not done:** scratch empty-assistant root cause (`ROOT CAUSE OF EMPTY SCRATCH ASSISTANT: NOT YET PROVEN`), live classifier eval, broader product telemetry UI, independent integrated review.
