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
  `docs/history/ux/ux-redesign-01/evidence/harness/run.sh` (capture.py headed + measure.py + ocr.py,
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
  durable en `docs/history/ux/ux-redesign-01/evidence/` (RESULTS.md actualizado). Verificación
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
