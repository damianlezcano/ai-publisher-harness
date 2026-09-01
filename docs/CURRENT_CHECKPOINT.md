# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual (UX_REDESIGN_01 — PRODUCT CORRECTION PASS, PARADA POR ROTACIÓN, 2026-08-31)

- **Current main commit: `3bd9bac`.** `git log --oneline -6` para el detalle.
- **UX_REDESIGN_01 correction pass: INICIADO y DETENIDO por ROTATE_SESSION_REQUIRED.**
  No se lanzó NINGÚN worker; no se integró ningún cambio del pass. La orquestadora
  (DeepSeek V4 Flash) agotó el presupuesto de contexto (~112K) en bootstrap +
  preservación + harness + exploración, antes de poder lanzar las dos tareas
  acotadas (funcional + visual). El gate `scripts/agent-launch` falló cerrado
  (ROTATE_SESSION_REQUIRED) al intentar lanzar el primer worker.
- **M11 NO iniciado.** `./scripts/verify` aún NO corrido en esta sesión (estado de
  `main` = commits previos cerrados; ver sección "Integrado en esta sesión").
- **Problema A (modelo gratis no funciona) — hipótesis de causa raíz documentada
  por la orquestadora, NO aún corregida.** Ver sección siguiente.
- **Tarea visual — inventario y especificación del frontend documentados, NO aún
  implementados.** Ver sección siguiente.

## Integrado en esta sesión (3 commits, todo preservado en `main`)

| Commit | Contenido |
| --- | --- |
| `72da998` | **fix(packaging): preservar los fixes validados del AppImage real** (trabajo sin commitear de la sesión previa de validación real). `OpenCodeBackend::ensure_ready` serializa el spawn con `startup_lock` (evita que un caller concurrente mate el sidecar arrancando lento en AppImage-FUSE); `project-provider/adapter.rs` acepta el catálogo real 1.18.25: envelope `{"data":[...]}`, `cost` como array de tiers (`cost_is_zero`), y retry de catálogo vacío por hasta 3 s; `fake-opencode-server` + `fake_process serve_http` reflejan la forma real y la ventana de boot; tests de regresión (14 agente + 20 provider) PASS. |
| `a6caecf` | **fix(harness): check-session-budget** — capturar el export de la sesión vía temp file. `$(opencode export ...)` truncaba el export grande (~64 KB) en el shell real y el gate fallaba cerrado (exit 4) en TODA corrida viva, lo que habría bloqueado cualquier lanzamiento de worker. Ahora redirige a temp file. `scripts/test-session-budget` PASS. |
| `3bd9bac` | **harness(agent-launch): rol high-visual → Cursor Grok 4.6 High** — asignación explícita del owner para la tarea visual. `HIGH_VISUAL_CURSOR_MODEL=cursor-grok-4.6-high` en `config/agent-models.env` + matcher de display en `scripts/agent-launch`. `scripts/test-agent-launch` PASS. |

Validación de `72da998`: `cargo test -p project-provider` (19+20), `-p project-agent`
(14), `cargo fmt --all -- --check`, `git diff --check`. Los 6 archivos preservados
corresponden exactamente a los fixes validados de empaquetado/runtime (revisados
uno a uno contra el árbol de trabajo antes de commitear).

## PROBLEMA A — MODELO GRATIS NO FUNCIONA (BLOQUEANTE, no corregido)

**Síntoma real:** el AppImage descubre y muestra un modelo gratis (p. ej. "Big
Pickle (Gratis)"), pero enviar un mensaje devuelve "No se pudo iniciar el
asistente de IA."

**Causa raíz documentada por la orquestadora (exploración de la ruta real):**

El flujo `agent_send` → `AppState::send_message_run` → `AgentService::run`
(`crates/project-agent/src/service.rs:48-103`) → `OpenCodeAgentEngine`:
- `ensure_ready()` OK (fix ya integrado).
- `open_session` (`crates/project-agent/src/opencode.rs:138-179`): `POST /session`
  → OK con el sidecar real 1.18.25.
- `send` (`opencode.rs:181-217`): `POST /session/{id}/prompt_async` → HTTP 204 OK.
- **`poll_session` (`opencode.rs:84-112`) — ROMPIDO:** parsea `GET /session/{id}`
  con `session_phase` (`opencode.rs:270-289`, duplicado en
  `crates/project-provider/src/adapter.rs:774-793`) esperando `status` top-level,
  `status.type/name`, o `time.completed`.
  - El sidecar real 1.18.25 **NO** produce ninguno de esos: el schema `Session`
    no tiene `status`; `time` es `{created,updated,compacting,archived}` con
    `additionalProperties:false` (sin `completed`).
  - La señal real de completado vive en **`GET /session/status`** →
    `{"<sessionID>":{"type":"busy"}}` → `{"type":"idle"}`, endpoint que la app
    NUNCA consulta.
  - Resultado: `session_phase` siempre devuelve `"working"`, el loop agota el
    deadline (120 s, `opencode.rs:20,107-108`) → `AgentError::Timeout` →
    `crates/project-app/src/error.rs:173-177` → "No se pudo iniciar el asistente
    de IA."
- **Forma de fake ≠ real:** `crates/fake-opencode-server/src/lib.rs:697-728` sirve
  `GET /session/{id}` como `{"id":"…","status":"idle","messages":[…]}` — la forma
  top-level-string que el parser espera pero el servidor real NO produce. Todos
  los tests pasan contra el fake mientras producción hace timeout.
- **Síntoma secundario (misma raíz):** `poll_test_session` / "Probar conexión"
  (`adapter.rs:405-438`) usa el mismo `session_phase` → también daría timeout.
- **Autenticación NO es la causa:** el tier gratis no requiere credencial
  (el provider opencode trae `apiKey:"public"`; sin `auth.json` en un arranque
  aislado; `big-pickle` devuelve HTTP 429 rate-limit, no 401). El `modelID` del
  payload coincide con el id del catálogo (verificado contra el sidecar real).

**Fix pendiente (tarea funcional acotada, autor `opencode-go/kimi-k2.7-code`):**
corregir `poll_session`/`session_phase` para consultar `GET /session/status` (o
la señal real de completado) contra el sidecar 1.18.25, y alinear
`fake-opencode-server` a la forma real. NO enmascarar el error con copy de UI.

## TAREA VISUAL (no implementada) — inventario del frontend

Estado actual del frontend (`app/`), mapeado por la orquestadora:
- `App.tsx:57` crea la primera conversación con `"Nueva conversación"`
  (`messages.conversation.defaultName`, `messages.ts:64`).
- **Banner "Modelo gratuito"** full-width aún presente: `App.tsx:170-172` +
  `ui/ProviderStatusBanner.tsx:12-17` (`messages.provider.banner.freeModel`).
- **Panel "Materiales"** (`<details>` abierto) en `WorkspaceView.tsx:135-168`
  (`messages.timeline.unattachedTitle` = "Materiales", `messages.ts:134`), con
  `MaterialItem` card grande y botón "Agregar archivo"
  (`messages.material.addFile`, `messages.ts:150`). Coexiste con el botón
  "Adjuntar material" del composer (`ComposerBar.tsx:292-302`,
  `messages.assistant.attachMaterial`, `messages.ts:124`).
- **Botón permanente "Renombrar"** en cada fila del sidebar:
  `ConversationsSidebar.tsx:157-164` (`messages.project.rename`).
- **Sin drag&drop activo:** el único handler DnD está en el componente huérfano
  `MaterialsPanel.tsx:189-211` (no montado en la app). Sin handlers React
  onDrop/onDragOver. Zona de scroll candidata: `div.workspace-timeline`
  (`WorkspaceView.tsx:122`, `.workspace-timeline` en `styles.css:1140-1145`).
- **Composer:** placeholder `"Ej.: Creá una actividad interactiva sobre la
  fotosíntesis"` (`messages.ts:122`); Enviar; modelo `(Gratis)/(De pago)`;
  Compartir (`ShareControl` en `PublishPanel.tsx:108-176`).
- **No existe** "Soltá los archivos acá", no hay icono paperclip, no hay
  "Escribí lo que querés crear...", no hay "Conversación nueva".
- CSS: `app/src/styles.css` global único. Serve: Vite :1420
  (`vite.config.ts:10-14`). Harness Playwright: `docs/ux-redesign-01/harness/`.

Tarea visual acotada: autor `Cursor Grok 4.6 High` (rol `high-visual/cursor`),
solo frontend (`app/src/*`, `messages.ts`, `styles.css`), reglas de producto del
design brief (sin panel Materiales, drag&drop con overlay temporal, un único
adjunto compacto, sin banner full-width, terminología "Conversación nueva",
rename secundario, Compartir secundario, composer como ancla).

## Próximo paso (sesión de orquestación siguiente)

1. Confirmar el presupuesto de la sesión nueva con `scripts/check-session-budget`
   (CONTINUE < 80K).
2. Lanzar las DOS tareas acotadas (functional author Kimi K2.7 Code → Block A;
   visual author Cursor Grok 4.6 High), cada una en un worktree:
   `git worktree add -b uxfix/functional <sibling>` y
   `git worktree add -b uxfix/visual <sibling>` (worktrees de esta sesión fueron
   creados y removidos vacíos).
3. Revisores `opencode-go/qwen3.8-flash` por tarea. AUTHOR ≠ REVIEWER.
4. Integrar commits revisados a `main`, correr `./scripts/verify`.
5. Build de AppImage real por el camino M10 aprobado, lanzar en Fedora, y
   aceptación real (incluye: "Hola" → respuesta LLM real con modelo gratis
   auto-descubierto; sin panel Materiales; drag&drop; sin banner duplicado;
   terminología Conversación; rename compacto; Settings X; Compartir).
6. Capturas Playwright headed + reporte final (formato de 25 puntos del brief).

## Worktrees (limpios al cierre)

`git worktree list` → solo `main` (`3bd9bac`), integración-only. Los worktrees
`uxfix-functional`/`uxfix-visual` fueron creados y removidos sin commits.

## Pines de sidecar (M10, `config/components.json` / ADR-0013)

- `opencode` 1.18.25, `cloudflared` 2026.8.3 (SHA-256 commiteados). Sin cambios.
- Binarios presentes: `sidecars/opencode-x86_64-unknown-linux-gnu` (1.18.25),
  `sidecars/cloudflared-x86_64-unknown-linux-gnu`.

## Model allocation (sesión cerrada por rotación)

- Orquestadora: `opencode-go/deepseek-v4-flash`. **Cero escalaciones a Qwen3.8
  Max ni DeepSeek V4 Pro.** Ningún worker lanzado en esta sesión.
- Gate de presupuesto `scripts/check-session-budget` funcional tras `a6caecf`
  (temp-file capture); midió CONTINUE (~76K) y luego ROTATE_SESSION_REQUIRED
  (~112K) al cierre.