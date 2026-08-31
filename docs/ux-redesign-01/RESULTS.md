# UX_REDESIGN_01 — Playwright headed visual + a11y gate results

Branch: `m-ux/t6-playwright`
App URL: `http://localhost:1420/`
Date: 2026-08-31

## Summary

All 10 required flows were captured at the three mandated viewports (1366×768, 1440×900, 1920×1080) with PNG, OCR and a11y-tree evidence. The layout-invariant measure script passed at all three viewports. No UX blockers, important or minor regressions were found in the redesigned UI.

| Check | Result |
| --- | --- |
| App reachable at :1420 | PASS |
| All 10 flows captured at 3 viewports | PASS |
| PNG evidence | 51 files |
| `.ocr.txt` per PNG (all non-empty) | 51 files |
| `.a11y.txt` per flow | 10 files |
| Layout measure invariants | PASS (3 viewports) |
| `capture.py` assertions | PASS (33/33) |

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

## Layout measure invariants

Seed: `workspace` at all three viewports.

| Viewport | workspace-grid | header model-selector | sidebar | composer bottom | composer-model-select | share-control | horizontal overflow |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1366×768 | absent | absent | present | 768 ≤ 768 | present | present | none |
| 1440×900 | absent | absent | present | 900 ≤ 900 | present | present | none |
| 1920×1080 | absent | absent | present | 1080 ≤ 1080 | present | present | none |

## Findings

No findings. All assertions and invariants passed.

## Reproduction

```bash
./docs/ux-redesign-01/harness/run.sh
```

The script checks `http://localhost:1420/`, runs `capture.py` (headed), `measure.py`, and `ocr.py`, and exits non-zero on any failure.
