## PHASE 2 — ORDINARYCHAT IS A HARD NO-KNOWLEDGE TURN (2026-09-18)

- **Scope:** bounded Phase 2 correction. Not Phase 3, not classifier/should_classify, not K6/PerItem/scratch, not NormalSemantic session redesign.
- **Problem:** after Phase 1, OrdinaryChat skipped `apply_route` preparation, but `prepare_knowledge_context` still defaulted to hybrid search for any non-exhaustive/thematic/inventory intent, and leftover `AgentRunInputs.knowledge` could still serialize or open a fresh RAG session.
- **Change:** `Intent::OrdinaryChat` is the execution source of truth. `apply_route` clears Knowledge fields and records `knowledge_used=false retrieval_mode=none query_embeddings=0`. `prepare_knowledge_context` returns immediately when the route does not use Knowledge (before sqlite/hybrid/embeddings). `dispatch_message_run` / `send_message_run` / `run_agent_with_inputs` strip leaked context. OrdinaryChat uses `open_session`, not `open_fresh_session`.
- **Not done (Phase 3):** NormalSemantic session design, scratch empty-completion root cause, session_id unification.
