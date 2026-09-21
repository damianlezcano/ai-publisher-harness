## PHASE 4 — OPENCODE SESSION FRONTIERS (2026-09-18)

- **Scope:** session responsibility after Phases 1–3. Not Phase 5. Not scratch empty-completion root cause. Not session_id unification. Not classifier/K6/PerItem internals. Not OrdinaryChat Phase 2 contract.
- **Found:** `NormalSemantic` already used `open_fresh_session` and did not overwrite the conversational cache. Continuity of pronouns such as “eso” did **not** come from OpenCode (EducAI history was not injected; `resolve_followup` does not bind a bare “eso”). `CorpusExhaustive` / `CorpusThematic` LLM synthesis used `open_session`, so serialized evidence could persist on the conversational transcript. A failed ephemeral `send` also dropped the conversational cache because `OpenCodeAgentEngine::send` keyed invalidation by `project_id`.
- **Decision:** keep ephemeral sessions for any Knowledge answer that serializes evidence (`normal` / `exhaustive` / `thematic`). Reconstruct bounded visible EducAI turns into that ephemeral prompt. OrdinaryChat and creation stay on `open_session`. Classifier / K6 / PerItem stay scratch.
- **Change:** `knowledge_uses_ephemeral_session`; bounded `<conversation_context>`; cancel map restored after an ephemeral turn; conversational cache survives ephemeral send failure.
- **Not done (Phase 5):** scratch empty-assistant root cause, live classifier eval, broader session observability.
