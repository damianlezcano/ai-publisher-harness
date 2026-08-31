# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current main commit: (see `git log -1` after the UX_REDESIGN_01 design commit)
- **M1-M10: CLOSED.** M10 implementado, revisado e integrado (T1-T5).
- **UX_RELEASE_GATE_01: APROBADO** (2026-08-31) — ver `docs/UX_RELEASE_GATE_01.md` §11.
- **UX_REDESIGN_01: APROBADO** (2026-08-31) — ver `docs/UX_REDESIGN_01_DESIGN.md`.
- **ADR-0014: Accepted** — historial de conversación durable en el aggregate `Project`.
- **ADR-0015: Accepted** — descubrimiento determinista del modelo gratis.
- **M11 (Component Updates + Windows CI): NO iniciado.** No se empezó work de M11.

## Decisiones de arquitectura aprobadas (UX_REDESIGN_01)

1. **Vocabulario:** "Conversación" (user-facing) ↔ `Project`/`ProjectId` (dominio interno).
   NO se introduce un aggregate `Conversation` separado.
2. **Historial durable:** `Project.messages: Vec<Message>` persistido en `project.json`
   (aggregate existente, atomic/CAS). **NO** localStorage como store autoritativo.
3. **Schema:** `project.json` v2 → **v3** (migración lossless; `messages` default vacío).
4. **Mensaje:** identidad estable, role (user/assistant), text, status (ok/failed/cancelled),
   timestamp, `material_ids`, `creation_ids`. Materials/Creations siguen siendo los recursos
   autoritativos; los mensajes solo los referencian (no duplican contenido).
5. **Durabilidad de envío:** persistir mensaje USER **antes** de ejecutar el agente; persistir
   mensaje ASSISTANT/resultado **después** del resultado exitoso. Fallos preservan el mensaje
   user y exponen estado recuperable. El historial sobrevive restart, cambio de conversación,
   fallo de proveedor y fallo de agente.
6. **Modelo gratis:** dinámico desde el catálogo de OpenCode (disponible, usable, `cost==0`,
   ranking determinista + tie-break `(provider_id, model_id)`). **Sin** hardcodear Big Pickle ni
   ningún ID de modelo. Selección explícita del usuario pisa la automática; la automática es
   efímera, la explícita persiste en `settings.json`.
7. **Proveedores:** reusar M7. Settings es opcional; el usuario puede usar EducAI con el modelo
   gratis auto-seleccionado sin configurar ChatGPT/Gemini/DeepSeek. NO crear una segunda capa de
   proveedores.
8. **UI objetivo:** LEFT lista de conversaciones / CENTER conversación / BOTTOM prompt + modelo +
   Compartir / SETTINGS con X que vuelve a la conversación exacta / recursos en contexto.
   NO restaurar el dashboard 2×2.
9. **Lista de conversaciones:** más recientes primero (según diseño aprobado), renameable,
   durable, backed by Project, sin exponer `ProjectId` crudo.
10. **Arquitectura existente:** reusar sin rediseñar M1 filesystem, M2 HTTP, M3 publication,
    M4 tunnel, M5 AgentEngine, M7 providers, M8 materials/previews, M10 packaging. Cualquier
    tarea que requiera un rewrite mayor debe devolver `ARCHITECTURE_ESCALATION_REQUIRED`.

## Task graph (T1-T7, ver `docs/UX_REDESIGN_01_DESIGN.md` §22)

- **T1** core: `Message` + schema v3 + migración + validación — `crates/project-core/src/lib.rs`.
- **T2** fs: rehidratación/validación de mensajes + tests de migración — `crates/project-fs`.
- **T3** service/facade/commands: `append_*`, `send_message`, DTOs (`MessageView`,
  `ProjectSummary{createdAt,updatedAt,shared}`, `ProjectView.messages`), `agent_send`/`project_open`/
  `project_list` — `crates/project-core`, `crates/project-app`, `app/src-tauri`.
- **T4** ranking determinista del modelo gratis — `crates/project-provider/src/service.rs`.
- **T5a–T5e** frontend: shell + sidebar + first-launch, ComposerBar, timeline + recursos en
  contexto, Settings (gear+X) + Compartir único, catálogo de copy — `app/src`.
- **T6** Playwright headed (1366×768 / 1440×900 / 1920×1080) + a11y (gate final).
- **T7** `scripts/verify` gate UX + docs.

## Model allocation (aprobada)

- Orchestrator/integración: `opencode-go/deepseek-v4-flash` (coordina e integra, no implementa todo).
- Coding normal/complejo: `opencode-go/kimi-k2.7-code`.
- Reasoning / review independiente: `opencode-go/qwen3.8-max`.
- LOW/visual/CSS/copy: Cursor Composer 2.5 (fallback `opencode-go/mimo-v2.5`).
- Cross-cutting difícil: Kimi K2.7 Code (fallback Cursor Grok 4.6 medium).
- HIGH_ARCHITECTURE: fresh `opencode-go/deepseek-v4-pro` SOLO en escalada.
- Prohibido: Big Pickle; GPT/Grok vía OpenCode Go. AUTHOR != REVIEWER.

## Hallazgo de entorno (pre-existente, requiere diagnóstico del orchestrator)

`./scripts/verify` falla en `cargo clippy` con `clippy::chunks-exact-to-as-chunks`
(`crates/project-preview/src/token.rs:27`): el entorno tiene `cargo/rustc/clippy 1.98.0`
(Fedora) mientras `rust-toolchain.toml` pinea `1.97.1` sin rustup que lo honre. **No** se
modificó `rust-toolchain.toml` ni la política de toolchain en esta sesión de arquitectura. El
orchestrator de implementación debe diagnosticar el mismatch exacto antes de tocar toolchain.

## Próximo paso

- Diagnóstico del mismatch de toolchain (Rust/clippy) como primera tarea de implementación.
- Luego T1 (schema v3 + `Message`) como primera tarea de producto del milestone de UX, con
  `scripts/agent-launch` para el worker asignado (Kimi K2.7 Code).
- **M11 NO iniciado.** B1/B2 siguen siendo blockers de release.

## Pines de sidecar (M10, en `config/components.json` / ADR-0013)

- `opencode` 1.18.25, `cloudflared` 2026.8.3 (SHA-256 commiteados). Sin cambios en esta sesión.
