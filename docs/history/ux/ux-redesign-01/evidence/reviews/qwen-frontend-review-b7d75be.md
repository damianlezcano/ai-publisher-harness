# Frontend Code/A11y/Regression Review — b7d75be (uxfix/visual)

Reviewer model: opencode-go/qwen3.8-flash (MODEL_REQUESTED==MODEL_ACTUAL). Read-only review of `git diff 2dcacc5 b7d75be -- app/src/`.
Verification run (read-only): `npm test` → 22 files / 170 tests passed; `npm run typecheck` (tsc --noEmit) clean.

VERDICT: REQUEST_CHANGES

FINDINGS:
- CODE_BLOCKER: none.
- CODE_IMPORTANT:
  1. ComposerBar.tsx:110-118 `pickFile()` is invoked with `void` and has no try/catch (ComposerBar.tsx:238, :279). `api.pickFile()` opens an unfiltered dialog (api.ts:71-75), and a rejecting `materialAddFromPath` (unsupported/duplicate file) becomes an unhandled promise rejection with zero user feedback. This is a regression of the error surfacing the removed WorkspaceView `addFile()` previously provided via `setMaterialError`/ErrorNotice, and is inconsistent with the new drop path, which does catch (WorkspaceView.tsx importRef). Add a catch with visible error state.
  2. Dead code / lost capability: after removing the Materiales panel, the `MaterialsPanel` default export and `MaterialItem` (the only caller of `api.materialRemove`, MaterialsPanel.tsx:83) are unreachable from the app — only `MaterialChip` is imported (ChatPanel.tsx:2). Materials can no longer be deleted or managed, yet `MaterialsPanel.tsx` plus its 8 tests survive, and `importDetailLabel` is now duplicated verbatim in WorkspaceView.tsx:24-33 and MaterialsPanel.tsx:29-37. Either delete the dead component/helper (keep `MaterialChip`) or record the deletion-capability removal as an explicit product decision; do not leave a duplicated helper.
- CODE_POLISH:
  - WorkspaceView.tsx drag-drop effect: if unmount races the `onDragDropEvent` promise, `fn()` is never called (`.then((fn) => { if (active) unlisten = fn })` leaks the listener); call `fn()` in the `!active` branch. Mirrors a pre-existing App.tsx pattern.
  - WorkspaceView import-details list keys on `item.sourceName` — duplicates across a batch collide as React keys; use index or name+status.
  - App.tsx:166 `bannerStatus !== "free"` is now unreachable (providerStatus can no longer be `"free"` and the banner returns null for it) — harmless belt-and-braces, but the `"free"` variant of `ProviderStatus` and `messages.provider.banner.freeModel` are now dead in-app; consider retiring in a follow-up.
  - Composer select placeholder "Modelo automático · Gratis" shows whenever a free model is selected but absent from the visible list (e.g. stale/removed model), which can mislabel an unavailable selection.
  - Untracked `app/package-lock.json` sits in the worktree (not in the commit) — flag to orchestrator for hygiene.
- A11Y_BLOCKER: none.
- A11Y_POLISH:
  - `.conversation-name` now truncates with ellipsis/nowrap but has no `title`/tooltip, so long conversation names are fully inaccessible visually.
  - `composer-model-select` rests at `color: var(--muted)` on a transparent background until hover/focus — verify 1.4.3 contrast in the resting state at gate time.
  - Icon-only +/✎ buttons carry matching `aria-label`+`title` (fine for 2.5.3 since the glyphs are `aria-hidden`); rename button `opacity:0` is revealed by `:focus-within`, so keyboard operability holds. Consider adding "(Ctrl+N)" to the new-conversation title for discoverability.

## 1. Code quality / correctness

The timeline merge in ChatPanel.tsx:46-73 is sound — stable keys (uuid ids), deterministic ISO-string sort with key tiebreak, pending-user attachments excluded via `referencedMaterialIds` to avoid double rendering, and the empty-state now correctly factors in unattached materials. Hook hygiene is good: Ctrl/Cmd+N effect (ConversationsSidebar.tsx:69-79) has proper cleanup, re-subscribes on `createConversation` identity, guards `busy`/`event.repeat`, and correctly ignores inputs/textareas/selects/dialogs. The `importRef` indirection avoids a stale-closure in the Tauri drag listener. `api.ts` is untouched — no backend command changes, contract preserved; `modelList`/`pickFile` null-coalescing in ComposerBar is defensive only. No `console.*`/message logging anywhere; provider/model IDs appear only in non-visible `<option value>` attributes (App.test asserts no `::` text); no secrets/paths in UI strings.

## 2. A11y

Sidebar remains a `nav` landmark with descriptive `aria-label`, items are buttons with `aria-current="page"`. Timeline keeps `aria-live="polite"` and authorship is conveyed by visible text ("Vos"/"Asistente"/"Material"), not color. Composer keeps the sr-only label, labelled Enviar/Cancelar, Ctrl/Cmd+Enter send, and a labelled paperclip that doubles as the accessible alternative to drag/drop; the drop overlay is a `role="status"` live announcement. Settings (X "Cerrar", focus trap, Esc) and Compartir are untouched by this diff, so their prior compliance stands.

## 3. Regression

`messages.ts` only adds keys (`resourceLabel`, `dropOverlay`, `automaticFree`) and rewords values — canonical vocabulary ("Conversación nueva", "Gratis", "Conversaciones" aria-label) is retained and `messages.test.ts` assertions were strengthened, not weakened. Test diffs replace the free-banner assertion with positive selector-option checks plus new Ctrl+N and drag-overlay tests; no assertions were deleted to mask behavior (`ProviderStatusBanner` free-case now asserts an empty DOM, which is the new contract). 170/170 vitest pass and `tsc --noEmit` is clean locally (read-only runs).

## 4. Bounded scope

Commit touches only `app/src/**` (14 files); no crates/, no api contract changes, no new product commands. The two IMPORTANT items are small, non-redesign fixes confined to the new code paths.
