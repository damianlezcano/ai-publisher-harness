# M9 Education UX Polish — Design

Status: **Implemented** (M9 T1-T10 integrated and verified; design was
Approved by the human owner, ADR-0012 Accepted). This document is the durable
design handoff for the M9 implementation session (a fresh
`opencode-go/deepseek-v4-flash` orchestrator).

## 1. Executive summary

M9 makes the M1-M8 product feel coherent, understandable, and trustworthy to a
non-technical education user, **without adding infrastructure**. It is almost
entirely a frontend/application-facade polish milestone:

- One **message catalog** (`app/src/messages.ts`) becomes the single source of
  user-facing Spanish copy, so terminology is auditable, deterministic, and
  i18n-ready (ADR-0012).
- **Canonical vocabulary** is fixed in `docs/UX.md` and reflected in the
  catalog. The only contested term (`Compartir` vs `Publicar`) is resolved by
  recommendation and flagged for the human owner.
- **Contextual empty states** and a **minimal first-run guide** replace ad-hoc
  placeholder text, each telling the user the next useful action.
- **Recoverable error guidance** maps the already-machine-readable `AppError.code`
  to a title + message + next action (frontend-only; no backend change).
- **Accessibility is unified** into a shared dialog/focus-trap primitive and a
  single polite live region, closing gaps in `ConfirmDialog`, `ProviderPanel`,
  and `QrDialog`.
- **Sharing becomes honest and legible**: explicit "temporary link" messaging,
  an intermediate "Compartiendo…" state, a project-titled QR dialog, and a
  light confirmation before stopping a live share.
- A small **visual system** (tokens, spacing, typography, button/badge hierarchy)
  and a deliberate **responsive desktop layout** replace incidental CSS.

The decisive architectural result: **zero Rust/core/domain changes, zero new
Tauri commands or capabilities, zero security-invariant changes.** M9 is
`app/src` + docs + tests, which keeps `scripts/verify` fast, offline, and
deterministic.

## 2. M8 / M9 / next-milestone boundary

| Milestone | Owns | Excludes |
| --- | --- | --- |
| M8 | Clipboard paste, multi-file import, material lifecycle, prompt attachments, safe previews, isolated web preview (CLOSED) | UX polish, terminology, first-run, sharing messaging |
| M9 | Education UX polish: terminology, empty states, first-run, loading/progress, success/error feedback, sharing/QR/temporary-link messaging, accessibility, responsive desktop layout, minimal visual system, keyboard shortcuts | packaging, sidecars, component updates, Windows release, provider/credential redesign, mobile, full i18n framework, cloud telemetry, rich Markdown rendering |
| M10 | Packaging (Linux AppImage/RPM first; sidecar bundling) | UX features; consumes M9's polished UI as the packaged surface |

M9 **consumes** M7/M8 capabilities unchanged: the free-model default
(ADR-0009), provider onboarding (ADR-0008), preview isolation (ADR-0010), and
attachment contract (ADR-0011) are all treated as existing surfaces to polish,
never to redesign.

## 3. Target user journeys

1. **First launch.** No projects exist. The Projects view shows a short,
   dismissible 5-step guide and a prominent "Crear proyecto". No tutorial overlay.
2. **Create a project.** One name field, autofocused, default suggested name
   ("Proyecto sin título" or `Proyecto N`), Enter to create, `Ctrl/Cmd+N` to open
   the form. Empty materials/creations/sharing panels each point to the next step.
3. **Add material.** Empty state explains what material is for; a picker (not
   only drag/drop) is the accessible path. Multi-import reports a one-line
   summary ("Se agregaron 3 archivos. 1 ya estaba, 1 no se pudo agregar.").
4. **Ask the AI.** Multiline composer, `Ctrl/Cmd+Enter` to send, example
   placeholder, visible attachments as chips. If the free model is unavailable and
   no provider is connected, the composer shows "Conectá una IA" instead of a
   dead end.
5. **View a creation.** Cards group by type with a clear "Abrir" / "Vista previa"
   and a single share-status toggle ("Se compartirá" / "Privado"). Success toast:
   "Tu recurso está listo."
6. **Share.** One "Compartir" action → "Compartiendo…" → "Compartido" with the
   link, "Copiar enlace", "Abrir enlace", "Mostrar QR", and a clear note that the link is
   temporary. QR dialog is large, titled with the project name, with copy/open
   nearby.
7. **Stop sharing.** "Dejar de compartir" asks a light confirmation ("tus
   estudiantes ya no podrán abrir el enlace"), then confirms "Dejaste de
   compartirlo."
8. **Recover from a failure.** A generation/publish/provider failure shows a
   message with the specific next action (Reintentar / Conectar IA / Abrir con la
   aplicación), never a raw code or stack trace.

## 4. Canonical UI terminology

Defined in `docs/UX.md` and implemented by `messages.ts`. Spanish (`es-AR`),
consistent verb/noun forms.

| Concept | Canonical label(s) | Notes |
| --- | --- | --- |
| App | EducAI | unchanged |
| Project | `Proyecto` / `Mis proyectos` (list heading) / `Nuevo proyecto` | singular/plural; list heading changes from "Tus proyectos" |
| Material | `Material` / `Materiales` (panel) | unchanged |
| Creation | `Creación` / `Creaciones` (panel) | unchanged |
| AI assistant | `Asistente` (panel) | replaces "Conversación" panel heading; the concept is "asistente de IA", "IA" alone is acceptable |
| Share action | **`Compartir`** | replaces "Publicar" |
| Sharing in progress | `Compartiendo…` | — |
| Shared state | `Compartido` | replaces "Publicado" |
| Not shared state | `No compartido` / `Local` | "Local" avoided in copy; use "No compartido" |
| Stop sharing | `Dejar de compartir` | unchanged |
| Public link | `Enlace para compartir` | replaces "Enlace público" |
| QR | `Código QR` / `Mostrar QR` | unchanged |
| Preview | `Vista previa` / `Abrir vista previa` | unchanged |
| Create | `Crear` / `Creando…` | — |
| Model | `Modelo` | — |
| Free model | `Gratis` | unchanged (ADR-0009) |

**Resolved (human-owned):** the primary share verb is **`Compartir`**
(`Compartiendo…`, `Compartido`, `Enlace para compartir`, `Dejar de compartir`).
`Publicar` is **not** used as a primary action in the UI. Internal domain
identifiers (`Publication*`, `publish`, `unpublish`, commands, DTO fields,
`PublicationManager`) are **not** renamed — this change is product language /
copy only, and only rendered copy changes.

## 5. First-run / empty-state strategy

- **First-run guide:** a dismissible, persistent (`localStorage` flag) card on
  the Projects view when zero projects exist, listing the five steps
  (crear proyecto → agregar material → pedir a la IA → mirar la creación →
  compartir). It is guidance, not a forced wizard: any step can be taken out of
  order and the guide disappears once the first project is created.
- **Empty states** use one `EmptyState` component: title, one-line body, and a
  single primary action (plus an optional secondary). States:
  - Projects: "Todavía no tenés proyectos" → `Crear proyecto`.
  - Materials: "Agregá material para darle contexto a la IA" → `Agregar archivo`
    (+ "o pegá una imagen con Ctrl+V").
  - Creations: "Pedile a la IA que cree algo" → secondary hint "Escribí en el
    asistente".
  - No AI usable: "No hay una IA conectada" → `Conectar IA` (see §13).
  - Not shared: "Este proyecto todavía no se comparte" → `Compartir`.
- Every empty state tells the user **what useful action to take next**; no empty
  state is a dead end.

## 6. Project UX improvements

- **Only name is required**; keep it that way. Validate non-empty trimmed name
  (existing `InvalidName` → friendly message).
- **Default name:** pre-fill `"Proyecto sin título"` (or `Proyecto N` on the
  Projects view) so a single Enter creates a project; the user can rename later.
- **Autofocus** the name input (already present) and select-on-focus for rename.
- **Enter** submits (already via form); `Esc` cancels the create/rename form.
- **Rename/delete** stay on the Projects row; delete keeps the typed-name
  confirmation (see §16). Rename keeps inline edit with Save/Cancel.

## 7. Prompt / composer UX

- **Multiline textarea** with auto-grow (rows expand to a max height, then
  scroll); no token streaming for visual effect.
- **Send:** `Ctrl/Cmd+Enter` sends; `Enter` inserts a newline (deliberate change
  from the current single-line-ish 3-row field).
- **Cancel:** `Esc` in the composer is reserved for closing dialogs, not
  cancelling generation; the visible "Cancelar" button remains the explicit
  generation-cancel control (a shortcut here risks accidental cancellation).
- **Attachments:** keep chips; add a small "Adjuntar material" affordance in the
  composer that surfaces the project's materials for selection (reuses existing
  authorized material IDs; no new backend).
- **Placeholder/examples:** a clearer placeholder and a rotating or static
  example ("Ej.: Creá una actividad interactiva sobre la fotosíntesis").
- **Progress:** single human message "Creando tu recurso…" with an indeterminate
  spinner; no technical stages.

## 8. Materials UX

- **Visual grouping:** cards stay; add a small kind icon/type label and clear
  type + size + date meta (already present). No new filesystem architecture.
- **Readable file types:** reuse `kindLabel` (Imagen / Documento PDF / Hoja de
  cálculo / Presentación / Texto / Archivo).
- **Duplicate messaging:** per-batch summary instead of a single generic note:
  "3 agregados · 1 ya estaba · 1 no se pudo agregar".
- **Multi-import summary:** render `MaterialsImportReport` as one summary line +
  (optional) a per-file detail list; partial failures never look like total
  failure.
- **Remove confirmation:** keep the inline per-card confirm (name shown); it is
  low-stakes and reversible-in-practice (source file untouched).
- **Attachment chips:** consistent chip style in composer and import summary.

## 9. Creations UX

- **Cards/list** grouped (optionally by kind) with type, size, created time, and
  a single share-status toggle.
- **Share eligibility/status:** replace the confusing pair ("Se compartirá" /
  "Marcar privado") with a clear toggle + state label: `Se compartirá` (on) /
  `Privado` (off), plus a clarifying line when the project is already shared
  ("Los recursos 'Se compartirá' ya están visibles en el enlace").
- **Actions per type** unchanged from M8: image/text → Vista previa + Abrir;
  web → Vista previa (aislada) + Abrir en navegador; document/pdf/other → Abrir.
- **No internal IDs, revisions, or paths** are ever rendered (already true; keep
  the `revision` value out of the UI).

## 10. Sharing UX

The complete, legible flow:

```
No compartido ──[Compartir]──▶ Compartiendo… ──▶ Compartido
                                                     ├─ Copiar enlace
                                                     ├─ Abrir enlace
                                                     ├─ Mostrar QR
                                                     └─ Dejar de compartir
```

- **Compartiendo…** intermediate state: `publish` is currently synchronous
  (returns after the tunnel is up), so this is a frontend busy state, not a new
  backend phase; the copy notes the first share may take a few seconds.
- **Obvious temporariness** (see §11) is shown inline on the shared panel.
- **Stop sharing** asks a light confirmation and then confirms; stopping one
  project never mentions other projects (PUBLIC semantics are per-project).
- The shared panel never exposes a port, hostname internals, or Cloudflare.

## 11. Temporary-link messaging

Honest, non-technical, shown on every shared panel (and inside the QR dialog):

> "Este enlace funciona mientras el recurso esté compartido. Si cerrás la
> aplicación, dejás de compartir o se corta la conexión, el enlace deja de
> funcionar."

Additional guidance, kept short:
- "Compartí este enlace con tus estudiantes. Es temporal: no es un sitio
  permanente."
- On app close with published projects, the existing warning ("los enlaces
  dejarán de funcionar") is retained and aligned with this wording.

We never claim permanent hosting.

## 12. QR UX

- **Large** QR (target ~320-400 px) for projector scanning.
- **Project title** as the dialog heading and in the image `alt`
  ("Código QR de <proyecto> para <url>").
- **Copy link and Open** buttons adjacent to the QR (not only on the parent
  panel).
- **Close/back:** `Esc` and a visible "Cerrar"; focus trapped and returned.
- No external QR service: the existing client-side `qrcode` package is used.

## 13. Provider-state integration

M9 does **not** redesign provider architecture. It adds a `ProviderStatusBanner`
(and an empty-state path in the composer) that reflects reality without lying to
a working user:

- **Default free model available (ADR-0009):** the app works with zero
  configuration. Show a subtle "Modelo gratuito" status; do **not** claim "no hay
  una IA conectada" as a blocker.
- **`SelectedModelView.requiresChoice` is true** (the stored model disappeared and
  only paid/unavailable models remain): show "No hay una IA conectada. Conectá tu
  cuenta para seguir creando." + `Conectar IA` (opens `ProviderPanel`).
- **Credential revoked / provider unavailable** surfaced from an agent/provider
  error: show "Necesitás volver a conectar tu cuenta." + `Conectar IA`.
- The existing "Conectá tu IA" app-bar action remains; the panel keeps its
  privacy note ("Tus claves se guardan de forma segura en tu computadora").

## 14. Loading / progress patterns

One vocabulary of human progress states; **no technical stages**:

| Action | State |
| --- | --- |
| Generate | "Creando tu recurso…" + indeterminate spinner |
| Import materials | "Agregando archivos…" (busy) |
| Publish | "Compartiendo…" (+ "puede tardar unos segundos") |
| Connect provider | "Conectando…" (per provider card) |
| Test connection | "Comprobando conexión…" |
| Load preview | "Abriendo vista previa…" / "Generando código QR…" |

Progress is announced via the app-level polite live region; spinners are
`aria-hidden` and paired with visible/live text.

## 15. Error recovery patterns

Frontend-only `guidance.ts` maps `AppError.code` → `{ title, message, actions }`
(rendered by `ErrorNotice` with `role="alert"`). Guidance has a **next action**
wherever one exists:

| Code | Message | Next action |
| --- | --- | --- |
| `ai_unavailable` | "El asistente no pudo iniciarse." | Reintentar (re-send); "si persiste, reiniciá la aplicación" |
| `ai_task_failed` | "No se pudo completar la creación." | Reintentar |
| `publish_failed` | "No pudimos compartir en este momento." | Reintentar; "comprobá tu conexión a Internet" |
| `network_error` | "No hay conexión a Internet." | Reintentar |
| `material_*` | (existing per-file messages) | per-file "omitir" / retry batch |
| `preview_unavailable` | "No pudimos mostrar la vista previa." | Abrir con la aplicación |
| `preview_too_large` | "Este recurso es grande." | Abrir con la aplicación |
| `credential_revoked` / `credential_invalid` | "Necesitás volver a conectar tu cuenta." | Conectar IA |
| `provider_unavailable` / `no_compatible_model` / `model_unavailable` | per ADR-0009 wording | Conectar IA / elegir otro modelo |
| `open_failed` | "No pudimos abrir el recurso." | Reintentar |
| `storage_unavailable` / `internal` | "Algo salió mal." | Reintentar / reiniciar |

**OpenCode restart** maps to `ai_unavailable` ("reintentá; si persiste, reiniciá
la aplicación") — no subprocess detail. **Cloudflare unavailable** maps to
`publish_failed` with a retry action — no "tunnel"/"Cloudflare" wording. A retry
action is rendered only when retrying is safe and meaningful (generation, publish,
import, open), never for a credential prompt.

## 16. Destructive-action policy

Avoid confirmation fatigue; confirm only where the loss is real and
hard-to-undo:

| Action | Confirm? | Kind |
| --- | --- | --- |
| Delete project | Yes | Typed name (destroys materials + creations; irreversible) |
| Remove material | Yes | Inline, name shown (deletes stored copy only; source untouched) |
| Stop sharing | Yes | Light (interrupts a live student link; re-share recovers) |
| Disconnect provider | Yes | Light (invalidates credential, affects all projects) |
| Cancel generation | No | Explicit button only |
| Toggle creation visibility | No | Easily reversible |
| Copy/open link, preview | No | — |

No new destructive confirmations beyond these.

## 17. Accessibility plan

Explicit pass, unified through shared primitives:

- **Dialog/focus management:** one `Dialog` component (from T3) provides focus
  trap, initial focus, `Esc`, and focus return. Migrate `ConfirmDialog`,
  `ProviderPanel`, `QrDialog`, and `PreviewModal` onto it (today only
  `PreviewModal` traps focus).
- **Live regions:** a single app-level `aria-live="polite"` region for toasts and
  progress; errors use `role="alert"`. Announce "Creando…", "Compartiendo…",
  "Agregando archivos…", and completions.
- **Keyboard navigation:** logical focus order; all interactive elements are real
  `<button>`/`<input>`/`<select>`/`<textarea>` with accessible names.
- **Visible focus:** keep/complete `:focus-visible` outline across all controls
  (including the app-bar, chips, and QR dialog).
- **Labels:** `sr-only` labels for inputs that rely on placeholder; `aria-label`
  for icon-only actions.
- **Contrast:** audit tokens to ≥ 4.5:1 for text (danger `#b3261e`, muted
  `#5a5a74`, primary `#4f46e5` all satisfy on their backgrounds).
- **QR dialog accessibility:** heading = project name, image `alt`, adjacent
  copy/open actions, trap + `Esc` + focus return.
- **Drag/drop alternative:** the "Agregar archivo" picker remains the
  keyboard-accessible path; drag/drop is an enhancement, never the only path.

## 18. Responsive desktop strategy

Desktop-first, three widths; no mobile:

| Width | Layout |
| --- | --- |
| Compact (≈800-959px) | Single column; panels stack in a deliberate order (chat → materials → creations → sharing) |
| Normal (≈960-1279px) | Two-column workspace grid (chat+materials left; creations+sharing right) |
| Wide (≥1280px) | Two-column grid with a max content width (readability); panels don't stretch to absurd widths |

- `minWidth: 800`, `minHeight: 600` (already set) stay; ensure no panel overflows
  at 800px and no horizontal scroll.
- Grid/panel behavior is implemented with CSS only (`@media` + tokens), so it is
  verifiable manually and stays free of JS layout logic.

## 19. Minimal visual system

A pragmatic set of CSS custom properties and three small components — not a
design system:

- **Tokens** (`styles.css`): color (bg/surface/border/text/muted/primary/danger/
  focus/ok/err), spacing scale (4/8/12/16/24/32), radius, and font scale.
- **Typography hierarchy:** page title (1.5rem), panel heading (1.05rem), body
  (1rem), meta (0.85rem), URL mono (0.85rem).
- **Button hierarchy:** `primary` / `secondary` / `danger` / `ghost` (quiet, for
  chip-remove and low-emphasis actions).
- **Cards:** project row, material card, creation card.
- **Status badges:** `Badge` component with `ok` (Gratis/Compartido),
  `neutral` (De pago/Privado), `warning`, `danger`.
- **Dialogs:** shared `Dialog` with the established 12px-radius surface.

## 20. Keyboard shortcuts decision

Adopt a **very small** set, implemented as local key handlers (no framework):

- `Ctrl/Cmd+Enter` — send prompt (composer).
- `Esc` — close the focused dialog/modal or cancel the create/rename form.
- `Ctrl/Cmd+N` — new project (Projects view).

Deliberately excluded: a shortcut for cancel-generation (visible button only) and
anything beyond these three. No global shortcut manager.

## 21. i18n readiness

- Structure: single `messages.ts` catalog keyed by stable semantic keys
  (ADR-0012); `es-AR` only today.
- No `react-i18next`/ICU/locale loader; no pluralization machinery beyond typed
  helper functions (`messages.material.addedCount(n)`).
- Dates/sizes already use `Intl` with `es-AR`; keep centralized in `labels.ts`.
- The catalog makes future localization a parallel-catalog + selection swap; it
  must not hard-code architecture that prevents that.

## 22. Security review implications

**No security-invariant change.** M9:

- adds no Tauri command, capability, or window (`default.json`/`preview.json`
  untouched);
- grants no clipboard/fs/shell/process permission (copy uses the existing
  user-gesture `navigator.clipboard.writeText`; no new plugin);
- changes no publisher, tunnel, provider, credential, or preview behavior;
- exposes no path, ID, revision, port, or hostname in any new copy.

Therefore M9 requires no independent security review as a security-invariant
task. A single checklist confirmation in the T10 gate verifies that no capability
file or command changed and that `git diff --check` is clean.

## 23. Deterministic test strategy

All offline, frontend-only (Vitest); the existing Rust suites are unaffected:

- **Catalog/terminology:** `messages.test.ts` asserts exact copy by key and that
  canonical terms appear (and forbidden terms — "Cloudflare", "OpenCode",
  "puerto", "tunnel", "localhost" — never appear in any catalog value).
- **Guidance:** `guidance.test.ts` asserts `errorGuidance(code)` returns the
  expected title/message/actions for every `ErrorCode`.
- **Empty states:** each empty-state scenario renders correct copy + action.
- **Destructive confirmations:** typed delete (project) requires exact name;
  inline material remove shows the name; stop-sharing and disconnect show a light
  confirm; cancel paths recover.
- **Keyboard flows:** `Ctrl/Cmd+Enter` sends; `Esc` closes dialogs; `Ctrl/Cmd+N`
  opens the new-project form.
- **Sharing states:** local → compartiendo (busy) → compartido; temporary-link
  copy present; stop-sharing confirm + "Dejaste de compartirlo".
- **Provider disconnected:** banner appears when `requiresChoice` or revoked;
  absent when the free model is available.
- **Accessibility interactions:** dialog focus trap/return, `role="alert"` on
  errors, `aria-live` announcements, labelled controls.
- **Responsive layout:** asserted indirectly (CSS token/class presence) plus a
  `matchMedia`-mocked smoke if trivial; detailed visual layout is manual.

## 24. Optional E2E / visual test strategy

**Defer Playwright / Tauri E2E / screenshot testing.** Rationale: M9 is pure
frontend; the interaction matrix is fully covered by deterministic component
tests, and Tauri/WebKitGTK E2E is fragile and violates the offline, deterministic
`verify` gate. Visual/responsive confidence comes from a manual
`scripts/smoke-ux` checklist (Fedora, graphical session, SKIPs when unavailable),
extending the existing smoke pattern. Revisit E2E at M10 if packaging confidence
demands it.

## 25. Manual demo flow

Coherent first-release demo (used by `scripts/smoke-ux`):

1. First launch → Projects view shows the first-run guide + empty state.
2. `Ctrl/Cmd+N` → name "Fotosíntesis" → Enter.
3. Materials empty state → "Agregar archivo" → add `manual.pdf` + a screenshot
   (Ctrl+V paste).
4. Composer → ask "Creá una actividad interactiva sobre la fotosíntesis" →
   `Ctrl/Cmd+Enter` → "Creando tu recurso…" → toast "Tu recurso está listo".
5. Creations → preview the web creation in the isolated window; confirm no IPC.
6. Share → "Compartiendo…" → "Compartido" → copy link / open / QR (project titled,
   large).
7. Scan the QR from a phone; confirm the temporary-link note is visible.
8. "Dejar de compartir" → light confirm → "Dejaste de compartirlo".

## 26. Module / API changes

**No Rust changes.** Frontend only:

```
app/src/messages.ts            NEW   message catalog (ADR-0012)
app/src/guidance.ts            NEW   errorCode -> {title,message,actions}
app/src/labels.ts              EXTEND helpers fold into/behind the catalog
app/src/components/ui/         NEW   Dialog, EmptyState, Toast, Badge, ErrorNotice,
                                     ProviderStatusBanner, useFocusTrap
app/src/components/*.tsx       EXTEND consume catalog + shared primitives
app/src/styles.css             EXTEND tokens, spacing, typography, badges, responsive
app/src/App.tsx                EXTEND single live region + provider-status wiring
docs/UX.md                     UPDATE canonical vocabulary + sharing messaging
docs/VERIFY.md                 UPDATE M9 section + gate
scripts/verify                 EXTEND M9 gate ("M9 contract passed" on messages.ts)
```

`api.ts`, `types.ts`, and the entire Rust workspace are untouched. Dependency
direction is unchanged (UI → project-app facade only).

## 27. ADR(s)

- **ADR-0012** — User-facing copy as a single message catalog (i18n-ready, no
  framework). Accepted; governs T1 and all copy work.

No security ADR is needed (no invariant changes).

## 28. Task breakdown

| # | Task | Level | Depends | Worktree | Ownership |
| --- | --- | --- | --- | --- | --- |
| 0 | Design + ADR-0012 + vocabulary approval | HIGH_ARCHITECTURE | — | — | V4 Pro + Human |
| 1 | Message catalog + terminology refactor (replace inline strings) | MEDIUM | 0 | `m9/messages` | app/src/messages.ts, labels.ts, all component copy |
| 2 | Visual system + responsive CSS tokens | MEDIUM | 0 | `m9/visual-system` | app/src/styles.css |
| 3 | Shared UI primitives + a11y hooks + guidance | MEDIUM | 1, 2 | `m9/shared-primitives` | app/src/components/ui/**, app/src/guidance.ts |
| 4 | Projects view UX (first-run, empty state, default name, Ctrl+N) | MEDIUM | 3 | `m9/projects-ux` | ProjectsView.tsx + test |
| 5 | Chat/composer UX (multiline, Ctrl+Enter, provider empty state) | MEDIUM | 3 | `m9/chat-ux` | ChatPanel.tsx + test |
| 6 | Materials UX (grouping, summary, duplicates, chips) | MEDIUM | 3 | `m9/materials-ux` | MaterialsPanel.tsx + test |
| 7 | Creations UX (cards, status clarity, actions) | MEDIUM | 3 | `m9/creations-ux` | CreationsPanel.tsx + test |
| 8 | Sharing UX + QR + temporary-link messaging + stop confirm | MEDIUM | 3 | `m9/sharing-ux` | PublishPanel.tsx, QrDialog.tsx + tests |
| 9 | Cross-cutting a11y + keyboard + error-recovery integration | MEDIUM_HIGH | 3,4,5,6,7,8 | `m9/a11y-pass` | App.tsx, shared, guidance wiring, tests |
| 10 | Gate + docs + verify + checkpoint (DoD) | MEDIUM | 9 | main | docs, scripts/verify, checkpoint |

Tasks 1 and 2 are independent after approval (task 0). Task 3 builds the shared
primitives on top of them. Tasks 4-8 are independent of each other (separate
panel files) once 3 lands. Task 9 integrates and hardens; task 10 gates.

## 29. Reasoning level per task

0 HIGH_ARCHITECTURE · 1 MEDIUM · 2 MEDIUM · 3 MEDIUM · 4-8 MEDIUM ·
9 MEDIUM_HIGH · 10 MEDIUM.

## 30. Proposed worktrees

`../ai-publisher-m9-messages`, `-visual-system`, `-shared-primitives`,
`-projects-ux`, `-chat-ux`, `-materials-ux`, `-creations-ux`, `-sharing-ux`,
`-a11y-pass` (+ review per task). Integration checkout (`main`) is lead-only.
Because panels share `styles.css` and `messages.ts`, panel authors must not edit
those shared files after T1/T2 land; any token/copy needs are raised to the lead
rather than edited ad hoc.

## 31. Implementation model allocation

Orchestrator: `opencode-go/deepseek-v4-flash` (fresh session after approval).

| Task | Author | Reviewer |
| --- | --- | --- |
| 1 | Cursor Composer 2.5 | OpenCode Go DeepSeek V4 Flash |
| 2 | Cursor Composer 2.5 | OpenCode Go DeepSeek V4 Flash |
| 3 | OpenCode Go DeepSeek V4 Flash | OpenCode Go Qwen3.8 Max |
| 4 | Cursor Composer 2.5 | OpenCode Go DeepSeek V4 Flash |
| 5 | Cursor Composer 2.5 | OpenCode Go DeepSeek V4 Flash |
| 6 | OpenCode Go DeepSeek V4 Flash | Cursor Composer 2.5 |
| 7 | Cursor Composer 2.5 | OpenCode Go DeepSeek V4 Flash |
| 8 | OpenCode Go DeepSeek V4 Flash | Cursor Composer 2.5 |
| 9 | OpenCode Go DeepSeek V4 Flash | OpenCode Go Qwen3.8 Max |
| 10 | OpenCode Go DeepSeek V4 Flash (lead) | OpenCode Go Qwen3.8 Max |

Fallbacks per `AGENT_POLICY.md`; `MODEL_REQUESTED == MODEL_ACTUAL` enforced via
`scripts/agent-launch`.

## 32. Author / reviewer

Author ≠ reviewer, cross-family where practical. No task here is a
security-invariant change, so the "security-review task" rule does not apply;
T10 performs the single checklist confirmation that no capability/command changed.
T9 (cross-cutting accessibility) and T3 (shared primitives) get a second-family
reviewer (Qwen) because subtle a11y/state bugs concentrate there.

## 33. Risks / debt

- **Terminology churn** across PRODUCT.md/UX.md and UI copy: mitigated by the
  catalog being the single executable reflection and by the vocabulary being
  fixed before T1.
- **"Compartir vs Publicar"** resolved by the owner: `Compartir` is the canonical
  user-facing verb; `Publicar` is not a UI action. Internal identifiers
  (`Publication*`, `publish`, `unpublish`) are unchanged.
- **Provider-status nuance:** the free-model default means "no AI connected" is
  rarely a true blocker; the banner must not lie to a working user (§13).
- **WebKitGTK focus/announcement quirks:** focus trap and `aria-live` behavior
  need manual smoke confirmation (`scripts/smoke-ux`); component tests cannot
  fully verify screen-reader output.
- **No visual regression testing:** visual/responsive confidence relies on manual
  smoke; accepted for a polish milestone, revisited at M10.
- **Shared-file contention:** panels share `styles.css`/`messages.ts`; sequencing
  (T1/T2 first, panels read-only on those files) prevents merge conflicts.
- **Message-key churn:** renaming a catalog key breaks frontend + tests; keys are
  semantic and additive.

## 34. Definition of Done M9

- [ ] ADR-0012 accepted and `docs/UX.md` canonical vocabulary confirmed before code.
- [ ] All user-facing copy lives in `messages.ts`; no inline literals remain in components; forbidden technical terms absent from the catalog (asserted by test).
- [ ] First-run guide + empty states for projects/materials/creations/AI/sharing, each with a next action.
- [ ] Project create uses a default name, autofocus, Enter/Esc, `Ctrl/Cmd+N`.
- [ ] Composer: multiline auto-grow, `Ctrl/Cmd+Enter` send, attachment affordance, example placeholder.
- [ ] Materials: batch import summary + per-file details; duplicate/remove messaging; chips.
- [ ] Creations: clear share-status toggle; no IDs/revisions rendered.
- [ ] Sharing: "Compartir → Compartiendo → Compartido", temporary-link messaging, stop-sharing confirm, project-titled QR with copy/open nearby.
- [ ] Provider-status banner surfaces "Conectar IA" only when genuinely needed.
- [ ] Error recovery maps every `ErrorCode` to actionable guidance.
- [ ] Accessibility: unified dialog focus trap/Esc/return; live regions; visible focus; contrast ≥ 4.5:1; drag/drop picker alternative.
- [ ] `./scripts/verify` (M9 gate), `git diff --check`, all frontend tests green.
- [ ] No M9-excluded scope (packaging, provider/credential redesign, mobile, full i18n framework, telemetry, rich Markdown rendering, new backend).

## 35. scripts/verify incremental

M9 keeps M8 and changes only the frontend gate string and adds the catalog
existence check. No new Rust suites:

```bash
# ... existing M0-M8 checks unchanged ...

# Frontend (existing pnpm suite covers M9 components + catalog/guidance tests)
pnpm --dir app run test
cargo check --manifest-path app/src-tauri/Cargo.toml   # unchanged
git diff --check
```

The final gate becomes:

```bash
if [[ -f app/src/messages.ts ]]; then
  printf 'verify: M9 contract passed\n'
elif [[ -f crates/project-preview/tests/preview_security.rs ]]; then
  printf 'verify: M8 contract passed\n'
# ...
```

`docs/VERIFY.md` gains a short M9 section documenting that M9 is frontend-only
and that the gate discriminates on `app/src/messages.ts`.

## 36. Explicit next-milestone scope

M10 = Packaging. Linux AppImage then RPM; bundle/manage OpenCode and `cloudflared`
as pinned sidecars; native CI for future Windows. M9 does **not** begin packaging.
M9 defers: rich Markdown rendering, save/download-as, PDF in-app rendering,
office renderers, revision/regenerate UI, cloud telemetry, a full i18n
framework, and any persistent preview infrastructure.
