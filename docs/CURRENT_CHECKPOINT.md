# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual (UX_REDESIGN_01 — PRODUCT CORRECTION PASS, INTEGRADO Y LISTO PARA REVISIÓN HUMANA, 2026-09-01)

- **Current main commit: `32a069b`.** `git log --oneline -8` para el detalle.
- **TASK A (modelo gratis real) — INTEGRADA, revisada, APPROVE.** Ver sección
  "TASK A — INTEGRADO" abajo.
- **TASK B (visual) — INTEGRADA en `main` tras resolver los 2 CODE_IMPORTANT**
  del re-review Qwen Flash. La corrección (`abd41bc`) y la revisión
  (`qwen-frontend-rereview-abd41bc.md` → **APPROVE**) están en `main`.
- **Playwright headed — CORRIDO (14 flujos, 3 viewports, 44/44 aserciones
  PASS).** Evidencia regenerada en `docs/ux-redesign-01/` (66 PNG + 66 OCR +
  14 a11y).
- **AppImage real — CONSTRUIDO y lanzado.** Respuesta real del modelo gratis
  confirmada: "Hola" → "¡Hola! ¿Cómo puedo ayudarte?" con `big-pickle`
  (opencode, cost 0), OpenCode 1.18.25 bundled.
- **PENDIENTE: aprobación VISUAL HUMANA del producto.** El AppImage quedó
  construido y lanzado; el owner debe inspeccionarlo. NO cerrar la aceptación
  visual final automáticamente.
- **M11 NO iniciado.**

## Integración en `main` (esta sesión)

| Commit | Contenido |
| --- | --- |
| `32a069b` | docs(ux): gate Playwright headed extendido a 14 flujos (drag-over, drop, attach, model selector) + evidencia regenerada; 44/44 PASS. |
| `88fd346` | merge de `uxfix/visual` en `main` (Task B completa: `b7d75be` + fix `abd41bc`). |
| `6dd3cec` | docs(ux): re-review Qwen3.8 Flash **APPROVE** de los 2 CODE_IMPORTANT (evidencia persistida). |
| `abd41bc` | fix(ux): restaurar errores de add-file en ComposerBar + eliminar panel Materiales muerto (autor Cursor Grok 4.6 High). |
| `66d15c4` | checkpoint previo (Task A integrada; Task B pendiente de fix). |

## TASK A — INTEGRADO (funcional, modelo gratis real)

| Commit | Contenido |
| --- | --- |
| `a3ef122` | merge de `uxfix/functional` en `main` (Task A completa). |
| `030d788` | fix(agent,provider,fake): señal terminal real `/session/status` + watermark + tests de scoping (rework de REQUEST_CHANGES). |
| `6735bd3` | fix(agent,provider,fake): polling real `/session/status` + `/session/{id}/message`. |

Causa raíz confirmada contra el sidecar real 1.18.25: `GET /session/status` es
la señal real (mapa vacío `{}` = idle/completed); el texto del asistente se
obtiene de `GET /session/{id}/message`. No se hardcodea ningún modelo
(`big-pickle` solo en tests/fake/catálogo).

## TASK B — INTEGRADO (visual, chat-first compacto)

- Autor visual: **Cursor Grok 4.6 High** (`b7d75be` en `uxfix/visual`), revisión
  visual Grok **APPROVE**.
- Re-review de código/a11y: **`opencode-go/qwen3.8-flash` fresh** → inicialmente
  **REQUEST_CHANGES** con 2 CODE_IMPORTANT; corregidos por un autor visual
  FRESH (`cursor-grok-4.6-high`, commit `abd41bc`) y re-revisados por
  `opencode-go/qwen3.8-flash` fresh → **APPROVE**
  (`docs/ux-redesign-01/reviews/qwen-frontend-rereview-abd41bc.md`).
- **Los 2 CODE_IMPORTANT y sus fixes:**
  1. `ComposerBar.pickFile()` sin try/catch → unhandled rejection sin feedback.
     Fix: try/catch + estado `pickError` + `<ErrorNotice>` visible (lenguaje
     llano, sin código/ruta). Test nuevo en `ComposerBar.test.tsx`.
  2. Código muerto tras quitar el panel Materiales: `MaterialsPanel` default +
     `MaterialItem` (único caller de `api.materialRemove`) + `importDetailLabel`
     duplicado. Fix: eliminar componente muerto (conservar `MaterialChip`),
     borrar `MaterialsPanel.test.tsx`, conservar contrato `api.materialRemove`
     y el import de `ChatPanel`.

## Playwright headed (14 flujos, 3 viewports)

- 1366×768, 1440×900, 1920×1080. `capture.py` 44/44 aserciones, `measure.py`
  PASS, `ocr.py` 66 imágenes.
- Flujos requeridos validados: 01 first-launch, 02 conversación activa, 03
  drag-over, 04 recurso dropeado, 05 interacción adjuntar compacto, 06 selector
  de modelo, 07 Settings open, 08 Settings X restaura misma conversación, 09
  sharing/QR, 10 respuesta exitosa del asistente (+ flujos de regresión 11-14
  y rename/resources originales).
- Clasificación UX: **UX_BLOCKER: ninguno. UX_IMPORTANT: ninguno. UX_POLISH:**
  nombre de modelo en el selector con ID del mock (artefacto de datos), y los
  polish A11Y ya conocidos (contraste resting de `composer-model-select`,
  tooltip de truncación). Evidencia: `docs/ux-redesign-01/*.png/.ocr.txt/.a11y.txt`.

## AppImage real (aceptación funcional real)

- Build: `./scripts/smoke-package appimage` con el workaround M10 aprobado
  (linuxdeploy de Fedora 44 falla → appimagetool directo). **PASS.**
- Ruta exacta: `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`
- Tamaño: 180.816.376 bytes (173M). SHA-256:
  `423cdb2815f23a01fa5e421c3203f47aea647cce3c1ea716fad1732e172fc8e2`
- Sidecars bundled verificados en el payload: `usr/bin/opencode` 1.18.25
  (sha256 `d91e0d33...`) y `usr/bin/cloudflared` 2026.8.3 (sha256
  `f29324fe...`, coincide con el manifest). `fetch-sidecars` reconciliado: los
  checksums del manifest (`opencode` es checksum del tar.gz) son correctos y la
  redescarga es determinista.
- **PATH-independencia:** `resolve_sidecar` resuelve primero
  `install_dir/<name>` y `install_dir/<name>-<triple>` (bundled en `usr/bin`
  del AppImage) antes de caer a `PATH`; confirmado en
  `crates/project-app/src/sidecar.rs` y `app/src-tauri/src/lib.rs`.
- **Lanzamiento gráfico real:** el AppImage corre (procesos `educai` +
  `WebKitNetworkProcess` + `WebKitWebProcess` estables; backend bundled
  `opencode serve` en puerto efímero, log "[agent] backend ready"). La sesión
  gráfica es headless (mutter sin superficie capturable): xdotool solo ve
  "mutter guard window" y las capturas de pantalla devuelven superficie vacía.
  El render del WebView no es capturable en esta sesión; la evidencia visual
  completa queda cubierta por el gate Playwright headed en
  `docs/ux-redesign-01/`.
- **Conversación real del modelo gratis (aceptación final):**
  - Sesión auto (sin modelo explícito): `POST /session` → `POST
    /session/{id}/prompt_async` con `{"parts":[{"type":"text","text":"Hola"}]}`
    → polling `GET /session/status` → `{}` → `GET /session/{id}/message`:
    `[user] Hola` / `[assistant] ¡Hola! ¿Cómo puedo ayudarte?`
  - Modelo usado: **`big-pickle` (providerID `opencode`)**, cost 0, OpenCode
    1.18.25 bundled. Sin provider de pago configurado (`/api/integration`
    `connections: []`; único provider OpenCode Zen `apiKey:"public"`).
  - Modelo explícito `ling-3.0-flash-fin-free` también respondió ("¡Hola! How
    can I help you today?", cost 0) tras un retry transitorio del upstream.

## Verificación final (esta sesión)

- `./scripts/verify` → **exit 0** (fmt, clippy, cargo test, frontend
  format/lint/typecheck/test 163, sidecars, M10, UX_REDESIGN_01 gate, tauri
  check). El drift preexistente de clippy (1.98.0 vs pin 1.97.1) NO se
  manifestó en esta corrida.
- `git diff --check` → limpio. `git status --short` → vacío (main limpio).

## Worktrees

- `main` → `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-harness`
  (integración, commit `32a069b`).
- `uxfix/functional` → `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-uxfix-functional`
  (commit `030d788`, integrado; worktree puede removerse).
- `uxfix/visual` → `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-uxfix-visual`
  (commit `abd41bc` sobre `b7d75be`, integrado; worktree puede removerse).
  Queda `app/package-lock.json` sin trackear en ese worktree (higiene conocida
  del review; el proyecto usa pnpm y el lockfile no está en el commit).

## Pines de sidecar (M10, `config/components.json` / ADR-0013)

- `opencode` 1.18.25, `cloudflared` 2026.8.3 (SHA-256 commiteados). Sin cambios
  de pin. Binarios presentes y reconciliados con el manifest.

## Model allocation (sesión de corrección cerrada)

- Orquestadora: `opencode-go/deepseek-v4-flash` (esta sesión, pane `w1F:p1`).
- TASK B fix author: `cursor-grok-4.6-high` (sesión `grok-visual-fix`, fresh,
  MODEL_ACTUAL confirmado; cerrada tras handoff PASS).
- TASK B fix reviewer: `opencode-go/qwen3.8-flash` (sesión
  `qwen-visual-fix-review`, fresh, MODEL_ACTUAL confirmado; cerrada tras
  APPROVE).
- **Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0 sesiones.**
- Budget: CONTINUE durante toda la sesión (18K al inicio, ~66K al lanzar
  reviewer; sin rotación requerida).

## Próximo paso (producto)

1. **Revisión humana del AppImage** (`app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`):
   lanzar y completar el flujo M9 (crear conversación → pedir a la IA → ver
   respuesta real del modelo gratis → compartir/QR). Aprobación visual final NO
   automática.
2. Si la revisión humana encuentra UX_BLOCKER/UX_IMPORTANT: nueva sesión de
   corrección con el mismo circuito (autor visual → reviewer qwen).
3. M11 NO iniciado.