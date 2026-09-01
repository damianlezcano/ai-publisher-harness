# Frontend Re-Review — Composer add-file error fix + Materials dead-code removal

Reviewer: qwen (opencode-go/qwen3.8-flash) — independent code/a11y/regression reviewer (READ-ONLY).
Subject: commit `abd41bc` on branch `uxfix/visual` (author: Cursor Grok 4.6 High).
Scope: the two CODE_IMPORTANT findings raised in `qwen-frontend-review-b7d75be.md`. Visual design
was already APPROVED; this pass verifies only the two fixes and regression risk.

VERDICT: APPROVE

FINDINGS:
- info: Both CODE_IMPORTANT findings are fully addressed.
  - Finding 1 (unhandled rejection in `ComposerBar.pickFile()`): the entire flow (dialog open +
    `api.materialAddFromPath`) is now wrapped in try/catch. Prior error state is reset at start
    (`setPickError(null)`), and a visible `ErrorNotice` is rendered when `pickError !== null`.
    `onMaterialsChanged` is intentionally NOT called on failure, and attachments/`showMaterialPicker`
    are only mutated on the success path.
  - Finding 2 (dead code): deleted the `MaterialsPanel` default export, `MaterialItem` (the only
    frontend caller of `api.materialRemove`), the now-unused `MaterialItemProps` interface, and the
    duplicated `importDetailLabel`. `MaterialChip` was KEPT. `MaterialsPanel.test.tsx` deleted.
- info: No lingering references to deleted symbols. The only remaining `MaterialsPanel` token in
  `app/src` is the legitimate module-path import in `ChatPanel.tsx`
  (`import { MaterialChip } from "./MaterialsPanel";`). `importDetailLabel` is now single-sourced in
  `WorkspaceView.tsx`.
- info: `api.materialRemove` remains in `api.ts` (backend contract preserved), matching the finding
  instruction to delete the dead caller rather than the contract.
- none blocking.

TEST_RESULTS:
- `cd app && npm run typecheck` (tsc --noEmit) — PASS, clean.
- `cd app && npm test` (vitest run) — PASS: 21 test files, 163 tests, 0 failures. (Down from 22 files
  after `MaterialsPanel.test.tsx` deletion; the new ComposerBar error test is included.)
- `git diff b7d75be abd41bc -- app/src/` — 4 files changed: `ComposerBar.tsx` (+try/catch +ErrorNotice
  +pickError state), `ComposerBar.test.tsx` (+dialog mock, +`material_add_from_path` error branch,
  +new plain-language error test), `MaterialsPanel.tsx` (dead code removed, `MaterialChip` kept),
  `MaterialsPanel.test.tsx` (deleted). 43 insertions / 444 deletions.
- File-scope guard: `git diff --name-only b7d75be abd41bc | grep -v '^app/src/'` — no files outside
  `app/src/**` were touched by the fix commit.

A11Y:
- The new error surface uses the shared `ErrorNotice`, which renders `role="alert"` — announced to
  assistive tech on appearance. This is consistent with the WorkspaceView drop path, which surfaces
  `materialError`/`sendError` through the exact same component, so picker-error and drop-error
  feedback are now uniform.
- No code/path leakage. `guidanceFromError` maps only by error `code` to fixed, human copy
  (`messages.error.materialUnsupported` = "No admitimos ese tipo de archivo.") and never renders the
  raw backend message, file path, or code token. The new test explicitly asserts both
  `material_unsupported` and `/tmp/bad.exe` are absent from the DOM.
- Plain-language Spanish copy is consistent with the non-technical user-facing philosophy.

REG.RISK:
- Low. The error banner renders only when `pickError !== null`, so the default composer layout is
  unchanged versus the previously-approved visual state — no visual redesign, no UX element altered
  beyond the two findings.
- Dialog-cancel path is preserved: `api.pickFile()` returns null on cancel and the code returns early
  via `if (!path) return`, so no spurious alert is shown on cancel.
- `ChatPanel.tsx` is byte-identical (empty diff vs b7d75be); `MaterialChip` import and usages
  unchanged. Backend contract (`api.materialRemove`, `materialsAddFromPaths`, `materialAddFromPath`,
  `pickFile`) intact in `api.ts`.

REVIEWED_COMMIT: abd41bc (branch uxfix/visual)

NOTE (out-of-scope polish for a future pass):
- `pickError` has no explicit dismiss affordance; it clears only when a new pick is attempted
  (`setPickError(null)` at the top of `pickFile()`). Consider a dismiss button and/or auto-clearing on
  the next successful add so a stale alert cannot linger after the user moves on.
- `api.materialRemove` is now frontend-dead (only its `MaterialItem` caller was removed). If the UI
  intentionally has no remove action anymore, a follow-up should confirm whether the binding is kept
  purely as a contract or should be documented as reserved.
