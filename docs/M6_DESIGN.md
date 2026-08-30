# M6 Desktop Shell and Project Workspace UI Design

Status: Approved and implemented. ADR-0007 is Accepted. M6 closed on
2026-08-30; `./scripts/verify` reports `M6 contract passed` and `git diff
--check` is clean.

## 1. Resumen ejecutivo

M6 convierte M1-M5 en la primera aplicación desktop utilizable por una persona
no técnica: shell Tauri 2 + frontend React/Vite/TS, con lista de proyectos,
workspace de proyecto (chat, materiales, creaciones), y Publicar/Dejar de
compartir con URL + QR. La lógica sigue en el backend; el frontend es un cliente
estrecho sobre comandos Tauri. Se introduce `crates/project-app` (application
core Tauri-free) que cablea ProjectService, PublicationManager y AgentService.

## 2. Boundary M5/M6/M7

| Milestone | Owns | Excludes |
| --- | --- | --- |
| M5 | AgentEngine/OpenCode, registro de outputs | UI |
| M6 | Desktop shell + workspace UI + publish/QR (primer producto usable) | provider onboarding, clipboard paste, preview interactivo |
| M7 | AI provider onboarding no técnico (conexiones, credenciales, selección simplificada, test de conexión) | attachments avanzados, preview embebido |
| M8 | Attachments / advanced resource UX (clipboard image paste, previews ricos, preview web embebido) | — |

**Secuencia resuelta:** M7 = AI provider onboarding; M8 = attachments/advanced
resource UX. Esto sustituye la numeración previa de `CODEX_HANDOFF.md` (M7
Attachments, M8 Preview); el resto de milestones (Education polish, Packaging,
Component updates) se re-numeran posteriormente por nombre.

## 3. ADR(s) propuestos

ADR-0007 (único): framework frontend + boundary Tauri. No hay ADR por decisión
visual. El command surface y event/state se documentan aquí (no son tradeoff
durable independiente).

## 4. Frontend framework decision

React 19 + Vite + TypeScript (ADR-0007). Tooling: pnpm (o npm) con lockfile;
Vitest + React Testing Library; ESLint + Prettier; `tsc --noEmit`. Componentes
propios mínimos (sin design system pesado); accesibilidad de base.

## 5. Desktop architecture

```
app/src (React/TS)
  -> Tauri commands (app/src-tauri/src/commands.rs)  [thin adapter: DTO + error map]
      -> crates/project-app  (AppState: application core, Tauri-free)
          -> project-core / project-fs / project-publication / project-agent
          -> project-publisher / project-tunnel / project-process (transitive)
```

`project-app` posee el estado compartido (ProjectService, PublicationManager,
AgentService con tipos concretos) y expone operaciones de alto nivel con DTOs
serializables + `AppError` con mensajes para usuario. El shell Tauri sólo valida
DTOs, mapea errores y llama a `project-app`. No hay lógica de dominio en el
frontend.

## 6. Dependency graph

```
app (frontend) --> app/src-tauri (commands) --> project-app
project-app --> project-core, project-fs, project-publication, project-agent
project-publication --> project-publisher, project-tunnel, project-process
project-agent --> project-core, project-fs, project-process
```

`project-app` NO importa Tauri. El frontend no importa crates Rust; el
src-tauri no importa lógica de dominio (sólo DTOs/AppState).

## 7. Tauri command surface

Commands nombrados por capacidad (no por implementación), todos validados y
map-reados a códigos de error seguros:

```
project_list, project_create, project_open, project_rename, project_delete
material_add_from_path, material_list          (add por drop/picker = path real)
creation_list, creation_set_visibility, creation_open
agent_send, agent_cancel, agent_status
publish, unpublish, publication_status
app_status                                      (backend health/version)
```

NO existen: `execute_shell`, `read_file`, `write_file`, `open_path`, ni passthrough
genérico. Cada command devuelve un DTO serializable; los errores se traducen a
códigos tipo `ErrorCode::AiUnavailable`, `ErrorCode::PublishFailed`.

## 8. Event model

Requests/responses por comandos (source of truth). Tauri events sólo para
estado asíncrono empujado: `agent://task` (Queued/Working/Completed/Failed/
Cancelled) y `publication://state` (Local/Publishing/Published/Unpublishing/
Error). El backend emite; el frontend escucha y refresca. No se usan events para
sustituir resultados de comandos.

## 9. Frontend state model

Backend = source of truth. Frontend: estado local de UI (proyecto abierto,
borradores de prompt) + un patrón de data-fetching simple (hooks + invalidación
manual tras mutación). Sin Redux; a lo sumo un store ligero si hace falta.

## 10. Navigation / layout

Dos vistas: **Projects** (lista + crear/abrir/renombrar/eliminar) y
**Workspace** (proyecto abierto con columnas/sections: chat, materiales,
creaciones, publicación). Layout simple de dos paneles (lista de proyectos a la
izquierda; contenido del proyecto a la derecha). No hay filesystem técnico.

## 11. Project list UX

Listar nombres (no IDs). Botones: Nuevo proyecto, Abrir, Renombrar, Eliminar.
Eliminar exige confirmación visible (nombre en el diálogo). Errores humanos.

## 12. Project workspace UX

Cabecera con nombre del proyecto. Secciones: Conversación (chat), Materiales,
Creaciones, Publicación. Traduce `inputs/workspace/outputs/publish` a conceptos
de usuario (nunca mostrar esos paths).

## 13. Chat / prompt UX

Historial simple de turnos, input, Enviar, Cancelar, estado de tarea (Generando…
/ Completado / Falló) y error. Sin streaming token-by-token (MVP); un estado
animado "Creando tu recurso…" es suficiente (M5 difirió streaming).

## 14. Material UX

Lista de materiales por nombre/tipo legible (manual.pdf, captura.png) sin
hashes/IDs/paths. Añadir vía drag & drop y file picker. Duplicados/validación
delegados a M1.

## 15. Drag / drop strategy

Frontend recibe paths reales del OS (`onDragDropEvent` de Tauri v2) y llama
`material_add_from_path(path)`. Backend valida: archivo regular (no symlink),
size/tipo según política, copia a `inputs/` vía M1 (nunca mueve el original),
devuelve el material. Sin paths arbitrarios desde el frontend.

## 16. Clipboard / paste decision

**Diferido a M8** (Attachments / advanced resource UX). M6 soporta drop + picker.
Pasar imagen desde clipboard es valioso pero añade scope; se documenta, no bloquea M6.

## 17. Creation UX

Creaciones como recursos legibles con nombre + tipo humano (Actividad
interactiva, Documento, Imagen…) + acción Abrir. Kind/visibility legibles sin
tabla técnica.

## 18. Public / private UX

Sin checkbox de "publicar" por proyecto en la lista. A nivel proyecto hay un
único botón Publicar. Por Creation, indicador simple "Se compartirá / Privado" y
una acción contextual para cambiarlo (muta `visibility` vía backend). Sin tabla
de metadata.

## 19. Publish / unpublish UX

Botón Publicar → `PublicationManager` → LocalPublisher → Tunnel → URL pública.
Publicado: URL + [Copiar enlace] [QR] [Abrir en navegador] [Dejar de compartir].
Dejar de compartir sólo despublica ese proyecto; otros siguen online (no reinicia
túnel innecesariamente).

## 20. Public URL UX

Mostrar URL pública al publicar. Copiar / abrir en navegador. No editable. No
persistida (Quick Tunnel URL es efímera); tras reinicio vuelve a Local.

## 21. QR strategy

QR = exactamente la public project URL (nada más). Generación **local/offline**
con la librería npm `qrcode` (sin servicio externo; la URL no sale del cliente).
Opcional futuro: generación Rust si se quiere backend-owned.

## 22. Local preview / open strategy

- Documentos (docx/pptx/xlsx/pdf/imagen): `creation_open(creation_id)` → backend
  resuelve la ruta (nunca un path del frontend) y abre con la app del sistema
  (plugin `opener`, multiplataforma).
- Web creation: abrir `index.html` en el navegador del sistema (simple/seguro).
  Preview web interactivo/embebido → **M8**.

## 23. Error UX

Errores técnicos traducidos: OpenCode ausente → "No se pudo iniciar el asistente
de IA"; tunnel falla → "No se pudo publicar en Internet"; archivo inválido → "No
pudimos agregar ese archivo". Nunca mostrar ECONNREFUSED/stack traces/JSON/exit
codes. Logs técnicos sólo internos.

## 24. Restart behavior

Al reiniciar: proyectos/materiales/creaciones persisten; agent runtime = stopped;
publication = Local; tunnel = stopped. UI reconstruye desde backend (no asume
runtime previo). Sin auto-publish ni auto-agent al arrancar.

## 25. Tauri security / capabilities

Capabilities mínimas: los commands permitidos, sin `shell`/`fs`/`process`
globales. Frontend sin acceso arbitrario a filesystem/process/shell. Todas las
acciones privilegiadas vía commands Rust estrechos.

## 26. Frontend security model

Guardas/tests: path arbitrario desde frontend, command invocation arbitrario,
drop malicioso/symlink, `creation_open` con traversal, creación de proyecto A
desde B, IDs de proyecto falsos, publish de proyecto inexistente, inyección de
URL, abuso de clipboard, XSS/HTML injection desde nombres de proyecto/archivo,
open externo inseguro, suposiciones de spoof de eventos, allow-list de commands.
Todos los labels se renderizan escapados (React por defecto; sin dangerouslySetInnerHTML).

## 27. Accessibility approach

Keyboard navigation, labels asociados, contraste, focus visible, indicadores de
carga con `aria-live`, anuncios de error. Evitar deuda obvia; sin certificación
completa en M6.

## 28. Testing strategy

- Backend (project-app): unit + tests de mapeo de errores/DTOs.
- Frontend: Vitest + React Testing Library (componentes), ESLint, `tsc`.
- Tauri command layer: tests de DTO/error mapping con servicios fake.
- Security: los guards de §26.
- `scripts/verify` incorpora tooling frontend (offline). E2E Tauri smoke opcional
  separado.

## 29. Deterministic backend / fakes for UI

Tests de UI sin OpenCode/Cloudflare reales: command layer contra
`FakeAgentEngine` + `FakeTunnel` + repositorios en temp dirs. Fixtures
determinísticos. Nunca Internet en verify.

## 30. Optional smoke / demo strategy

`scripts/smoke-desktop` (manual Fedora): arranca la app (dev), lista proyectos,
crea un proyecto, verifica IPC básico. La demo IA/tunnel end-to-end (§ demo
target) permanece manual, usando la config OpenCode de desarrollo existente.

## 31. Task breakdown

| # | Task | Nivel | Depende | Worktree | Ownership |
| --- | --- | --- | --- | --- | --- |
| 0 | Diseño/ADR approval | HIGH_ARCHITECTURE | — | — | Codex + Human |
| 1 | `project-app`: AppState + DTOs + error mapping + wiring M1-M5 | HIGH_CODING | 0 | `m6/app-core` | crates/project-app/** |
| 2 | Tauri bootstrap + commands + capabilities + state | MEDIUM_HIGH | 1 | `m6/tauri-shell` | app/src-tauri/** |
| 3 | Frontend scaffold + Projects UI (list/create/open/rename/delete) | MEDIUM | 2 | `m6/projects-ui` | app/src projects/layout |
| 4 | Workspace UI: chat + materiales | MEDIUM | 3 | `m6/workspace-ui` | app/src chat/materials |
| 5 | Workspace UI: creaciones + publicación/QR | MEDIUM | 4 | `m6/publish-ui` | app/src creations/publish |
| 6 | Frontend tests + security tests + verify + smoke | MEDIUM | 5 | `m6/frontend-tests` | tests, scripts |
| 7 | Gate/docs/demo | HIGH_ARCHITECTURE | 6 | main | docs, verify |

## 32. Reasoning level por tarea

1 HIGH_CODING · 2 MEDIUM_HIGH · 3 MEDIUM · 4 MEDIUM · 5 MEDIUM · 6 MEDIUM · 7 HIGH_ARCHITECTURE.

## 33. Proposed worktrees

`../ai-publisher-m6-app-core`, `-tauri-shell`, `-projects-ui`, `-workspace-ui`,
`-publish-ui`, `-frontend-tests` (+ review por tarea). Integration checkout
(main) es Codex-only.

## 34. Model allocation

| Task | Author | Reviewer |
| --- | --- | --- |
| 1 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash |
| 2 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash |
| 3 | Cursor Composer 2.5 (o DeepSeek Flash) | Cursor Grok 4.6 medium |
| 4 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 5 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 6 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 7 | Codex (DeepSeek V4 Pro, lead) | DeepSeek V4 Flash |

Frontend bien definido → Composer/DeepSeek antes que Grok. Grok sólo en
integración/state complejo. `MODEL_REQUESTED == MODEL_ACTUAL`.

## 35. Author / reviewer

Author != reviewer, cross-family cuando sea práctico. Frontend visual review
opcional con otro worker si aporta. Lead integra y corre verify.

## 36. Riesgos / deuda

- Boundary Tauri/frontend: mantener el frontend como cliente fino; riesgo de
  leak de lógica al frontend (mitigado por commands DTO-only + review).
- Drag & drop pasa paths reales del OS: backend debe re-validar (symlink/size/
  tipo); no confiar en el path.
- `opener`/system-open multiplataforma; Windows se difiere pero la abstracción
  es portable.
- Clipboard image paste diferido (M8).
- Preview web completo (interactivo) diferido a M8.
- Provider onboarding diferido: la demo real usa config OpenCode de desarrollo.
- Sin packaging/updater/code signing (M10/M11).
- Herramientas frontend nuevas (pnpm/vitest) deben integrarse a verify sin
  romper determinismo offline.

## 37. Definition of Done M6

- [ ] ADR-0007/design aceptado antes de código.
- [ ] Shell Tauri 2 + React/TS corren en Fedora (dev).
- [ ] `project-app` cablea M1-M5; comandos Tauri estrechos y sin shell/fs/process genéricos.
- [ ] Lista/crear/abrir/renombrar/eliminar proyectos (delete con confirmación).
- [ ] Workspace con chat (send/cancel/estado), materiales (drop+picker), creaciones (abrir/visibilidad).
- [ ] Publicar/Dejar de compartir con URL + QR (offline) + estados claros.
- [ ] Errores traducidos a mensajes humanos; sin terminología técnica en UI.
- [ ] Tests frontend + backend (fakes) offline en verify; guardas de seguridad.
- [ ] `./scripts/verify`, `git diff --check`, review, handoff.
- [ ] Sin M6-excluded (provider onboarding UX final, packaging, Windows, auto-publish, IDE/terminal).

## 38. scripts/verify incremental

M6 conserva M5 y añade (offline, determinista):

```bash
# Rust (existente + project-app)
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
# ... suites nombradas M1-M5 existentes ...
cargo test --locked -p project-app --all-targets

# Frontend (introducido en M6)
corepack/pnpm install --frozen-lockfile   # reproducible
pnpm --dir app run format --check
pnpm --dir app run lint
pnpm --dir app run typecheck
pnpm --dir app run test

# Tauri
cargo check --manifest-path app/src-tauri/Cargo.toml

git diff --check
```

Comandos reales se fijan al elegir stack (no ficticios). Sin OpenCode/Cloudflare/
Internet/AI en verify. E2E/smoke desktop manual.

## 39. Explicit M7 / M8 scope

- **M7 = AI provider onboarding** (no técnico): cuentas/suscripciones del usuario,
  proveedores OpenCode, mecanismos de auth (API key, OAuth/device flow), detección
  de proveedor, listado de modelos, selección simplificada, "probar conexión",
  almacenamiento seguro de credenciales, errores de proveedor, defaults/modelos
  gratuitos. M6 NO intenta resolver esto.
- **M8 = Attachments / advanced resource UX**: clipboard image/screenshot paste,
  attachments más ricos, previews embebidos/interactivos, mejoras de experiencia
  de recursos. No se implementa en M6.
