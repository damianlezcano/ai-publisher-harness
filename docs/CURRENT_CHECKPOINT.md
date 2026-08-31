# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual (SAFE CHECKPOINT — fin de sesión de orquestación UX_REDESIGN_01, 2026-08-31)

- Current main commit: `9019b33` (merge T3). `git log --oneline -5` para el detalle.
- **M1-M10: CLOSED.** UX_RELEASE_GATE_01: APROBADO. UX_REDESIGN_01: APROBADO.
  ADR-0014 / ADR-0015: Accepted. **M11 NO iniciado.**
- **Toolchain resuelto:** `rustup` instalado en user scope (`~/.cargo`, `~/.rustup`);
  `rust-toolchain.toml` pinea 1.97.1 y ahora es honrado (`rustc/cargo/clippy 1.97.1`).
  El Rust de Fedora (`/usr/bin/rustc` 1.98.0) NO fue alterado. `./scripts/verify` → **M10 contract passed**.
- **T1-T4: INTEGRADOS y VERIFICADOS** en `main` (backbone completo del historial durable).
  `./scripts/verify` verde tras cada lote.
- **T5-T7: NO iniciados.** Siguiente tarea: **T5a** (shell + sidebar + first-launch).
- Trabajo de producto T1-T4 realizado en worktrees `../ai-publisher-ux-t*` (ver §Worktrees);
  los panes de agentes de esas tareas fueron cerrados (checkpoint); los worktrees quedan para auditoría.

## Tareas integradas en `main` (UX_REDESIGN_01)

| Tarea | Rama (branch) | Commits | Estado |
| --- | --- | --- | --- |
| T1 message domain + schema v3 + migración + validación | `m-ux/t1-message-core` | `04082a2` + fix `2f5912c` | MERGED, review APPROVED |
| T2 fs rehidratación + tests de migración/durabilidad | `m-ux/t2-fs-migration` | `ccf1b22` | MERGED, review APPROVED |
| T3 facade `send_message` + DTOs + commands | `m-ux/t3-service-facade` | `7964b16` | MERGED, review APPROVED |
| T4 ranking determinista free-model (ADR-0015) | `m-ux/t4-free-model` | `3c9c61b` | MERGED, review APPROVED |

Merge commits en `main`: `cee9ed7` (T4), `85705e7` (T1), `db6680d` (T2), `9019b33` (T3).

## Detalle técnico integrado

1. **Schema v3:** `PROJECT_SCHEMA_VERSION = 3`; v2 → pure bump, v1 → reglas existentes
   (creations private, route None) luego bump; lector acepta 1/2/3, persist sólo 3;
   `messages` con `#[serde(default)]`. `MAX_MESSAGE_TEXT_CHARS = 40_000`.
2. **Message:** `MessageId` (UUIDv7), `MessageRole` (user|assistant), `MessageStatus`
   (ok|failed|cancelled), `created_at`, `material_ids`, `creation_ids`. `MessageView`
   (strings) en DTOs. `IdGenerator::message_id`.
3. **Durabilidad de envío:** `AppState::send_message_persist` persiste el mensaje USER
   (prompt crudo + material_ids) ANTES de emitir `agent://task/working` y antes de correr
   el agente; `send_message_run` (fuera del lock `projects`) anexa assistant ok/failed/
   cancelled al final. Fallo NUNCA pierde el mensaje del usuario. Referencias validadas
   como subconjunto del project (core `append_*`).
4. **DTOs:** `ProjectSummary` += `createdAt`, `updatedAt`, `shared` (derivado de
   `list_published`); `ProjectView` += `messages`.
5. **Ranking free-model:** ADR-0015 determinista — usable (`!deprecated`) → free
   (`free==true`) → rank (opencode+recommended > opencode > recommended > any free) →
   tie-break `(provider_id, model_id)` asc. `default_free_model` y `pick_free` reescritos.
   Sin nombres de modelo hardcodeados; selección explícita persiste en `settings.json`.

## Model allocation usada (sesión cerrada)

- Orchestrator/integración: `opencode-go/deepseek-v4-flash` (esta sesión).
- Autores T1-T4: `opencode-go/kimi-k2.7-code`. Revisores T1-T4: `opencode-go/qwen3.8-max`.
- `MODEL_ACTUAL == MODEL_REQUESTED` verificado por `scripts/agent-launch` en cada worker
  (T2 requirió re-verificación manual del modelo activo: confirmado Kimi).
- AUTHOR != REVIEWER cumplido en las 4 tareas.

## Próximo paso (resumir aquí)

- **T5a** App shell + `ConversationsSidebar` (nuevo) + first-launch bootstrap:
  `app/src/App.tsx`, `app/src/components/ConversationsSidebar.tsx`. Frontend: `app/src/types.ts`
  ya debe reflejar `ProjectSummary.createdAt/updatedAt/shared` y `ProjectView.messages`
  (agregar `MessageView`). Autor: Cursor Composer 2.5 (LOW/MEDIUM) o Kimi para wiring;
  revisor: Qwen3.8 Max. Ver `docs/UX_REDESIGN_01_DESIGN.md` §16, §22.
- Luego T5b, T5c, T5d, T5e (frontend), T6 (Playwright headed), T7 (verify gate + docs).

## Worktrees (para auditoría; NO borrados en este checkpoint)

`git worktree list`:
- `../ai-publisher-ux-t1` → `m-ux/t1-message-core` (2f5912c, merged)
- `../ai-publisher-ux-t1-review` → `m-ux/t1-review`
- `../ai-publisher-ux-t2` → `m-ux/t2-fs-migration` (ccf1b22, merged)
- `../ai-publisher-ux-t2-review` → `m-ux/t2-review`
- `../ai-publisher-ux-t3` → `m-ux/t3-service-facade` (7964b16, merged)
- `../ai-publisher-ux-t3-review` → `m-ux/t3-review`
- `../ai-publisher-ux-t4` → `m-ux/t4-free-model` (3c9c61b, merged)
- `../ai-publisher-ux-t4-review` → `m-ux/t4-review`
- Checkout principal: `main` (9019b33), integración-only.

Limpieza de worktrees históricos: al cierre del milestone (WORKTREES.md).

## Pines de sidecar (M10, en `config/components.json` / ADR-0013)

- `opencode` 1.18.25, `cloudflared` 2026.8.3 (SHA-256 commiteados). Sin cambios en esta sesión.

## Pendientes de release (sin cambio)

- B1/B2 (UX_RELEASE_GATE_01) se resuelven con T5 (frontend chat-first).
- **M11 NO iniciado.**