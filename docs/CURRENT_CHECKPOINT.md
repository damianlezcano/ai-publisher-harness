# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual (UX_REDESIGN_01 — PRODUCT CORRECTION PASS, PARADA POR ROTACIÓN 2, 2026-08-31)

- **Current main commit: `a3ef122`.** `git log --oneline -8` para el detalle.
- **TASK A (modelo gratis real) — INTEGRADA EN `main`, revisada, APPROVE.**
  Ver sección "TASK A — INTEGRADO" abajo.
- **TASK B (visual) — commit de autor `b7d75be` en `uxfix/visual`, revisión
  visual Grok APPROVE, revisión de código Qwen Flash REQUEST_CHANGES (2
  CODE_IMPORTANT).** La corrección NO se lanzó: la orquestadora alcanzó
  ROTATE_SESSION_REQUIRED y el gate `scripts/agent-launch` falló cerrado. NO
  integrar `b7d75be` a `main` hasta resolver los 2 CODE_IMPORTANT.
- **Playwright headed, AppImage real, y aceptación final: NO corridos.**
  M11 NO iniciado.

## TASK A — INTEGRADO (funcional, modelo gratis real)

| Commit | Contenido |
| --- | --- |
| `a3ef122` | merge de `uxfix/functional` en `main` (Task A completa). |
| `030d788` | fix(agent,provider,fake): señal terminal real `/session/status` + watermark + tests de scoping (rework de REQUEST_CHANGES). |
| `6735bd3` | fix(agent,provider,fake): polling real `/session/status` + `/session/{id}/message`. |

**Causa raíz confirmada contra el sidecar real 1.18.25** (validación del autor
Kimi contra `sidecars/opencode-x86_64-unknown-linux-gnu`):
- `GET /session/status` es la señal real: `{"<sessionID>":{"type":"busy"}}` →
  mapa vacío `{}` = idle/completed. El sidecar NUNCA emite
  `{"type":"idle"}` y NO tiene `status` en el objeto Session; `time` no tiene
  `completed`.
- El texto del asistente se obtiene de `GET /session/{id}/message` (array con
  `{info:{role},parts:[{type:text,text}]}`), no del objeto Session.
- Fix aplicado: `poll_session`/`session_phase` en
  `crates/project-agent/src/opencode.rs` y `poll_test_session` en
  `crates/project-provider/src/adapter.rs` consultan `/session/status`
  scoped por session id; watermark de contador de mensajes del asistente antes/
  después de `prompt_async` para evitar texto stale en sesiones reutilizadas;
  `fake-opencode-server` alineado a la forma real (mapa vacío terminal +
  `{type:busy}`, mensajes `{info:{role},parts}`).
- **No se hardcodea ningún modelo** (`big-pickle` solo en tests/fake/catálogo).
- Reviewer funcional: `opencode-go/qwen3.8-flash` (fresh), REQUEST_CHANGES →
  rework `030d788` → **APPROVE** (re-review del mismo reviewer, permitido por
  misma tarea). Ver `docs/ux-redesign-01/reviews/qwen-functional-review-6735bd3.md`
  y `qwen-functional-rereview-030d788.md`.
- Validación local en `main`: `cargo fmt --all -- --check` OK,
  `cargo test -p project-agent -p project-provider -p fake-opencode-server`
  green (18 agent adapter / 24 provider adapter + suites), `git diff --check` OK.

## TASK B — PENDIENTE (visual, NO integrada)

- Autor visual: **Cursor Grok 4.6 High** (fresh, MODEL_ACTUAL confirmado),
  commit `b7d75be` `fix(ux): make the default shell feel like a compact chat`
  en `uxfix/visual` (14 archivos de `app/src/**`, 617+/264-).
  Cumple el brief: sin panel Materiales, overlay de drop "Soltá los archivos
  acá" solo en drag-over, un solo Adjuntar compacto (paperclip), sidebar Chat
  compacto con rename contextual, "Conversación nueva", composer
  [mensaje][Enviar] como ancla, banner "Modelo gratuito" eliminado (Gratis solo
  en el selector), Compartir secundario, Settings gear + X que restaura la
  misma conversación.
- Reviewer visual: **Cursor Grok 4.6 High fresh** → **APPROVE** (sin
  UX_BLOCKER/UX_IMPORTANT; solo UX_POLISH). Ver
  `docs/ux-redesign-01/reviews/grok-visual-review-b7d75be.md`.
- Reviewer de código/a11y/regresión: **`opencode-go/qwen3.8-flash` fresh** →
  **REQUEST_CHANGES** con 2 CODE_IMPORTANT:
  1. `ComposerBar.tsx` `pickFile()` sin try/catch → unhandled promise
     rejection sin feedback de error (regresión del surfacing que `addFile()`
     previo daba vía `setMaterialError`/ErrorNotice). Añadir catch + estado de
     error visible.
  2. Código muerto: tras quitar el panel Materiales, `MaterialsPanel` default
     export + `MaterialItem` (único caller de `api.materialRemove`) quedan
     inalcanzables; `importDetailLabel` duplicado verbatim en
     `WorkspaceView.tsx` y `MaterialsPanel.tsx`. Eliminar el componente muerto
     (conservar `MaterialChip`) o registrar la pérdida de capacidad de
     borrado como decisión de producto explícita; no dejar helper duplicado.
  - CODE_POLISH: leak de listener si unmount corre contra la promesa de
    `onDragDropEvent`; keys de React en la lista de detalles de import;
    variante `"free"` de `ProviderStatus`/`messages.provider.banner.freeModel`
    muertas; placeholder "Modelo automático · Gratis" puede rotular un modelo
    ausente; `app/package-lock.json` sin trackear (higiene).
  - A11Y: sin blockers; polish de contraste de `composer-model-select` y
    `title`/tooltip en nombres largos.
  - Ver `docs/ux-redesign-01/reviews/qwen-frontend-review-b7d75be.md`.
- **Bloqueado en esta sesión por ROTATE_SESSION_REQUIRED (~110K→128K).** El
  gate `scripts/agent-launch` falló cerrado al intentar lanzar el fix worker
  Grok (`grok-visual-fix`). NO se lanzó. NO se integró.

## Próximo paso (sesión de orquestación siguiente, budget CONTINUE < 80K)

1. Lanzar un FRESH autor visual **Cursor Grok 4.6 High** en un worktree nuevo
   sobre `main` (`a3ef122`) o reusar `uxfix/visual` (rama ya en `b7d75be`),
   con SOLO los 2 CODE_IMPORTANT + polish de la review Qwen Flash. Mismo brief
   visual. Autor NUEVO (la sesión grok-visual fue cerrada).
2. Re-revisión del diff resultante: fresh **Cursor Grok 4.6 High** (visual,
   solo si cambia UX) y fresh **opencode-go/qwen3.8-flash** (código/a11y).
   AUTHOR ≠ REVIEWER.
3. Integrar el commit visual revisado a `main`, correr `./scripts/verify`
   (nótese drift preexistente de toolchain clippy documentado en
   `UX_REDESIGN_01_DESIGN.md` §28).
4. Playwright headed (1366×768, 1440×900, 1920×1080) con captures: first
   launch, conversación activa, drag-over, recurso dropeado, selector de
   modelo, settings, sharing, respuesta real del modelo gratis.
5. Build de AppImage real (camino M10 aprobado), lanzamiento en Fedora,
   verificación real (bundled OpenCode 1.18.25 + cloudflared, sin PATH externo,
   modelo gratis automático, respuesta LLM real, UI chat simple), reportar
   path + SHA-256.

## Worktrees (al cierre de esta sesión)

- `main` → `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-harness`
  (integración, commit `a3ef122`).
- `uxfix/functional` → `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-uxfix-functional`
  (commit `030d788`, integrado; worktree puede removerse).
- `uxfix/visual` → `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-uxfix-visual`
  (commit `b7d75be`, PENDIENTE de fix/review/integración; NO tocar hasta la
  próxima sesión).

## Pines de sidecar (M10, `config/components.json` / ADR-0013)

- `opencode` 1.18.25, `cloudflared` 2026.8.3 (SHA-256 commiteados). Sin cambios.
- Binarios presentes: `sidecars/opencode-x86_64-unknown-linux-gnu` (1.18.25),
  `sidecars/cloudflared-x86_64-unknown-linux-gnu`.

## Model allocation (sesión cerrada por rotación)

- Orquestadora: `opencode-go/deepseek-v4-flash`.
- TASK A author: `opencode-go/kimi-k2.7-code` (sesión `kimi-freefix`, cerrada).
- TASK A reviewer: `opencode-go/qwen3.8-flash` (sesión `qwen-functional-review`,
  cerrada tras APPROVE).
- TASK B author: `cursor-grok-4.6-high` (sesión `grok-visual`, cerrada tras
  handoff + APPROVE visual).
- TASK B visual reviewer: `cursor-grok-4.6-high` fresh (sesión
  `grok-visual-review`, cerrada tras APPROVE).
- TASK B code reviewer: `opencode-go/qwen3.8-flash` fresh (sesión
  `qwen-frontend-review`, cerrada tras REQUEST_CHANGES).
- **Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0 sesiones.**
- Budget gate: CONTINUE (~55K) al inicio, CHECKPOINT_WARNING (~91-95K), luego
  ROTATE_SESSION_REQUIRED (~110K) y ~128K al cierre. Cero workers lanzados tras
  la rotación (gate respetado).