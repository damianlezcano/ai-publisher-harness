## PHASE 1 REWORK — SEMANTIC CLASSIFIER IS THE AMBIGUOUS-INTENT AUTHORITY (2026-09-18)

- **Scope:** bounded Phase 1 correction. Not Phase 2, not K6/PerItem/scratch/session_id.
- **Problem:** the first Phase 1 gate still used linguistic helpers (`has_open_question_lead` / `OPEN_QUESTION_WORDS`) to skip the semantic classifier or locally force `NormalSemantic`, which is language-fragile.
- **Change:** `should_classify` is structural (`knowledge_may_apply`: persisted index OR current-turn attachments OR prior referent). Ambiguous wording with Knowledge available consults the LLM classifier, which may return `OrdinaryChat`. Deterministic catch-all is OrdinaryChat, not open-question → NormalSemantic. Helpers remain for retrieval internals only. Telemetry still records `knowledge_available`, `knowledge_needed_for_turn`, `classifier_invoked`.
- **Not done (Phase 2):** full OrdinaryChat contract against embeddings/RAG leftovers.
