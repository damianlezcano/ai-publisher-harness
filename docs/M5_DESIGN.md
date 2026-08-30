# M5 OpenCode Agent Integration Design

Status: Approved for implementation. ADR-0006 is Accepted.

## 1. Resumen ejecutivo

M5 integra OpenCode como motor agente detrás de un port `AgentEngine`. El usuario
describe un recurso, el agente lo genera en `workspace`/`outputs`, y la app lo
registra como `Creation` (privada por defecto). No hay UI, drag/drop, QR ni
auto-publicación. Un solo backend `opencode serve` (loopback HTTP) sirve a todos
los proyectos, con una sesión por proyecto y working directory aislado por
proyecto. OpenCode permanece invisible y confinado detrás del adapter.

## 2. Boundary M4/M5/M6

| Milestone | Owns | Excludes |
| --- | --- | --- |
| M4 | Un Quick Tunnel + base URL de sesión | OpenCode/IA, visibilidad, snapshots |
| M5 | AgentEngine port, OpenCode serve adapter, sesiones por proyecto, detección/registro de outputs | UI, chat frontend, drag/drop, QR, credential UX, auto-publish |
| M6 | Chat/creaciones UX sobre AgentEngine | No reabre M5 (sin nueva lógica de agente) |

La generación no publica por Internet. Publicación sigue siendo acción explícita
(M3/M4) independiente de la generación.

## 3. ADRs propuestos

ADR-0006 (único): boundary `AgentEngine`, estrategia `opencode serve` HTTP,
aislamiento de config/sesión, sandbox de filesystem, y modelo de registro de
outputs. No hay ADRs triviales adicionales.

## 4. OpenCode version / API research

- **Instalado:** `opencode --version` → `1.18.25`.
- **`opencode serve`** (verificado): `--port` (0 = efímero), `--hostname`
  (default `127.0.0.1`), `--mdns` (false; activarlo fuerza `0.0.0.0` — NO usar),
  `--cors`, `--print-logs`, `--log-level`, `--pure` (sin plugins externos).
- **Endpoints reales (oficiales):**
  - `GET /global/health` → `{ healthy, version }`
  - `GET /doc` → OpenAPI 3.1 (introspección del contrato real en runtime)
  - `GET/POST /session` · `GET/PATCH/DELETE /session/:id`
  - `POST /session/:id/message` (prompt síncrono)
  - `POST /session/:id/prompt_async` (async, 204)
  - `POST /session/:id/abort` · `POST /session/:id/fork`
  - `GET /session/:id/diff` (cambios de archivos de la sesión)
  - `GET /session/:id/todo`
  - `GET /event` (SSE; primer evento `server.connected`, luego eventos del bus)
  - `GET /config` · `/app` · `/provider` · `/model` · `/agent`
- **Auth:** `OPENCODE_SERVER_PASSWORD`/`OPENCODE_SERVER_USERNAME` (basic auth).
- **Aislamiento de config:** `opencode debug paths` confirma que
  `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_CACHE_HOME`/`XDG_STATE_HOME` reubican
  config/data/cache/state. Verificado: `XDG_CONFIG_HOME=/x` → config `/x/opencode`.
- **Permisos:** config `permission` (allow/ask/deny, incl. `external_directory`)
  + `agent` (tools/permissions por agente) + flag `--auto` (auto-aprobar lo no
  denegado). La config resuelta del dev hoy incluye `permission.external_directory`
  y un `plugin` global — evidencia de que la config del desarrollador puede
  interferir (motivo del aislamiento).

## 5. AgentEngine contract

```rust
pub trait AgentEngine: Send + Sync {
    /// Asegura backend listo (lazy start) y devuelve el estado/versión.
    fn ensure_ready(&self) -> AgentResult<AgentBackendInfo>;

    /// Crea/abre la sesión de un proyecto (idempotente por project_id).
    fn open_session(&self, project: &AgentProject) -> AgentResult<AgentSession>;

    /// Envía un prompt; bloquea hasta completar, fallar, o cancelar.
    fn send(&self, session: &AgentSession, req: AgentPrompt) -> AgentResult<AgentTask>;

    /// Cancela una tarea en curso.
    fn cancel(&self, session: &AgentSession) -> AgentResult<()>;

    /// Estado actual del backend.
    fn status(&self) -> AgentStatus;

    /// Detiene el backend (idempotente).
    fn shutdown(&self) -> AgentResult<()>;
}
```

`AgentProject { project_id, directory }`, `AgentSession { id, project_id }`,
`AgentPrompt { text, model: Option<ModelRef> }`, `AgentTask { id, status }`.

## 6. OpenCodeAdapter strategy

`OpenCodeAgentEngine` implementa `AgentEngine` sobre `project-process` +
cliente HTTP mínimo (reqwest, o cliente std). Mapeo:

- `ensure_ready` → spawn `opencode serve` + `GET /global/health` (readiness) +
  check de versión.
- `open_session` → `POST /session` con `directory` = project root (cache por
  project_id).
- `send` → `POST /session/:id/prompt_async` (o `message` síncrono en hilo) +
  poll de estado hasta `idle`/`failed`, luego leer mensaje final + `GET /diff`.
- `cancel` → `POST /session/:id/abort`.
- `shutdown` → `request_stop`/`wait`/`force_kill` del `ChildGuard`.

No se embebe OpenCode ni se usa el SDK JS/TS. El contrato se re-deriva de
`GET /doc` (OpenAPI) en tests de compatibilidad, no se asume hardcoded.

## 7. Process lifecycle

Lazy start: primer `send` (o `ensure_ready`) arranca `opencode serve`; prompts
posteriores reusan; `shutdown` (o cierre de app) lo detiene. No se inicia si el
usuario sólo abre/publica un proyecto. Un `ChildGuard` (extraído a
`project-process`) garantiza spawn con argv explícitos, env aislado, captura
stdout/stderr, readiness vía `/global/health`, detección de exit, stop limpio y
kill controlado (sin huérfanos). Patrón idéntico a M4.

## 8. Loopback / binding

`--hostname 127.0.0.1` explícito, `--port 0` (efímero) o puerto elegido. Nunca
`--mdns` (fuerza `0.0.0.0`). OpenCode queda como backend interno local; Cloudflare
sólo publica el `LocalPublisher`. Internet nunca alcanza `opencode serve`. Opcional:
`OPENCODE_SERVER_PASSWORD` para basic auth sobre loopback (hardening documentado).

## 9. Config isolation

El adapter pasa `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_CACHE_HOME`/
`XDG_STATE_HOME` a un directorio gestionado por la app (p.ej.
`<app-data>/opencode`), más `--pure` (sin plugins externos). Así la config del
desarrollador (`~/.config/opencode`: permisos, plugins, credenciales) no afecta
el producto. Nuestro config aislado define permisos/agente/skills del producto.
No se implementa credential UI todavía.

## 10. Project filesystem permissions

Working directory = **raíz del proyecto** (para que OpenCode lea `inputs/` y
escriba `workspace/`/`outputs/`). Sandbox declarada en el config aislado:

- `external_directory`: **deny** (sin acceso fuera del proyecto; sin `~/.ssh`,
  `/etc`, otros proyectos, home completo).
- `edit` sobre `inputs/**` y `publish/**`: **deny** (inputs inmutables; publish
  generado sólo por M3).
- `project.json`: **deny** directo (los metadatos se tocan vía core APIs).
- Herramientas del agente: conjunto mínimo (read/edit/glob/grep/bash) con bash
  restringido a comandos no destructivos.

Patrones exactos se fijan contra el schema de config instalado durante la
implementación (no se inventa sintaxis). `--auto` aprueba lo permitido y mantiene
lo denegado denegado → el usuario final nunca aprueba permisos técnicos.

## 11. Session model

Una sesión OpenCode por proyecto (mapa `project_id -> session_id`, caché
volátil). Reuse entre prompts del mismo proyecto para conservar contexto
("hacelo más fácil para 10 años" modifica el trabajo previo). Sesión efímera por
ejecución: tras restart de la app se recrea si hace falta (los archivos del
proyecto persisten; el historial de chat no es requisito de M5). No se persisten
detalles frágiles de OpenCode.

## 12. Prompt / task model

`AgentPrompt { text, model?, attachments? }`. Los materiales de `inputs/` se
exponen al agente por ubicación (working dir), no por selección manual; M5 define
la API para adjuntar referencias opcionales a materiales (`AgentRequest {
project_id, prompt, selected_materials?: Vec<MaterialRef> }`) sin construir UI.
Task = una ejecución de prompt con `id` y `status` (Queued/Running/Completed/
Failed/Cancelled).

## 13. Streaming / polling decision

MVP: **polling** (no SSE). `prompt_async` + `GET /session/:id` hasta estado
`idle`, con timeout y soporte de cancel (`abort`). Evita complejidad SSE y es
determinista para tests. SSE (`GET /event`) queda documentado como evolución
futura (streaming de UI en M6).

## 14. Output detection strategy

Estrategia combinada, con prioridad a evidencia estructurada:

1. `GET /session/:id/diff` (cambios de archivos de la sesión) — evidencia
   filesystem de OpenCode, no texto del LLM.
2. Scan de `outputs/` por artefactos nuevos/creados bajo la sesión.
3. NUNCA se parsea el texto final del LLM como única fuente.

Resultado: lista de `Artifact { relative_path, kind, byte_size, sha256 }`.

## 15. Creation registration strategy

`AgentService` (project-agent) convierte `Artifact` en `Creation` vía
`ProjectService` de project-core:

- **ID**: UUIDv7 (igual que M1).
- **CreationKind**: inferido por extensión/estructura controlada (web si hay
  `index.html`; docx/pptx/xlsx/pdf/imagen por tipo), no por contenido.
- **visibility**: `private` por defecto, siempre.
- **display_name**: nombre de archivo/directorio saneado.
- **timestamps**: del reloj del core.
- **hash/byte_size**: SHA-256 + tamaño (consistente con M1).

Errores de registro no eliminan artefactos ya generados.

## 16. Public / private semantics

Todo output nuevo nace `private`. M5 no infiere publicidad desde filename/
contenido/heurística. La capa de producto futura (o un flag explícito en el
prompt/task) marcará `public` de forma explícita; M5 sólo provee el contrato
`CreationIntent { public: bool }` opcional por artefacto (default `false`) para
que una capa superior pueda decidir. Más seguro para MVP.

## 17. Multi-project strategy

Un solo backend `opencode serve` global + una sesión por proyecto (working dir
distinto). No se lanza un proceso por proyecto. Aislamiento por directorio +
sandbox (external_directory deny) evita que el proyecto A toque B.

## 18. Concurrency model

- Prompts del **mismo proyecto**: serializados (mutex por proyecto).
- Prompts de **distintos proyectos**: pueden ser paralelos si OpenCode lo
  permite; MVP los serializa también (un solo backend, un worker) salvo que se
  justifique paralelismo.
- Cancelación, cierre de app durante generación, crash de OpenCode, e
  invalidación de sesión: definidos en §19/§20.

## 19. Cancellation model

`cancel()` → `POST /session/:id/abort`. Estado → `Cancelled`. Los archivos
parciales quedan en el proyecto (no se borran automáticamente); la siguiente
acción puede continuar o el usuario descarta manualmente. Sin kill del proceso
por cancelación de un prompt (sólo se aborta la sesión).

## 20. Failure / recovery model

Cubrir: binario faltante, startup fallido, readiness timeout, HTTP no disponible,
respuesta malformada, creación de sesión fallida, fallo de generación, fallo de
tool/command, crash a mitad de tarea, cancelación, fallo de permiso de
filesystem, fallo de registro de output, archivos parciales, sesión obsoleta tras
restart. Política MVP: preservar archivos, tarea marcada `failed`, backend puede
reiniciarse en la próxima petición, sesión se recrea si es necesario, no borrar
artefactos del usuario. Sin daemon de recuperación.

## 21. Compatibility / version policy

`GET /global/health` devuelve `version`. El adapter verifica un **rango
soportado** (p.ej. `>=1.18 <2`); si no coincide → `AgentError::IncompatibleVersion`
claro, sin comportamiento indefinido. La versión exacta testeada se registra en
`config/opencode-version.env`. `GET /doc` (OpenAPI) permite re-derivar el contrato
en tests. Updater: diferido a packaging; M5 sólo documenta compatibilidad.

## 22. Logging / observability

```
[agent] starting backend
[agent] ready version=...
[agent] session created project=...
[agent] task started
[agent] task completed
[agent] task failed
[agent] cancelled
[agent] stopped
```

Nunca: API keys, prompts completos por defecto, contenido de documentos, env
completo, credenciales.

## 23. Security model

- Loopback-only (127.0.0.1), sin `--mdns`.
- Sandbox: external_directory deny, inputs/publish read-only o deny, project.json
  deny, sin acceso a otros proyectos/`~/.ssh`/`/etc`/secrets/home.
- Config aislada + `--pure` (sin plugins/credenciales del desarrollador).
- argv explícitos (sin shell), env mínimo.
- Output detection por evidencia filesystem/API, no por texto del LLM.
- Visibility default `private`; sin heurísticas.
- Tests: A no toca B; inputs inmutables; publish inaccesible; traversal/symlink;
  path de output malicioso; inyección de comando; working dir arbitrario; bind
  0.0.0.0 rechazado; leak de secrets/env; interferencia de config del usuario;
  output fuera del proyecto; respuesta API no confiable; metadata de artefacto
  maliciosa.

## 24. Deterministic fake strategy

- `FakeAgentEngine` (in-memory): para probar `AgentService` (registro de
  Creations, lifecycle de sesión, cancelación, fallos) sin subprocess ni red.
- `fake_opencode_server` (HTTP, test-only): responde los endpoints mínimos
  (`/global/health`, `/session`, `/session/:id`, `/prompt_async`, `/abort`,
  `/diff`, `/event`) con respuestas scripteadas (incluidas malformadas). El
  `OpenCodeAgentEngine` se apunta a él vía base URL inyectada. Cero Internet.
- El `ChildGuard` se prueba con `fake_process` (bin genérico extraído de M4).

Todos offline/determinísticos, en `scripts/verify`.

## 25. Optional real smoke

`scripts/smoke-opencode` (manual, no en verify): 1) localizar `opencode` (skip si
falta), 2) `opencode serve` (loopback, XDG aislado), 3) proyecto temporal,
4) prompt simple determinista, 5) detectar/registrar output, 6) shutdown, 7)
cleanup siempre. Preferir modelo barato/gratuito; no usar credenciales caras.

## 26. Module / dependency graph

```
project-agent
  ├── project-core   (Project/ProjectService, Creation registration)
  ├── project-fs     (workspace/outputs path resolution)
  ├── project-process (ChildGuard, extraído de project-tunnel)
  └── (HTTP client: reqwest con feature mínima, o cliente std)

project-process (NUEVO, extraído)
  ├── (std + nix) — ChildGuard + fake_process

project-tunnel (refactor)
  ├── project-process  (ChildGuard)
  ├── (resolver, log, cloudflare) — sin cambios de comportamiento

project-publication · project-publisher · project-core: sin cambios de M5
```

`project-core` no gana dependencias de agente/tunnel/OpenCode. `project-agent` es
el único crate que conoce OpenCode (sólo dentro del adapter).

## 27. Tests

| Nivel | Cobertura |
| --- | --- |
| Unit | modelos (AgentRequest/Task/status), parser de versión/rango, mapeo de errores |
| Fake engine | send exitoso, resultado, cancelación, fallos, lifecycle de sesión |
| HTTP adapter (fake server) | readiness, version check, create session, prompt_async+poll, abort, diff parsing, respuestas malformadas, timeout, crash, reuso de sesión |
| Registration | artefacto→Creation, default private, kind por extensión, IDs/hashes/timestamps, error de registro no borra artefacto |
| Security | A vs B, inputs inmutables, publish deny, traversal/symlink, output path malicioso, command injection, working dir arbitrario, bind 0.0.0.0 rechazado, leak env, config isolation, output fuera del proyecto, respuesta API no confiable |
| Lifecycle | lazy start, reuse, shutdown, restart, sesión obsoleta, multi-proyecto |

## 28. Task breakdown

| # | Task | Nivel | Depende | Worktree | Ownership |
| --- | --- | --- | --- | --- | --- |
| 0 | Diseño/ADR approval | HIGH_ARCHITECTURE | — | — | Codex (yo) + Human |
| 1 | Extraer `project-process` (ChildGuard) de project-tunnel + migrar tests (M1-M4 verdes) | HIGH_CODING | 0 | `m5/process` | project-process, project-tunnel |

La tarea 1 es un refactor PRE-M5 **funcionalmente equivalente**: `project-tunnel`
no cambia comportamiento observable; `project-process` contiene sólo
infraestructura genérica de subprocess. Gate obligatorio tras extraer:
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace --all-targets`, `./scripts/verify`, `git diff --check`.
Si M4 cambia de comportamiento: STOP.
| 2 | `project-agent` scaffold + modelos + `AgentEngine` port + `FakeAgentEngine` | MEDIUM | 1 | `m5/agent-models` | project-agent/** |
| 3 | `OpenCodeAgentEngine` + HTTP client + `fake_opencode_server` + supervisor | HIGH_CODING | 2 | `m5/opencode-adapter` | adapter, fake server |
| 4 | `AgentService` (registro de Creation, lifecycle, cancelación) + integración project-fs | HIGH_CODING | 3 | `m5/agent-service` | AgentService, registration |
| 5 | Lifecycle/security/registration tests | MEDIUM | 4 | `m5/agent-tests` | tests |
| 6 | Gate/docs/verify + smoke + ADR | HIGH_ARCHITECTURE | 5 | main | docs, scripts |

## 29. Reasoning level por tarea

1 HIGH_CODING · 2 MEDIUM · 3 HIGH_CODING · 4 HIGH_CODING · 5 MEDIUM · 6 HIGH_ARCHITECTURE.

## 30. Worktrees propuestos

`../ai-publisher-m5-process`, `-agent-models`, `-opencode-adapter`,
`-agent-service`, `-agent-tests` (+ review por tarea). Integration checkout (main)
es Codex-only.

## 31. Model allocation

| Task | Author | Reviewer |
| --- | --- | --- |
| 1 | Cursor Grok 4.6 medium | OpenCode Go Kimi K2.7 Code |
| 2 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 3 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash |
| 4 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash |
| 5 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 6 | Codex (DeepSeek V4 Pro, lead) | DeepSeek V4 Flash |

Fallbacks según AGENT_POLICY; OpenCode Go nunca GPT/Grok; `MODEL_REQUESTED ==
MODEL_ACTUAL` vía `scripts/agent-launch`.

## 32. Author / reviewer

Author != reviewer, cross-family cuando sea práctico. Lead integra commits
revisados y corre `./scripts/verify` tras cada batch.

## 33. Riesgos / deuda

- OpenCode evoluciona rápido: mitigado con version-range check + `/doc` re-deriva.
- Sandbox depende del schema de permisos de la versión instalada (pinned en
  implementación); no es aislamiento de OS-level completo.
- `--auto` aprueba lo no denegado: la seguridad descansa en el deny-list +
  external_directory deny; revisar en review de seguridad.
- Extracción `project-process` toca M4 (refactor mecánico; M1-M4 deben seguir verdes).
- Un solo backend global serializa prompts (MVP); paralelismo cross-proyecto diferido.
- La config aislada aún sin credential UI (proveedores se configuran fuera de M5).

## 34. Definition of Done M5

- [ ] ADR-0006/design aceptado antes de código.
- [ ] AgentEngine port estable; dominio sin dependencia OpenCode.
- [ ] `opencode serve` loopback-only, lazy start, readiness/version check, stop limpio.
- [ ] Config aislada (XDG + --pure) y sandbox (external_directory deny, inputs/publish protegidos).
- [ ] Sesión por proyecto, reuse, cancelación; prompts del mismo proyecto serializados.
- [ ] Output detection por diff/scan (no texto del LLM); Creations registradas private por defecto.
- [ ] Tests determinísticos offline (fake engine + fake server) + smoke real opcional; M1-M4 verdes.
- [ ] `./scripts/verify`, `git diff --check`, review de seguridad independiente, handoff.
- [ ] Sin dependencias M5 prohibidas (UI/chat/drag-drop/QR/credential UX/packaging/updater/Windows/auto-publish).

## 35. scripts/verify incremental

M5 conserva M4 y añade suites offline nombradas:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked -p project-process --test child_supervisor
cargo test --locked -p project-agent --test agent_engine
cargo test --locked -p project-agent --test opencode_adapter
cargo test --locked -p project-agent --test agent_service
cargo test --locked -p project-agent --test agent_security
# + suites M1-M4 existentes
git diff --check
```

Smoke real (`scripts/smoke-opencode`) manual/opcional, NUNCA en verify.

## 36. Explícitamente M6+

Chat UI / frontend / drag-drop UX / QR UI / credential & provider UX / skills de
producto completas / streaming UI / packaging / sidecars versionados + updater /
Windows release. M6 conecta chat+creaciones sobre el AgentEngine de M5 sin
reabrir su lógica.
