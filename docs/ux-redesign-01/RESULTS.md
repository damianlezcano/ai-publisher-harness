# UX_REDESIGN_01 — Playwright headed visual + a11y gate results

Branch: `main` (Task G integrated, merge `2451c50`; fresh T6 validation run)
App URL: `http://localhost:1420/`
Date: 2026-09-01

## Summary

Fresh T6 run against **current main `2451c50`** (Task G integrated). All 17 flows were captured at
the three mandated viewports (1366×768, 1440×900, 1920×1080) with PNG, OCR and a11y-tree evidence.
The layout-invariant measure script passed at all three viewports. The 14 original UX_REDESIGN_01
flows were updated where Task G/F changed the DOM (rename via the "…" conversation menu;
attachments as `attachment-chip` with `Abrir`; share menu auto-opens after publish) and three new
Task G/F-focused flows were added: **15 creation-actions** (Creation card shows Abrir/Compartir and
Abrir opens without error), **16 delete-confirm** (type-name-to-confirm delete flow with plain
language), **17 no-duplicate-assistant** (assistant content renders exactly once, no raw green
duplicate). No UX blockers or UX_IMPORTANT findings were found. This is the T6 headed validation
gate evidence; the AppImage (T7) and human acceptance remain pending.

| Check | Result |
| --- | --- |
| App reachable at :1420 | PASS |
| All 17 flows captured at 3 viewports | PASS |
| PNG evidence | 78 files |
| `.ocr.txt` per PNG (all non-empty) | 78 files |
| `.a11y.txt` per flow | 17 files |
| Layout measure invariants | PASS (3 viewports) |
| `capture.py` assertions | PASS (57/57) |

## Note on screenshot dimensions

The new chat-first shell uses `height: 100vh` with internal scrolling containers (`conversation-main`, `workspace-timeline`). Playwright `full_page=True` therefore captures the viewport (e.g. 1440×900) rather than an expanded document. This is expected for the new layout and is consistent across all viewports.

## Note on a11y trees

One accessibility tree is captured per flow at the default viewport (1440×900), since a11y content is viewport-independent. Multi-state flows capture the a11y tree for the representative final/dialog state.

## Note on QR OCR

The QR dialog image is dominated by the QR code, which tesseract reads as empty. For the three `08-qr-<vp>.png` files, `capture.py` writes a fallback `.ocr.txt` containing the DOM-extracted dialog title and button labels so every PNG has a non-empty OCR file.

## Per-flow assertion matrix

All flows were captured at **1366×768, 1440×900, 1920×1080**.

### 01 — First launch

**Evidence:** `01-first-launch-{1366x768,1440x900,1920x1080}.png/.ocr.txt`; `01-first-launch.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| One conversation open automatically | PASS | `01-first-launch-1440x900.ocr.txt` shows "Nueva conversación" selected |
| Sidebar titled "Conversaciones" | PASS | a11y `[heading] Conversaciones` |
| Free model "Gratis" badge shown | PASS | a11y option `big-pickle (Gratis)` + OCR "Gratis" |
| No raw `::` id text visible | PASS | a11y contains no "::" |
| No 2×2 grid | PASS | `.workspace-grid` count == 0 |
| No top-bar model selector | PASS | `.app-shell-header .model-selector` count == 0 |
| No "Conectá tu IA" button | PASS | No "Conectá tu IA" in page content |
| No "Mis proyectos" text | PASS | No "Mis proyectos" in page content |

### 02 — Conversation list

**Evidence:** `02-conversation-list-{1366x768,1440x900,1920x1080}.png/.ocr.txt`; `02-conversation-list.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Newest-first order | PASS | Sidebar names = `["Fracciones", "Sistema solar", "Fotosíntesis"]` |
| Timestamps present | PASS | 3 `.conversation-timestamp` elements |
| Shared badge on shared project | PASS | OCR "Fotosíntesis … Compartido"; a11y `Fotosíntesis Compartido …` |

### 03 — Rename

**Evidence:** `03-rename-saved-{vp}.png/.ocr.txt`, `03-rename-cancelled-{vp}.png/.ocr.txt`; `03-rename-saved.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Inline rename "Sistema solar" → "El sistema solar" via Guardar | PASS | OCR shows "El sistema solar" after save |
| Esc cancels rename | PASS | Cancelled screenshot still shows "Sistema solar" |

### 04 — Send prompt

**Evidence:** `04-send-working-{vp}.png/.ocr.txt`, `04-send-completed-{vp}.png/.ocr.txt`; `04-send-completed.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Working state shows "Creando tu recurso…" | PASS | DOM `.chat-status` text = "Creando tu recurso…" |
| User message bubble appears | PASS | Prompt text found in `.message-user .message-text` |
| Assistant message + inline creation card appear without reopening conversation | PASS | Messages: 2 → 4; creation cards: 1 → 2; no page navigation |

### 05 — Resources in conversation context

**Evidence:** `05-resources-{vp}.png/.ocr.txt`; `05-resources.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Material chips on user message | PASS | Chips include `manual.pdf` and `esquema-fotosíntesis.png` |
| Inline creation card on assistant message | PASS | `.message-assistant .creation-card` count ≥ 1 |
| Unattached "Materiales" lists only unreferenced materials | PASS | Unattached list contains `diapo.pptx`; `manual.pdf` not duplicated |

### 06 — Settings

**Evidence:** `06-settings-open-{vp}.png/.ocr.txt`, `06-settings-closed-{vp}.png/.ocr.txt`; `06-settings-open.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Gear opens dialog titled "Configuración" | PASS | `.provider-dialog h2` = "Configuración"; a11y `[dialog] Configuración` |
| Close X has aria-label "Cerrar" | PASS | a11y `[button] Cerrar` |
| Exact same conversation shown after close | PASS | Header before/after = "Fotosíntesis" |

### 07 — Share

**Evidence:** `07-share-busy-{vp}.png/.ocr.txt`, `07-share-shared-{vp}.png/.ocr.txt`, `07-share-menu-{vp}.png/.ocr.txt`; `07-share-menu.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Click "Compartir" → busy "Compartiendo…" | PASS | Trigger text contains "Compartiendo" |
| Busy resolves to shared "Compartido" | PASS | Trigger text contains "Compartido" |
| Menu has Copiar enlace / Abrir enlace / Mostrar QR / Dejar de compartir | PASS | a11y shows `[menuitem] Copiar enlace`, `[menuitem] Abrir enlace`, `[menuitem] Mostrar QR`, `[menuitem] Dejar de compartir` |

### 08 — QR dialog

**Evidence:** `08-qr-{vp}.png/.ocr.txt`; `08-qr.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| "Mostrar QR" opens QR dialog | PASS | `.qr` element visible; dialog rendered |
| QR dialog has copy/open links | PASS | DOM buttons include `Copiar enlace` and `Abrir enlace` |

### 09 — Stop sharing

**Evidence:** `09-stop-confirm-{vp}.png/.ocr.txt`, `09-stop-stopped-{vp}.png/.ocr.txt`; `09-stop-confirm.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| "Dejar de compartir" opens confirmation | PASS | `.dialog h2` contains "Dejar de compartir" |
| Confirm returns to "Compartir" state | PASS | Trigger text = "Compartir", no longer "Compartido" |

### 10 — Restart persistence (D1)

**Evidence:** `10-restart-before-{vp}.png/.ocr.txt`, `10-restart-after-{vp}.png/.ocr.txt`; `10-restart-after.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| After send, messages persisted | PASS | Before reload: 4 messages |
| After page.reload(), conversation reopens | PASS | After reload: 4 messages; title = "Fotosíntesis" |
| Both user and assistant messages restored | PASS | Message count unchanged; user + assistant bubbles present |

### 11 — Drag-over overlay

**Evidence:** `11-drag-over-{1366x768,1440x900,1920x1080}.png/.ocr.txt`; `11-drag-over.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Drop overlay "Soltá los archivos acá" appears only while dragging over | PASS | `.drop-overlay` present on drag-over; OCR shows "Soltá los archivos acá" |
| Overlay is a live region announced to AT | PASS | `[region] Asistente` + overlay rendered during drag-over only |

### 12 — Dropped resource

**Evidence:** `12-drop-resource-{1366x768,1440x900,1920x1080}.png/.ocr.txt`; `12-drop-resource.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Overlay dismissed on drop | PASS | `.drop-overlay` count == 0 after drop |
| Dropped files appear as conversation resources | PASS | "receta.pdf" and "hoja.png" in body; import summary "2 agregados" |

### 13 — Compact attachment interaction

**Evidence:** `13-attach-closed-{vp}.png/.ocr.txt`, `13-attach-menu-{vp}.png/.ocr.txt`; `13-attach-menu.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Single compact paperclip opens the attach menu | PASS | `.composer-attach-menu` opens on click; a11y `[button] Adjuntar` |
| Menu offers "Agregar archivo" | PASS | OCR "Agregar archivo" |
| Menu lists existing materials as chips | PASS | manual.pdf, esquema-fotosíntesis.png, diapo.pptx chips in menu |

### 14 — Model selector

**Evidence:** `14-model-selector-{1366x768,1440x900,1920x1080}.png/.ocr.txt`; `14-model-selector.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Compact model selector in composer | PASS | `#composer-model-select` present |
| Free model shown with "Gratis" suffix | PASS | option "big-pickle / Gratis" |
| No raw `::` id text in selector | PASS | a11y/OCR contain no "::" |
| Selector labelled "Modelo" | PASS | `[combobox] Modelo` |

### 15 — Creation card actions (Task G)

**Evidence:** `15-creation-actions-{vp}.png/.ocr.txt`; `15-creation-actions.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Assistant message shows an inline creation card | PASS | `.message-assistant .creation-card` ≥ 1 |
| Creation card has "Abrir" | PASS | card text contains Abrir |
| Creation card has "Compartir" | PASS | card text contains Compartir |
| Abrir on a web creation surfaces no error | PASS | no `.error` in card after click |

### 16 — Delete conversation confirmation (Task F)

**Evidence:** `16-delete-confirm-{vp}.png/.ocr.txt`, `16-delete-confirmed-{vp}.png/.ocr.txt`; `16-delete-confirm.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| "…" menu → "Eliminar conversación" opens confirmation | PASS | dialog rendered |
| Confirm title "¿Eliminar esta conversación?" | PASS | `.dialog h2` |
| Confirm body is plain language (mensajes + recursos) | PASS | `.dialog p` |
| Eliminar disabled before typing the name | PASS | button disabled |
| Eliminar enabled after typing the conversation name | PASS | button enabled |
| Deleted conversation removed from sidebar | PASS | name absent from list |

### 17 — No duplicate assistant rendering (Task E)

**Evidence:** `17-no-duplicate-assistant-{vp}.png/.ocr.txt`; `17-no-duplicate-assistant.a11y.txt`

| Assertion | Result | Evidence |
| --- | --- | --- |
| Assistant content renders in one bubble | PASS | `.message-assistant` == 1 |
| Assistant text renders exactly once | PASS | `.message-assistant .message-text` == 1 |
| No raw green duplicate status | PASS | `.chat-status.ok` == 0 |
| Assistant text is plain (no raw tooling output) | PASS | text = "Listo." |

## Layout measure invariants

Seed: `workspace` at all three viewports.

| Viewport | workspace-grid | header model-selector | sidebar | composer bottom | composer-model-select | share-control | horizontal overflow |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1366×768 | absent | absent | present | 768 ≤ 768 | present | present | none |
| 1440×900 | absent | absent | present | 900 ≤ 900 | present | present | none |
| 1920×1080 | absent | absent | present | 1080 ≤ 1080 | present | present | none |

## Findings

- UX_BLOCKER: none.
- UX_IMPORTANT: none.
- UX_POLISH (pre-approved; not blocking this pass):
  - Model option in the composer selector renders the mock model ID ("big-pickle") as its display
    name — a mock-data artifact (the real catalog provides human display names); verified not a
    real-app regression.
  - `composer-model-select` resting contrast and `.conversation-name` truncation tooltip remain the
    known A11Y_POLISH items from the prior review (already classified out-of-scope polish).
- Harness-only notes (no product change):
  - Flows 03/05/07 selectors were updated to the Task G/F DOM (menu-based rename,
    `attachment-chip`, share menu auto-opens after publish). `mock-inject.js` `app_status`
    returns `agent: "ready"` to match the Task C readiness contract (composer enabled only when
    backend is ready).
- T6 headed validation of Task G behavior: chat-first layout, sidebar, composer primary, rename
  via "…" menu, delete confirmation UX, Creation card Abrir/Compartir, share/QR/stop, attachment
  presentation, settings X return, restart persistence, drag/drop, no duplicate assistant render —
  all PASS. This gate does NOT exercise a real AI/network path (mocked Tauri IPC boundary); the
  real-provider assistant response and the full human acceptance scenario remain for the product
  owner / T7.

## Reproduction

```bash
./docs/ux-redesign-01/harness/run.sh
```

The script checks `http://localhost:1420/`, runs `capture.py` (headed), `measure.py`, and `ocr.py`, and exits non-zero on any failure.
