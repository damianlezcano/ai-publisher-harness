# Current Checkpoint

## KNOWLEDGE ANSWER GROUNDING FIX (2026-09-20)

- **Scope:** minimal production prompt contract after a live A/B **proved** Thematic/Exhaustive misgrounding. Not a new architectural phase. Not classifier, retrieval, session, tools, or scratch-permission work.
- **ROOT CAUSE OF KNOWLEDGE ANSWER MISGROUNDING: PROVEN** — CorpusThematic/CorpusExhaustive built valid evidence and used ephemeral Knowledge sessions, but the answer prompt did not override the OpenCode build-agent filesystem/workspace instruction. Same evidence/session/model/agent/cwd/tools/human prompt: current contract → `FILESYSTEM_MISGROUNDED`; identical turn plus Knowledge grounding → `GROUNDED`; current contract again → `FILESYSTEM_MISGROUNDED`.
- **Fix:** `knowledge_answer_grounding_instruction()` in `project-agent` `augment_prompt`, applied only when serialized Knowledge answer context has `retrieval_mode` `normal` / `thematic` / `exhaustive`. OrdinaryChat, Creation, classifier/K6/PerItem scratch are unchanged. `<knowledge_evidence trust="untrusted">` is preserved. Still one answer call per turn.
- **Contract:** evidence is the documentary source; an empty cwd is not an empty Knowledge corpus; use `source_name`/`source_label` when present; do not invent beyond evidence; do not force a positive over an authorized exhaustive negative.

Superseded dated operational entries were moved verbatim to [`docs/checkpoints/`](checkpoints/) (`docs/checkpoints/README.md`). Those files are historical snapshots, not current authority.
