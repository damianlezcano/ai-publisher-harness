## PHASE 5 — CONTINUITY, TRANSITIONS, AND LIFECYCLE (2026-09-19)

- **Scope:** multi-turn alternation after Phases 1–4. Not Phase 6. Not scratch empty-assistant root cause. Not session_id unification. Not classifier/K6/PerItem internals.
- **Found:** ephemeral Knowledge already used `open_fresh_session` plus bounded EducAI history. The cancel map was restored only after a successful ephemeral `send`; a failed ephemeral `send` left abort targeting the dead ephemeral session. `conversation_context` already skipped failed turns and stripped evidence markup.
- **Decision:** keep per-turn routing. Restore the conversational cancel target even when ephemeral `send` fails. Reconstruct follow-up continuity from bounded visible EducAI history + referents + this-turn retrieval. Do not persist OpenCode session ids.
- **Change:** `EphemeralCancelRestore` Drop guard; `[lifecycle]` / prompt-context structural fields (`session_role`, `session_reused`, conversation_context counts); UTF-8-safe context budget; multi-turn tests for OrdinaryChat↔Knowledge, follow-ups, attachments, material scope A→B, cancel/failure, reopen, model pin per turn, inventory→PerItem, exhaustive local→OrdinaryChat.
- **Not done (Phase 6):** scratch empty-assistant root cause (`ROOT CAUSE OF EMPTY SCRATCH ASSISTANT: NOT YET PROVEN`), live classifier eval, broader product telemetry UI.
