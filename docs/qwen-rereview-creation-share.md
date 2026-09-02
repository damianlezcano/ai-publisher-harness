# Independent Code / A11y / Correctness Re-Review

Diff: `3ba7c5a..857d98c` (fix diff, 14 files, +554/-94); full combined diff `3a7c6d1..857d98c`.
Branch `corr/creation-share-ux-pass`, HEAD `857d98c`, clean. READ-ONLY re-review.

## APPROVE

### Verification performed

- `git diff --check 3ba7c5a..857d98c` → clean
- `cargo fmt --check -p project-agent -p project-app` → clean
- `pnpm typecheck` (app) → clean
- `cargo test -p project-agent` → agent_service 9/9, opencode_adapter 20/20, unit 11/11
- `cargo test -p project-app --test app_facade` → 32/32
- `pnpm vitest run` on the 4 touched test files → 78/78

### Findings (M1 + m1-m7 + LOW/NIT) — dispositions verified in code

- **M1 (MAJOR) — Fixed (option a).** `crates/project-agent/src/service.rs:338` `merge_artifacts` now uses the sidecar diff when it contains a registrable (non-materials) file and falls back to `scan_workspace_artifacts` **only** when the diff is empty. New test `later_turn_does_not_reregister_prior_workspace_files` (`crates/project-agent/tests/agent_service.rs:392`) asserts turn 2 registers 1 (not 2) with a prior-turn `index.html` still on disk; pre-existing `workspace_scan_registers_when_diff_is_empty` (`:341`) still passes. `merge_artifacts_keeps_diff_and_does_not_scan_prior_files` and `merge_artifacts_scans_when_diff_is_empty` unit tests added (`service.rs:552,570`).
- **m1 — Fixed.** `registrar.rs:146-170`: root `workspace/index.html` now yields `None` from `web_display_name` → `fallback_display_name` returns the human label `Actividad` (`DEFAULT_WEB_DISPLAY_NAME`, `:128`) for stems `index`/`htm(l)`; parent-folder names still win for nested entries (`actividad-2/index.html` → `actividad-2`). Test `root_index_html_uses_human_display_name` (`:392`).
- **m2 — Fixed.** `sidecar_component_ok` (`registrar.rs:272-283`) mirrors snapshot `validate_component` (`crates/project-fs/src/publication_snapshot.rs:453-463`): Windows-reserved stems rejected at all depths via `is_windows_reserved_stem` (`:241-268`, identical stem logic), and root-level `materials.html`/`files` skipped (`RESERVED_ROOT_SIDECARS`, `:141`). Test `copy_skips_unpublishable_roots_and_keeps_nested_index` (`:409`).
- **m3 — Fixed.** `registrar.rs:364-367`: the `index.html` skip is now `dest == dest_root.join("index.html")` (root only); nested `slides/index.html` is copied. Verified by the same test.
- **m4 — Fixed.** Both scan (`service.rs:318-345,372-414`) and copy (`registrar.rs:129-144,285-298`) skip dependency/build trees (`node_modules`, `dist`, `build`, `target`, `vendor`, `venv`, `__pycache__`, `coverage`, `bower_components`) and cap depth=8, files=500, bytes=32 MiB with saturating accumulators. Tests `workspace_scan_skips_dependency_trees` (`service.rs:583`) and the copy test.
- **m5 — Fixed.** `opencode.rs:118-127`: the 2 s idle grace now starts regardless of artifacts, and `/diff` is fetched **once** when the grace begins (`idle_artifacts` cached at `:120`) instead of every 20 ms tick; empty replies return after the grace via `IDLE_WITHOUT_TEXT_GRACE` (`:22`). Test `send_empty_assistant_without_files_completes_after_idle_grace` (`tests/opencode_adapter.rs:325`) asserts completion in <5 s; `send_never_idle_times_out` / `send_idle_without_new_assistant_message_times_out` still pass (no timeout regression).
- **m6 (A11y) — Fixed.** `app/src/components/CreationsPanel.tsx:87,100`: Abrir gets `aria-label="{Abrir}: {displayName}"`; Compartir gets a state-aware `"{action}: {displayName}"`. Visible-text prefix preserved (label-in-name OK), no ARIA misuse. Test `gives each creation's actions a distinct accessible name` (`CreationsPanel.test.tsx`) renders two creations and asserts 4 distinct names; all card/button tests updated to the compound names.
- **m7 — Fixed.** `web_sidecar_sibling_is_copied_into_outputs_and_publish` (`crates/project-app/tests/app_facade.rs`) uses the **real** registrar and asserts `app.js` lands in both `outputs/<id>/` and `publish/`, and the display name is `Actividad`.
- **L1 — Fixed.** `crates/project-app/src/app.rs:1369`: unused `_has_creations` param dropped from `assistant_reply_text`; call site updated (`:933`). Behavior identical.
- **L2 — Fixed.** `app/src/messages.ts`: unused `agent.ready` string removed; `App.test.tsx:479-480` negative assertion switched to the literal string. `.composer-model` / `.composer-model-select` CSS removed (`app/src/styles.css`). No remaining refs.
- **L3 — NOT fixed (accepted residual, see notes).** `app.rs:1305-1323` unchanged.
- **L4 — Fixed.** `registrar.rs:100-110`: sidecar copy is best-effort after the Creation exists (`let _ = copy_web_sidecars(...)`), so a copy failure no longer hides the primary. Source relative path now rejects empty/absolute/`.`/`..` segments (`:194-201`) and dest construction rejects `.`/`..` (`:353-358`) — defense-in-depth.
- **N1 — Fixed.** `opencode.rs:401-405`: `content: ""` falls through to `parts` (guard changed to `&& !text.trim().is_empty()`). Tests `empty_content_falls_through_to_parts` and `nonempty_content_wins_over_parts` (`:525-546`).

### Invariants (full combined diff `3a7c6d1..857d98c`)

1. **No duplicate-Creation regression (M1):** confirmed — non-empty diff path never re-registers leftover prior-turn files; empty-diff fallback still registers scanned workspace files.
2. **Generic behavior:** no Pasapalabra-specific hardcoding anywhere in non-test `crates/` or `app/src`.
3. **Path safety:** `normalize_output_path` / `validate_workspace_artifact_path` (`opencode.rs:475-523`) unchanged — still reject project-root trees, `..`/empty segments, and absolute paths without a `/workspace/` segment; new sidecar source/dest validation adds no traversal vector. `traversal_artifact_path_is_rejected_and_not_registered` passes.
4. **B2/B3 identity:** card Abrir (`preview_open_web`, `creation.id`) and card Compartir (`onShare(creation.id)` → `publish_creation(_, Some(id))`) target the same Creation; `app.publish` retained as `publish_creation(_, None)`; publish tests (`publish_promotes_the_generated_web_creation_as_the_public_entry`, `publish_without_creation_id_still_promotes_the_latest_web`, `creation_path_rejects_cross_project_id`) pass; sidecars land in the same `outputs/<id>/` dir the snapshot bundles.
5. **B5:** `ChatPanel.tsx:119-122` still hides only completed, non-error, empty-text, no-creation bubbles; errors/cancellations render `role="alert"`; untouched by the fix.
6. **B6:** `App.tsx` not in the fix diff; one-readiness-one-notification and the closed late-unlisten race preserved; the removed toast string does not alter listener logic.
7. **B7:** `ComposerBar.tsx` / `ProviderPanel.tsx` / `ModelSelector.tsx` not in the fix diff; composer has no model selector, Configuración keeps it, discovery stays dynamic.
8. **Delete confirmation:** `ConfirmDialog.tsx` / `ConversationsSidebar.tsx` are absent from both diffs — "Sí" contract intact.
9. **A11y:** per-Creation accessible names added; Settings dialog semantics untouched; no ARIA misuse introduced.
10. **Tests:** new/modified tests are deterministic and meaningful (compound accessible-name assertions, on-disk `outputs/`/`publish/` reads, real-file workspace fixtures, timing-bounded idle-grace); modified existing tests still assert their original intents.
11. **Scope:** fix diff is 14 files, all traceable to M1/m1-m7/L1-L4/N1; no M11 work, no unrelated refactor; `docs/UX.md` consistent.

### Residual notes (non-blocking)

- **LOW** — L3 (`app.rs:1305-1323`) remains: explicit Compartir on a non-web Creation does not demote an existing public Web, so the URL root can still be owned by a different artifact. It was a pre-existing LOW/NIT, is unchanged by this pass, and M1 removes its worst concrete manifestation (stale-duplicate promotion via the no-id fallback). Acceptable to defer; revisit if product wants any public Web demoted when the target is not a Web.
- **NIT** — `registrar.rs:103` discards the copy error via `let _ =` with no log line; the reviewer suggested "log and continue." A debug/warn would aid diagnosis without changing behavior.
- **NIT** — Skip lists exclude generic dir names (`build`, `dist`, `target`, `materials`) at any depth; a legitimately named activity folder could theoretically be excluded from scan/copy. Improbable for educational artifacts and consistent with the requested m4 fix; acceptable tradeoff.

**Verdict: APPROVE.** All MAJOR and MINOR findings resolved and test-covered; invariants preserved; no regressions detected; formatting/typecheck/targeted suites green.
