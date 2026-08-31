# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual (UX_REDESIGN_01 CLOSED — fin de sesión de orquestación, 2026-08-31)

- Current main commit: `9424e47` (merge T7). `git log --oneline -15` para el detalle.
- **M1-M10: CLOSED.** **UX_REDESIGN_01: COMPLETO y CERRADO** (T1-T7 integrados,
  `./scripts/verify` verde, gate Playwright headed PASS). **M11 NO iniciado.**
- **`./scripts/verify` → PASS** (M0/M1 contracts, cargo fmt/clippy/test, frontend
  format/lint/typecheck/test, M10 alignment, **UX_REDESIGN_01 contract passed**).
  `git diff --check` clean, `git status` clean.
- **Toolchain:** rustup en user scope pinea 1.97.1 y es honrado; el Rust de
  Fedora no fue alterado. (Drift de §28 del diseño resuelto.)
- **Gate visual final:** `docs/ux-redesign-01/RESULTS.md` — 10 flujos × 3
  viewports (1366×768, 1440×900, 1920×1080), 51 PNG + 51 OCR (no vacíos) + 10
  a11y trees + harness reproducible (`docs/ux-redesign-01/harness/`). 33/33
  aserciones PASS. **Sin UX_BLOCKER ni UX_IMPORTANT.** B1/B2/D1/D3 cerrados.
- **Política de modelos persistida (directiva del owner 2026-08-31):** Qwen3.8
  Flash = revisor por defecto; Qwen3.8 Max = escalación-only; Kimi K2.7 Code =
  worker de código primario; Composer/MiMo = LOW; sesiones descartables por tarea;
  cierre inmediato de panes; rotación de orquestador ~80K/100K/130K; sin Big
  Pickle; sin GPT/Grok vía OpenCode Go. Persistida en `docs/AGENT_POLICY.md`,
  `config/agent-models.env`, `scripts/agent-launch`, `scripts/test-agent-launch`.

## Tareas integradas en `main` (UX_REDESIGN_01)

| Tarea | Rama (branch) | Commits | Estado |
| --- | --- | --- | --- |
| T1 message domain + schema v3 + migración | `m-ux/t1-message-core` | `04082a2`+`2f5912c` | MERGED (sesión previa) |
| T2 fs rehidratación + tests | `m-ux/t2-fs-migration` | `ccf1b22` | MERGED (sesión previa) |
| T3 facade `send_message` + DTOs + commands | `m-ux/t3-service-facade` | `7964b16` | MERGED (sesión previa) |
| T4 ranking determinista free-model (ADR-0015) | `m-ux/t4-free-model` | `3c9c61b` | MERGED (sesión previa) |
| T5a App shell chat-first + sidebar + first-launch | `m-ux/t5a-app-shell` | `5b4d751`+fix `004d871` | MERGED, review APPROVED |
| T5b ComposerBar bottom (prompt + Modelo + share slot) | `m-ux/t5b-composer-bar` | `c64d3b0` | MERGED, review APPROVED |
| T5c timeline + resources-in-context (chips/cards) | `m-ux/t5c-timeline` | `7a7d12a`+`6fa2e7a`+`d549336` | MERGED, review APPROVED |
| T5d Settings Configuración (X) + single Compartir | `m-ux/t5d-settings-share` | `39d8f99` | MERGED, review APPROVED |
| T5e copy catalog (vocabulario Conversación + dead keys) | `m-ux/t5e-copy` | `e8a85aa` | MERGED, review APPROVED |
| T6 Playwright headed visual + a11y gate | `m-ux/t6-playwright` | `51f105c`+rework `3c3cda9` | MERGED, review APPROVED |
| T7 verify UX gate (fail-closed) + status del diseño | `m-ux/t7-verify-gate` | `e1b3ad6`+`3829dad` | MERGED, review APPROVED |

Merge commits en `main`: `cee9ed7` (T4), `85705e7` (T1), `db6680d` (T2), `9019b33`
(T3), `a4a7749`+`6ed219d` (T5a), `cc3eed8` (T5b), `a2ddb03` (T5c), `0e089bd`
(T5d), `c7cdc17` (T5e), `d711ca8` (T6), `9424e47` (T7), `760e05f` (hardening
test) y `7069b53` (política de modelos).

## Detalle técnico integrado (chat-first)

1. **Shell chat-first:** `App.tsx` = header (título + gear Configuración), sidebar
   `ConversationsSidebar` (nav, newest-first, aria-current, rename inline, badge
   Compartido, timestamp), main = `WorkspaceView`. First-launch: lista vacía →
   crea y abre conversación por defecto; si no, abre la primera (newest).
   Sin 2×2, sin strip técnico en el header, sin "Mis proyectos" como pantalla.
2. **Timeline:** `ChatPanel` renderiza `ProjectView.messages` (user bubbles con
   chips de material; assistant bubbles con `CreationCard`s inline; failed/
   cancelled como error `role=alert`). Reconcilia vía refresh en cada evento
   `agent://task` (App ya refresca). Echo optimista `pendingUser` dedup.
   Materiales: en el timeline si están referenciados, o en la sección
   colapsable "Materiales" si no — nunca en ambos.
3. **ComposerBar (bottom):** prompt (Ctrl/Cmd+Enter), attach chips, paste de
   imagen, selector de modelo compacto (solo `name` + badge Gratis; nunca
   provider_id/model_id crudo; sin caveat en la vista por defecto), slot
   `shareAction`.
4. **Settings:** gear → ProviderPanel con título "Configuración", X con
   `aria-label="Cerrar"`, Esc (useFocusTrap), backdrop; restaura la conversación
   exacta (App no desmonta el estado).
5. **Compartir único (D3):** `ShareControl` en el composer: "Compartir" →
   "Compartido" → menú (Copiar enlace / Abrir enlace / Mostrar QR / Dejar de
   compartir + confirmación + nota de honestidad temporal). Sin panel Compartir
   permanente y sin switch `Se compartirá` por creación.
6. **Gate Playwright:** harness con shim `__TAURI_INTERNALS__` (DTOs nuevos,
   persistencia de mensajes, eventos `agent://task`, persistencia localStorage
   para el flujo restart). 10 flujos × 3 viewports, OCR + a11y. Resultados en
   `docs/ux-redesign-01/RESULTS.md`.

## Model allocation usada (sesión cerrada)

- Orchestrator/integración/checkpoints: `opencode-go/deepseek-v4-flash`.
- Autores T5a-T5d: `opencode-go/kimi-k2.7-code` (4 tareas + fixes en-sesión).
- Autores T5e/T7 (LOW): `opencode-go/mimo-v2.5` (Composer 2.5 no disponible vía
  cursor-agent; fallback según política).
- Autor T6 (harness Playwright, tests significativos): `opencode-go/kimi-k2.7-code`.
- Revisores T5a-T7: `opencode-go/qwen3.8-flash` (6 revisiones). **Cero
  escalaciones a Qwen3.8 Max ni DeepSeek V4 Pro.**
- `MODEL_ACTUAL == MODEL_REQUESTED` verificado por `scripts/agent-launch` en
  todos los workers (6 autores + 6 revisores).
- AUTHOR != REVIEWER cumplido; cross-family (Kimi/MiMo → Qwen3.8 Flash).

## Próximo paso

- **M11 NO iniciado.** Pendientes de release B1/B2 (UX_RELEASE_GATE_01) resueltos
  por UX_REDESIGN_01 (B1/B2 cerrados, D1 y D3 verificados por Playwright).
- Siguiente milestone: planificar según `docs/PRODUCT.md` / `docs/UX.md`.
- `docs/ux-redesign-01/harness/run.sh` reproduce el gate (requiere vite en :1420).

## Worktrees (limpios al cierre del milestone)

`git worktree list` (los worktrees de T1-T7 fueron removidos al cierre; las ramas
mergeadas permanecen). Checkout principal: `main` (`9424e47`), integración-only.

## Pines de sidecar (M10, `config/components.json` / ADR-0013)

- `opencode` 1.18.25, `cloudflared` 2026.8.3 (SHA-256 commiteados). Sin cambios.