# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual (HUMAN_PRODUCT_REVIEW — FRESH CORRECTION PASS, TASK D INTEGRADA, ORQUESTADOR EN ROTACIÓN, 2026-09-01)

- **Current main commit: `f44d507`** (merge de Task D). `git log --oneline -12`
  para el detalle.
- **Progreso del pass:** Task A INTEGRADA (`e6389ea`, merge de
  `corr/a-creation-contract`). Task B INTEGRADA (`88761be`, merge de
  `corr/b-attachment-flow`). Task C INTEGRADA (`c94e114`, merge de
  `corr/c-startup-states`; autor kimi `fd1d928`, revisor qwen APPROVE tras
  REQUEST_CHANGES — MAJOR-1 resuelto y con cobertura de regresión, restantes
  NIT no bloqueantes). **Task D INTEGRADA (`f44d507`, merge de
  `corr/d-conversation-terms`; autor Composer 2.5 commit `18ac233`, revisor
  qwen3.8-flash APPROVE sin hallazgos)**. `./scripts/verify` PASS en main tras
  D (189 tests frontend, cargo verde, M10 + UX_REDESIGN_01 contract ok).
  Worktrees de Task D removidos, branch borrado. Panes author/reviewer de D
  cerrados.
- **Session budget: CHECKPOINT_WARNING (91.5K, 80K-99,999)**. Este orquestador
  rota en el punto seguro: Task D completa, integrada y verificada, panes
  cerrados. NO se inician Tasks E-G. NO M11. NO bypass del gate.
- **M11 NO iniciado.** Nada de M11 en esta corrección.
- **Trabajo previo integrado y conservado** (UX_REDESIGN_01): Task A modelo
  gratis real (`a3ef122`), Task B visual (`88fd346`), Playwright 44/44,
  AppImage real construido y verificado (detalles al final). NO reiniciar.
- **Pendiente: la revisión humana real del AppImage generó 16 hallazgos UX**
  (sección "Hallazgos humanos" abajo). Este pass los corrige. A-D done, E-G pendientes.

## Hallazgos humanos (16) y causas raíz confirmadas (YA investigadas)

1. Layout chat mucho más cercano al concepto. OK.
2. Panel Materiales permanente eliminado. OK, correcto.
3. Drag & drop funciona.
4. **Recursos dropeados aparecen a la derecha y parecen producidos por el
   asistente.** Causa: `WorkspaceView.importRef` (`app/src/components/WorkspaceView.tsx:66-91`)
   importa materiales vía `materialsAddFromPaths` pero NO los adjunta al
   mensaje pendiente (`ComposerBar` mantiene `attachmentIds` en estado local,
   `app/src/components/ComposerBar.tsx:52`). Los materiales no adjuntos se
   renderizan como `message-resource` en el timeline (`ChatPanel.tsx:193-198`),
   fuera de la burbuja del usuario.
5. **Sidebar/título muestra "Proyecto sin título 1".** Causa: el default
   activo es `messages.conversation.defaultName = "Conversación nueva"`
   (`app/src/messages.ts:64`) pero persisten proyectos de esquemas anteriores
   con nombres legacy; además `messages.project.defaultName = "Proyecto sin
   título"` (`messages.ts:92`) vive en el catálogo y `ProjectsView.tsx:33` lo
   usa. Backend `create_project` exige nombre no vacío (no auto-nombra).
6. **"No se pudo iniciar el asistente de IA." en primer arranque aunque luego
   funciona.** Causa: el backend es lazy y `ensure_ready` tiene timeout de 30s
   (`crates/project-opencode/src/backend.rs:16` DEFAULT_STARTUP). En arranque
   frío del AppImage el sidecar tarda; el primer `agent_send`/`model_get_selected`
   puede caer en `BackendNotReady`/`Timeout` → `AppError::from_agent`
   (`crates/project-app/src/error.rs:164-185`) → `ErrorCode::AiUnavailable`
   mensaje "No se pudo iniciar el asistente de IA." (línea 177). El frontend
   NO expone estados STARTING/READY/FAILED (nunca llama a `appStatus`/`agent_status`;
   solo escucha `agent://task`, `app/src/App.tsx:80-108`).
7. El modelo gratis responde tras el arranque. OK.
8. **Respuesta con `/tmp/opencode/...`, `node`, rutas `.js`, instrucciones
   shell.** Causa: el texto del asistente ES la respuesta cruda del LLM
   (`result.task.message` → `send_message_run` en `crates/project-app/src/app.rs:906-926`
   lo persiste como mensaje de asistente). No hay system-prompt/instrucción
   que fuerce lenguaje plano; `augment_prompt` solo agrega el bloque de
   materiales (`crates/project-agent/src/service.rs:176-188`).
9. **El usuario no identifica dónde quedó el juego.** Causa: la "creación"
   existe como `Creation` (registrador `FilesystemCreationRegistrar`,
   `crates/project-agent/src/registrar.rs:30-92`) y se renderiza como
   `CreationCard` (`app/src/components/CreationsPanel.tsx:30-99`) DENTRO de la
   burbuja del asistente (`ChatPanel.tsx:129-144`), pero sin botones
   "Abrir"/"Compartir" en la tarjeta (solo "Vista previa"/"Abrir en
   navegador") y sin una presentación clara de creación.
10. **Se espera: creación visible con [Abrir] [Compartir].** Ver arriba.
11. **Resultado renderizado dos veces: mensaje normal + texto verde crudo.**
    Causa CONFIRMADA: `App.tsx:91` guarda `event.message` crudo del evento
    `agent://task` en `agentMessage`; `ChatPanel.tsx:231-233` lo pinta como
    `.chat-status.ok` verde, ADEMÁS de la burbuja del asistente persistida
    (`ChatPanel.tsx:128`) que se refresca con `refreshConversation`
    (`App.tsx:98`). Duplicado de contenido asistente.
12. Usuario adjuntó archivo de texto con datos del rosco. OK (flujo existe).
13. Usuario pidió usar esos datos. OK.
14. **El flujo real de adjunto/contexto falló.** Causa: si el material se
    agrega por drag&drop, NO se adjunta a `attachmentIds` del composer →
    `agent_send(projectId, prompt, attachmentIds=[])` → el agente nunca ve el
    archivo. El aprovisionamiento backend EXISTE y es correcto
    (`resolve_attachments` `app.rs:781-829`, `provision_attachments`
    `service.rs:146-174`, prompt "Materiales adjuntos… están en la carpeta
    materials"). El eslabón roto es FRONTEND: drop → adjuntar al mensaje.
15. Los recursos deben seguir entendibles sin dashboard Materiales.
16. **No hay forma contextual de eliminar conversaciones.** `project_delete`
    existe end-to-end (commando `commands.rs:80-89`, `AppState::delete_project`
    `app.rs:310-317`, `ProjectService::delete_project` `project-core/src/lib.rs:807-811`,
    `api.ts:31`) pero NINGÚN componente lo llama; `ConfirmDialog` existe
    (type-name-to-confirm, `ConfirmDialog.tsx:15-48`) pero no está cableado.
    **Bug adicional detectado:** `delete_project` NO hace `unpublish` → entrada
    stale en `PublicationManager.published` y el proyecto aparece "shared"
    hasta reiniciar.

## Contratos clave actuales (para los workers)

- **Message/Project:** `Project.messages: Vec<Message>` schema v3
  (`crates/project-core/src/lib.rs:342-357, 400-420`); `Message { id, role,
  text, status, createdAt, materialIds, creationIds }`. Validación: user msg
  solo `material_ids`, assistant msg solo `creation_ids`.
- **Creation:** `Creation { id, displayName, kind: Web|Document|Image|File,
  visibility, relativePath, contentType?, byteSize, revision,
  parentCreationId?, createdAt }` (`lib.rs:378-399`). UI capabilities
  (open/preview/publish) se derivan en facade, no se almacenan.
- **DTOs:** `ProjectSummary {id,name,createdAt,updatedAt,shared}`,
  `ProjectView {id,name,materials,creations,messages,publication}`,
  `MessageView`, `CreationView {id,displayName,kind,visibility,byteSize,
  createdAt,revision}` (`crates/project-app/src/dtos.rs`, `app/src/types.ts`).
- **Frontend estado:** `App.tsx` (conversations/selectedId/conversation/
  agentPhase/agentMessage/settingsOpen), `WorkspaceView` (pendingUser/
  sendError/drag-drop/import), `ChatPanel` (timeline derivada),
  `ComposerBar` (prompt/attachmentIds/model selector), `ConversationsSidebar`
  (lista, rename inline, NO delete). `PublishPanel` = ShareControl (single
  Compartir en bottom bar).
- **Agent:** `AgentService::run` (`service.rs:49-103`) ensure_ready →
  open_session(workspace) → provision_attachments → send → registrar artifacts
  (skip `materials/`). `agent://task` events desde `commands.rs:274-322`.
  Backend status en `OpenCodeBackend.status()` → `BackendStatus` enum
  (`crates/project-opencode/src/status.rs:1-7`): Stopped|Starting|Ready|Failed;
  expuesto a UI solo vía `agent_status`/`app_status` (sin uso frontend).
- **Attachments:** `resolve_attachments` autoriza contra materiales del
  proyecto; copia a `workspace/materials/<n>-<name>`; augment_prompt lista.
  Read path seguro (`project-fs` validate_read_path). Cleanup: sin cleanup
  explícito de `workspace/materials/` tras run (aceptado).
- **Publicación:** `publish/unpublish/publication_status`, túnel Cloudflare,
  QR frontend. TEMPORAL; honestidad de enlace. Reusar tal cual.
- **Pins sidecar:** opencode 1.18.25, cloudflared 2026.8.3 (M10, sin cambios).
- **Copy catálogo:** `app/src/messages.ts` = catálogo ejecutable (ADR-0012);
  tests verdes deben seguir.

## Plan de tareas de esta corrección (ejecución por la sesión siguiente)

Modelos (política activa, AGENT_POLICY.md): orquestador
`opencode-go/deepseek-v4-flash`; funcional `opencode-go/kimi-k2.7-code`;
revisión funcional/código `opencode-go/qwen3.8-flash`; producto/UX frontend
**Cursor Grok 4.6 High** (solo vía Cursor, NUNCA OpenCode Go); revisión UX
independiente: Cursor Grok 4.6 High FRESH; LOW mecánico: Composer 2.5 o
`opencode-go/mimo-v2.5`. AUTHOR != REVIEWER. Una tarea = un worktree. Cada
worker: verificar MODEL_REQUESTED == MODEL_ACTUAL antes del task; handoff
compacto; cerrar panes tras PASS+APPROVE (CONTEXT_LEAK si queda idle).

| # | Tarea | Autor | Revisor | Ownership | AC (resumen) |
| --- | --- | --- | --- | --- | --- |
| A | ~~Contrato de CREACIÓN user-facing + fin de fuga técnica~~ **HECHA** (`e6389ea`) | kimi-k2.7-code | qwen3.8-flash | ~~`crates/project-agent`, `crates/project-app/src/app.rs`, dtos, `app/src/components/CreationsPanel.tsx`~~ | Creación con Abrir/Compartir; respuesta asistente en lenguaje plano (build_instruction en service.rs); sin paths/comandos en UX normal. APPROVE. |
| B | ~~Flujo real de adjunto/contexto~~ **HECHA** (`88761be`) | kimi-k2.7-code | qwen3.8-flash | ~~`app/src/components/{WorkspaceView,ComposerBar,ChatPanel}.tsx` + tests~~ | Drop/import → material adjunto al mensaje del usuario (attachmentIds lift a WorkspaceView, controlado a ComposerBar); llega al agente vía `agent_send` con ids; sin resource-item duplicado; tests deterministas. APPROVE (nits: race agentPhase estrecho, reset de attachmentIds al cambiar de proyecto — no bloqueantes). |
| C | ~~Error falso de arranque (STARTING/READY/FAILED)~~ **HECHA** (`c94e114`) | kimi-k2.7-code | qwen3.8-flash | ~~`app/src/App.tsx`, `WorkspaceView.tsx`, `messages.ts`, `types.ts`, tests~~ | Estados explícitos vía poll de `app_status`; "Preparando el asistente…"; solo fallo terminal real; `failed` recuperable (retry + auto-poll); tests cold/delayed/failure/recovery. APPROVE. |
| D | ~~Terminología de conversación~~ **HECHA** (`f44d507`) | Composer 2.5 | qwen3.8-flash | ~~`app/src/messages.ts`, `App.tsx`, tests, legacy naming~~ | `conversationDisplayName()` render-time: legacy "Proyecto sin título"/"Proyecto sin título N" → "Conversación nueva"; 8 AC cubiertos (default, legacy, user-renamed, sidebar, header, restart, ordering, sin Project terminology en DOM). APPROVE sin hallazgos. |
| E | Duplicado/texto verde | LOW (Composer 2.5 / mimo-v2.5) | qwen3.8-flash | `app/src/App.tsx`, `app/src/components/ChatPanel.tsx` | Eliminar doble render del contenido asistente; test de regresión |
| F | Eliminar conversación (backend semántica + UI) | Backend kimi; UI **Cursor Grok 4.6 High** | qwen3.8-flash (código) | backend: `crates/project-app/src/app.rs` (delete + unpublish), `crates/project-publication`, tests; UI: sidebar menú ⋮, ConfirmDialog, selección post-delete | Delete durable, fail-closed, recursos solo-exclusivos, no half-delete, confirmación en lenguaje llano, tests 10+ |
| G | Pass visual producto/UX **Cursor Grok 4.6 High** | Cursor Grok 4.6 High | Cursor Grok 4.6 High FRESH (UX) + qwen3.8-flash (código/a11y) | `app/src` (App shell, sidebar, timeline, composer, creación, adjuntos, settings X, menú) | Chat tipo mensajería; adjuntos en el mensaje; creación Abrir/Compartir; menú contextual; sin dashboard |
| T6 | Playwright headed | LOW | qwen3.8-flash | `app/` harness | 3 viewports, 16 capturas, aserciones |
| T7 | AppImage real + `./scripts/verify` | LOW/Composer | qwen3.8-flash | packaging M10 | AppImage con sidecars, lanzamiento real, verificación completa |

Orden sugerido: A → B → C (backend funcional, cada una con su worktree) →
D/E (LOW) → F-backend → F-UI + G (Grok) → review Grok → review qwen →
Playwright → AppImage → verify. Integrar solo commits revisados. NO M11.
**A, B, C, D YA integradas. La sesión siguiente empieza con Task E.**

## Worktrees

- `main` → `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-harness`
  (integración, `f44d507`; NO es workspace de autor).
- Worktrees de Task A, B y C removidos tras integración.
- Worktree de Task D (`../ai-publisher-corr-01-d`, `corr/d-conversation-terms`)
  y worktree de review D (`../ai-publisher-corr-01-d-review`) removidos; branch
  borrado tras integración de `f44d507`.
- Nuevos worktrees de la corrección: crear en paths hermanos (ej.
  `../ai-publisher-corr-01-<task>`) por tarea.

## Verificación y pruebas

- Frontend: `cd app && pnpm format:check && pnpm lint && pnpm typecheck && pnpm test`.
- Rust: `cargo fmt --check && cargo clippy --all-targets && cargo test`.
  (Drift de toolchain: rustup no instalado; clippy 1.98.0 vs pin 1.97.1 — el
  drift NO se manifestó en la corrida previa; si aparece, es preexistente.)
- Gate completo: `./scripts/verify` (exit 0 al final del pass).
- `git diff --check` limpio; `git status --short` limpio antes del handoff.
- Budget: `scripts/check-session-budget` ANTES de cada lanzamiento de worker
  (CONTINUE <80K; CHECKPOINT_WARNING 80K-99,999; ROTATE 100K-129,999; HARD >=130K).

## Aceptación final real (M9/M10 ya aprobados, no repetir)

- AppImage previo: `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`,
  180.816.376 bytes, SHA-256 `423cdb2815f23a01fa5e421c3203f47aea647cce3c1ea716fad1732e172fc8e2`.
- Modelo gratis real confirmado: `big-pickle` (providerID `opencode`), cost 0,
  respuesta "¡Hola! ¿Cómo puedo ayudarte?". `modelGetSelected`/`default_free_model`
  determinista (ADR-0015); NO hardcodear nombres (solo tests/fake).
- PATH-independencia y sidecars bundled ya verificados (M10).
- **Este pass NO re-testea M1-M10; solo integra las correcciones A-G, corre el
  Playwright headed y construye un AppImage NUEVO para revisión humana.**

## Model allocation (sesión anterior cerrada)

- Orquestador previo: `opencode-go/deepseek-v4-flash` (cerrada en bootstrap
  HARD). **Este pass:** deepseek-v4-flash (A/B/C/D integradas; rota en
  CHECKPOINT_WARNING tras Task D).
- **Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0 sesiones.** (seguir así;
  Qwen3.8 Max solo con ESCALATION_REASON explícito).
- Task A: autor `opencode-go/kimi-k2.7-code`, revisor `opencode-go/qwen3.8-flash`
  (APPROVE tras REQUEST_CHANGES). Ambos panes cerrados. Grok NO usado (G/F-UI
  pendientes).
- Task B: autor `opencode-go/kimi-k2.7-code`, revisor `opencode-go/qwen3.8-flash`
  (APPROVE, nits no bloqueantes). Commit `a16a07c`. Ambos panes cerrados.
- Task C: autor `opencode-go/kimi-k2.7-code` (`fd1d928`), revisor
  `opencode-go/qwen3.8-flash` (REQUEST_CHANGES → APPROVE; MAJOR-1 resuelto con
  regresión; NIT-1 poll cadence + MINOR-1 transient resend anotados no
  bloqueantes). Ambos panes cerrados. Backend sin cambios.
- **Task D: autor Composer 2.5 (`task-d-author`, commit `18ac233`), revisor
  `opencode-go/qwen3.8-flash` (`task-d-review`, APPROVE sin hallazgos). Ambos
  panes cerrados.** Grok NO usado (G/F-UI pendientes).

## Próximo paso (inmediato)

1. **Rotar orquestador** (budget CHECKPOINT_WARNING alcanzado tras integrar
   Task D; punto seguro: repo limpio, Task D verificada, panes de D cerrados).
2. Sesión nueva: leer este checkpoint, correr `scripts/check-session-budget`,
   continuar con **Task E** (duplicado/texto verde del contenido asistente,
   LOW) usando el circuito autor→reviewer→integración con el orden
   A→B→C→D→E→F→G→Playwright→AppImage. Tasks A-D NO se repiten (el diff de
   `f44d507`/`18ac233` es contexto).
3. NO iniciar M11. Terminar en AppImage NUEVO + `./scripts/verify` + STOP para
   aprobación humana.