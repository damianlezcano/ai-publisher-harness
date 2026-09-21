## SCRATCH PERMISSION FIX — TOOL-SAFE TEXT-PRODUCING PROFILE (2026-09-19)

- **Scope:** minimal correction after the live A/B that **proved** empty scratch assistants. Not a new architectural phase. Not Fases 1–6. Not Creation. Not CompletedWithoutOutput removal.
- **ROOT CAUSE OF EMPTY SCRATCH ASSISTANT: PROVEN** — scratch `POST /session` permission profile with global `permission:"*"` `action:"deny"` on OpenCode 1.18.25. Same directory/prompt/model/agent/fresh session with ordinary `external_directory` deny produces text; the `*` deny produces `parts=[]` `text_len=0` `finish=None` `error=false`. D → E → D was good → empty → good.
- **Also proven:** per-tool `"*"` denies (no global `*`) still empty the model `activeTools` via `Permission.disabled` (`pattern === "*"` exact) and yield the same empty assistant.
- **Fix:** `scratch_tool_free_permission` now denies builtin coding tools with pattern `"**"` (execution deny, tools stay visible) and keeps `external_directory` `"*"` deny. No global `*` rule. Classifier and summarizer still share the helper. `CompletedWithoutOutput` remains the fail-safe.
- **Live:** control A text; candidate B (`*` per-tool) empty; candidate C (`**` + `external_directory`) text `finish=stop`. Post-implement confirmation uses the helper.
