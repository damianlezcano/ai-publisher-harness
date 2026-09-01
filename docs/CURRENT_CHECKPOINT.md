# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual (HUMAN_PRODUCT_REVIEW — FRESH CORRECTION PASS, TASK G INTEGRADA, ORQUESTADOR SESSION HEALTHY, 2026-09-01)

- **Current main commit: `2451c50`** (merge de Task G). La corrección A-G sigue
  INTEGRADA y verificada (ver detalle abajo). `git log --oneline -14` para el
  detalle.
- **TASK G INTEGRADA (`2451c50`).** Autor `cursor-grok-4.6-high` (HIGH_VISUAL,
  `corr/g-product-ux-pass`, commit `e345520`, pane `w1M:p1`), revisor UX
  independiente `cursor-grok-4.6-high` FRESH (`corr/g-product-ux-review`, pane
  `w1N:p1`, **APPROVE**, 5 nits no bloqueantes NIT-1..5), revisor
  código/a11y `opencode-go/qwen3.8-flash` FRESH
  (`corr/g-product-ux-a11y-review`, pane `w1P:p1`, **APPROVE**, verificado con
  tsc/eslint/prettier/vitest 201/201 en worktree detached, LOW/NIT no
  bloqueantes). `./scripts/verify` PASS en main tras G (EXIT=0: cargo verde,
  201 FE, fmt/lint/typecheck, M10 + UX_REDESIGN_01 contracts). Sin ciclos
  REQUEST_CHANGES (ambos APPROVE a la primera). Panes author y ambos reviewers
  cerrados tras APPROVE/integración. Worktrees `../ai-publisher-corr-01-g`,
  `../ai-publisher-corr-01-g-review`, `../ai-publisher-corr-01-g-a11y` y
  branches `corr/g-product-ux-pass`(-review/-a11y-review) a limpiar en cierre.
  Alcance Task G: solo `app/src` (20 archivos, +248/-97), sin backend, sin M11.
- **Session budget: CONTINUE al cierre de G (verificar antes del próximo
  lanzamiento)**. La sesión debe checkpointear Task G limpio y detenerse; la
  siguiente fase de validación (Playwright headed, AppImage NUEVO real, revisión
  humana) es un gate separado, no parte de Task G.
- **TASK F INTEGRADA (`6ea0e67`).** Autor `opencode-go/kimi-k2.7-code`
  (commits `26411f1` + fixes `14365c0` en `corr/f-conversation-delete`),
  revisor `opencode-go/qwen3.8-flash` FRESH (pane `w1K:p1`): REQUEST_CHANGES →
  APPROVE. El REQUEST_CHANGES fue por MAJOR-1 (delete-while-generating dejaba
  archivos huérfanos: `AgentService::run` hacía `create_dir_all` sin verificar
  metadata y el delete no cancelaba/serializaba contra el agente). Fixes
  aplicados por el MISMO autor (círculo autor→revisor respetado): delete
  serializa contra el per-project lock y cancela el run en vuelo antes de
  remover; `run_agent_with_inputs` falla rápido si metadata ya no existe;
  `AgentService::run` limpia el árbol huérfano si el proyecto desaparece
  mid-run; UI deshabilita "Eliminar" mientras la conversación genera; tests
  fail-closed (unpublish-failure aborta con datos intactos) y de error de
  delete (dialogo queda abierto, item preservado). `./scripts/verify` PASS en
  main tras F (199 tests frontend, cargo verde, M10 + UX_REDESIGN_01 contract
  ok). Panes author (`w1J:p1`) y reviewer (`w1K:p1`) cerrados tras APPROVE.
  Worktree `../ai-publisher-corr-01-f` y `../ai-publisher-corr-01-f-review`
  removidos; branch `corr/f-conversation-delete` y `-review` borrados.
- **Session budget: CONTINUE (58K al cierre de F)**. La sesión está sana y NO
  debe ampliar el trabajo automáticamente. El siguiente trabajo alto-valor es
  **Task G (pass visual producto/UX, Cursor Grok 4.6 High)** y debería idealmente
  arrancar desde una sesión de orquestador FRESH (Task G es trabajo de producto,
  no funcional; el contrato exige Grok solo vía Cursor). Este orquestador
  prefiere checkpointear Task F limpio y detenerse.
- **Progreso del pass:** Task A INTEGRADA (`e6389ea`). Task B INTEGRADA
  (`88761be`). Task C INTEGRADA (`c94e114`; autor kimi `fd1d928`, revisor qwen
  APPROVE tras REQUEST_CHANGES). Task D INTEGRADA (`f44d507`; autor Composer
  2.5 `18ac233`, revisor qwen APPROVE). Task E INTEGRADA (`cea141e`; fix LOW
  `60dc786`, revisor qwen FRESH APPROVE). **Task F INTEGRADA (`6ea0e67`; autor
  kimi `26411f1`+`14365c0`, revisor qwen REQUEST_CHANGES→APPROVE — ver detalle
  arriba)**. **Task G INTEGRADA (`2451c50`; autor Cursor Grok 4.6 High
  `e345520`, revisor UX Cursor Grok 4.6 High APPROVE, revisor código/a11y qwen
  APPROVE — ver detalle arriba)**. `./scripts/verify` PASS en main tras G.
  Tasks A-G hechas; **resta la validación final (T6 Playwright headed, T7
  AppImage NUEVO real, aprobación humana)**.
- **M11 NO iniciado.** Nada de M11 en esta corrección.
- **Trabajo previo integrado y conservado** (UX_REDESIGN_01): Task A modelo
  gratis real (`a3ef122`), Task B visual (`88fd346`), Playwright 44/44,
  AppImage real construido y verificado (detalles al final). NO reiniciar.
- **Pendiente: la revisión humana real del AppImage generó 16 hallazgos UX**
  (sección "Hallazgos humanos" abajo). Este pass los corrige. **A-G done;
  pendiente validación final (Playwright headed, AppImage NUEVO real, aprobación
  humana).**

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
16. **No hay forma contextual de eliminar conversaciones.** ~~`project_delete`
    existe end-to-end (commando `commands.rs:80-89`, `AppState::delete_project`
    `app.rs:310-317`, `ProjectService::delete_project` `project-core/src/lib.rs:807-811`,
    `api.ts:31`) pero NINGÚN componente lo llama; `ConfirmDialog` existe
    (type-name-to-confirm, `ConfirmDialog.tsx:15-48`) pero no está cableado.
    **Bug adicional detectado:** `delete_project` NO hace `unpublish` → entrada
    stale en `PublicationManager.published` y el proyecto aparece "shared"
    hasta reiniciar.~~ **RESUELTO EN TASK F (`6ea0e67`):** menú contextual "…"
    por conversación (Renombrar / Eliminar conversación) en
    `ConversationsSidebar.tsx`; `ConfirmDialog` cableado (type-name-to-confirm,
    copy en lenguaje llano); `delete_project` hace `unpublish` primero
    (fail-closed); delete serializa contra el agente (cancela run en vuelo) y
    limpia huérfanos; UI deshabilita Eliminar mientras genera; selección
    post-delete correcta (inactiva queda, activa → siguiente predecible, última
    → estado vacío "No hay conversaciones").

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
| E | ~~Duplicado/texto verde~~ **HECHA** (`cea141e`) | LOW (orquestador deepseek-v4-flash; raíz ya confirmada) | qwen3.8-flash | `app/src/App.tsx`, `app/src/components/ChatPanel.tsx`, `app/src/messages.ts`, tests | Eliminar doble render del contenido asistente (`.chat-status.ok` verde transitorio duplicaba la burbuja persistida; backend siempre persiste el mensaje terminal en `send_message_run`). `setAgentMessage(null)` en completed; se mantienen spinner working + `.err` failed (a11y role="alert"). 193 tests, verify PASS. APPROVE (NIT: CSS `.chat-status.ok` muerto). |
| F | ~~Eliminar conversación (backend semántica + UI)~~ **HECHA** (`6ea0e67`) | kimi-k2.7-code (`26411f1` + fixes `14365c0`) | qwen3.8-flash | ~~`crates/project-app/src/app.rs` (delete + unpublish + serialización agente), `crates/project-agent/src/service.rs` (cleanup huérfanos), `app/src/{App,components/ConversationsSidebar}.tsx`, `messages.ts`, `styles.css`, tests~~ | Menú "…" contextual (Renombrar/Eliminar), ConfirmDialog type-name, delete durable + fail-closed + unpublish primero, sin huérfanos (serialización/cancel contra agente + cleanup mid-run), selección post-delete correcta, última → estado vacío, renombrar preserva id/orden/activa, tests 13 AC. REQUEST_CHANGES→APPROVE (MAJOR-1 resuelto). |
| G | ~~Pass visual producto/UX~~ **HECHA** (`2451c50`) | Cursor Grok 4.6 High (`e345520`) | Cursor Grok 4.6 High FRESH (UX, APPROVE) + qwen3.8-flash (código/a11y, APPROVE) | ~~`app/src` (App shell, sidebar, timeline, composer, creación, adjuntos, settings X, menú)~~ | Chat tipo mensajería; adjuntos en el mensaje (📄 nombre [Abrir]); creación card icon+kind+Abrir/Compartir (EducAI decide el opener); URL de compartir visible; Settings con X en título (vuelve a la misma conversación); selector de modelo sin raw ids / sin hardcode de modelo gratis; sidebar "Conversaciones"; sin dashboard; sin fuga técnica. APPROVE (UX: 5 nits no bloqueantes NIT-1..5; código/a11y: LOW/NIT). |
| T6 | Playwright headed | LOW | qwen3.8-flash | `app/` harness | 3 viewports, 16 capturas, aserciones |
| T7 | AppImage real + `./scripts/verify` | LOW/Composer | qwen3.8-flash | packaging M10 | AppImage con sidecars, lanzamiento real, verificación completa |

Orden sugerido: A → B → C (backend funcional, cada una con su worktree) →
D/E (LOW) → F-backend → F-UI + G (Grok) → review Grok → review qwen →
Playwright → AppImage → verify. Integrar solo commits revisados. NO M11.
**A, B, C, D, E, F, G YA integradas (main `2451c50`). La siguiente fase es la
VALIDACIÓN FINAL: T6 Playwright headed, T7 AppImage NUEVO real, aprobación
humana del product owner. NO es M11.**

## Worktrees

- `main` → `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-harness`
  (integración, `2451c50`; NO es workspace de autor).
- Worktrees de Task A, B, C, D, E removidos tras integración.
- Worktree de Task F (`../ai-publisher-corr-01-f`, `corr/f-conversation-delete`)
  y worktree de review F (`../ai-publisher-corr-01-f-review`,
  `corr/f-conversation-delete-review`) removidos; branches borrados tras
  integración de `6ea0e67`.
- Worktrees de Task G (`../ai-publisher-corr-01-g` `corr/g-product-ux-pass`,
  `../ai-publisher-corr-01-g-review` `corr/g-product-ux-review`,
  `../ai-publisher-corr-01-g-a11y` `corr/g-product-ux-a11y-review`) cerrados en
  cierre de sesión tras integración de `2451c50`; branches a borrar.

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

- **Task G: autor `cursor-grok-4.6-high` (HIGH_VISUAL vía Cursor,
  `task-g-author`, pane `w1M:p1`, commit `e345520` en
  `corr/g-product-ux-pass`), revisor UX independiente `cursor-grok-4.6-high`
  FRESH (`task-g-ux-review`, pane `w1N:p1`, `corr/g-product-ux-review`,
  APPROVE con nits NIT-1..5), revisor código/a11y `opencode-go/qwen3.8-flash`
  FRESH (`task-g-a11y-review`, pane `w1P:p1`,
  `corr/g-product-ux-a11y-review`, APPROVE con LOW/NIT). Sin ciclos
  REQUEST_CHANGES. Merge `2451c50`, `./scripts/verify` PASS (EXIT=0). Panes
  cerrados tras APPROVE/integración; worktrees/branches de G a limpiar en
  cierre.**

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
- **Task E: fix LOW commit `60dc786` en `corr/e-duplicate-render` (implementado
  por el orquestador deepseek-v4-flash — desviación de proceso documentada: la
  raíz ya estaba confirmada en el checkpoint y el cambio es acotado; el circuito
  autor→revisor se respetó para la revisión), revisor `opencode-go/qwen3.8-flash`
  FRESH (`task-e-review`, pane `w1F:p18`, APPROVE tras verificar contrato backend
  `send_message_run`, tests 193/193, tsc/eslint/prettier). NIT no bloqueante:
  CSS `.chat-status.ok` (`app/src/styles.css:407`) quedó muerto (solo referencias
  en selectores negativos de test). Merge `cea141e`, branch borrado, worktree y
  pane de review cerrados.** Grok NO usado (G/F-UI pendientes).
- **Task F: autor `opencode-go/kimi-k2.7-code` (`task-f-author`, pane `w1J:p1`,
  commits `26411f1` + fixes `14365c0`), revisor `opencode-go/qwen3.8-flash`
  (`task-f-review`, pane `w1K:p1`, FRESH — REQUEST_CHANGES → APPROVE tras
  verificar MAJOR-1 resuelto con lock-ordering sólido, fail-closed testeado, y
  gates verdes 199 FE + cargo + fmt/clippy). Ambas panes cerrados. Merge
  `6ea0e67`, branches `corr/f-conversation-delete`(-review) borrados, worktrees
  removidos. Hallazgos residuales NO bloqueantes anotados por el revisor:
  MINOR TOCTOU (si delete completa en la ventana entre el pre-check de
  `run_agent_with_inputs` y que `AgentService::run` adquiera el lock del agente,
  puede quedar un scratch dir `projects/<id>/workspace` sin datos de usuario;
  fix recomendado: chequeo de existencia autoritativo DENTRO del lock del agente
  en `run`, antes de `create_dir_all`), NIT comment del pre-check (usa lock
  distinto al del agente), NIT test de serialización usa dos AppState
  independientes (pasa vía cleanup, no vía el mecanismo "waits"; añadir twin
  single-instance), NIT disable UI solo cubre la conversación seleccionada.
  Grok NO usado (G pendiente).**

## Mapa de Task F (contexto histórico — YA EJECUTADO en `6ea0e67`, conservado como contexto)

Backend delete ya existe end-to-end pero NO des-publica (bug hallazgo 16):
(MAPEO ORIGINAL — Task F ya resuelta arriba; se conserva para auditoría del
estado previo.)

- `project_delete` Tauri: `app/src-tauri/src/commands.rs:80-89`.
- `AppState::delete_project`: `crates/project-app/src/app.rs:310-317` — solo
  `self.projects.lock().delete_project(&pid)`, NO llama `self.unpublish` →
  entrada stale en `PublicationManager.published` (proyecto "shared" hasta
  restart). **Fix Task F: delete debe unpublish antes/consistente.**
- `ProjectService::delete_project`: `crates/project-core/src/lib.rs:807-811`:
  `get` → `repository.delete` (metadata) → `content.remove_project_tree`.
  `FilesystemProjectRepository::delete` borra el dir del proyecto
  (`project-fs/src/lib.rs:495-505`); `remove_project_tree` borra el árbol si
  existe (`project-fs/src/lib.rs:754-760`). Owner del árbol = proyecto (única
  duración; sin recursos compartidos entre proyectos: materials/creations son
  por-proyecto, `inputs/<id>` y `outputs/<id>` bajo `projects/<pid>/`).
- `AppState::unpublish`: `app.rs:1183-1189` → `PublicationManager::unpublish`
  (`crates/project-publication/src/manager.rs:306-340`, AlreadyLocal si no
  publicada, idempotente). Tauri `unpublish`: `commands.rs:349-353`.
- List ordenado por `updated_at` desc (`project-fs/src/lib.rs:438-442`);
  `rename_project` actualiza `updated_at` (`project-core/lib.rs:798-806`) →
  renombrar mueve al tope. Requisito F "order semantics unchanged": preservar
  esta regla (updated_at desc), no reintroducir otra.
- Rename UI ya existe: inline ✎ en `ConversationsSidebar.tsx:24-52,172-180`
  (usa `api.projectRename`, `api.ts:29-30`). Delete NO está cableado en la UI.

Frontend conversación:

- `app/src/App.tsx`: `conversations/selectedId/conversation` state;
  `refreshConversations` (28-33), `openConversation` (40-54), efecto inicial
  auto-crea default si lista vacía (56-86). Selection post-delete debe vivir
  aquí (refrescar lista, elegir activa, limpiar si última).
- `ConversationsSidebar.tsx` (189 líneas): props `conversations/selectedId/
  onSelect/onRefresh`; rename inline + ✎; NO delete. Agregar menú ⋮ contextual
  (Renombrar / Eliminar conversación) + ConfirmDialog. Copy catálogo en
  `app/src/messages.ts` (conversations.* / common.*); NO ProjectId/paths/términos
  técnicos en UX.
- `ConfirmDialog.tsx` (type-name-to-confirm, 15-48) existe y está testado
  (`ConfirmDialog.test.tsx`) pero NO cableado en conversaciones. UX F pide
  confirmación humana simple: "¿Eliminar esta conversación?" / "Se eliminarán
  los mensajes y los recursos asociados a esta conversación." / [Cancelar]
  [Eliminar]; visualmente destructivo; sin delete silencioso.
- `api.projectDelete`: `app/src/api.ts:31`. `AppError`/`errorMessage`:
  `api.ts:103-115`.
- Tests frontend: patrón `App.test.tsx` (mock invoke/listen), vitest + testing
  library; 193 tests verdes en main. Backend tests: `crates/project-app/tests/
  app_facade.rs` (delete ya en `project_lifecycle`), `project-fs/tests/
  project_lifecycle.rs` (delete: 1138, 1157, 1173), `project-publication/tests/`
  (unpublish idempotente).

Decisión ownership recursos: en la arquitectura actual NO hay recursos
compartidos entre conversaciones (cada proyecto es dueño exclusivo de su árbol);
el único estado cruzado durable es `PublicationManager.published` (manejar con
unpublish). Por lo tanto NO se requiere ARCHITECTURE_ESCALATION por ownership:
delete del proyecto borra su árbol completo (mensajes+materials+creations) y
debe unpublish primero para no dejar entrada stale. Si el autor encontrara una
referencia cruzada real no contemplada, debe parar y escalar, no adivinar.

## Próximo paso (inmediato)

Task G queda CERRADA e integrada en `2451c50` con `./scripts/verify` PASS
(EXIT=0) y budget CONTINUE. **NO ampliar el trabajo automáticamente en esta
sesión.**

1. **Checkpointear Task G limpio y detenerse** (preferido): este orquestador ya
   capturó el handoff durable; la validación final es un gate separado, no parte
   de Task G. M11 NO debe iniciar.
2. Sesión FRESH para la **VALIDACIÓN FINAL**: leer este checkpoint, correr
   `scripts/check-session-budget` (esperar CONTINUE), ejecutar **T6 Playwright
   headed** (3 viewports, 16 capturas, aserciones; revisor qwen3.8-flash),
   luego **T7 AppImage NUEVO real** con sidecars + `./scripts/verify`, y STOP
   para **aprobación humana del product owner** (solo el humano acepta el
   AppImage final). NO es M11.
3. **Seguimiento recomendado NO bloqueante (de las reviews de G):**
   - (UX NIT-1 / qwen LOW) `PublishPanel.tsx`: la URL pública es `<p>` dentro de
     `role="menu"`; envolver en `role="group"` (o mover los `<p>` al contenedor
     del popover) para no saltar el texto en lectores de pantalla.
   - (qwen LOW) `ComposerBar.tsx` `modelOptionLabel`: al caer a etiqueta genérica
     ("De pago"/"Gratis") cuando `name===modelId`, agregar el nombre del
     proveedor para evitar opciones indistinguibles.
   - (qwen LOW) `useShareControl.ts`/`WorkspaceView.tsx`: `onShare` en una
     tarjeta de creación abre el menú del ShareControl del composer (mismo hook,
     distinta ubicación); enfocar/anunciar el menú revelado.
   - (qwen/UX NIT) `messages.timeline.resourceLabel`, CSS `.message-resource`,
     `humanSize` (export sin uso) quedaron muertos; cleanup de catálogo/CSS.
   - (F review) chequeo de existencia de proyecto autoritativo DENTRO del lock
     del agente en `AgentService::run` + test single-instance delete↔agent.
4. NO iniciar M11. Terminar en AppImage NUEVO + `./scripts/verify` + STOP para
   aprobación humana.