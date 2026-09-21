## PHASE 3 — INTENT RESOLUTION PRECEDENCE (2026-09-18)

- **Scope:** consolidate follow-up / creation pre-gates, semantic classifier, deterministic fallback, clamps, and `BoundRoute`. Not Phase 4. Not session/scratch/K6/PerItem internals.
- **Found:** the live order already matched the desired precedence. Two seams could still compete with a valid semantic decision: (1) `resolve_intent_with` mapped classifier `Err` onto `DeterministicAdapter` with `DeterministicBypass` provenance; (2) `clamp_summary_depth` re-ran local summary wording detectors for every post-classifier turn.
- **Change:** fallback is exclusive replacement (`SemanticFallback` provenance, one delegate call). The compact/K6 clamp runs only on a trusted semantic `WholeCorpusSummary` / `PerSourceSummary`. Routing telemetry adds `classifier_confidence`, `fallback_used`, `fallback_reason`, `pre_gate_used`, `clamp_applied`, `resolved_intent`, `uses_knowledge` without logging prompts.
- **Not done (Phase 4):** NormalSemantic session design, scratch empty-completion root cause, session_id unification, live classifier eval.
