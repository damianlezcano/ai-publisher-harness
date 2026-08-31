# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current milestone: M8 — Attachments / Advanced Resource UX — **IMPLEMENTATION_COMPLETE**
- Current phase: COMPLETE (implementación integrada, verificada y cerrada)
- Current main commit: ver `git log -1` (commit de cierre M8)
- M1-M7: completados y cerrados
- M8: completado — ADR-0010 y ADR-0011 implementados
- verify: PASS — "M8 contract passed"
- git diff --check: limpio
- Siguiente acción: M9 (polish de UX educativo; contenido a diseñar luego). NO comenzar M9.

## Alcance M8 implementado

- **remove_material**: `ProjectService::remove_material` + `ProjectContentStore::remove_material`
  (metadata bajo concurrencia optimista, luego `inputs/<id>` con containment/symlink);
  el original nunca se toca. Tests core + fs.
- **Clipboard image paste**: comando `material_add_image` vía DOM `paste` (sin
  privilegio de clipboard); backend revalida (magic-byte PNG/JPEG/GIF/WEBP/BMP/
  SVG-root), cap 25 MB, dedup SHA-256 (duplicate=true), nombre sintético
  `captura-<ts>.<ext>`, creación atómica.
- **Multi-file import**: `materials_add_from_paths` -> `MaterialsImportReport` con
  resultado determinista por archivo (`added`/`duplicate`/`unsupported`/`failed`),
  fallo parcial permitido, dedup en proyecto y en batch, rechazo pre-lectura por
  tamaño (metadata) y symlink/directorio, originales nunca modificados.
- **Prompt attachments**: `AgentRequest.attachments` (nuevo, aditivo); frontend
  envía solo `MaterialId`; backend autoriza contra el proyecto actual (cross-project
  -> `AttachmentInvalid`), provisiona `workspace/materials/<n>-<safe>` ANTES de
  `open_session`, aumenta el prompt con nombres sanitzados, excluye `materials/`
  de artifacts. `AgentEngine` y `AgentPrompt` intactos.
- **Preview tiers**: `preview_data` (imagen/text-Markdown escapado, cap 2 MB,
  base64+contentType, nunca un path); PDF/office via system handler; web -> vista
  aislada.
- **Generated-web boundary (ADR-0010)**: crate `project-preview` (servidor
  loopback 127.0.0.1, token 128-bit de un solo uso, GET/HEAD, containment
  canónico + symlink reject, sin directory listing, MIME controlado + nosniff,
  teardown on close/Drop); ventana `preview` con capability `preview.json`
  VACÍA (cero permisos); URL y capabilities creadas backend-side; `on_navigation`
  fija el webview al origin del token; fallback a system browser disponible detrás
  de la misma interfaz `preview_open_web`. Los 8 invariantes de ADR-0010 están
  como tests (`preview_security`).
- **Tauri**: comandos nuevos + `agent_send(attachmentIds)`; main window sin
  permisos nuevos.
- **Frontend**: paste handler, chips de attachments, cards de materiales con
  Abrir/Quitar + confirmación, reporte de import por archivo, preview modal
  a11y (focus trap, Escape, focus return), acciones por tipo de creación, 47 tests.
- **Smoke**: `scripts/smoke-preview` (manual, Fedora, SKIP sin entorno gráfico).

## Task graph M8 (estado)

| # | Task | Estado | Autor | Reviewer |
| --- | --- | --- | --- | --- |
| 0 | Design + ADR approval | DONE | V4 Pro + Human | — |
| 1 | `remove_material` core+fs (+ tests) | INTEGRADO | DeepSeek V4 Flash | Grok 4.6 medium |
| 2 | `project-app` import/preview/attachment facade + DTOs + errors | INTEGRADO | DeepSeek V4 Flash | Grok 4.6 medium (re-review OK) |
| 3 | `AgentRequest.attachments` + provisioning | INTEGRADO | Cursor Grok 4.6 medium | DeepSeek V4 Flash |
| 4 | `project-preview` crate + security suite | INTEGRADO | Cursor Grok 4.6 medium | DeepSeek V4 Flash (security) |
| 5 | Tauri commands + `preview.json` + preview window | INTEGRADO | DeepSeek V4 Flash | Grok 4.6 medium (re-review OK: nav pin) |
| 6 | Frontend paste/chips/cards/preview UI + a11y + tests | INTEGRADO | Cursor Composer 2.5 | DeepSeek V4 Flash |
| 7 | Named suites + verify gate + smoke + docs/VERIFY | INTEGRADO | DeepSeek V4 Flash | Grok 4.6 medium |
| 8 | Gate/docs + verify + checkpoint | DONE | DeepSeek V4 Flash (lead) | — |

## Gates M8

- `cargo fmt --all -- --check` PASS
- `cargo clippy --locked --workspace --all-targets -- -D warnings` PASS
- `cargo test --locked --workspace --all-targets` PASS (505)
  - project-app materials 21 / attachments 6 / preview 9
  - project-agent agent_attachment 6
  - project-preview preview_security 10 / preview_lifecycle 4
- frontend: format, lint, typecheck, test (47) PASS
- `cargo check --manifest-path app/src-tauri/Cargo.toml` PASS
- `./scripts/verify` -> "M8 contract passed"
- `git diff --check` limpio
- Smoke: `scripts/smoke-preview` NO corrido (requiere sesión gráfica manual); la
  verificación de clipboard/drag real y de la ventana preview sin IPC queda para
  el smoke manual opcional.

## Documentos para la próxima sesión fresca

- CODEX_HANDOFF.md, docs/AGENT_POLICY.md, docs/ARCHITECTURE.md, docs/SECURITY.md
- docs/M8_DESIGN.md (completo, DoD marcado)
- docs/decisions/0010-*, 0011-* (implementados)
- docs/VERIFY.md (§M8), docs/WORKTREES.md, docs/MULTI_AGENT_WORKFLOW.md, docs/TESTING.md
- docs/CURRENT_CHECKPOINT.md (este documento)