# REQ-DOCS-001 independent review handoff

- Status: `IN_REVIEW`
- Integration base (current `main`): `0585c4ea97ab0ca8b2ac46771f5eb5b166a81122`
- Review SHA: `4f53811` on `req-docs-001/d1-inventory`
- Author worktree: `../ai-publisher-req-docs-001-d1`
- Review checkout: `../ai-publisher-req-docs-001-review`
- Reviewer must not be the author of this worktree or this Orchestrator session
- Human gate after Reviewer `PASS`: owner approval of the documentation map and entry-point wording (`TARGETED_HUMAN`)

Inspect:

1. Target `docs/` taxonomy vs `requirement.md` and `migration-ledger.md`
2. Content preservation (`git diff --find-renames 0585c4e`)
3. Entry-point responsibility split (README / START_CODEX / CODEX_HANDOFF / AGENTS / RUNTIME / engineering docs / prompts)
4. Root README repository map
5. Reference integrity report and `scripts/verify` path retargeting
6. Merge with REQ-HARNESS-002/003 short-intent/budget work preserved with new paths
7. No product/runtime behavior change; no Architecture Contract / ADR semantic change

Verdict: `PASS` or `REWORK` only.
