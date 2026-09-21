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
  Evidencia durable de reviews en `docs/history/reviews/qwen-review-creation-share.md`,
  `docs/history/reviews/qwen-rereview-creation-share.md`, `docs/history/reviews/ux-rereview-creation-share.md` (commit `05a2c2a`).
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
