# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual (FRESH APPIMAGE POST-CORRECCIÓN CONSTRUIDO Y VERIFICADO TÉCNICAMENTE — TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE, M11 NO INICIADO, 2026-09-01)

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