# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual (UX/FUNCTIONAL FIXES PASS — MODALES, DETALLES DE CONVERSACIÓN Y ADJUNTOS = COMPLETE, FRESH REAL APPIMAGE `7f5714e6…` CONSTRUIDO, VERIFICACIÓN TÉCNICA COMPLETA, M11 NO INICIADO, GLIBC FUERA DE SCOPE, 2026-09-02)

- **PASS = COMPLETE (orquestador/autor: OpenCode/DeepSeek V4 Flash, sesión FRESH).** Pass ACOTADO de corrección UX/funcional sobre: (A) modal Configuración, (B) modal Detalles de la conversación, (C) "Abrir carpeta contenedora" repetido por archivo, (D) preview/apertura de archivos mal tipada, (E) imagen adjunta usada como input vs abierta, (F) sugerencia persistente del archivo anterior en el composer. **M11 NOT STARTED.** **GLIBC portability blocker PERMANECE FUERA DE SCOPE y NO resuelto aquí** (ver UNRESOLVED NEXT PACKAGING BLOCKER). Commit de implementación: `1a29c80`.
- **PROBLEMAS OBSERVADOS / CAUSA RAÍZ / CORRECCIÓN (A-F):** **(A)** Configuración desbalanceado/sin adaptación: `.dialog` base no tenía `max-height` ni scroll interno → todo el modal scrolleaba incluyendo header; `.session-logs` (pre de logs) NO tenía clase CSS → sin scroll interno. **Fix:** `Dialog.tsx` envuelve children en `.dialog-body` (flex 1, `overflow-y:auto`, min-height 0); `.dialog` gana `max-height:min(88vh,760px); overflow:hidden`; header sticky (`flex:0 0 auto`); `.provider-dialog` = `max-width:min(680px,calc(100vw - 32px))`; `.session-logs` = `max-height:220px; overflow:auto` (scroll interno). Logs efímeros (in-memory, sin persistencia) intactos. **(B)** Conversation Details no se adaptaba: usaba el `.dialog` base (max-width 420px, sin max-height) → contenido cortado. **Fix:** `.conversation-details-dialog` = `max-width:min(560px,calc(100vw - 32px))`; base `max-height:min(88vh,760px)`; body scrollea internamente; header/quitar sin cortarse. **(C)** "Abrir carpeta contenedora" se repetía por cada fila (`material_open_folder`/`creation_open_folder` por item). **Fix:** UNA acción por sección (Material subido → `materials_open_folder`, Creaciones generadas → `creations_open_folder`), comandos NUEVOS a nivel proyecto que resuelven los roots canónicos `inputs/`/`outputs/` vía `FilesystemProjectContentStore::materials_dir`/`creations_dir` (nuevos, con la misma disciplina symlink/containment que `material_path`/`creation_dir`); las filas ahora muestran nombre + tamaño + un "Abrir" (preview) individual, sin folder-open repetido. **(D)** Preview mal tipada: `PreviewModal` clasificaba SOLO por `contentType.startsWith("image/")` y TODO lo demás lo decodificaba como texto (`atob`+`TextDecoder`) → PNG/binario renderizados como basura. **Fix:** clasificación por contentType + **sniff de magic bytes** (PNG/JPEG/GIF/WebP) con fallback a imagen aunque el tipo declarado sea genérico; text-like (`text/*`, json, xml, yaml, js) y `text/html` → preview textual ESCAPADO (nunca HTML crudo); binarios no previsualizables → metadata (nombre/tamaño/kind) + "Abrir con la aplicación" (`onOpenExternal` → `materialOpen`/`creationOpen`), NUNCA texto basura. `CreationsPanel` y `ConversationDetails` pasan meta + open-external. **(E)** Imagen adjunta vs abierta: el adjunto del composer ya era input del turno (`attachmentIds` → `agent_send` → `resolve_attachments` → provision a `workspace/materials/` + prompt); la confusión era semántica de UI. **Fix:** preview = acción explícita separada (Abrir en chip/timeline/details); el flujo de envío NO dispara `material_open`/`preview_data` (test FE `WorkspaceView "uses an attached image as turn input without opening any preview"`); test backend NUEVO `attached_image_is_provisioned_as_creation_input_without_opening` (attachments 7/7) prueba que los bytes de la imagen llegan a `workspace/materials/` y la creación se registra. **(F)** Sugerencia del archivo anterior: el composer tenía un **material picker persistente** que listaba TODOS los materiales del proyecto como sugerencia al reabrir "Adjuntar". **Fix (root cause):** se eliminó el picker; "Adjuntar" abre SIEMPRE el diálogo nativo de archivo (el dedup `material_add_from_path` re-adjunta un material ya subido devolviendo el existente); el área de adjuntos del composer refleja SOLO el borrador del turno y se limpia tras send (`setAttachmentIds([])`); sin lista de sugerencias stale. CSS muerto del picker eliminado.
- **`./scripts/verify` EXIT=0 EN MAIN POST-PASS** (cargo fmt/clippy/test verdes, FE **236/236** en 21 archivos — antes 228 —, format/lint/typecheck, fetch-sidecars --check, M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Targeteds (todos en esta sesión, sin reuso):** `app_facade` **40/40** (+`folder_open_rejects_invalid_project_before_opening`), `attachments` **7/7** (+`attached_image_is_provisioned_as_creation_input_without_opening`), `opencode_adapter` **28/28**, `app_provider` **14/14**, `agent_service` **10/10**, `project-fs project_lifecycle` (incl. nuevo `materials_and_creations_folders_resolve_to_owned_fixed_roots`), preview `preview_security` 10/10 + `preview_lifecycle` 4/4 + `project-app --test preview` 9/9 (regresión-only, NO reabiertos). Nuevos tests FE: PreviewModal binario/sniff/html-escaped (3), ConversationDetails folder-por-sección + preview txt + PNG como imagen (2), ComposerBar dialog-nativo + sin-picker (2), WorkspaceView imagen-como-input + borrador-limpio (2), ProviderPanel clase responsiva/session-logs (1). `pnpm --dir app run build` (tsc + vite) OK.
- **FRESH REAL APPIMAGE CONSTRUIDO (working tree del pass = commit `1a29c80`) = PASS.** Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS)**. Artefacto: `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.931.064 bytes**, **SHA-256 `7f5714e665c16e844fbbf36a0cfca9ebaedf08e2d5b7770461e50f763b011152`** (NUEVO, difiere del previo `a227d1d1…`), timestamp 2026-09-02 18:56 -0300. **Frontend embebido NUEVO:** el binario `usr/bin/educai` del payload embebe `assets/index-BgrhG3j6.js` + `assets/index-DqoasHvf.css` (los hashes del `pnpm build` de este pass; el stale `index-BnZwruSz.js` NO está presente).
- **SIDECAR PINS VERIFICADOS EN PAYLOAD (byte-idénticos al sidecar fetcheado):** opencode **1.18.25** (payload sha `d91e0d33…`) y cloudflared **2026.8.3** (payload sha `f29324fe…`). `--version` en payload: opencode `1.18.25`, cloudflared `2026.8.3`. Sin repin/upgrade silencioso.
- **LANZAMIENTO REAL FEDORA/WAYLAND (DISPLAY=:0) = PASS.** Instancias stale del AppImage previo (`821048` @mount `EducAIeNAGFE`, `834278` @mount `EducAImNMMpj`) y sidecars huérfanos TERMINADOS (no usados como evidencia). Lanzado el artefacto NUEVO (PID 871903, setsid, PATH restringido `/tmp/opencode/path-indep-bin:/usr/bin:/bin`, `--debug`, stderr capturado en `/tmp/opencode/educai-launch-modal-pass.log`) con log: `[EducAI][INFO] startup version=0.1.0` + `[agent] backend starting → ready`, **SIN falso error de arranque**. WebKitNetworkProcess + WebKitWebProcess activos (UI usable). Sidecar hijo desde el mount propio del AppImage (`/tmp/.mount_EducAIOmFOjA/usr/bin/opencode`, port 35851, `/global/health` HTTP 200 `{"healthy":true,"version":"1.18.25"}`; `readlink /proc/<pid>/exe` = mount del AppImage → **PATH-independencia PASS**; `command -v opencode` = fail en el PATH restringido). cloudflared **2026.8.3** en el payload del mount.
- **REGRESIÓN-ONLY (NO reabierto, `./scripts/verify` EXIT=0 + targeteds):** "hola" contextual, chat secuencial, Creation request, Preview/Abrir (preview_lifecycle 4/4, preview_security 10/10, project-app preview 9/9), turn-link/causalidad de turno, modelo por conversación (rename + select/clear + aislamiento), logs de esta sesión efímeros, arquitectura de publicación real (publication suites verdes), Enter/Shift+Enter, badge compartido, delete «Sí», aislamiento de conversaciones — sin regresiones.
- **ESTADO DEL REPO:** main limpio (working tree clean después del build), HEAD `1a29c80` (commit de implementación del pass; el commit de checkpoint doc lo sigue). M11 NO INICIADO. Sin worktrees/branches/workers temporales. AppImage fresco en su ruta canónica.
- **MODELOS/POLÍTICA:** Cursor NO usado. GPT vía OpenCode Go PROHIBIDO — NO usado. Fase 100% OpenCode/DeepSeek V4 Flash (orquestador/autor), sin workers LLM (fase determinista: build + verify + probes contra sidecar real + tests). Session budget CONTINUE.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO funcional: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `7f5714e6…`, escenario ampliado con los 6 casos de prueba de este pass: (1) conversación con txt + imagen → abrir Detalles, listado de materiales, modal completo con scroll interno, preview correcto de txt e imagen; (2) creación simple con txt; (3) adjuntar imagen y pedir "agregala en el encabezado" → la imagen se usa como input de creación (sin abrir preview); (4) reabrir composer → NO sugiere el archivo anterior; (5) abrir Configuración → log viewer con scroll interno y solo-sesión; (6) redimensionar ventana → ambos modales se adaptan. Además: "hola" → saludo real, pregunta normal, Creation con adjunto, secuencial, Preview, Compartir, detalles desde el título, cambiar modelo por conversación sin afectar otras, Logs de esta sesión, y el escenario Finding 6 (modelo que falla → modelo que funciona → respuesta SOLO de ese turno). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## UNRESOLVED NEXT PACKAGING BLOCKER (registrado, NO abordado en esta sesión)

- **LINUX APPIMAGE PORTABILITY / REPRODUCIBLE BUILD BASELINE.** El AppImage construido en Fedora (`7f5714e6…`) fue observado fallando en KDE Neon 24.04 / Ubuntu Noble-family host porque las librerías bundladas requieren símbolos GLIBC más nuevos (GLIBC_2.42 / GLIBC_2.43 y requisitos ABI relacionados). **NO corregido en este pass por directiva: PERMANECE FUERA DE SCOPE del pass de modales/adjuntos y NO resuelto aquí.** Tras la re-acceptación funcional humana del AppImage fresco, se requiere un pass dedicado: **LINUX APPIMAGE PORTABILITY / REPRODUCIBLE BUILD BASELINE PASS** (build en un entorno baseline/glibc más viejo, verificación de símbolos GLIBC requeridos vs host destino, y definición de un baseline reproducible).

## Estado previo (FRESH REAL APPIMAGE POST-CONVERSATION-UX `a227d1d1…` — FINDINGS 1-6 VALIDADOS, TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE, preservado)

- **AppImage `a227d1d1…` (desde main `2b122e5`, merge `6886ba9`):** 180.931.064 bytes, SHA-256 `a227d1d1805a15570b71cd9fa9c1d6d09fd12787d6d392a4584cdddfbd809ee5`, FE embebido `index-BnZwruSz.js` + `index-UHkEOOtE.css`, sidecars opencode 1.18.25 (`d91e0d33…`) + cloudflared 2026.8.3 (`f29324fe…`). Lanzamiento real Fedora/Wayland PASS (PATH-independencia, sidecar propio del mount). **Finding 6 validado en runtime real** (ancla de turno inmutable + parent estricto + evicción de sesión en errores + fencing de workspace-scan; T1 fallido no resucita en T2/T4). **Findings 1-5 validados** ("Procesando tu solicitud…" transitorio; Conversation Details con rename/modelo por-conversación/material+creaciones; composer limpio tras send; card sin label redundante; Logs de esta sesión solo-sesión). **`./scripts/verify` EXIT=0** (FE 228/228, adapter 28/28, app_facade 39/39, app_provider 14/14, agent_service 10/10, preview 9/9 + 4/4 + 10/10). **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE, NO HUMAN ACCEPTED. M11 NO INICIADO.**

## Estado previo (CONVERSATION UX / PER-CONVERSATION MODEL / SESSION LOGGING / MODEL-SWITCH CAUSALITY PASS = COMPLETE — REVIEWS APPROVE, INTEGRADO EN MAIN `6886ba9`, preservado)

- **PASS = COMPLETE.** Orquestador FRESH (OpenCode/DeepSeek V4 Flash) bootstrapeó en main `456be54` (checkpoint previo: AppImage `832408b6…` técnicamente listo, human re-acceptance pendiente), clasificó el trabajo (Findings 1-6 + logging; Finding 6 = deep async/state causality → author Codex), lanzó UN solo autor Codex CLI (cuenta OpenAI/ChatGPT del owner, gpt-5.6-terra, continuado en gpt-5.6-luna por capacidad, worktree `../ai-publisher-ux-conversation-pass`, branch `corr/ux-conversation-pass`), 2 reviews independientes FRESH, ciclo de fixes acotado R1-R6 + tests FE, y fusionó. **Merge `6886ba9` en main** (ort, 30 archivos, +1203/−422, commits del autor `a61fc08`…`4d51a03` preservados). **`./scripts/verify` EXIT=0 EN MAIN POST-MERGE** (FE **228/228** en 21 archivos, cargo fmt/clippy/test verdes, format/lint/typecheck, fetch-sidecars --check, M10 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). Targeteds post-merge: `opencode_adapter` **28/28**, `app_facade` **39/39**, `project-agent --lib` **16/16**, `agent_service` **10/10**, session_log unit tests, FE **233/233 antes de quitar ModelSelector muerto → 228/228 final**. Working tree limpio.
- **REVIEW PRODUCT/UX INDEPENDIENTE (OpenCode DeepSeek V4 Flash, FRESH, `review-ux2`) = APPROVE.** "Procesando tu solicitud…" correcto y general (solo transitorio, nunca mensaje assistant persistido); Conversation Details descubrible por el título; rename/modelo/archivos jerárquicos y claros; modelo claramente por-conversación; sin control global duplicado (Configuración = proveedores + Logs); card de Creation simplificada con a11y preservada; composer limpio tras send con material retenido; Log Viewer "Logs de esta sesión" solo-sesión (in-memory, se pierde al reiniciar), niveles/autoscroll/Clear/Copy, metadata-only; causalidad de switch OK. REQUIRED findings resueltos: "Predeterminado de Configuración" ahora llama `conversation_model_clear` (vive, no error) + copy centralizado en `messages.ts`. Nits no bloqueantes: helpers muertos (`titleAria`/`selectAriaLabel`/`progress.creating`).
- **REVIEW CÓDIGO/CORRECTNESS INDEPENDIENTE (OpenCode Qwen 3.8 Flash, FRESH, `review-code2b`) = APPROVE.** MAJOR 1-4 y MINOR 5-8 resueltos: validación de modelo pineado contra `model_list()` al enviar (miss → WARN + fallback global, sin cuelgue); `conversation_model_clear` persiste `None`; tests de aceptación (validación/persistencia/aislamiento/clear, seguridad de rutas propias, `failed_send_evicts_cached_session_for_next_turn`, timeouts ajustados, ring 500/niveles/args/clear, ConversationDetails/title-click/composer-clean/Logs); a11y (aria-label `Detalles de <name>`, live region acotada + Refresh); robustez (evicción en TODOS los errores de send, `Poisoned` recuperado, deadline ÚNICO compartido anchor+poll). MINORs residuales no bloqueantes tras cleanup final `4d51a03`: casos B/D/E de Finding 6 con cobertura parcial (genérico is_err / log / Conflict sin test Rust dedicado), fallback send-time sin test directo, copy `requires_choice` re-apuntado a detalles de conversación, filtro de modelos conectados/gratis en ConversationDetails, `clearLogs` con try/catch. Sin regresión de seguridad (log metadata-only, sin archivos; folder-open solo rutas canónicas propias).
- **ROOT CAUSE EXACTO DE FINDING 6 (evidenciado con sidecar REAL pineado 1.18.25, NO adivinado):** `poll_session` recomputaba `originating_user_id = last_user_message_id` de la lista VIVA de mensajes en cada poll → un mensaje posterior podía cambiar qué user ID se trataba como el turno actual; además los fallbacks lenientes de `message_belongs_to_turn` (`(_,None)=>true`, `(None,Some)=>true`) podían atribuir un mensaje assistant stale (con `finish:"stop"` y parentID de un turno previo, o sin parentID) al turno actual. Tras un fallo de T1, si el último user message seguía siendo el de T1, la respuesta tardía de T1 (p.ej. lista de equipos) satisfacía el check y se entregaba como respuesta a "Hola" (T2).
- **FIX FINDING 6 (preservando causalidad correcta existente):** `a61fc08` captura el user message id del turno DESPUÉS de `prompt_async` (anchoring inmutable, id NO presente en el snapshot pre-send) y lo mantiene fijo mientras se polea; exige parentID EXACTO + `finish:"stop"`; evicción de la sesión cacheada en TODOS los errores de send (el siguiente turno abre sesión NUEVA). `f327c87` cercas el fallback de workspace-scan a archivos AUSENTES al inicio del turno (los leftovers de un turno fallido NO se registran en un turno posterior). `message_belongs_to_turn` estricto `_ => false`. Sin heurísticas de contenido, sin sleeps, deadline único acotado.
- **TRACE REAL Finding 6 (probe `/tmp/opencode/causality-probe`, sidecar pineado 1.18.25, XDG aislado, motor del worktree):** T1 `ses_f9c6fd94fffeDk0X1ULulz5kZF` modelo `missing/provider`, user `msg_063902701001RGKM58k04apJTM`, terminal `TaskFailed("timed out")` a `1788376816726`; T2 en sesión DISTINTA `ses_f9c6f6398ffe4UdSN2L5FwP9fV` modelo `opencode/big-pickle`, user `msg_063909c6f001BEiV69B4gpzETm` → assistant `msg_063909c7c001iFGKDrPWISJPUG` (parent=T2 user, `finish=stop`), `Completed` `1788376821440`, respuesta SOLO saludo "¡Hola! ¿En qué puedo ayudarte?", artifacts=[]. T1 NO resucitó, NO se atribuyó a T2. Trace crudo: `/tmp/opencode/causality-runtime-trace.txt`.
- **SEMÁNTICA DE MODELO POR-CONVERSACIÓN:** `Project.model` opcional (provider/model ids) en `project.json` con serde default (sin migración, schema v3 intacto); `resolve_agent_inputs` usa el modelo explícito de la conversación (validado contra `model_list()` al enviar, miss → WARN + fallback global) o el default global si `None`; `conversation_model_select`/`conversation_model_clear` bloqueados durante turno activo (try_lock → `Conflict`), lock `Poisoned` recuperado; el cambio aplica SOLO a turnos futuros, nunca muta turnos completados.
- **LOGGING / LOG VIEWER:** buffer en memoria de 500 entradas (OnceLock + Mutex + VecDeque, ring), espejo a stderr (`[EducAI][LEVEL] ...`), niveles ERROR/WARN/INFO/DEBUG (default INFO) con `--debug`/`--log-level` vía `configure_from_args` en lib.rs setup; SOLO-proceso (sin archivos, sin logs de sesiones previas tras restart); viewer "Logs de esta sesión" en Configuración con niveles, autoscroll, Refresh, Clear, Copy. Seguridad: solo metadata (ids, conteos, `safe_file_name`, duraciones, model/provider ids) — NUNCA prompts, texto de mensaje, credenciales, tokens, headers auth, contenidos de attachments ni HTML/CSS/JS generados.
- **FINDINGS 1-5 RESUELTOS:** (1) "Procesando tu solicitud…" transitorio general; (2) Conversation Details desde el título (rename, modelo por-conversación, Material subido + Creaciones generadas con "Abrir carpeta contenedora" solo rutas propias validada); (3) composer sin chip/sugerencia stale tras send (material/historial/Details retenidos); (4) Creation card sin label redundante (kind label + a11y preservada); (5) Configuración → Logs.
- **MODELOS/POLÍTICA:** Cursor NO usado (quota agotada). GPT vía OpenCode Go PROHIBIDO — NO usado; GPT únicamente vía Codex CLI (cuenta OpenAI/ChatGPT del owner): autor gpt-5.6-terra → gpt-5.6-luna (capacidad). Reviews: Product/UX = OpenCode DeepSeek V4 Flash FRESH; Code/Correctness = OpenCode Qwen 3.8 Flash FRESH. Ambos APPROVE sin REQUEST_CHANGES final.
- **ESTADO DEL REPO:** main limpio, HEAD `6886ba9` (merge), M11 NO INICIADO. Sin worktrees/branches/workers temporales (limpio al cierre). AppImage previo `832408b6…` SÍ queda STALE tras este merge → requiere rebuild.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximos gates: (1) **FRESH REAL APPIMAGE BUILD + TECHNICAL VERIFICATION** desde main `6886ba9` (`scripts/smoke-package appimage`, sidecars pineados 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real Fedora/Wayland, probe real del chat + switch de modelo); luego (2) **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** (escenario: conversación nueva, "hola" → saludo real, pregunta normal, Creation con adjunto, secuencial, Preview, Compartir, detalles de conversación desde el título, cambiar modelo por conversación sin afectar otras, Logs de esta sesión, y el escenario Finding 6: modelo que falla → volver a modelo que funciona → "Hola" → respuesta SOLO de ese turno). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (FRESH REAL APPIMAGE POST-MESSAGE-SELECTION — preservado)

- **FRESH REAL APPIMAGE CONSTRUIDO DESDE MAIN `5c17e13` (checkpoint del merge msg-selection `9e2c851`) = PASS (sesión FRESH, deepseek-v4-flash, orquestador).** El AppImage previo `30238e4f…` era **STALE** (construido desde `f3e9f30`, predata el merge `9e2c851`). Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS)**. Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.881.912 bytes**, **SHA-256 `832408b677be75a7b9c12f53348d7ef032ccdbcfc1e9418a17f98eab668c429d`** (NUEVO, difiere del stale `30238e4f…`), timestamp 2026-09-02 14:57:35 -0300, source commit `5c17e13` (main HEAD, working tree clean antes del build, sin cambios de producto sin commitear). Build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool tras el error esperado de linuxdeploy en Fedora 44).
- **PROVENANCE/EMBEDDED FRONTEND:** el binario `usr/bin/educai` del payload embebe el dist regenerado EN ESTE BUILD. Frontend embebido `assets/index-5bMlDLhr.js` + `assets/index-UHkEOOtE.css` (los MISMOS hashes del build previo — esperado: el diff del pass msg-selection `99e8f52..9e2c851` es SOLO Rust, 5 archivos +298/−106, sin cambios FE). **El binario fresh embebe el texto honesto de fallo "No recibimos una respuesta" (probe UTF-8 en binario = True)**. No hay hardcode "Listo." de respuesta: las únicas 2 ocurrencias literales "Listo." en el binario son strings del SYSTEM PROMPT de instrucción (service.rs: "por ejemplo: \"Listo. Creé el recurso…\" / "ni respondas solo \"Listo.\" antes de haber escrito el recurso"), NO un fallback de reply — verificado por contexto UTF-8 en binario.
- **SIDECAR PINS VERIFICADOS EN PAYLOAD (byte-idénticos al sidecar fetcheado):** opencode **1.18.25** (payload sha `d91e0d33…` = binario extraído; el pin del manifest `58a3729a…` es del tarball, consistente con checkpoints previos) y cloudflared **2026.8.3** (payload sha `f29324fe…` = pin `config/components.json`). `--version` en payload: opencode `1.18.25`, cloudflared `2026.8.3`. Sin repin/upgrade silencioso.
- **`./scripts/verify` EXIT=0 EN MAIN POST-BUILD** (cargo fmt/clippy/test verdes, FE **228/228** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 version 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Targeteds post-merge:** `opencode_adapter` **27/27** (incluye `send_selects_only_new_turn_terminal_text_and_excludes_reasoning`, `sequential_sends_select_each_current_turn_response`, `growing_assistant_message_resets_grace_until_stop`), `app_facade` **37/37** (incluye `missing_agent_text_does_not_become_misleading_listo`), `agent_service` **10/10**, `project-agent --lib` **15/15**, preview `preview_lifecycle` 4/4 + `preview_security` 10/10 + `project-app --test preview` 9/9. Sin reuso de resultados viejos (todo corrido en esta sesión).
- **LANZAMIENTO REAL FEDORA/WAYLAND (DISPLAY=:0) = PASS.** Procesos viejos EducAI (PIDs 424166/424466 @`pnbpGK`, 503638/503938 @`CFNFfP`, del artifact stale `30238e4f…`) TERMINADOS (stale, no usados como evidencia). Lanzado el artefacto NUEVO (PID 571232, setsid, 14:58) con PATH restringido `/tmp/opencode/path-indep-bin:/usr/bin:/bin` (sin opencode/cloudflared externos; `command -v opencode` = fail en PATH restringido). Log: `[agent] backend starting → ready`, SIN falso error de arranque. WebKitNetworkProcess + WebKitWebProcess activos (UI usable). Sidecar hijo desde el mount propio del AppImage (`/tmp/.mount_EducAIjdhhkl/usr/bin/opencode`, port 35237, `/global/health` HTTP 200, `{"healthy":true,"version":"1.18.25"}`; `readlink /proc/<pid>/exe` = mount del AppImage → **PATH-independencia PASS**). cloudflared **2026.8.3** en payload del mount (`/tmp/.mount_EducAIjdhhkl/usr/bin/cloudflared`, `--version` = 2026.8.3).
- **ADAPTER REAL PROBE (el adapter corregido `OpenCodeAgentEngine` de MAIN `5c17e13` compilado en probe standalone y corrido contra el sidecar REAL pineado 1.18.25 en vivo) = PASS.** Probe `realprobe` (crate temporal en `/tmp/opencode/realprobe`, path-dep a `project-agent` de main, modelo `opencode/big-pickle` free, config XDG aislada, 4+2 turnos en UNA sesión):
  - **CASE A "hola"** → `Completed` 4.4s, message `"¡Hola! ¿En qué puedo ayudarte?"` — **respuesta REAL contextual, NO "Listo."**.
  - **CASE B pregunta normal** → `"París"` (responde exactamente esa pregunta).
  - **CASE C Creation/tool turn** → `Completed` 9.2s, message `"Creé `index.html` en el directorio de trabajo con un saludo de la actividad 'Prueba Real'."` + **archivo real `index.html` en disco** (artifacts en adapter = [] porque `/diff` real devuelve `[]` para archivos commiteados → fallback documentado workspace-scan en `service.rs`); sin nudge; final `finish:"stop"` con texto final correcto.
  - **CASE D turno secuencial** → `"Mercurio"` (correcto para M4, sin one-turn-behind, sin stale reuse).
  - **TURN5 lento** → tarea de 30 archivos `test_01..30.txt`: `Completed` a los **38.3s** (>15s grace viejo, NO cortado) con texto final real; 30 archivos en disco. **TURN6 post-abort** → `"4"` (sesión usable tras cancel).
- **SLOW-BUT-VALID (>15s) EXPLÍCITO = PASS (probe3).** Tarea de 30 archivos `s_01..30.txt`: `Completed` a los **40.1s** con texto terminal real (`"Serie `s_01.txt` a `s_30.txt` creada: 30 archivos…"`), 30 archivos en disco. **El viejo grace de 15s NO corta la respuesta lenta** — la espera corre hasta `finish:"stop"` (acotada por `task_timeout`). Sin sleeps introducidos.
- **BOUNDED DEADLINE / FAILURE = PASS.** probe3 con `task_timeout=4s`: `TaskFailed("timed out")` a los **4.0s** (sin espera infinita). probe4 (tarea que no termina, abort a los 10s desde hilo): `cancel()` → `Ok(())`, el send termina limpio en el deadline absoluto 120s (`TaskFailed("timed out")`), **sin fake "Listo."**; **el turno siguiente en la MISMA sesión responde `"18"` correctamente** (sin envenenar el siguiente turno, sin stale reuse). Probe1 TURN6 post-abort idem (`"4"`). Cobertura determinista ya en adapter 27/27 (`send_never_idle_times_out`, `send_idle_without_new_assistant_message_times_out`).
- **TRACE REAL "hola" (sidecar 1.18.25, `/session/<id>/message`) = evidencia de selección:** user `msg_0634da8790…` (text "hola") → assistant `msg_0634da88d0…` con **`parentID` = id del user message** (turn-link correcto), parts `[]` en T+1..3, `finish:"stop"` + parts `text:"¡Hola! ¿En qué puedo ayudarte?"` en T+4. **La selección autoritativa (watermark + turn-link VIVO + `finish:"stop"` + solo parts `type=="text"`) eligió exactamente ese mensaje nuevo turn-linked.** Sin heurística de longitud/keyword, sin content-length, sin sleeps.
- **RESPONSE-SELECTION REGRESSION (§16) = PASS (verificado en main + binario + runtime):** sin hardcode "Listo." de fallback (solo strings de instrucción del system prompt en binario); identidad de mensaje assistant correcta; identidad de turno stale NO reusada (watermark + parentID == último user message VIVO); selección final respeta el turno actual; `finish:"tool-calls"` NO terminal (nunca displayeado como completado — cubierto por `send_does_not_treat_intermediate_text_as_terminal_before_artifacts` y 4+ trazas reales); `finish:"stop"` ÚNICO terminal normal; sin heurística de contenido. Pinned sidecar real 1.18.25 usado en TODOS los probes (NO mocked).
- **PREVIEW/ABRIR PRESERVADO (regresión-only, NO reabierto):** preview `preview_lifecycle` **4/4**, `preview_security` **10/10**, `project-app --test preview` **9/9**, FE `CreationsPanel.test.tsx` **12/12** (Abrir usa `preview_open_web` con el MISMO `creation.id` que Compartir). Adjuntos, cards, publicación Cloudflare real, modelo en Configuración, delete «Sí», Enter/Shift+Enter, badge, aislamiento, storage, workspace binding: sin cambios (diff msg-selection = solo backend chat, tree limpio, verify EXIT=0).
- **MODELOS/POLÍTICA:** Cursor quota agotado — **NO usado**. **GPT vía OpenCode Go PROHIBIDO en esta fase** — NO usado. Orquestación 100% OpenCode/DeepSeek V4 Flash. Sin workers LLM lanzados (fase determinista: packaging + probes contra sidecar real + tests). Session budget CONTINUE (~12K al inicio).
- **ESTADO DEL REPO:** main limpio (working tree clean antes y después del build), HEAD `5c17e13`, M11 NO INICIADO. Sin worktrees/branches/workers temporales (los probes son crates temporales bajo `/tmp/opencode/realprobe`, fuera del repo). AppImage fresco en su ruta canónica.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `832408b6…` (escenario §22: lanzar el AppImage exacto, conversación NUEVA, enviar `hola`, observar respuesta contextual real — si es "Listo." el humano FALLA de inmediato; luego pregunta normal, Creation, secuencial, Preview, Share/update). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (OPENCODE ASSISTANT MESSAGE SELECTION / FINAL RESPONSE SEMANTICS PASS = COMPLETE — REVIEWS APPROVE, INTEGRADO EN MAIN, M11 NO INICIADO, 2026-09-02)

- **PASS = COMPLETE.** Orquestador FRESH (deepseek-v4-flash, sesión CONTINUE ~66K) bootstrapeó (checkpoint `99e8f52`, main limpio, worktree autor `../ai-publisher-msg-selection-pass` = `corr/msg-selection-pass` head `d0ca259` SIN mergear, base `405ecfe`, budget CONTINUE, sesión Luna 279K CERRADA y NO reutilizada), lanzó 2 reviews independientes FRESH sobre diff `405ecfe..d0ca259` (5 archivos, +298/−106) en worktree de review `../ai-publisher-msg-selection-review` (detached `d0ca259`), y fusionó. **Merge `9e2c851` en main** (ort, 5 archivos, +298/−106, commits del autor `fe636cc`, `ca58d19`, `969c001`, `d0ca259` preservados). **`./scripts/verify` EXIT=0 EN MAIN POST-MERGE** (cargo fmt/clippy/test verdes, FE **228/228** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). Targeteds post-merge: `opencode_adapter` **27/27**, `app_facade` **37/37**, `agent_service` **10/10**. Working tree limpio.
- **REVIEW PRODUCT/UX INDEPENDIENTE (OpenCode DeepSeek V4 Flash, FRESH, `product-ux-review` pane `w1F:p1W`, worktree de review) = APPROVE.** CASE A: no "Listo." fallback en código de producto; vacío+sin creación → `MessageStatus::Failed` + "No recibimos una respuesta. Probá de nuevo.", vacío+con creación → texto honesto de creación (app.rs). CASE B: `authoritative_assistant_text` devuelve solo el mensaje nuevo turn-linked `finish:"stop"`, parts `type=="text"` (reasoning/step excluidos). CASE C: `finish:"tool-calls"` NO terminal; grace 15s y seam `with_idle_grace` REMOVIDOS → texto intermedio nunca cierra el turno, turnos lentos corren hasta `finish:"stop"` (acotados por `task_timeout` 120s). CASE D: watermark + `parentID == id del último user message VIVO` mantienen causalidad secuencial. Verificado por reviewer: `opencode_adapter` 27/27, `app_facade` 37/37, `project-agent --lib` 15/15. Sin regresión a Preview. NOTAS no bloqueantes: parentID estricto con fallback leniente cuando falta parentID; el "Listo." real del modelo sigue surfaciendo por diseño (semántico, correcto).
- **REVIEW CÓDIGO/CORRECTNESS INDEPENDIENTE (OpenCode Qwen 3.8 Flash, FRESH, `code-review` pane `w1F:p1X`, worktree de review) = APPROVE.** Hardcode "Listo." eliminado (app.rs:1448-1459) sin regresión blank (AgentRunView.message siempre Some). Maquinaria de grace REMOVIDA por completo (IDLE_WITHOUT_TEXT_GRACE, ACK_WITHOUT_ARTIFACTS_GRACE, idle_since/idle_artifacts, with_idle_grace); grep repo-wide sin referencias colgantes. Selección turn-autoritativa (watermark + turn-link VIVO + `finish:"stop"` único terminal); `tool-calls` NO terminal; `message_text` solo parts `type=="text"`, fallback a `content` solo sin parts. **Espera ACOTADA:** deadline absoluto `now + task_timeout` (DEFAULT_TASK 120s) → Timeout → `TaskFailed("timed out")`; failed/error/failure → `TaskFailed`; abort → `Cancelled`; non-2xx → Http error. Cubierto por `send_never_idle_times_out` / `send_idle_without_new_assistant_message_times_out`. Sin sleeps reintroducidos (solo tick 20ms). NOTAS no bloqueantes: comentario duplicado colgante en `opencode.rs:118` (cosmético); comentario/doc de `assistant_message_is_terminal` (373-374) y nombre de test `growing_assistant_message_resets_grace_until_stop` aún referencian semántica de grace removida; `crates/project-provider/src/adapter.rs:439` conserva su propio `last_assistant_text_from_messages` pero es el path separado de connection-test (acotado, honesto "Conectado."), NO el chat reply flow — no tocado, aceptable. Comandos corredos por reviewer (worktree, pipefail): `opencode_adapter` 27 passed EXIT=0, `app_facade` 37 passed EXIT=0, `project-agent --lib` 15 passed EXIT=0, `cargo fmt --check` EXIT=0, `cargo clippy --all-targets -D warnings` EXIT=0. `./scripts/verify` NO corrido por reviewer (sidecars gitignored ausentes en worktree), corrido por orquestador en main = EXIT=0.
- **AUTOR (OpenCode / GPT-5.6 Luna, `msg-selection-luna`, pane `w1F:p1T` — CERRADO, NO reutilizado):** commits `fe636cc` (selección autoritativa: requiere mensaje assistant nuevo + turn-linked + `finish:"stop"` + solo parts de texto humano; texto vacío → NUNCA "Listo."), `ca58d19` (correlación de turno: `originating_user_id` recomputado de la lista VIVA en cada poll, no del snapshot pre-send), `969c001` (reset de grace por firma de progreso), `d0ca259` (eliminación del ack-grace early-return 15s + maquinaria muerta: espera solo `finish:"stop"` o deadline de tarea). Total 5 archivos, +298/−106 (vs base `405ecfe`).
- **ROOT CAUSE EXACTO DE "Listo." (evidenciado con sidecar REAL pineado 1.18.25, NO adivinado):** mecanismo B. Live OpenCode 1.18.25 devuelve para `"hola"` un mensaje assistant con `parentID` apuntando al user message del turno, parts que evolucionan `[] → step-start → reasoning → text → step-finish` y `finish:"stop"` recién al final (latencia 4–19s variable). El código viejo: (1) `assistant_reply_text(None/empty)` en `app.rs` hardcodeaba `"Listo."`; y (2) el ack-grace de 15s podía cortar un turno lento y devolver mensaje vacío → `"Listo."`. El adaptador viejo además seleccionaba el último texto assistant de TODA la sesión (sin watermark ni turn-link) y concatenaba parts incluyendo reasoning.
- **SELECCIÓN ANTES:** `last_assistant_text_from_messages` iteraba TODA la sesión (sin watermark ni linkage) → texto de turnos previos o intermedios; `assistant_reply_text(None)` → hardcode `"Listo."`; grace 15s podía devolver vacío. **SELECCIÓN DESPUÉS:** watermark `assistant_index >= before_assistant_count` (ancla determinista del pass previo) + `message_belongs_to_turn` con `parentID == id del último user message VIVO` + `assistant_finish == "stop"` como único terminal normal + `message_text` que toma SOLO parts `type=="text"` (excluye reasoning/step/system) y cae a `content` solo si no hay parts; app.rs: vacío+sin creación → `MessageStatus::Failed` + texto honesto ("No recibimos una respuesta. Probá de nuevo."), vacío+con creación → texto honesto de creación sin explicación, NUNCA "Listo.".
- **RUNTIME REAL (preservado del autor, sidecar pineado 1.18.25, `sidecars/opencode-x86_64-unknown-linux-gnu` — verificado presente, `--version` = 1.18.25):** TASK HOLA → saludo real ("¡Hola! ¿Cómo puedo ayudarte?…", incl. una corrida lenta de 19.3s que ANTES devolvía vacío); TASK QUESTION → "París"/"París."; TASK CREATION → texto final real ("Listo. Creé `index.html`…") + archivo `index.html` en disco (artifacts en adapter = [] porque `/diff` real devuelve `[]` para archivos commiteados → fallback documentado workspace-scan en `service.rs`, tests existentes `workspace_scan_registers_when_diff_is_empty`); TASK SEQ_Q4 → "Marte". Mensajes dumpados con IDs/finish/parentID/parts reales. Sin empty en ninguna corrida tras `d0ca259`. El orquestador NO re-corrió el probe live (no rehace el diagnóstico del autor; evidencia real preservada + tests + reviews).
- **TESTS:** adapter **27/27** (nuevos: `send_selects_only_new_turn_terminal_text_and_excludes_reasoning`, `sequential_sends_select_each_current_turn_response`, `growing_assistant_message_resets_grace_until_stop`; FakeServer ahora agrega user message + assistant con `parentID` real y soporta `messages_sequence`), `app_facade` **37/37** (nuevo: `missing_agent_text_does_not_become_misleading_listo`), `agent_service` **10/10**, FE **228/228**. Unit `opencode.rs` nuevos: `human_text_parts_win_over_mixed_content`, `final_text_requires_new_linked_stop_message`.
- **GATE de no-regresión (confirmado):** `finish:"tool-calls"` NO terminal; `finish:"stop"` ÚNICO terminal normal; Creation no terminal independiente; sin nudge; un turno activo por conversación; user message persistido antes de ejecución; Creation del turno origen; PREVIEW/ABRIR = PASS humano (NO reabierto); sin heurística de texto, sin filtro literal "Listo.", sin sleeps, deadline de fallo ACOTADO preservado; M11 NO INICIADO. NIT menor no bloqueante: comentario duplicado en `opencode.rs:118-120` (cosmético).
- **POLÍTICA/MODELOS:** Cursor quota agotado — NO usado. GPT vía OpenCode Go PROHIBIDO para este pass de reviews (autor Luna fue OpenCode Go GPT-5.6 Luna por directiva explícita del owner en el pass previo; en ESTA sesión de orquestación/review SOLO DeepSeek V4 Flash y Qwen 3.8 Flash, sin GPT). Reviews: Product/UX = OpenCode DeepSeek V4 Flash FRESH; Code/Correctness = OpenCode Qwen 3.8 Flash FRESH; ambas APPROVE sin REQUEST_CHANGES → sin Codex fixer. **Session budget CONTINUE (~66K al merge).**
- **PROXIMO GATE (único): FRESH REAL APPIMAGE BUILD + TECHNICAL VERIFICATION** desde main `9e2c851` (`scripts/smoke-package appimage`, sidecars pineados opencode 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real Fedora/Wayland, probe real del chat "hola"/question/creation/seq), luego **HUMAN PRODUCT-OWNER RE-ACCEPTANCE**. **M11 NO INICIADO — NO configurar M11 como próximo.**

## Estado previo (FRESH REAL APPIMAGE POST-CHAT-CAUSALITY CONSTRUIDO DESDE MAIN `f3e9f30`/MERGE `2091ec3` — BUILD + VERIFICACIÓN TÉCNICA COMPLETA, TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE, M11 NO INICIADO, 2026-09-02)

- **FRESH REAL APPIMAGE CONSTRUIDO DESDE MAIN `f3e9f30` (checkpoint del merge chat-causality `2091ec3`) = PASS (sesión FRESH, deepseek-v4-flash, orquestador).** El AppImage previo `40403c69…` era **STALE** (construido 10:54 desde `71ff7bf`, predata el merge `2091ec3`). Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS)**. Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.877.816 bytes**, **SHA-256 `30238e4f5940e2834f614c88bb2d92f89f3b993c963b3bff2a9166230c384ab8`** (NUEVO, difiere del stale `40403c69…`), timestamp 2026-09-02 12:38 -0300, source commit `f3e9f30` (main HEAD, working tree clean antes del build, sin cambios de producto sin commitear). Build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool tras el error esperado de linuxdeploy en Fedora 44).
- **PROVENANCE/EMBEDDED FRONTEND:** el binario `usr/bin/educai` embebe el dist regenerado EN ESTE BUILD (dist generado 12:37:25, binario 12:37:57): referencia exacta `assets/index-5bMlDLhr.js` + `assets/index-UHkEOOtE.css` (hashes NUEVOS de este build); el asset stale `index-BFehLbJS.js` NO está presente. Marcador de corrección `turnId` presente en el binario embebido. `index.html` del dist referencia `index-5bMlDLhr.js`.
- **SIDECAR PINS VERIFICADOS EN PAYLOAD (byte-idénticos al sidecar fetcheado):** opencode **1.18.25** (payload sha `d91e0d33…` = binario extraído; el pin del manifest `58a3729a…` es del tarball, consistente con checkpoints previos) y cloudflared **2026.8.3** (payload sha `f29324fe…` = pin `config/components.json`). `--version` en payload: opencode `1.18.25`, cloudflared `2026.8.3`. Sin repin/upgrade silencioso.
- **`./scripts/verify` EXIT=0 EN MAIN POST-BUILD** (cargo fmt/clippy/test verdes, FE **228/228** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 version 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Targeteds:** `opencode_adapter` **24/24**, `app_facade` **36/36**, `agent_service` **10/10**, preview `preview_lifecycle` 4/4 + `preview_security` 10/10 + `project-app --test preview` 9/9. Sin reuso de resultados viejos (todo corrido en esta sesión).
- **LANZAMIENTO REAL FEDORA/WAYLAND (DISPLAY=:0) = PASS.** Procesos viejos EducAI (PIDs 10188 @08:34, 242114 @11:02, montajes `.mount_EducAIecNFPh`/`.mount_EducAIJCJKGB`) TERMINADOS (stale, no usados como evidencia). Lanzado el artefacto NUEVO (PID 416057, setsid, 12:44) con PATH restringido `/tmp/opencode/path-indep-bin:/usr/bin:/bin` (sin opencode/cloudflared externos; `command -v opencode` = fail). Log: `[agent] backend starting → ready`, SIN falso error de arranque. WebKitNetworkProcess + WebKitWebProcess activos (UI usable). Sidecar hijo desde el mount propio del AppImage (`/tmp/.mount_EducAIBPhCpe/usr/bin/opencode`, port 41849, HTTP 200 en `/global/health`; `readlink /proc/<pid>/exe` = mount del AppImage → **PATH-independencia PASS**). cloudflared presente en payload del mount.
- **TARGETED CHAT-TURN CAUSALITY VALIDATION = PASS (runtime real, NO solo mocked).** Validación con el sidecar REAL 1.18.25 empaquetado (port 41849, modelo `opencode/big-pickle` free, config app aislada): **tarea real con tool real** creó archivos en un workspace real. Traza real de mensajes `/session/<id>/message?limit=`:
  - **CASE B (turno de creación):** user → assistant `finish:"tool-calls"` (intermedio, con tool part) → assistant `finish:"stop"` (final) + texto "Created `smoke.txt`…" + artefacto real `smoke.txt`=`smoke-ok`. **El artefacto `live.txt` apareció en disco a las 12:50:58 MIENTRAS el último mensaje seguía en `finish:"tool-calls"`/None; el marker `stop` recién llegó a las 12:52:04** → artefacto antes de `stop` NO es terminal (evidencia real, sin nudge).
  - **CASE C (turnos secuenciales):** 2 turnos completos en una sesión: turno1 `smoke.txt`/`smoke-ok` + turno2 `second.txt`/`second-ok`, cada uno con su `finish:"tool-calls"`→`finish:"stop"` y su resultado correspondiente al request origen; sin one-turn-behind. Además probe del adapter real (abajo) hizo 3 turnos.
  - **CASE E (cancel/failure):** `POST /session/<id>/abort` = HTTP 200; la sesión sigue usable (2º prompt 204) y el turno post-abort produjo `after_abort.txt` con secuencia `tool-calls`→`stop` (sin envenenar el siguiente turno). Sin espera infinita: la espera de ack-grace (15s) devuelve `Completed` con texto vacío si el agente está genuinamente idle sin `stop` (limitación conocida documentada) y el timeout de tarea mapea a `TaskFailed("timed out")` (test `send_idle_without_new_assistant_message_times_out`).
  - **`/session/status` REAL = `{}`** (mapa vacío) aun con trabajo activo → `{}` NO es terminal (consistente con el root cause). `/diff` real devuelve `[]` para archivos commiteados → el path de Creation usa el fallback de workspace-scan (test `workspace_scan_registers_when_diff_is_empty` verde) — comportamiento por diseño, no regresión.
- **ADAPTER REAL PROBE (el adapter corregido `OpenCodeAgentEngine` compilado contra el sidecar REAL 1.18.25 en vivo, 3 turnos, tarea de artefacto real, sin nudge) = PASS.** `ensure_ready` version=1.18.25; turn1 `Completed` (6.9s, msg "Created `probe1.txt`…", archivo en disco); turn2 `Completed` (15s grace, archivo `probe2.txt` en disco); turn3 (misma sesión) `Completed` (112.4s, `probe3.txt`). Los 3 archivos en disco con el contenido exacto. `PROBE_EXIT=0`.
- **COMPLETION-MARKER SEMANTICS (evidencia real):** `finish:"tool-calls"` = INTERMEDIO (NO terminal) — 4+ trazas reales lo muestran con trabajo/artefactos continuando; `finish:"stop"` = ÚNICO terminal normal (todos los turns completos lo tienen); artefactos observados antes de `stop` NO cierran el turno (evidencia `live.txt` 12:50:58 vs `stop` 12:52:04); ausencia de terminal normal con condición de fallo/cancel explícita NO cuelga infinito (abort 200 + timeout a `TaskFailed("timed out")`). Determinista cubierto por `opencode_adapter` 24/24 (incluye `send_does_not_treat_intermediate_text_as_terminal_before_artifacts`, `send_does_not_treat_brief_listo_as_complete_before_artifacts`, `send_completes_on_explicit_stop_without_files`, `send_idle_without_new_assistant_message_times_out`).
- **CREATION CORRELATION = PASS (donde el tooling lo permite).** Creation pertenece al turno origen (turn_id → `Map<projectId, turnId>` en App.tsx; eventos stale ignorados); no requiere el siguiente envío (marker stop → retorno con fetch de artefactos); scan/artefacto no cierra el turno antes del marker; sin re-registro duplicado (`later_turn_does_not_reregister_prior_workspace_files` verde). Preview/Abrir sigue abriendo la misma Creation (`preview_lifecycle` 4/4, `preview_security` 10/10, `project-app --test preview` 9/9, FE `CreationsPanel` mismo `creation.id`).
- **PREVIEW/COMPARTIR/ETC. PRESERVADO (no regresión):** Preview/Abrir = PASS humano (NO reabierto). Adjuntos, cards, publicación Cloudflare real, modelo en Configuración, delete «Sí», Enter/Shift+Enter, badge, aislamiento, storage, workspace binding: sin cambios en este build (tree limpio, verify EXIT=0). Compartir/update cubierto por `app_facade` 36/36 (incluye `publish_promotes_the_generated_web_creation_as_the_public_entry`, `later_turn_updates_the_same_web_creation_and_refreshes_publish`).
- **ESTADO DEL REPO:** main limpio (working tree clean), HEAD `f3e9f30`, M11 NO INICIADO. Sin worktrees/branches/workers temporales. AppImage fresco en su ruta canónica.
- **MODELOS/POLÍTICA:** Cursor quota agotado — NO usado. Esta fase 100% OpenCode (deepseek-v4-flash orquestador). Sin workers (fase determinista: packaging + probes + tests). Session budget CONTINUE.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `30238e4f…` (escenario §19: conversación nueva, adjuntar `datosrosco.txt`, pedir UNA VEZ un Pasapalabra/Rosco, SIN nudge "donde está?"/"podes?"/"me avisas?", el chat sigue solo, respuesta final corresponde al request, Creation aparece, Abrir funciona, luego un segundo request secuencial sin one-turn-behind). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (CHAT TURN CAUSALITY / RESPONSE CORRELATION PASS = COMPLETE — REVIEWS APPROVE, INTEGRADO EN MAIN, M11 NO INICIADO, 2026-09-02)

- **CHAT TURN CAUSALITY / RESPONSE CORRELATION PASS = COMPLETE.** Orquestador FRESH (deepseek-v4-flash) retomó en `84abbed`, verificó el estado durable (main limpio, autor SIN mergear, worktree `../ai-publisher-corr-03-chat-causality` limpio y alcanzable en `4d1f657`), lanzó 2 reviews independientes FRESH y fusionó. **Merge `2091ec3` en main** (ort, 9 archivos, +202/−83), preservando los commits del autor `e1c16ff`, `49bbc3d`, `4d1f657`. **`./scripts/verify` EXIT=0 en main post-merge** (FE **228/228**, cargo fmt/clippy/test verdes, contracts M10 + UX_REDESIGN_01, fetch-sidecars --check, cargo check src-tauri, git diff --check). Targeteds post-merge: `opencode_adapter` **24/24**, `app_facade` **36/36**. Working tree limpio.
- **REVIEW PRODUCT/UX INDEPENDIENTE (OpenCode DeepSeek V4 Flash, FRESH) = APPROVE.** El diff resuelve el problema humano: el turno cierra solo con el marker terminal determinista (`info.finish == "stop"`); el retorno temprano por "idle + texto + artefactos" fue ELIMINADO (`opencode.rs`); el texto intermedio `finish:"tool-calls"` ("Listo."/"Voy a preparar la actividad.") NUNCA es terminal; los artefactos solos NO cierran el turno (`4d1f657`); la identidad de turno se preserva vía el `MessageId` durable del usuario (turn_id) encadenado `app.rs → AgentRunView → AgentTaskEvent → App.tsx` (mapa `projectId→turnId`, eventos terminales stale con turnId no coincidente IGNORADOS); Creation se registra y persiste DENTRO del run origen (nunca drenada por el siguiente mensaje). Verificado por el reviewer: `opencode_adapter` 24/24, `app_facade` 36/36, FE 228/228, `project-agent` lib 14/14. Sin regresión a Preview/Abrir ni a cards/share. NITs no bloqueantes: campo muerto `idle_without_text_grace` (escrito, no leído); fallback de grace 15s puede surfacer texto intermedio solo si el agente está genuinamente idle >15s sin `stop` (limitación conocida documentada, estrictamente mejor que el comportamiento previo); gap menor de cobertura FE para el guard de `turnId` en App.tsx (sin test dedicado).
- **REVIEW CÓDIGO/CORRECTNESS INDEPENDIENTE (OpenCode Qwen 3.8 Flash, FRESH) = APPROVE.** Semántica terminal: `finish:"tool-calls"` nunca terminal; `finish:"stop"` (último mensaje assistant, `info` o top-level) es el ÚNICO terminal normal con refresh de `/diff` en ese punto; deadline absoluto de tarea garantiza sin espera infinita; timeout mapea a `TaskFailed("timed out")`. Causality: persist-before-run con `MessageId` → `turn_id` por el path durable (legacy `run_agent` solo test); `inFlightRef` ahora `projectId→turnId` map; eventos terminales mismatch descartados; keying por proyecto impide leaks entre conversaciones. Polling/one-turn: retorno temprano por artefactos eliminado; `idle_since`/`last_artifact_fetch` resetean en fases busy; mutex por proyecto + gate working del FE serializan; cancelled/failed liberan el lock. Creation timing: marker stop → retorno inmediato con fetch fresco de artefactos → card aparece en el evento completante, no en el siguiente envío; sin path de re-registro duplicado. Verificado: `opencode_adapter` 24/24, `app_facade` 36/36, FE 228/228, `cargo fmt --check` limpio. NIT/LOW: campo muerto `idle_without_text_grace`; fallback grace 15s (mismo límite documentado); App.tsx guard de `turnId` sin test FE dedicado; ENV: `cargo check -p educai` no corre en el worktree por sidecar gitignored ausente (consistente por inspección, cubierto por el facade compilado).
- **AUTOR (OpenCode / GPT-5.6 Luna, `high-coding-luna`, pane `corr-03-luna` = `w1G:p1`, worktree `../ai-publisher-corr-03-chat-causality`):** commits `e1c16ff` (turn_id threading + terminal marker), `49bbc3d` (completion = `finish:"stop"`, heurística de texto ELIMINADA), `4d1f657` (artefacto NO completa un turno antes del marker terminal). Total 9 archivos, +202/−83.
- **ROOT CAUSE (evidenciado con runtime real opencode 1.18.25, preservado):** OpenCode 1.18.25 reporta `/session/status` como `{}` (mapa vacío → fase "idle" por default) MIENTRAS el trabajo está activo; los mensajes assistant intermedios tienen `info.finish: "tool-calls"` y solo el mensaje final tiene `info.finish: "stop"`. El adapter trataba "idle + texto assistant + artefactos" como terminal → el texto intermedio ("Listo."/"Voy a hacerlo.") cerraba el turno antes del trabajo real, y el trabajo pendiente se drenaba en el siguiente envío (one-turn-behind, Creation aparecía tras el nudge). Traza real: `15:05:31 status={} diff=[]` → `15:05:35 step-start/reasoning` → `15:05:36 finish:"tool-calls" + tool + step-finish` → trabajo → artefacto `smoke-ok` → final `finish:"stop"`. `status={}` NO es terminal. **CICLO DE VIDA BEFORE/AFTER:** BEFORE M1 → ejecución R1 → texto intermedio/finish tool-calls → lógica vieja terminal → usuario envía M2 → trabajo R1 pendiente drenado → R1 aparece tras M2; AFTER M1 → ejecución R1 → finish tool-calls intermedio → R1 sigue activo → trabajo/artefacto completo → Creation observada → finish:"stop" final → R1 terminal → respuesta final + Creation presentadas para M1.
- **COMPLETION SIGNAL (determinista, §7/§10):** el ÚLTIMO mensaje assistant debe tener `info.finish == "stop"`. Los artefactos solos NUNCA completan un turno (fix `4d1f657`). Grace de ack (15s) conservado como fallback de Q&A corto. Sin heurística de texto, sin sleeps, sin patch "Listo".
- **TURN IDENTITY (§5/§8):** el `MessageId` durable del mensaje de usuario es la identidad lógica del turno (`turn_id`), preservado `AgentRunInputs → AgentRunView → AgentTaskEvent → UI` (`app/src/App.tsx` ahora `Map<projectId, turnId>`; eventos terminales stale con `turnId` que no coincide con el turno activo son IGNORADOS). Un turno agente activo por conversación: verificado. Creation pertenece al turno origen (diff→scan ocurre dentro del poll del turno, con refresh forzado en terminal).
- **RUNTIME VALIDATION REAL (preservada, §17):** sidecar pineado opencode **1.18.25** (`sidecars/`), modelo `opencode/big-pickle` (free), tarea real que creó `smoke.txt` con `smoke-ok` vía tool real. Traza multi-etapa con timestamps reales (arriba). No se necesita nudge.
- **POLÍTICA/MODELOS:** Cursor quota agotado (directiva 2026-09-02): **ningún Cursor usado** en este pass (rutas 100% OpenCode: autor GPT-5.6 Luna, Product/UX DeepSeek V4 Flash, Code/Correctness Qwen 3.8 Flash). Sesión orquestador economizada (budget ~20K; la previa rotó en 126K por duplicar trabajo). **PROXIMO GATE (único): FRESH REAL APPIMAGE BUILD + TECHNICAL VERIFICATION** desde main `2091ec3` (`scripts/smoke-package appimage`, sidecars pineados opencode 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real Fedora/Wayland), luego **HUMAN PRODUCT-OWNER RE-ACCEPTANCE**.
- **GATES de no-regresión (confirmados):** PREVIEW/ABRIR = PASS humano (NO reabrir). Adjuntos, cards, publicación Cloudflare real, modelo en Configuración, delete «Sí», Enter/Shift+Enter, badge, aislamiento, storage intactos. AppImage fresco `40403c69…` previo. **NO HUMAN ACCEPTED. M11 NO INICIADO.**

## Estado previo (CHAT TURN CAUSALITY PASS — TRABAJO DE AUTOR COMPLETO Y VERIFICADO EN WORKTREE, ORQUESTADOR ROTADO ANTES DE REVIEWS/MERGE, M11 NO INICIADO, 2026-09-02)

- **CHAT TURN CAUSALITY / RESPONSE CORRELATION PASS = TRABAJO DE AUTOR COMPLETO y VERIFICADO (3 commits en `corr/chat-causality-pass`), PERO NO REVISADO NI MERGEADO.** El orquestador (FRESH, deepseek-v4-flash) bootstrapeó, cableó el rol Luna en el launcher, lanzó el autor OpenCode GPT-5.6 Luna, hizo 2 ciclos acotados de fix con inspección propia, verificó `./scripts/verify` EXIT=0 en el worktree, y alcanzó **ROTATE_SESSION_REQUIRED (126,719 tokens)** ANTES de lanzar las reviews independientes → **Product/UX review y Code/Correctness review PENDIENTES (sesión FRESH siguiente).** Worktree autor limpio, branch `corr/chat-causality-pass` SIN mergear. Main limpio. **M11 NO INICIADO.**
- **AUTOR (OpenCode / GPT-5.6 Luna, `high-coding-luna`, pane `corr-03-luna` = `w1G:p1`, worktree `../ai-publisher-corr-03-chat-causality`):** commits `e1c16ff` (turn_id threading + terminal marker), `49bbc3d` (completion = `finish:"stop"`, heurística de texto ELIMINADA), `4d1f657` (artefacto NO completa un turno antes del marker terminal). Total 9 archivos, +202/−83. `./scripts/verify` EXIT=0 en worktree (FE **228/228**, cargo verde, contracts, fetch-sidecars --check). Adapter **24/24**, app_facade **36/36**. Author handoff STATUS: PASS. Session Luna seguía disponible/idle al rotar.
- **ROOT CAUSE (evidenciado con runtime real opencode 1.18.25):** OpenCode 1.18.25 reporta `/session/status` como `{}` (mapa vacío → fase "idle" por default) MIENTRAS el trabajo está activo; los mensajes assistant intermedios tienen `info.finish: "tool-calls"` y solo el mensaje final tiene `info.finish: "stop"`. El adapter trataba "idle + texto assistant + artefactos" como terminal → el texto intermedio ("Listo."/"Voy a hacerlo.") cerraba el turno antes del trabajo real, y el trabajo pendiente se drenaba en el siguiente envío (one-turn-behind, Creation aparecía tras el nudge). Traza real: `15:05:31 status={} diff=[]` → `15:05:35 step-start/reasoning` → `15:05:36 finish:"tool-calls" + tool + step-finish` → trabajo → artefacto `smoke-ok` → final `finish:"stop"`. `status={}` NO es terminal.
- **COMPLETION SIGNAL (determinista, §7/§10):** el ÚLTIMO mensaje assistant debe tener `info.finish == "stop"`. Los artefactos solos NUNCA completan un turno (fix `4d1f657`). Grace de ack (15s) conservado como fallback de Q&A corto. Sin heurística de texto, sin sleeps, sin patch "Listo".
- **TURN IDENTITY (§5/§8):** el `MessageId` durable del mensaje de usuario es la identidad lógica del turno (`turn_id`), preservado `AgentRunInputs → AgentRunView → AgentTaskEvent → UI` (`app/src/App.tsx` ahora `Map<projectId, turnId>`; eventos terminales stale con `turnId` que no coincide con el turno activo son IGNORADOS). Un turno agente activo por conversación: verificado. Creation pertenece al turno origen (diff→scan ocurre dentro del poll del turno, con refresh forzado en terminal).
- **TESTS NUEVOS:** `send_does_not_treat_intermediate_text_as_terminal_before_artifacts` (artefacto en /diff + finish tool-calls NO completa; espera el stop y devuelve el texto FINAL + artefacto), `only_stop_finish_marks_the_latest_assistant_message_terminal`, `send_completes_on_explicit_stop_without_files`, `sequential_sends_keep_distinct_turn_ids_and_ordered_results` (CASE D), turn_id presente en completed/cancelled/failed (CASE H). FakeServer ahora soporta `prompt_response_finish`.
- **RUNTIME VALIDATION REAL (§17):** sidecar pineado opencode **1.18.25** (`sidecars/`), modelo `opencode/big-pickle` (free), tarea real que creó `smoke.txt` con `smoke-ok` vía tool real. Traza multi-etapa con timestamps reales (arriba). No se necesita nudge.
- **POLÍTICA/MODELOS:** Cursor quota agotado (directiva 2026-09-02): **ningún Cursor usado**. Se agregó rol `high-coding-luna` (`config/agent-models.env` = `opencode-go/gpt-5.6-luna`, `scripts/agent-launch` + `scripts/test-agent-launch`, commit `7f37ca7` en main) y nota temporal en `docs/AGENT_POLICY.md`. Reviews pendientes: **Product/UX = OpenCode DeepSeek V4 Flash (o Qwen 3.8 Flash); Code/Correctness = OpenCode Qwen 3.8 Flash** (ambas FRESH, sin Cursor).
- **PROXIMO PASO (sesión FRESH de orquestador):** (1) budget CONTINUE; (2) inspect diff `b04a7ed..4d1f657` en el worktree (o reusar el pane `corr-03-luna` si sigue vivo y mismo task); (3) **Product/UX review FRESH** (DeepSeek V4 Flash o Qwen 3.8 Flash, §19-20); (4) **Code/Correctness review FRESH** (Qwen 3.8 Flash, §21-22); (5) fix loop si REQUEST_CHANGES (reusar Luna si disponible, si no FRESH); (6) **merge gate §24** (root cause evidenciado, sin nudge, correlación turno correcta, texto intermedio no terminal, Creation del turno origen, chat ordenado, active-turn determinista, switching seguro, runtime real PASS, reviews APPROVE, preview PASS, M11 NO INICIADO); (7) merge a main + `./scripts/verify` en main + checkpoint durable + limpieza; (8) **next gate: FRESH REAL APPIMAGE BUILD + TECHNICAL VERIFICATION, luego HUMAN PRODUCT-OWNER RE-ACCEPTANCE.**
- **GATES de no-regresión (confirmados):** PREVIEW/ABRIR = PASS humano (NO reabrir). Adjuntos, cards, publicación Cloudflare real, modelo en Configuración, delete «Sí», Enter/Shift+Enter, badge, aislamiento, storage intactos. AppImage fresco `40403c69…` previo. **NO HUMAN ACCEPTED. M11 NO INICIADO.**

## Estado previo (FRESH REAL APPIMAGE POST-CORRECCIÓN CONSTRUIDO DESDE MAIN `71ff7bf` — BUILD + VERIFICACIÓN TÉCNICA COMPLETA, TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE, M11 NO INICIADO, 2026-09-02)

- **FRESH REAL APPIMAGE CONSTRUIDO DESDE MAIN `71ff7bf` (post-merge corrección `5e4b170`) = PASS (sesión FRESH, deepseek-v4-flash, validación técnica determinista).** Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS).** Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.877.816 bytes**, **SHA-256 `40403c697adbf2e2596a225856e6c0b377a92f3e66068a6176e209bc2228149d`** (NUEVO; el previo `ec336881…` era STALE — construido desde `ebeac0e`, predata el merge de corrección `5e4b170`), timestamp 2026-09-02 10:54:56 -0300, source commit `71ff7bf` (main HEAD, working tree clean antes del build, sin cambios de producto sin commitear). Build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool tras el error esperado de linuxdeploy en Fedora 44). **Sidecars bundlados pineados verificados en payload y mount en vivo:** opencode **1.18.25** (binario extraído `d91e0d33…`, byte-idéntico al `sidecars/opencode-x86_64-unknown-linux-gnu` fetcheado; el pin del manifest `58a3729a…` es del tarball de origen) y cloudflared **2026.8.3** (sha `f29324fe…` = pin `config/components.json`). `--version` en payload: opencode `1.18.25`, cloudflared `2026.8.3`. **Frontend embebido correcto:** el binario embebe exactamente `assets/index-BFehLbJS.js` + `assets/index-UHkEOOtE.css` (idénticos al `dist` generado en este build desde `71ff7bf`; el asset stale `index-DJsNCuJZ.js` NO está presente). Markers de corrección en el dist embebido: "Dejar de compartir", "Compartido", "Creando tu recurso".
- **`./scripts/verify` EXIT=0** (cargo fmt/clippy/test verdes, FE **228/228** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 version 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Lanzamiento real en Fedora/Wayland (DISPLAY=:0):** backend `[agent] starting → ready` SIN falso error de arranque, sin errores en log; WebKitNetworkProcess + WebKitWebProcess activos (UI usable). **PATH-independencia:** lanzado con PATH restringido `/tmp/opencode/path-indep-bin:/usr/bin:/bin` (sin opencode/cloudflared externos; `command -v opencode` = fail); el sidecar opencode hijo corre desde el mount propio del AppImage (`/tmp/.mount_EducAINKfoAO/usr/bin/opencode`, port 33999, HTTP responde); cloudflared presente en el payload del mount.
- **TARGETED REQUEST-COMPLETION (A) = PASS (donde el tooling lo permite).** `cargo test -p project-agent --test opencode_adapter` **23/23**: `send_does_not_treat_brief_listo_as_complete_before_artifacts`, `send_tolerates_transient_diff_errors_during_ack_wait`, `send_completes_brief_listo_after_ack_grace_when_no_files_appear`. El ack breve no corta el trabajo requerido; sin "donde esta?"/nudge. Generación real OpenCode live = territorio HUMAN RE-ACCEPTANCE.
- **TARGETED PREVIEW (B) = PASS.** `cargo test -p project-preview --test preview_lifecycle` **4/4** (token root 200, teardown invalida token), `--test preview_security` **10/10**, `cargo test -p project-app --test preview` **9/9** (`web_preview_starts_and_closes_by_token`, foreign creation 404, oversized resource). Misma Creation que Compartir (mismo `creation.id`) cubierto por FE `CreationsPanel.test.tsx`.
- **TARGETED SHARE/UPDATE (C/E/H) = PASS (donde el tooling lo permite).** `cargo test -p project-app --test app_facade` **35/35**: `publish_promotes_the_generated_web_creation_as_the_public_entry` (publish/index.html contiene el markup generado, NO "Material del proyecto"), `later_turn_updates_the_same_web_creation_and_refreshes_publish` (update in-place + refresh snapshot, misma URL), `new_distinct_web_does_not_replace_an_already_published_snapshot`, `creation_path_rejects_cross_project_id`, `set_creation_visibility_toggles`, `delete_unpublishes_before_removing_data`, `delete_aborts_when_unpublish_fails_leaving_project_intact`. FE `App.test.tsx`: `shows the sidebar Compartido badge as soon as the conversation is shared` (badge inmediato, sin rerender extra), `refreshes the conversation list when a share-related task completes`. FE `PublishPanel.test.tsx`: menuitem "Dejar de compartir" **enabled** + clase `danger` cuando share activo; stop-sharing confirm + unpublish. CSS verificado: `.share-control-menu button.danger` `--danger`/`--danger-soft`/`--muted`/`:focus-visible`. **Cloudflare público real con el artifact actualizado NO se valida determinísticamente en esta fase → territorio HUMAN RE-ACCEPTANCE.**
- **TARGETED CONVERSATION ISOLATION (F) = PASS.** FE `App.test.tsx`: `shows only the selected conversation's messages when switching` y `ignores a late agent result from another conversation` (resultado async tardío de A no pinta en B). Re-entry sin "working" restaurado: `re-enables the composer after a rejected agent_send`.
- **TARGETED CHAT ORDERING (I) = PASS.** agent_send persiste el mensaje de usuario antes de ejecutar (persist-before-run); UI un turno in-flight por conversación; serialización por proyecto (`same_project_runs_are_serialized`, agent_service **10/10**); `send_tolerates_transient_diff_errors_during_ack_wait` (stale ack no aborta el turno).
- **TARGETED KEYBOARD (G) = PASS.** FE `ComposerBar.test.tsx`: `Enter sends; Shift+Enter inserts a newline without sending`, `does not send while IME composition is active` (isComposing), `does not send whitespace-only prompts`, `does not send when the composer is busy`, `does not send an empty prompt`.
- **STORAGE DOC CONSISTENCY = VERIFICADO (sin migración).** `docs/STORAGE_LAYOUT.md` consistente con el código: root = Tauri `app_data_dir()` (`app/src-tauri/src/lib.rs:63`), id `com.educai.publisher` (`tauri.conf.json:5`), XDG aislado vía `--pure` + XDG_* (`project-opencode/src/lib.rs:24,62-72`), preview temp `m8-preview-` (`app.rs:731`, fuera del app-data), `0o700` (`app.rs:1405`), publish = snapshot `PublicationSnapshotStore`, `revision=1`, AppImage = media de paquete, NO contenedor de datos. Sin discrepancia material.
- **M11 NO INICIADO.** Sin redesign de storage. Sin sleeps/force-rerenders. Sin claim de aceptación humana.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `40403c69…` (escenario §22: sin falso error de arranque; conversación nueva; request de creación SIN nudge "donde está?"; Creation aparece; Abrir muestra el contenido real; Compartir publica el contenido real; "Compartido" inmediato; Dejar de compartir legible; modificar Creation compartida → refresh de URL pública refleja el cambio; segunda conversación aislada; chat secuencial coherente; Enter envía; Shift+Enter newline; rename/delete; confirmación con «Sí»; persistencia tras reinicio). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (HUMAN-ACCEPTANCE CORRECTION PASS — CONVERSATION STATE / CREATION LIFECYCLE / PREVIEW / SHARE REACTIVITY / CHAT UX — COMPLETE, REVIEWS APPROVE, INTEGRADO EN MAIN, M11 NO INICIADO, 2026-09-02)

- **PASS DE CORRECCIÓN HUMANA (STATE / CREATION LIFECYCLE / PREVIEW / SHARE / CHAT UX) = COMPLETE Y NO ES M11.** Corrige los hallazgos A–I del product owner sobre el AppImage real (falso "Listo." antes de ejecutar; Abrir en blanco; "Dejar de compartir" ilegible; actualización de Creation compartida que no llega a la URL pública; fuga de estado entre conversaciones; Enter/Shift+Enter; badge "Compartido" tarde; respuestas fuera de orden). Finding D (Cloudflare público muestra el artifact real) se PRESERVA.
- **INTEGRADO EN MAIN.** Autor (Cursor Grok 4.6 High FRESH) commits `99f6f7d` + fix de review `18b7c11` en `corr/state-lifecycle-pass` (worktree `../ai-publisher-corr-02-state-lifecycle`, base main `66d1a99`). **Merge `5e4b170` en main** (ort, 26 archivos, +1346/−108; docs/STORAGE_LAYOUT.md nuevo). **`./scripts/verify` EXIT=0 en main post-merge** (FE **228/228**, cargo verde, M10 + UX_REDESIGN_01 contracts, fetch-sidecars --check, cargo check src-tauri, git diff --check). Working tree limpio.
- **REVIEWS INDEPENDIENTES:** Product/UX Cursor Grok 4.6 High FRESH = **APPROVE** (`99f6f7d`). Código/a11y `opencode-go/qwen3.8-flash` FRESH = **REQUEST_CHANGES → APPROVE**: MAJOR (send-failure dejaba "working" permanente; fix `18b7c11` `onSendEnd` limpia inFlight + fase idle en ambas ramas del catch, composer re-habilitado, retry real, sin "working" fantasma al re-entrar, con test), MINOR (wipe-before-CAS podía borrar outputs viejos si el replace fallaba; fix write-then-prune `prune_after_replace_keeps_the_new_primary_and_drops_stale_sidecars`), LOW (transient `/diff` en la espera de ack abortaba el turno; fix tolera el error y sigue polleando, `send_tolerates_transient_diff_errors_during_ack_wait`). Re-review Qwen FRESH = **APPROVE** (verificados los 3 fixes, invariantes del diff combinado, storage doc exacto). Re-review UX acotado Cursor Grok 4.6 High FRESH = **APPROVE** (`18b7c11`: recuperación de fallo honesta en voseo, sin spinner permanente, sin duplicados, sin regresión A–I).
- **Clasificación de riesgo:** A, B, C, E, F, G, H, I son acotados (poller, preview routing, CSS, overwrite in-place + republish ADR-0004, frontend isolation/correlation). Sin STOP: no hubo migración de schema, no se rediseñó AgentEngine, no se reestructuró el FS. `revision` sigue en `1`. Sin sleeps UI arbitrarios ni force-rerenders.
- **A (falso Listo):** `poll_session` no trata un ack breve (`Listo.`/`OK`/…) + idle + sin artifacts como terminal; espera gracia (15s prod) y reconsulta `/diff`; si el estado sale de idle, el timer resetea. Instrucción: escribir archivos primero; nunca un "Listo." suelto antes. El usuario NUNCA necesita "donde esta?"/"podes?"/"seguí". Estado transitorio "Creando tu recurso…". Tests: `send_does_not_treat_brief_listo_as_complete_before_artifacts`, `brief_ack_detects_listo_and_ignores_real_replies`.
- **B (Abrir en blanco):** preview token-root (`/preview/<token>/` y sin slash) sirve `index.html`, igual que el publisher; nested dirs siguen 404. WebView navega a `{base}index.html`. Misma Creation que Compartir (mismo `creation.id`). Tests: `preview_lifecycle` token root 200, `preview.rs` GET base URL.
- **C (Dejar de compartir):** `.share-control-menu button.danger` usa `--danger` (peso 600) sobre fondo transparente, hover `--danger-soft`, disabled `--muted`, `:focus-visible` outline; habilitado cuando share activo (sin aspecto disabled falso). Test: menuitem enabled + class `danger`.
- **E (update de Creation compartida):** match por kind+display_name → overwrite `outputs/<id>/` (mismo id, visibilidad, revision=1). Si el proyecto ya está published **y** la Creation registrada ya es pública, `refresh_published_snapshot` hace replace de ADR-0004 (misma `publicationRoute`, sin re-engagement del túnel). Una actividad distinta (`actividad-2`) crea id nuevo y **no** hijackea la URL. Si republish falla, el mensaje del asistente es honesto: "El recurso local se actualizó, pero el enlace compartido no. Volvé a pulsar Compartir." Tests: `later_turn_updates_the_same_web_creation_and_refreshes_publish`, `new_distinct_web_does_not_replace_an_already_published_snapshot`, `later_turn_does_not_reregister_prior_workspace_files` (M1) intacto.
- **F (fuga entre conversaciones):** `selectedIdRef` sincrónico en `openConversation`; `refreshConversation` no aplica si el id ya no es el seleccionado; `WorkspaceView` solo si `conversation.id === selectedId`; key por id; eventos `agent://task` ajenos no pintan el timeline visible; in-flight per-conversation restaurado al volver. Tests: switch A/B, late foreign result, hold-open no muestra A mientras carga B, re-entry sin "working" restaurado.
- **G (Enter/Shift+Enter):** Enter envía; Shift+Enter newline; IME (`isComposing` / keyCode 229) no envía; whitespace no envía; busy respetado. Tests cubren los cinco.
- **H (badge Compartido):** `onRefresh` del workspace refresca conversación **y** `project_list`. Test: badge en sidebar al compartir, sin pulsar "+" ni otra acción.
- **I (orden de turnos):** `agent_send` persiste el mensaje de usuario **antes** de retornar; UI un turno in-flight por conversación (`sendingRef` + `onSendStart`); si `agent_send` rechaza, `onSendEnd` limpia inFlight + fase idle. AgentService ya serializaba por proyecto. Tests: persist-before-run, sequential ChatPanel order, segundo send bloqueado, re-enable tras reject.
- **Storage:** **`docs/STORAGE_LAYOUT.md`** (219 líneas, verificado contra el código por Qwen) documenta el layout real: root = Tauri `app_data_dir()` (`~/.local/share/com.educai.publisher` en Linux, `com.educai.publisher`), `settings.json` global, `projects/<id>/{project.json,inputs,workspace,outputs,publish}`, OpenCode XDG aislado (`--pure`), preview temp `m8-preview-*` (fuera del app-data), publicación = snapshot inmutable (ADR-0004). **El AppImage es media de paquete ejecutable, NO el contenedor de datos persistentes.** Sin migración. Puntero en `docs/ARCHITECTURE.md`.
- **Gates:** `pnpm format:check && pnpm lint && pnpm typecheck && pnpm test` en `app/` = **228/228**. `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked` = PASS. `./scripts/verify` = EXIT=0 (worktree autor y main post-merge, incluye `cargo check` src-tauri).
- **Runtime:** preview loopback real (HTTP 200 del artifact en token root + `index.html`); update+republish con FakeAgentEngine (mismo id, snapshot `publish/` actualizado). **Cloudflare público live y AppImage humano = HUMAN RE-ACCEPTANCE** (Finding D ya validado por el product owner; no se finge PASS de red pública).
- **LIMITACIONES NO BLOQUEANTES (conocidas):** (1) un "Listo." aislado real de Q&A corta (whitelist exacta) espera el grace completo mostrando "Creando tu recurso…" — heurística aceptada, mitigada por prompt; (2) el texto honesto de republish solo aplica cuando la refresh falla (si el modelo ignora la instrucción y mintea un archivo privado nuevo, la coincidencia in-place no se da) — cubre el camino reportado "cambiar el fondo"; (3) en 15s sin archivos un "Listo." puede caer como reply final (test cubre, necesario para acks de chat corto).
- **NO REGRESAR:** session `?directory=` binding; attachments al agente; cards Abrir/Compartir mismo id; publish del artifact (no "Material del proyecto"); modelo en Configuración; discovery free; X de Configuración vuelve a la misma conversación; toast duplicado; delete «Sí»; sin falso error de arranque genérico.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE DE ESTE PASS. NO HUMAN ACCEPTED. M11 NO INICIADO.** Próximo gate y ÚNICO: (1) **FRESH REAL APPIMAGE BUILD + VERIFICACIÓN TÉCNICA** desde main `5e4b170` (`scripts/smoke-package appimage`, sidecars pineados opencode 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real Fedora/Wayland); (2) **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco: adjunto rosco → creación sin "donde esta?"; Abrir muestra el juego real; Compartir → URL pública con el juego (sin "Material del proyecto"); modificar el juego → misma card/URL refleja el cambio; conversación nueva aislada; Enter/Shift+Enter; badge "Compartido" inmediato; "Dejar de compartir" legible. **NO iniciar M11. NO afirmar aceptación humana desde OpenCode.**

## Estado previo (FRESH APPIMAGE POST-CORRECCIÓN CONSTRUIDO Y VERIFICADO TÉCNICAMENTE — TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE, M11 NO INICIADO, 2026-09-01)

- **FRESH REAL APPIMAGE CONSTRUIDO DESDE MAIN `ebeac0e` (post-corrección Creation/Share/Chat) = PASS (sesión FRESH, deepseek-v4-flash, validación técnica determinista).** Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS).** Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.861.432 bytes**, **SHA-256 `ec3368811bdf65679e8271e571da383d4837aec9ce5ddccb885778373bea6392`** (NUEVO; el previo `930ee074…` era STALE — construido desde `773278d`, predata el merge de corrección `ebeac0e`), timestamp 2026-09-01 23:24:41 -0300, source commit `ebeac0e` (HEAD `1aba3f0` checkpoint, working tree clean antes del build, sin cambios de producto sin commitear). Build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool tras el error esperado de linuxdeploy en Fedora 44). **Sidecars bundlados pineados verificados en payload y en mount en vivo:** opencode **1.18.25** (sha256 `58a3729a…` = pin) y cloudflared **2026.8.3** (sha256 `f29324fe…` = pin, idéntico al pin `config/components.json`). **Frontend embebido correcto:** el binario embebe exactamente `assets/index-DJsNCuJZ.js` + `assets/index-CbAI0ZTD.css` (idénticos a los del `dist` generado en este build desde `ebeac0e`). **Lanzamiento real en Fedora/Wayland (DISPLAY=:0):** backend `[agent] starting → ready`, SIN falso error de arranque, sin errores en log. **PATH-independencia:** lanzado con PATH `/tmp/opencode/path-indep-bin:/usr/bin:/bin` (sin opencode/cloudflared externos); el sidecar opencode hijo corre desde el mount propio del AppImage (`/tmp/.mount_EducAIIJNecl/usr/bin/opencode`, port 45237); cloudflared presente en el payload del mount. **`./scripts/verify` EXIT=0** (cargo fmt/clippy/test verdes, FE **217/217** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 version 0.1.0, UX_REDESIGN_01 contract, cargo check src-tauri, git diff --check).
- **TARGETED CREATION/SHARE RUNTIME VALIDATION = PASS (unit/integración mockeada, evidencia del contrato).** `cargo test -p project-app --test app_facade` **32/32**: `publish_promotes_the_generated_web_creation_as_the_public_entry` (el `publish/index.html` contiene el markup generado y NO "Material del proyecto"), `web_sidecar_sibling_is_copied_into_outputs_and_publish` (sidecars CSS/JS/imágenes copiados), `publish_without_creation_id_still_promotes_the_latest_web`, `run_agent_registers_creation_private_by_default`, `set_creation_visibility_toggles`, `creation_path_rejects_cross_project_id`, `delete_unpublishes_before_removing_data`. `cargo test -p project-agent --test agent_service` **9/9**: `later_turn_does_not_reregister_prior_workspace_files` (dedupe M1), `workspace_scan_registers_when_diff_is_empty` (scan solo si diff vacío), `web_sidecar_assets_are_not_separate_creations` (assets de sidecar no son Creations separadas), `traversal_artifact_path_is_rejected_and_not_registered` (seguridad de paths), `run_registers_scripted_artifacts_as_private`. Genericity confirmada: sin hardcode Pasapalabra (cualquier `.html/.htm` = `Web`, título "Actividad" para `index.html` raíz).
- **TARGETED ABRIR/COMPARTIR VALIDATION = PASS (donde el tooling lo permite).** Tests FE `CreationsPanel.test.tsx`: Abrir usa `preview_open_web` con el MISMO `creation.id` (no `creation_open` genérico); Compartir asocia la MISMA creación; `aria-label="{Abrir}: {displayName}"`/`"{Compartir}: {displayName}"` por card. Publicación del artifact (no raíz del proyecto) cubierta por el test Rust `publish_promotes_the_generated_web_creation_as_the_public_entry`. **Nota de límite:** networking público real de Cloudflare (URL pública abierta mostrando el juego, sin "Material del proyecto") NO se valida determinísticamente en esta fase técnica → **territorio de HUMAN RE-ACCEPTANCE.**
- **TARGETED CHAT REGRESSION = PASS (FE 217/217).** `ChatPanel.test.tsx`: no renderiza burbuja assistant completada vacía etiquetada solo "Asistente" (B5), no duplica contenido asistente en `.chat-status.ok` verde (B6/B11), working status no duplica, error no duplica, failed line solo cuando la burbuja más nueva coincide (Blocker B post-T7), materials adjuntos solo en burbuja del usuario. Toast "Tu recurso está listo." eliminado (un evento lógico = una notificación).
- **TARGETED SETTINGS VALIDATION = PASS.** `App.test.tsx`: `keeps the model selector out of the composer` (B7/H — composer = adjuntar/mensaje/enviar, sin selector permanente, sin "Modelo gratuito" banner); `opens settings from the gear button, shows the model selector there, and restores the conversation on close` (B7/H — ModelSelector en Configuración, X cierra y vuelve EXACTO a la misma conversación, sin reset). Default free/model discovery del backend intacto (sin hardcode Big Pickle; el test usa `big-pickle` solo como fixture de mock de prueba).
- **DELETE-CONFIRMATION «SÍ» REGRESSION = PASS.** `ConfirmDialog.test.tsx`: acepta `Sí/sí/SI/si` + espacios alrededor; rechaza `s i`/`siii`/`no`/solo-espacios/vacío/título-exacto-cuando-configurado; Enter NO puede saltar la validación; Cancel nunca borra; botón deshabilitado hasta válido; `confirmText` explícito (proyectos) conserva matching exacto. Semántica destructiva backend intacta.
- **REVIEW-FIX REGRESSIONS REPRESENTADAS EN MAIN Y CUBIERTAS POR TESTS = VERIFICADO.** Dedupe (M1), títulos accesibles por card (m6), sidecar copy (m2/m7), nested `index.html` (m3), scan caps/bounds (m4), idle/race polling (m5), tests real-registrar — todos presentes en el merge `ebeac0e` y verificados por los tests Rust/FE listados arriba.
- **M11 NO INICIADO.** Sin fuga de alcance: sin redesign de infra de publicación, sin cambios destructivos Task F, sin tocar runtime/session-directory, sin rerun de exploración Product/UX amplia.
- **PROVENANCE DEL ARTEFACTO:** source commit `ebeac0e` (HEAD `1aba3f0`), build 2026-09-02 02:24 UTC (23:24 -0300) vía `scripts/smoke-package appimage`, SHA-256 `ec3368811bdf65679e8271e571da383d4837aec9ce5ddccb885778373bea6392`, 180.861.432 bytes. El previo `930ee074…` en la misma ruta fue reemplazado por este build (rm -rf del dir appimage por smoke-package + rebuild). Sidecars pins: opencode **1.18.25**, cloudflared **2026.8.3** (verificados, sin repin). Limpieza: procesos/mounts AppImage de prueba removidos, working tree limpio.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `ec336881…` (escenario §17/§19: adjunto `datosrosco.txt` + prompt real → Creation card [Abrir][Compartir], URL pública con el juego real y sin "Material del proyecto", sin burbuja vacía, sin toast duplicado, modelo en Configuración, «Sí» para eliminar). **NO afirmar aceptación humana desde OpenCode.** NO iniciar M11.

## Estado previo (PASS CORRECCIÓN PRODUCT/UX CREACIÓN/COMPARTIR/CHAT — COMPLETE, REVIEWS APPROVE, INTEGRADO EN MAIN, M11 NO INICIADO, 2026-09-01)

- **PASS DE ACEPTACIÓN HUMANA (CREACIÓN / SHARE / CHAT UX) = COMPLETE Y NO ES M11.** Corrige
  los 7 bloqueadores PRODUCT/UX hallados por el product owner en el AppImage real (asistente
  generó un Rosco/Pasapalabra desde `datosrosco.txt`, pero solo apareció prosa, la URL
  pública mostraba "Material del proyecto", el asistente pidió abrir archivos a mano, hubo
  burbuja vacía "Asistente", toast duplicado "Tu recurso está listo.", y selector de modelo
  permanente en el composer).
- **INTEGRADO EN MAIN.** Autor (Cursor Grok 4.6 High FRESH) commit `3ba7c5a` + fix de review
  `857d98c` en `corr/creation-share-ux-pass` (worktree `../ai-publisher-corr-01-creation-share`,
  base main `3a7c6d1`). **Merge `ebeac0e` en main** (ort, 30 archivos, +1320/−300).
  Evidencia durable de reviews en `docs/qwen-review-creation-share.md`,
  `docs/qwen-rereview-creation-share.md`, `docs/ux-rereview-creation-share.md` (commit `05a2c2a`).
- **B1 (card de creación):** `opencode.rs` `normalize_output_path` acepta paths session-relative
  (`rosco.html` → `workspace/rosco.html`) y absolutos solo si contienen `/workspace/`;
  `service.rs` `merge_artifacts` usa el diff del sidecar cuando trae un archivo registrable y
  el workspace scan SOLO como fallback si el diff queda vacío (prevención de duplicados M1);
  cualquier `.html/.htm` es `Web`; el registrar guarda webs como `index.html` y copia sidecars
  (CSS/JS/imágenes) a `outputs/<id>/` — genérico, sin hardcode Pasapalabra.
  **B2 (Abrir/Compartir = misma creación):** `publish(projectId, creationId?)` fluye de la card
  → `useShareControl` → Tauri `commands.rs` → `app.rs publish_creation`; Abrir usa el mismo
  `creation.id` (`preview_open_web`).
  **B3 (URL pública muestra la creación, no "Material del proyecto"):** `app.rs
  prepare_share_visibility` marca PÚBLICA la creación objetivo (id preferido, si no el último
  web, si no la última) y degrada otros webs públicos antes del snapshot; test
  `app_facade.rs publish_promotes_the_generated_web_creation_as_the_public_entry` assert que
  `publish/index.html` contiene el markup generado y NO contiene "Material del proyecto".
  **B4 (sin abrir-archivo-manual):** `service.rs build_instruction` ordena escribir un recurso
  web estático con `index.html` como entrada, dice que EducAI mostrará Abrir/Compartir, y
  prohíbe pedir abrir/doble clic/explorador.
  **B5 (burbuja vacía):** poll ignora texto asistente vacío; `ChatPanel.tsx` no renderiza
  burbuja assistant completada vacía sin creations; errores/cancel siguen como `role="alert"`.
  **B6 (toast duplicado):** toast "Tu recurso está listo." ELIMINADO (un evento lógico = una
  notificación); listener `agent://task` registrado una vez con refs + `unlisten` cancelado
  (sin re-suscripción por `selectedId`).
  **B7 (modelo a Configuración):** composer = adjuntar/mensaje/enviar (+ slot Compartir);
  `ModelSelector` en `ProviderPanel` (Configuración); default free/model discovery del backend
  intacto (sin hardcode Big Pickle); X de Configuración = `setSettingsOpen(false)` → vuelve
  EXACTO a la misma conversación.
- **REVIEW PRODUCT/UX INDEPENDIENTE (Cursor Grok 4.6 High FRESH) = APPROVE** (pane cerrado,
  sesión previa). 2 residuales NO bloqueantes: (1) título de card caía a "index" cuando el
  modelo escribía `index.html` en la raíz → **RESUELTO en el fix de review (m1)**: la raíz
  `index.html`/`index.htm` ahora se titula "Actividad"; carpetas padre siguen ganando en
  anidados (`actividad-2/index.html` → "actividad-2"); (2) Compartir sigue también en la
  bottom bar además de la card — consistente con el pass.
- **REVIEW CÓDIGO/A11Y/CORRECTNESS FRESH (`opencode-go/qwen3.8-flash`) = REQUEST_CHANGES →
  APPROVE.** Primer review sobre `3a7c6d1..3ba7c5a`: **M1 MAJOR** (el scan de workspace
  re-registraba artifacts de turnos previos → cards duplicadas en turnos siguientes y
  promoción de duplicado stale en el fallback sin-id) + **m1-m7 MINOR** (título "index";
  sidecar copy podía producir Creation no publicable — reserved roots/stems; `index.html`
  anidado descartado; scan/copy sin capping ni exclusión de árboles de dependencias; poll de
  `/diff` cada 20ms con 120s de timeout si vacío; a11y: botones Abrir/Compartir sin nombre
  accesible por creación; sin cobertura del path filesystem sidecar) + LOW/NIT (L1-L4, N1).
  **Fix acotado por el MISMO autor (Cursor Grok 4.6 High FRESH, commit `857d98c`, 14 archivos
  +554/−94):** M1 vía opción (a) — diff del sidecar autoritativo, scan solo si diff vacío
  (`later_turn_does_not_reregister_prior_workspace_files` + `workspace_scan_registers_when_diff_is_empty`
  verdes); m1 título humano "Actividad"; m2 `sidecar_component_ok` replica `validate_component`
  del snapshot (reserved stems a cualquier profundidad, `materials.html`/`files` solo raíz);
  m3 skip de `index.html` solo en `dest_root`; m4 skip `node_modules/dist/build/target/vendor/venv/
  __pycache__/coverage/bower_components` + caps profundidad 8 / archivos 500 / bytes 32 MiB;
  m5 grace idle 2s arranca aunque no haya files y `/diff` se trae una vez al iniciar el grace;
  m6 `aria-label="{Abrir}: {displayName}"` / `"{Compartir}: {displayName}"` por card; m7 tests
  real-registrar (`web_sidecar_sibling_is_copied_into_outputs_and_publish`); L1 param muerto
  eliminado; L2 dead code eliminado (messages.agent.ready, CSS `.composer-model*`); L4 copy
  best-effort + validación `..` en source; N1 `content:""` cae a `parts`. **L3 NO fixed por
  diseño** (note de demotion de webs públicas para target no-web, pre-existente LOW, M1 le
  quita su peor manifestación — aceptado por el revisor). **Re-review FRESH
  (`opencode-go/qwen3.8-flash`) = APPROVE** (verificado: diff 3ba7c5a..857d98c, invariantes 1-11
  del diff combinado, targeted tests verdes, `pnpm typecheck` + `cargo fmt --check` + `git diff
  --check`; residuales no bloqueantes: LOW L3, NIT log del copy error, NIT skip de nombres
  genéricos).
- **RE-REVIEW UX ACOTADO (Cursor Grok 4.6 High FRESH) = APPROVE** sobre los DOS cambios de
  comportamiento visible del fix: (1) título "Actividad" para `index.html` raíz — lenguaje de
  aula, sin fuga del nombre de archivo, consistente con B1-B3; (2) sin cards duplicadas en
  turnos siguientes — el docente ve UNA card nueva por actividad, y el fallback latest-Web de
  Compartir ya no puede promover un re-registro stale. B1-B3 (Abrir/Compartir sobre el mismo
  artifact registrado; share público de la creación) intactos.
- **VERIFICACIÓN EN WORKTREE AUTOR (post-fix `857d98c`):** `pnpm format:check/lint/typecheck`
  OK, **vitest 217/217** (21 archivos), `cargo fmt --check` + `clippy -D warnings` + `cargo
  test --locked --workspace --all-targets` verdes (584 tests), **`./scripts/verify` EXIT=0**.
  **VERIFICACIÓN EN MAIN POST-MERGE (`ebeac0e`): `./scripts/verify` EXIT=0** (FE 217/217,
  cargo verde, contracts M10 + UX_REDESIGN_01, fetch-sidecars --check, cargo check src-tauri,
  git diff --check). Evidencia = unit/integración mockeada; NO AppImage real, NO Cloudflare
  live, NO generación OpenCode live (no se reclama aceptación humana).
- **DELETE-CONFIRMATION «SÍ» = PRESERVADO (commit `3a7c6d1`, intacto en este pass).**
  `ConfirmDialog.tsx`/`ConversationsSidebar.tsx` sin cambios en este diff; `normalizeConfirmation`
  acepta `Sí/sí/SI/si` (+ espacios) y cadenas ajenas nunca confirman; Enter no saltea;
  Cancel nunca borra; flujo de proyectos conserva matching exacto del título.
- **M11 NO INICIADO.** Sin fuga de alcance: sin redesign de infra de publicación, sin cambios
  destructivos Task F, sin tocar runtime/session-directory (no reabiertos).
- **PRÓXIMO GATE (siguiente sesión FRESH):** (1) **FRESH REAL APPIMAGE BUILD + VERIFICACIÓN
  TÉCNICA** desde main `ebeac0e` (`scripts/smoke-package appimage`, sidecars pineados
  opencode 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real
  Fedora/Wayland); (2) **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco (escenario
  real §17/§15: adjunto rosco + prompt real → creación card [Abrir][Compartir], URL pública
  con el juego y sin "Material del proyecto", sin burbuja vacía, sin toast duplicado, modelo
  en Configuración, «Sí» para eliminar). NO iniciar M11. NO afirmar aceptación humana desde
  OpenCode. Rotación de sesión previa en `3251ffd` (orquestador previo alcanzó ~106K).

## Estado previo (CONFIRMACIÓN DE ELIMINACIÓN CON «SÍ» — CAMBIO FRONTEND BOUNDED, INTEGRADO, 2026-09-01)

- **DELETE-CONFIRMATION «SÍ» (frontend, acotado) INTEGRADO.** Cambio puntual sobre el
  diálogo compartido `app/src/components/ConfirmDialog.tsx` para la ELIMINACIÓN DE
  CONVERSACIÓN: ya NO se exige escribir el título exacto de la conversación; ahora se
  confirma con la afirmación **«Sí»**. `normalizeConfirmation` (trim → toLowerCase →
  NFD → strip U+0300–U+036f) acepta `Sí`/`sí`/`SI`/`si` y tolera espacios al inicio/final.
  Cadenas ajenas (`No`, `borrar`, el propio título, `s i`, `siii`, solo-espacios, vacío)
  NO habilitan el botón. **Enter NO puede saltar la confirmación** (input fuera de
  `<form>`, botones `type="button"`, `useFocusTrap` solo mapea Escape/Tab); el botón
  `danger` sigue `disabled={!ready || busy}`; Cancel/Escape/backdrop → `onCancel` nunca
  `onConfirm`.
- **SIN FUGA DE ALCANCE / SIN RELAJAR TAREA F.** La regla `ready` quedó:
  `confirmText !== undefined ? value === confirmText : normalizeConfirmation(value) ===
  normalizeConfirmation(messages.common.confirmYes)`. El flujo de PROYECTOS
  (`ProjectsView.tsx`, pasa `confirmText={deleting.name}`) conserva el matching **exacto**
  original (case/accent/sensitive, sin trim) → byte-idéntico al pre-cambio. El flujo de
  CONVERSACIÓN (`ConversationsSidebar.tsx`, pasa solo `confirmPrompt`, sin `confirmText`)
  usa la rama afirmativa. `commitDelete` (guard in-flight/busy, fail-closed, reset solo
  en éxito) y toda la semántica destructiva/persistencia/unpublish/filesystem de Task F
  quedaron **intactas** (diff solo frontend, 5 archivos, sin Rust/tauri/api).
- **A11Y / COPY.** `confirmPrompt` se asocia al input vía `aria-describedby` (`<p
  id="confirm-prompt">`); foco inicial en el input; `role="dialog" aria-modal` intactos.
  Copy voseo: `confirmYes: "Sí"`, `confirmPrompt: "Para confirmar, escribí Sí."`,
  `confirmNameLabel: "Confirmación"` (label sr-only genérico, aceptado).
- **REVIEWS INDEPENDIENTES (qwen3.8-flash, sesiones FRESH):** primera →
  **REQUEST_CHANGES** (should-fix scope-leak del matching + should-fix `aria-describedby`;
  nits de tests); fix acotado aplicado → re-review **APPROVE**. Nota: la sugerencia
  literal del reviewer (`confirmText === messages.common.confirmYes ? …`) habría roto
  `ConfirmDialog.test.tsx` (que pasa `confirmText` explícito); se resolvió con la regla
  explícito=exacto / ausente=afirmativo.
- **VERDE:** vitest FE **214/214** (21 archivos), `tsc --noEmit` 0, `eslint` 0,
  `prettier --check` 0, **`./scripts/verify` EXIT=0** (cargo check, contracts M10 +
  UX_REDESIGN_01, fetch-sidecars).
- **PENDIENTE (fuera de este cambio acotado):** el AppImage `930ee074…` se construyó
  desde `773278d` y **NO incluye** este cambio; el próximo AppImage fresco +
  re-aceptación humana deben incluirlo. El pase grande de corrección de aceptación
  humana (8 ítems) sigue pendiente y **debe preservar** esta confirmación con «Sí»
  (ítem 8). M11 **NO INICIADO**. El orquestador rota en este checkpoint.

## Estado previo (APPIMAGE NUEVO POST-T7 CONSTRUIDO Y VERIFICADO — TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE, M11 NO INICIADO, 2026-09-01)

- **APPIMAGE NUEVO POST-T7 REAL = PASS (sesión FRESH, deepseek-v4-flash, validación técnica completa).** AppImage NUEVO construido desde main `773278d` (post-T7 human blocker merge `d6f97ab` + checkpoint `773278d`) con el packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS).** Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.816.376 bytes**, **SHA-256 `930ee074bfbe40b4cf1e5c9582c93b884d695d6348bf7521e764ade5b9f6834d`** (NUEVO; difiere del stale T7 `3dba67a8…`), timestamp 2026-09-01 20:47:14 -0300, source commit `773278d`, build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool), repo limpio (working tree clean) antes del build, sin cambios de producto sin commitear. Sidecars bundlados pineados verificados en el payload extraído: opencode **1.18.25** y cloudflared **2026.8.3** (cloudflared SHA-256 `f29324fe…` idéntico al pin `config/components.json`). **Lanzamiento real en Fedora/Wayland (DISPLAY=:0):** app corre con WebKitNetworkProcess + WebKitWebProcess activos, backend `[agent] starting → ready` SIN falso error de arranque, sin errores en log. **PATH-independencia:** lanzado con PATH sin opencode/cloudflared; el sidecar opencode hijo se ejecuta desde el mount propio del AppImage (`/tmp/.mount_EducAIGcoBlM/usr/bin/opencode`, port 42523). **Frontend embebido correcto:** el binario embebe exactamente `assets/index-Dt0XeFOc.js` + `assets/index-CxEdFXeO.css` (idénticos nombres a los del `dist` generado en este build desde main `773278d`; los markers del fix — CSS `.conversation-menu-dropdown button.danger` / `danger-soft` y JS "Eliminar conversación" / `chat-status.err` — presentes en el dist embebido y `external_directory` en el binario). **`./scripts/verify` EXIT=0** (cargo fmt/clippy/test, FE 202/202, format/lint/typecheck, M10 + UX_REDESIGN_01 contracts, fetch-sidecars --check, cargo check src-tauri, git diff --check). **Targeted Blocker A runtime (probes directos contra el sidecar real empaquetado 1.18.25, 127.0.0.1:42523):** `POST /session` + body JSON `directory` → la sesión queda ligada al mount del AppImage (`/tmp/.mount_EducAIGcoBlM/usr`) — reproduce el bug; `POST /session?directory=%2Ftmp%2Fopencode%2Fpostt7-evidence%2Fws-test` → la sesión queda ligada al workspace EducAI deseado (campo `directory` en `GET /session`). **Secuencia de requests (sin aceptar "hola" único):** 3 prompts en la MISMA sesión ligada (hola → "Hola. ¿En qué puedo ayudarte?"; 2º → "Sí, sigo la conversación. ¿Qué necesitas?"; 3º → "Recibido, tercer mensaje. ¿En qué trabajamos?"), 1 conversación/sesión NUEVA con contexto de adjunto (rosco.txt → "Listo."), todos con `cwd` del message = `/tmp/opencode/postt7-evidence/ws-test`, sin ASK external_directory (permission deny bindeado), sin espera ~120s (respuestas en ~1s), y sin misclasificar fallo en vuelo como arranque. **Targeted Blocker B (vitest `ChatPanel.test.tsx`):** "does not duplicate a persisted failed assistant message as raw error text" PASS + "still renders a failed status line when an earlier failed bubble is not the newest message" PASS (fix de review preservado: burbuja histórica NO suprime fallo nuevo). **Targeted Blocker C (vitest `App.test.tsx` menu + CSS):** menu ⋮ Renombrar/Eliminar con `role=menu`/`menuitem` PASS; CSS `.conversation-menu-dropdown button.danger` con `color: var(--danger)` sobre superficie (contraste legible), hover `--danger-soft`, disabled `--muted`, nowrap + padding compacto, copy español intacto. Limpieza: instancias de prueba del AppImage (nueva y stale T7) terminadas, mounts `/tmp/.mount_EducAI*` removidos, worktrees limpios (solo `main`), branch único `corr/a-creation-contract` preexistente sin tocar. **Status: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED. M11 NO INICIADO. Gate siguiente y único: HUMAN PRODUCT-OWNER RE-ACCEPTANCE sobre ESTE AppImage nuevo (`930ee074…`).** Limitaciones para validación humana: (1) secuencia real-provider en el AppImage con modelo gratis y adjunto rosco se dejó al escenario §17 humano; (2) la validación de prompts usó el modelo gratis `big-pickle` determinista; (3) la visibilidad/contraste visual final del menú y los flows UI completos se confirman en el escenario humano; (4) no se ejecutó el escenario §17 completo (es humano).

## Estado previo (POST-T7 HUMAN BLOCKER PASS INTEGRADO — BLOQUEADORES A/B/C CORREGIDOS, ESPERANDO NUEVO APPIMAGE + RE-ACEPTACIÓN HUMANA, 2026-09-01)

- **POST-T7 HUMAN BLOCKER PASS INTEGRADO (`d6f97ab`).** El product owner probó
  el AppImage real T7 y encontró 3 bloqueadores; este pass los corrigió y
  fusionó. **Blocker A (raíz CONFIRMADA):** el adapter mandaba el directorio de
  sesión como campo JSON `directory`, que opencode 1.18.25 IGNORA (los campos
  desconocidos del body se descartan; NO es `additionalProperties:false`).
  La sesión quedaba ligada al cwd del sidecar (mount del AppImage), el agente no
  veía los adjuntos, colgaba en un ASK `external_directory` sin responder y el
  timeout de tarea de 120s se mapeaba como error de arranque falso
  "No se pudo iniciar el asistente de IA.". **Fix:** `with_directory_query`
  (`crates/project-opencode/src/lib.rs`) envía `POST /session?directory=<percent-encoded workspace>`
  (probado contra el sidecar real empaquetado 1.18.25: el query bindea el
  workspace y el asistente responde "listo"; el JSON body NO bindea). Se agrega
  body `permission: [{external_directory,* ,deny}]` para que el agente no
  pregunte por directorios externos (los adjuntos están dentro del workspace
  ligado, no se ocultan). El timeout en vuelo de tarea ahora mapea a
  `TaskFailed` → "No se pudo completar la creación." (honesto); el timeout de
  arranque `ensure_ready` SIGUE mapeando a `AiUnavailable` → "No se pudo iniciar
  el asistente de IA." (real). **Blocker B (doble render):** `ChatPanel` suprimía
  el `.chat-status.err` con `hasPersistedFailure` si existía CUALQUIER burbuja
  fallida histórica → podía ocultar un fallo nuevo sin burbuja persistida o en la
  ventana pre-refresh. Fix (`21e9e5b`): la supresión solo aplica si la burbuja
  MÁS NUEVA del timeline es assistant failed/cancelled con `text === agentMessage`;
  cualquier otro caso muestra el `.chat-status.err` (role=alert) una vez.
  **Blocker C (menú Eliminar):** `.danger` (texto blanco) ganaba sobre
  `background: transparent` del dropdown → texto blanco en menú blanco. Fix:
  `.conversation-menu-dropdown button.danger` con `--danger` sobre superficie,
  nowrap, padding compacto, hover `--danger-soft`, disabled `--muted`; copy
  español intacto. **Autor:** Cursor Grok 4.6 High (`corr/post-t7-blockers`,
  `b106d07b` + fixes `21e9e5b`). **Reviews:** UX Cursor Grok 4.6 High FRESH
  (APPROVE + re-APPROVE), código/a11y `opencode-go/qwen3.8-flash` FRESH
  (REQUEST_CHANGES → APPROVE; MAJOR = supresión ligada a burbuja más nueva, no a
  historial). **Evidencia runtime:** probes directos del orquestador contra el
  sidecar real del AppImage T7 (opencode 1.18.25, 127.0.0.1:36771) —
  `POST /session` + body JSON directory → `directory=/tmp/.mount_EducAIGjCKDD/usr`
  (NO bindea); `POST /session?directory=%2Ftmp%2F...` → bindea el workspace y
  `prompt_async` responde "listo". **Tests:** `./scripts/verify` EXIT=0 en main
  post-merge (cargo 565+, FE 202/202, fmt/lint/typecheck, M10 + UX_REDESIGN_01
  contracts, fetch-sidecars --check, git diff --check). **M11 NO iniciado.**
  **Siguiente gate:** construir AppImage NUEVO desde `d6f97ab`, verificación
  técnica, y re-aceptación humana del product owner (escenario real §15).
- **T7 APPIMAGE NUEVO REAL = PASS (sesión FRESH, deepseek-v4-flash).** AppImage

- **T7 APPIMAGE NUEVO REAL = PASS (sesión FRESH, deepseek-v4-flash).** AppImage
  NUEVO construido desde main `d25f957` (Task G integrada via `2451c50`) con el
  packaging canónico M10 `scripts/smoke-package appimage` (fetch-sidecars →
  `cargo tauri build --bundles appimage` → fallback documentado a appimagetool →
  inspección payload sidecars). **EXIT=0 (smoke-package PASS).** Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`,
  **180.816.376 bytes**, **SHA-256 `3dba67a83223394efa697f3e95ff6ad46ae504093df931459d2eea9b05259bd7`**
  (NUEVO; difiere del previo `423cdb28…`), timestamp 2026-09-01 17:35:27 -0300.
  Sidecars bundlados pineados y verificados en el payload y en el mount en vivo:
  opencode **1.18.25** y cloudflared **2026.8.3** (cloudflared SHA-256
  `f29324fe…` idéntico al pin). **Lanzamiento real en Fedora/Wayland
  (DISPLAY=:0):** app corre con WebKit renderer + network process activos,
  backend `[agent] starting → ready` SIN falso error de arranque, sin errores en
  log. **PATH-independencia:** lanzado con PATH sin opencode/cloudflared; el
  sidecar opencode hijo se ejecuta desde el mount propio del AppImage
  (`/tmp/.mount_EducAI…/usr/bin/opencode`). **Frontend Task G verificado:**
  el binario embebe exactamente `assets/index-CJy6dhvp.js` +
  `assets/index-BlpY7WEx.css` (idénticos al `dist` de main generado en este
  build). **`./scripts/verify` EXIT=0** (cargo fmt/clippy/test ok — 85 suites
  ok —, pnpm format/lint/typecheck ok, FE 201/201, fetch-sidecars --check ok,
  M10 version alignment 0.1.0, UX_REDESIGN_01 contract ok, cargo check
  src-tauri ok, git diff --check ok). Limpieza de procesos/mounts AppImage de
  prueba completada. **Status: TÉCNICAMENTE READY FOR HUMAN REVIEW. M11 NO
  iniciado. Gate siguiente y único: HUMAN PRODUCT-OWNER ACCEPTANCE (el humano
  abre el AppImage y corre el escenario real §15).**
- **T7 PENDIENTE (próxima sesión FRESH):** ~~AppImage NUEVO real desde main
  `2451c50`, sidecars pineados (opencode 1.18.25, cloudflared 2026.8.3, SIN
  cambiarlos), `./scripts/verify` PASS contra artefacto fresco, y luego STOP para
  aprobación humana del product owner.~~ **HECHO (ver arriba). M11 NO iniciar.
  Gate siguiente y único: HUMAN PRODUCT-OWNER ACCEPTANCE.**
- **ESTADO PREVIO (T6 y Task G) — ver secciones históricas abajo.**

- **Current main commit: `d6f97ab`** (merge del post-T7 human blocker pass). La
  corrección A-G + post-T7 A/B/C sigue INTEGRADA y verificada (ver detalle
  arriba). `git log --oneline -14` para el detalle.
- **T6 PLAYWRIGHT HEADED = PASS (sesión fresh, deepseek-v4-flash).** Ejecutado
  contra main `2451c50` (Task G integrada) con el harness canónico
  `docs/ux-redesign-01/harness/run.sh` (capture.py headed + measure.py + ocr.py,
  Vite dev server en :1420, boundary Tauri mockeado via `mock-inject.js`).
  **57/57 aserciones PASS**, MEASURE PASS (3 viewports), OCR 78/78 imágenes,
  EXIT=0. 17 flows × 3 viewports (1366×768, 1440×900, 1920×1080): PNG 78, `.ocr.txt`
  78, `.a11y.txt` 17. Flows 15-17 nuevos específicos de Task G/F: (15) Creation
  card Abrir/Compartir + Abrir sin error; (16) delete confirm type-name-to-confirm
  con copy llano (titulo/body/Eliminar habilitado tras teclear nombre/item
  removido); (17) asistente renderiza UNA vez (sin `.chat-status.ok` verde, texto
  llano). Flows 01-14 actualizados al DOM Task G (rename via menú "…",
  attachments `attachment-chip` con Abrir, share menu auto-open post-publish) y
  `mock-inject.js` `app_status → agent:"ready"` (contrato Task C; composer habilitado
  solo con backend ready). Sin hallazgos UX_BLOCKER/UX_IMPORTANT; solo POLISH
  pre-aprobado (nombre display del mock, contraste, tooltip truncation). Evidencia
  durable en `docs/ux-redesign-01/` (RESULTS.md actualizado). Verificación
  EXIT=0. **Budget al cierre de T6: ROTATE_SESSION_REQUIRED (111K) → la sesión
  checkpointea T6 y SE DETIENE; T7 NO inicia en esta sesión.**
- **T7 PENDIENTE (próxima sesión FRESH):** AppImage NUEVO real desde main
  `2451c50`, sidecars pineados (opencode 1.18.25, cloudflared 2026.8.3, SIN
  cambiarlos), `./scripts/verify` PASS contra artefacto fresco, y luego STOP para
  aprobación humana del product owner. M11 NO iniciar. Gate siguiente y único:
  HUMAN PRODUCT-OWNER ACCEPTANCE.
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
Tasks A-G hechas; **T6 Playwright headed PASS (57/57); T7 AppImage NUEVO
   real PASS (ver arriba); resta solo la aprobación humana del product
   owner**.
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
| T6 | ~~Playwright headed~~ **HECHA (PASS, 57/57)** | LOW (deepseek-v4-flash) | qwen3.8-flash | `docs/ux-redesign-01/harness/` | 3 viewports, 17 flows, 57 aserciones, 78 PNG + 78 OCR + 17 a11y; flows 15-17 Task G/F (creación Abrir/Compartir, delete confirm, no-duplicado asistente); measure + ocr PASS. EXIT=0. |
| T7 | ~~AppImage real + `./scripts/verify`~~ **HECHA (PASS)** | LOW/Composer | qwen3.8-flash | packaging M10 | AppImage con sidecars, lanzamiento real, verificación completa |

Orden sugerido: A → B → C (backend funcional, cada una con su worktree) →
D/E (LOW) → F-backend → F-UI + G (Grok) → review Grok → review qwen →
Playwright → AppImage → verify. Integrar solo commits revisados. NO M11.
**A, B, C, D, E, F, G YA integradas (main `2451c50`). VALIDACIÓN FINAL:
T6 Playwright headed PASS y T7 AppImage NUEVO real PASS (main `d25f957`).
Resta solo la aprobación humana del product owner. NO es M11.**

## Worktrees

- `main` → `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-harness`
  (integración, `ebeac0e`; NO es workspace de autor).
- Creation/Share/UX pass: worktree autor `../ai-publisher-corr-01-creation-share`
  (`corr/creation-share-ux-pass`, commits `3ba7c5a` + `857d98c`). INTEGRADO vía
  merge `ebeac0e`. A remover + branch a borrar en cierre de sesión.
- Post-T7 pass: worktree autor `../ai-publisher-corr-01-postt7`
  (`corr/post-t7-blockers`), UX review `../ai-publisher-corr-01-postt7-review`
  (`corr/post-t7-ux-review`), a11y review `../ai-publisher-corr-01-postt7-a11y`
  (`corr/post-t7-a11y-review`). A remover/branches a borrar en cierre de sesión
  tras integración de `d6f97ab`.
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

- **AppImage NUEVO POST-T7 (main `773278d`, post-T7 blocker fixes integrados):**
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`,
  180.816.376 bytes, SHA-256
  `930ee074bfbe40b4cf1e5c9582c93b884d695d6348bf7521e764ade5b9f6834d`,
  timestamp 2026-09-01 20:47:14 -0300. **ESTE es el artefacto para la
  re-aceptación humana.** (AppImage previo T7 `3dba67a8…` es STALE — predata los
  fixes A/B/C; NO usar como evidencia de aceptación.)
- Modelo gratis real confirmado: `big-pickle` (providerID `opencode`), cost 0,
  respuesta "¡Hola! ¿Cómo puedo ayudarte?". `modelGetSelected`/`default_free_model`
  determinista (ADR-0015); NO hardcodear nombres (solo tests/fake).
- PATH-independencia y sidecars bundled verificados en M10 y re-verificados en
  T7 contra el AppImage NUEVO (opencode 1.18.25 + cloudflared 2026.8.3 en
  payload; launch con PATH sin sidecars usa el bundled).
- **Este pass NO re-testea M1-M10; solo integró las correcciones A-G, corrió el
  Playwright headed (T6) y construyó/verificó el AppImage NUEVO (T7) para
  revisión humana.**

## Model allocation (sesión anterior cerrada)

- **Creation/Share/UX human-acceptance pass (COMPLETE, integrado `ebeac0e`): orquestador
  `opencode-go/deepseek-v4-flash` (esta sesión, budget CONTINUE al cierre). Autor
  `cursor-grok-4.6-high` vía Cursor (`corr/creation-share-ux-pass`, commits `3ba7c5a` +
  fix `857d98c`). Revisor UX independiente `cursor-grok-4.6-high` FRESH (sesión previa,
  APPROVE). Revisor código/a11y `opencode-go/qwen3.8-flash` FRESH (`creation-share-code-review`,
  pane `w1F:p1G`, REQUEST_CHANGES con M1 MAJOR + m1-m7 MINOR + LOW/NIT). Fix acotado por
  autor Cursor Grok 4.6 High FRESH (`creation-share-fix`, pane `w1F:p1H`, commit `857d98c`).
  Re-review código/a11y `opencode-go/qwen3.8-flash` FRESH (`creation-share-rereview`, pane
  `w1F:p1J`, APPROVE). Re-review UX acotado `cursor-grok-4.6-high` FRESH
  (`creation-share-ux-rereview`, pane `w1F:p1K`, APPROVE). Merge `ebeac0e` + evidencia
  `05a2c2a`, `./scripts/verify` EXIT=0 en main (FE 217/217). Panes reviewers cerrados tras
  APPROVE; fixer/author a cerrar en cierre de sesión; worktree autor + branch
  `corr/creation-share-ux-pass` a limpiar. Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0
  sesiones.**
- **Post-T7 human blocker pass (este): orquestador `opencode-go/deepseek-v4-flash`
  (rota en ROTATE_SESSION_REQUIRED 117K tras merge/cleanup). Autor
  `cursor-grok-4.6-high` vía Cursor (`postt7-author`, pane `w1F:p1A`,
  `corr/post-t7-blockers`, commits `b106d07b` + fixes `21e9e5b`). Revisor UX
  independiente `cursor-grok-4.6-high` FRESH (`postt7-ux-review`, pane `w1F:p1B`,
  `corr/post-t7-ux-review`, APPROVE + re-APPROVE tras MAJOR). Revisor código/a11y
  `opencode-go/qwen3.8-flash` FRESH (`postt7-a11y-review`, pane `w1F:p1C`,
  `corr/post-t7-a11y-review`, REQUEST_CHANGES → APPROVE; MAJOR resuelto en
  `21e9e5b`). Merge `d6f97ab`, `./scripts/verify` EXIT=0 en main. Panes author y
  ambos reviewers a cerrar en cierre de sesión; worktrees/branches a limpiar.
  Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0 sesiones.**
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

> **Nota (cambio «Sí» integrado en `main`):** el pase grande de corrección de
> aceptación humana (7 ítems, COMPLETO en `ebeac0e`) **preservó** la nueva
> confirmación de eliminación de conversación con «Sí» (ítem 8 del pase previo), y
> el **próximo AppImage fresco** debe construirse desde un `main` que ya la incluya
> y también las correcciones Creation/Share/Chat (el actual `930ee074…` es de
> `773278d` y NO trae este pass).

**PASS CREATION/SHARE/CHAT INTEGRADO Y VERIFICADO (`ebeac0e`).** Repo en
`TÉCNICAMENTE LISTO PARA RE-ACEPTACIÓN HUMANA` en cuanto se construya el AppImage
fresco. El ÚNICO gate siguiente es: (1) **FRESH REAL APPIMAGE BUILD + VERIFICACIÓN
TÉCNICA** desde main `ebeac0e`, y (2) que el **product owner re-corra el escenario
real §17/§15** sobre ESE AppImage nuevo. Solo el humano puede marcar HUMAN
ACCEPTED. M11 NO iniciar.

1. Construir AppImage NUEVO desde main `ebeac0e` con `scripts/smoke-package
   appimage` (fetch-sidecars → `cargo tauri build --bundles appimage`), sidecars
   pineados SIN cambiar (opencode 1.18.25, cloudflared 2026.8.3), `./scripts/verify`
   EXIT=0 contra el artefacto fresco, lanzamiento real en Fedora/Wayland con PATH
   sin sidecars, y luego entregar al product owner.
2. El product owner re-corre el escenario real §15 sobre el AppImage NUEVO:
   conversación nueva + adjunto de rosco + prompt real → el asistente responde y
   genera la creación; card de creación [Abrir][Compartir] (título humano, no
   "index"; sin cards duplicadas en turnos siguientes); Abrir funciona; el agente
   usa el archivo; Compartir produce URL pública usable con EL JUEGO (no "Material
   del proyecto"); sin burbuja vacía "Asistente"; sin toast duplicado; modelo en
   Configuración; menú "…" → Eliminar conversación con confirmación «Sí»;
   renombrar/eliminar conversación, reinicio y delete persistido. Solo el humano
   acepta el AppImage final. NO afirmar aceptación humana desde OpenCode.
3. NO iniciar M11. Este pass queda en TÉCNICAMENTE LISTO esperando el AppImage
   nuevo y la re-aceptación humana.
2. **Seguimiento recomendado NO bloqueante (de las reviews de G):**
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
   - (re-review code/a11y NIT) `registrar.rs`: el error del copy sidecar
     best-effort se descarta con `let _ =` sin log; agregar debug/warn.
   - (re-review code/a11y NIT) Skip lists (`build`, `dist`, `target`,
     `materials`) a cualquier profundidad podrían excluir una carpeta de
     actividad con ese nombre; improbable en este dominio, aceptado.
   - (re-review code/a11y LOW) `app.rs prepare_share_visibility`: Compartir
     explícito de una card no-web no degrada un Web público existente (la raíz de
     la URL puede no ser el artifact de la card); M1 le quitó su peor
     manifestación; revisitar solo si el producto quiere democión de cualquier
     Web público cuando el target no es Web.
   - (F review) chequeo de existencia de proyecto autoritativo DENTRO del lock
     del agente en `AgentService::run` + test single-instance delete↔agent.
3. NO iniciar M11. El pass de corrección queda en TÉCNICAMENTE READY FOR HUMAN
   REVIEW esperando aceptación humana.
