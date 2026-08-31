# UX_RELEASE_GATE_01 — Visual validation against the approved simple-chat direction

Status: **APPROVED by the human owner (2026-08-31) — no implementation done, no milestone
started. Decision record in §11.**
Date: 2026-08-31
Scope: Real EducAI frontend, reviewed visually per the approved chat-first product direction. **No M11 work was started.**

> Read this report with the evidence under `docs/ux-release-gate-01/` (screenshots, per-screenshot
> OCR text, and Chromium accessibility trees).

---

## 1. Purpose

Before any new milestone work, this gate visually inspects the **current** UI against the approved
simple-chat product direction and classifies deviations so the human owner can decide what the next
UX milestone must fix. This is a **validation gate, not an implementation milestone.**

---

## 2. Method and evidence

Because the current Tauri shell cannot be driven directly by Playwright, the frontend was run in the
**real Vite dev server** (`pnpm --dir app run dev`, unmodified `app/` source) with the **Tauri IPC
boundary mocked** (in-memory backend implementing the exact `api.ts` command surface, plus the
`agent://task` event stream and a simulated publish delay). Product source was **not** edited; the
harness is a page-injected `window.__TAURI_INTERNALS__` shim.

- Browser: Playwright Chromium (headed-capable); screenshots are the captured evidence.
- Viewports: **1366×768, 1440×900, 1920×1080**.
- Reviewer classification used the screenshots, per-screenshot **OCR text**, Chromium
  **accessibility trees**, and **element geometry/computed-style measurements** (the reviewing model
  cannot view pixels directly; the PNGs are provided for human confirmation).

Evidence index (all under `docs/ux-release-gate-01/`):

| Flow | Screenshot(s) | OCR / a11y |
| --- | --- | --- |
| 1 First launch | `01-first-launch-{1366x768,1440x900,1920x1080}.png` | `.ocr.txt`, `01-first-launch.a11y.txt` |
| 2 Conversation/project list | `02-projects-list-{3 viewports}.png` | `.ocr.txt` |
| 3 Create + open | `03-create-project-opened.png` | `.ocr.txt` |
| 4 Workspace (dashboard) | `04-workspace-{3 viewports}.png` | `.ocr.txt`, `04-workspace.a11y.txt` |
| 5 Send prompt (busy + done) | `05-send-working.png`, `05-send-completed.png` | `.ocr.txt` |
| 6 Model selector | `06-model-selector-closed.png`, `06-model-selector-option-selected.png` | `.ocr.txt` |
| 7 Provider disconnected | `07-disconnected-{3 viewports}.png`, `07-disconnected-workspace.png` | `.ocr.txt`, `07-disconnected.a11y.txt` |
| 8 Settings (provider dialog) | `08-settings.png`, `08-settings-on-list.png` | `.ocr.txt`, `08-settings.a11y.txt` |
| 9 Return from settings | `09-return-from-settings.png` | `.ocr.txt` |
| 10 Materials | (covered by `04-workspace`) | — |
| 11 Creations | (covered by `04-workspace`) | — |
| 12 Share (busy + shared) | `12-share-busy.png`, `12-share-shared.png` | `.ocr.txt`, `12-share-shared.a11y.txt` |
| 13 QR | `13-qr.png` | `.ocr.txt`, `13-qr.a11y.txt` |
| 14 Stop sharing (confirm + stopped) | `14-stop-confirm.png`, `14-stop-stopped.png` | `.ocr.txt` |
| 15 Rename conversation | `15-rename.png` | `.ocr.txt` |

Harness (not committed): `/tmp/opencode/ux-review/` (`mock-inject.js`, `capture.py`, `measure.py`).

---

## 3. Observed current UI

### 3.1 Global shell (every screen, including first launch)

A persistent **top app-bar** shows, from left to right:

- `EducAI` title
- **Model selector**: label `Modelo`, a `<select>` already pre-filled with the raw model id
  `big-pickle (Gratis)` under group `Recomendado`, a `Gratis` badge, and the caveat
  *"Los modelos gratis pueden cambiar con el tiempo."*
- `Conectá tu IA` button (opens the provider dialog)
- Below the bar, a `Modelo gratuito` status banner.

Measured (1440×900): the selector occupies **729×77 px** of the top bar (the whole right half).
This technical strip is the **first thing the user sees** on first launch, before any product content.

### 3.2 First launch

`01-first-launch` shows (top to bottom): the app-bar strip above, then the `Empezá con EducAI`
5-step guide card, then the empty state `Todavía no tenés proyectos` with `Crear proyecto`.
Three affordances for "create" coexist (header `Nuevo proyecto`, guide, empty-state button).

### 3.3 Conversation list (`Mis proyectos`)

`02-projects-list` is a separate screen (not a side panel). Rows are `name` + `Abrir` /
`Renombrar` / `Eliminar`. Newest-first ordering is honored by backend order, but **no timestamps or
shared-state indicator** are shown on the list. Concept and vocabulary are **"proyecto"**, not
conversation.

### 3.4 Workspace (a project) — the central screen

`04-workspace` is a **2×2 panel dashboard**, not a chat:

```
┌─────────────────────────────┬─────────────────────────────┐
│ Asistente  (chat panel)     │ Creaciones                 │
│  …chat-log…                 │  creation cards + switches │
│  ┌ prompt textarea ──────┐  │  + Abrir en navegador      │
│  └ Adjuntar material Enviar┘ │                            │
├─────────────────────────────┼─────────────────────────────┤
│ Materiales                 │ Compartir                  │
│  Agregar archivo / cards   │  [Compartir] or shared URL │
│                            │  / QR / Dejar de compartir │
└─────────────────────────────┴─────────────────────────────┘
```

Measured geometry (1440×900, viewport 900 px tall; content height **1089 px** — vertical scroll):

| Panel | x,y | w×h |
| --- | --- | --- |
| Asistente | 144,253 | 568×402 |
| Materiales | 144,671 | 568×394 |
| Creaciones | 728,253 | 568×402 |
| Compartir | 728,671 | 568×394 |

- Grid `1fr 1fr` (≥960 px); at ≥1280 px content is capped at **1200 px** and centered (at 1920×1080
  there are ~360 px empty side margins).
- **The prompt textarea sits mid-screen (y≈376, bottom≈466)** inside the chat panel — not pinned to
  the window bottom. The composer is one of four equal panels.
- **Chat history is not persistent**: user messages are client-only state. Verified: send a message,
  leave with `← Proyectos`, reopen → the chat is empty (0 user messages).
- **Generated creations do not appear after "ready"**: verified with a faithful snapshot backend —
  after `Creando tu recurso…` → `Listo.` + toast `Tu recurso está listo.`, the Creations panel still
  shows the old count until the project is reopened. The `registeredCreationIds` from the
  `agent://task` completion event are ignored by the UI.

### 3.5 Send prompt

`05-send-working`: user message chip + `Creando tu recurso…` + spinner. `05-send-completed`:
`Listo.` status + toast `Tu recurso está listo.` (Creations panel not updated — see 3.4).

### 3.6 Provider disconnected

`07-disconnected` (banner on every screen) and `07-disconnected-workspace` (inside a project):
- Banner: `No hay una IA conectada. Conectá tu cuenta para seguir creando.` + `Conectar IA`.
- Inside the chat panel: the **same message repeated** — an empty state `No hay una IA conectada` /
  `Conectar IA` **plus** a still-rendered composer with placeholder and a disabled `Enviar`.
- Model selector collapses to `Sin modelos`.

### 3.7 Settings (provider dialog)

`08-settings`: dialog titled `Conectá tu IA`, a `Cerrar` text button in the header (no **X**), the
privacy note, `Recomendados` → provider card `Gratis` with `Conectar`. This dialog **is** the whole
settings surface; there is no general Settings screen.

### 3.8 Return from settings

`09-return-from-settings`: closing the dialog returns to the exact prior workspace (conversation
state preserved while the panel is open). Functionally correct; the approved direction asks for a
close **X**.

### 3.9 Share / QR / stop sharing

- `12-share-busy`: `Compartiendo… puede tardar unos segundos`.
- `12-share-shared`: `Compartido` badge, the URL, `Copiar enlace` / `Abrir enlace` / `Mostrar QR` /
  `Dejar de compartir`, plus both temporary-link notes. Correct per M9/UX.md.
- `13-qr`: dialog **titled with the project name**, large QR, copy/open adjacent, `Cerrar`. Correct.
- `14-stop-confirm` / `14-stop-stopped`: light confirmation (`Cancelar`/`Confirmar`), then
  `Dejaste de compartirlo`. Correct per M9.

---

## 4. What already matches the approved direction (green)

- Newest conversations first (backend ordering) — **but** not labeled/dated in the UI.
- Conversations/projects can be renamed (inline `Renombrar` + `Guardar`).
- The default free model is auto-selected (`big-pickle (Gratis)` preselected; badge `Gratis`).
- Provider connection is optional and lives in a dialog that returns to the conversation.
- Sharing copy is non-technical and honest (temporary-link messaging, `Compartir` verb), QR is
  project-titled, stop-sharing has a light confirmation.
- No technical terms such as "tunnel", "server", "Cloudflare", "port", "OpenCode" leak into the
  workspace copy itself.

---

## 5. Deviations from the approved product direction

| Approved direction | Current UI | Deviation |
| --- | --- | --- |
| Chat-first; conversation is the center | 2×2 dashboard of 4 equal panels (`Asistente` is one of four) | **Major** — looks like a technical dashboard |
| LEFT: conversation list | List is a separate `Mis proyectos` screen; inside a project there is no list | **Major** |
| CENTER: current conversation | Chat panel is top-left quadrant; chat history is not restored on reopen | **Major** |
| BOTTOM: prompt + model selector + share | Prompt is mid-screen inside a panel; model selector is in the top app-bar; share is a separate panel | **Major** |
| SETTINGS: separate screen/dialog with close X | Provider dialog only; close is a `Cerrar` text button, not an X | Partial |
| RESOURCES in conversation context | Materials/Creations are separate dashboard panels | **Major** |
| Share visible but not dominant before content | `Compartir` is a full equal panel always shown, plus a per-creation `Se compartirá` switch → share controls exist in 2 places | **Major** |
| No technical terminology | Raw model id `big-pickle` and model-change caveat rendered in the top bar on every screen | Major |
| No unnecessary cards/panels | Four panels + badges + banners + notices | Major |
| Primary action obvious | Within the chat panel yes; the overall screen reads as "manage 4 panels" | Partial |

---

## 6. Findings by severity

### UX_BLOCKER

**B1 — The interface is dashboard-first, not chat-first.**
The workspace is a 2×2 grid of four equal panels (Asistente / Materiales / Creaciones / Compartir).
The conversation is not the center of the screen, there is no persistent left conversation list, the
prompt is not anchored at the bottom, and resources are not shown from within the conversation. This
fails the approved product direction at its core. Evidence: `04-workspace-*`, geometry §3.4.

**B2 — Technical model/provider surface dominates the default UI.**
On every screen, including first launch, the top strip shows the raw model id `big-pickle`, a
`Modelo` selector, `Conectá tu IA`, the `Modelo gratuito` banner and *"Los modelos gratis pueden
cambiar con el tiempo."* This is exactly the "technical power leaking into default UX" the product
philosophy forbids, and it precedes any product content for a first-time non-technical teacher.
Evidence: `01-first-launch-*`, `04-workspace-*`, OCR.

### UX_IMPORTANT

**I1 — Chat history is not persistent.** User messages are client-only; reopening a conversation
shows an empty chat (verified). A chat-first product must restore the conversation.
Evidence: probe (send → leave → reopen → 0 messages); `04-workspace` shows no history surface.

**I2 — Generated creations don't surface after completion.** `Tu recurso está listo.` appears, but
the Creations panel is stale until the project is reopened (verified with a faithful backend
snapshot). The completion event already carries `registeredCreationIds`, which the UI ignores.
Evidence: `05-send-completed`.

**I3 — No persistent left conversation list; navigation is a separate screen.**
Switching conversations means leaving the workspace to `Mis proyectos`.

**I4 — Resources are dashboard panels, not conversation context.** Materials and Creations are
side-by-side panels; the approved direction wants them anchored to the selected conversation.

**I5 — Share is over-exposed and duplicated before content exists.** The `Compartir` panel is equal
in weight to the chat on an empty project, and the per-creation `Se compartirá` switch adds a second
share surface. Share should be a single action near the composer, revealed when there is something to
share.

**I6 — Model selector placement and content.** It sits in the top app-bar (729 px wide) exposing an
internal id; per the direction it belongs near the prompt (bottom) or in Settings.

**I7 — Disconnected state repeats the same message 3×.** Banner + chat empty state + disabled
composer (`No hay una IA conectada` twice + `Sin modelos`). Evidence: `07-disconnected-workspace`.

### UX_POLISH

**P1 — Three "create" affordances on first launch** (header button, guide, empty state).
**P2 — `Los modelos gratis pueden cambiar con el tiempo.`** noise in the always-visible top bar.
**P3 — 1200 px content cap leaves ~360 px empty side margins at 1920×1080**; the workspace column is
wasted for a chat surface.
**P4 — Settings close is a text `Cerrar` button, not the approved **X**.
**P5 — The conversation list shows no shared/private indicator or timestamps**, so "newest first"
is invisible to the user.
**P6 — Provider dialog titled `Conectá tu IA`, not a Settings concept**, and there is no app
settings surface at all.

---

## 7. Visual review criteria — answers

| Question | Answer |
| --- | --- |
| First thing the user sees? | A technical model/provider strip (B2), not a conversation. |
| Primary action obvious? | `Enviar` is clear inside the chat panel, but the screen overall reads as a 4-panel manager. |
| Too much information? | Yes — 4 panels + banner + badges + caveat notices on a 900 px-tall window (content 1089 px). |
| Anything repeated? | Disconnected message ×3; `Se compartirá` switch + `Compartir` panel; 3× "create" on first launch. |
| Chat or dashboard? | Dashboard. |
| Technical states overexposed? | Yes — model id, `Modelo` selector, caveat, `Conectá tu IA` on default screens. |
| Hierarchy clear? | No — the app-bar strip competes with the page heading; 4 panels are co-equal. |
| Controls where users expect them? | No — model selector top-right (expected near prompt), composer mid-screen (expected bottom), share in a panel (expected as one action). |
| First-time user understands? | Mostly yes via the guide, but the surrounding technical strip dilutes it. |
| Matches the approved mockup direction? | No — the primary direction (LEFT list / CENTER chat / BOTTOM composer+model+share) is not implemented. |

---

## 8. Recommended target layout (frontend-only)

```
┌─────────────┬──────────────────────────────────────────────┐
│  Conversaciones        │  <conversación actual>            │
│  [+ Nueva conversación]│   ┌────────────────────────────┐  │
│  · Fracciones (newest) │   │  user message              │  │
│  · Sistema solar       │   │  "Tu recurso está listo."  │  │
│  · Fotosíntesis        │   │   [creation card: Abrir /  │  │
│    (compartida)        │   │    Vista previa / Compartir]│  │
│                        │   └────────────────────────────┘  │
│  [renombrar/eliminar]  │   ┌────────────────────────────┐  │
│                        │   │  Materiales (en contexto)  │  │
│                        │   │  chips/accordion           │  │
│                        │   └────────────────────────────┘  │
│                        │                                   │
│  ⚙ (settings)          │  ┌────────────────────────────┐  │
│                        │  │ [prompt…]  [Modelo ▾] [Compartir] │
│                        │  └────────────────────────────┘  │
└─────────────┴──────────────────────────────────────────────┘
```

- Left column: persistent conversation list, newest first, inline rename, shared indicator.
- Center: the active conversation; creations appear **inline** as chat messages; materials shown in
  context (collapsible).
- Bottom bar (fixed): prompt + compact model selector + a single Share action/status.
- Settings: a real settings surface (gear) with a close **X**, returning to the same conversation;
  provider connection lives there.
- Free model: auto-selected and shown as a subtle `Gratis` badge only; the raw model id and caveat
  move out of default view.

---

## 9. Exact frontend-only changes needed (for the future UX milestone)

All within `app/src/`; **no Rust, no Tauri commands/capabilities, no backend.**

1. **App shell** (`App.tsx` + new `ConversationsSidebar.tsx`): persistent left sidebar; remove the
   top model selector from the app-bar; make the workspace a single conversation column + fixed
   bottom composer bar. Keep `ProjectsView`/list data as the source for the sidebar.
2. **Composer relocation** (`ChatPanel.tsx` / new `ComposerBar.tsx`): pin prompt at window bottom,
   `Ctrl+Enter` send, attach-material affordance, `Cancelar` while working.
3. **Model selector** (`ModelSelector.tsx`): compact control in the bottom bar (or Settings);
   suppress the raw id (`big-pickle`) and the "cambian con el tiempo" caveat in default view; keep
   `Gratis` badge.
4. **Resources in conversation context** (`MaterialsPanel.tsx`, `CreationsPanel.tsx` re-layout):
   render materials as in-context chips/accordion and creations as inline chat messages; reuse
   existing `creation` cards/actions and `preview`/`creation_open` commands unchanged.
5. **Creations appear on completion** (App.tsx event handler): on `agent://task` completed, use
   `registeredCreationIds` to render the new creation inline (no backend change).
6. **Share as a single action** (`PublishPanel.tsx` → compact bar control/dialog): one `Compartir`
   action + status; keep the existing temporary-link messaging, QR dialog, and stop-sharing
   confirmation intact.
7. **Settings surface** (`ProviderPanel.tsx` + new wrapper): gear entry, dialog with close **X**
   returning to the previous conversation; provider list unchanged inside.
8. **Disconnected-state de-duplication** (`ChatPanel.tsx` / `ProviderStatusBanner.tsx`): one clear
   message + one `Conectar IA` action; don't render a disabled composer alongside it.
9. **Copy/simplification** (`messages.ts`): keep all canonical Spanish copy; only reduce redundant
   notices; no terminology regressions (catalog tests must stay green).
10. **Conversation restore**: reuse existing messages/workspace data if the backend already stores
    conversation text (needs a read-only check); otherwise defer — see §11 decision D1.

---

## 10. Areas that must remain untouched

- **Backend/Rust workspace** (`crates/`, `app/src-tauri/`): no new commands, capabilities, or
  windows; `api.ts` command names unchanged.
- **Security invariants**: publisher/tunnel/provider/credential behavior, preview isolation, publish
  tree exposure — all unchanged.
- **Sharing semantics & copy**: temporary-link honesty, `Compartir` verb, stop-sharing confirmation,
  project-titled QR — already correct (M9) and reused as-is.
- **The catalog contract** (`messages.ts`) and its tests: copy changes only, additive.
- **Packaging (M10) artifacts and `scripts/verify`**: untouched.
- **No M11 scope** (component updates, Windows CI): not started.

---

## 11. Decision record — resolved by the human owner (2026-08-31)

**UX_RELEASE_GATE_01 = APPROVED.** The observed current 2×2 dashboard UI is **not** the desired
target UI. The approved product direction is the simple chat-first interface documented by this
gate and the product decisions below (LEFT conversation list / CENTER current conversation /
BOTTOM prompt + model + Compartir / SETTINGS separate / RESOURCES in-context).

- **D1 — Conversation persistence: YES.** Conversation history must survive application restart.
  **No persistence mechanism (localStorage or otherwise) is chosen or implemented in this
  session.** Restoring history requires a **bounded architecture decision** (backend persistence
  vs. client-side store) that must be written and accepted before any implementation. A
  frontend-only restore is only viable if a read-only backend check confirms the conversation text
  is already stored.
- **D2 — Vocabulary:** "Conversación" is the primary user-facing container concept. The internal
  `Project` / `ProjectId` domain model remains unchanged unless the architecture review proves a
  minimal additive change is necessary.
- **D3 — Share prominence:** "Compartir" is an action in the bottom conversation action area. The
  permanent `Compartir` dashboard panel is **not** part of the target UI.
- **D4 — Gate:** approved; see the status header and the approval record below.

### Approval record

| Item | Resolution |
| --- | --- |
| UX_RELEASE_GATE_01 | **APPROVED** |
| B1 / B2 | **Must be fixed before release** (B1 dashboard-first UI; B2 technical model/provider surface) |
| D1 | Durable conversation persistence **required**; bounded architecture decision required before implementation |
| D2 | **Conversación** user-facing / **Project** internal |
| D3 | **Compartir** in the bottom action bar |
| Next step | Bounded architecture delta for the chat-first UX (design/acceptance; no code in this session) |
| M11 | **NOT started** |

### Mandated follow-up (design only; not started in this session)

1. Produce the bounded architecture delta that enables the chat-first UX, covering at least the
   D1 persistence decision (backend vs. client-side storage) and the D2 vocabulary mapping
   (Conversación user-facing ↔ Project/ProjectId internal). The delta must be approved by the
   owner before the UX milestone is planned.
2. The UX milestone (post-M11) must resolve B1/B2 and the §9 frontend-only change list. B1/B2 are
   release blockers regardless of milestone order.

---

## 12. Review basis

- Source of truth read: `AGENTS.md`, `docs/AGENT_POLICY.md`, `docs/CURRENT_CHECKPOINT.md`,
  `docs/M9_DESIGN.md`, `docs/UX.md`, `docs/PRODUCT.md`, `CODEX_HANDOFF.md`.
- **No approved mockup/wireframe file exists in the repository**; the reference direction is the one
  stated in this gate (LEFT list / CENTER conversation / BOTTOM prompt+model+share / SETTINGS
  separate / RESOURCES in-context), cross-checked against `docs/UX.md` and `docs/M9_DESIGN.md`.
- All findings were reproduced against the running real frontend (mocked backend boundary), not by
  source inspection alone.