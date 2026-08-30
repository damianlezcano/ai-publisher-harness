# M4 Cloudflare Quick Tunnel Design

Status: Approved and implemented. ADR-0005 is Accepted. M4 closed on
2026-08-29; `./scripts/verify` reports `M4 contract passed` and `git diff
--check` is clean.

## 1. Resumen ejecutivo

M4 expone el único `LocalPublisher` local a Internet mediante **un** Cloudflare
Quick Tunnel por sesión. Todos los proyectos publicados comparten ese túnel y
siguen usando sus rutas M3 durables:

```
https://<random>.trycloudflare.com/fotosintesis-a7k2
https://<random>.trycloudflare.com/sistema-solar-k91p
```

M4 introduce el crate `project-tunnel` con un port `TunnelProvider`, un
supervisor de proceso y el adapter `CloudflareQuickTunnel`. `PublicationManager`
depende de la abstracción, nunca de Cloudflare. La URL pública es estado de
sesión (runtime-only); nada de tunnel/PID/URL se persiste en `project.json`.

## 2. Boundary M3/M4/M5

| Milestone | Owns | Excludes |
| --- | --- | --- |
| M3 | Visibilidad, snapshot, ruta durable, ciclo de vida del único publisher local | Tunnel, URL pública, QR, UI, OpenCode |
| M4 | Un Quick Tunnel, base URL de sesión y URL pública por ruta | Snapshot contents, visibilidad, política del publisher local |
| M5 | OpenCode AgentEngine detrás de un port estable | Tunnel/URL públicas; no altera visibilidad ni snapshots |

M4 une una base URL efímera a la ruta de M3; no asume permanencia. M3 no cambia
su contrato; M4 añade orquestación encima.

## 3. ADRs propuestos

ADR-0005 cubre: boundary `TunnelProvider`, modelo de proceso cloudflared, y
semántica efímera de Quick Tunnel. Un solo ADR es suficiente; no hay ADRs
triviales adicionales.

## 4. Dependencias / módulos

Nuevo crate `crates/project-tunnel`:

```
crates/project-tunnel/
  src/lib.rs            # re-exports
  src/model.rs          # LocalOrigin, PublicBaseUrl, TunnelState, TunnelSession
  src/error.rs          # TunnelError, TunnelResult
  src/port.rs           # trait TunnelProvider
  src/resolver.rs       # BinaryResolver (PATH dev; sidecar future)
  src/supervisor.rs     # spawn/read/reap/kill del proceso
  src/log.rs            # extracción defensiva de la URL + logging seguro
  src/cloudflare.rs     # CloudflareQuickTunnel (impl TunnelProvider)
  src/fake.rs           # FakeTunnel (test helper)
  src/bin/fake_cloudflared.rs  # fake ejecutable para tests
  tests/models.rs
  tests/supervisor.rs
  tests/cloudflare.rs
```

Dirección de dependencias (sin ciclos):

```
project-publication -> project-tunnel (port + modelos)
project-publication -> project-publisher (existente)
project-publication -> project-fs / project-core (existente)

project-tunnel -> (std, nix)   # NO depende de project-core/fs/publisher/publication
```

`project-tunnel` no conoce `Project`, `Creation`, `inputs/workspace/outputs`,
`publish`, OpenCode, IA ni UI. Sólo `LocalOrigin`, ciclo de vida y `PublicBaseUrl`.

Dependencias nuevas de workspace: `nix` (señales POSIX, cfg unix) para shutdown
graceful. Sin tokio en `project-tunnel` (supervisor síncrono con hilo lector),
coherente con el trait síncrono `LocalPublisher`.

## 5. TunnelProvider contract

```rust
pub trait TunnelProvider: Send {
    /// Inicia el túnel hacia `origin` y espera (bloqueante, con timeout)
    /// hasta obtener la base URL pública o fallar. Idempotente en intención.
    fn start(&mut self, origin: LocalOrigin) -> TunnelResult<TunnelSession>;

    /// Sesión activa (base URL pública) si está corriendo.
    fn session(&self) -> Option<TunnelSession>;

    /// Estado actual.
    fn state(&self) -> TunnelState;

    /// Detiene el túnel (SIGTERM con kill controlado como fallback) y hace reap.
    fn stop(&mut self) -> TunnelResult<()>;

    fn is_running(&self) -> bool;
}
```

`TunnelSession` es un handle runtime con `base_url: PublicBaseUrl`. No expone
PID, path absoluto ni handles de proceso. Los errores son tipados (`StartFailed`,
`StartupTimeout`, `UrlNotDetected`, `ProcessExited { code }`, `AlreadyRunning`,
`NotRunning`, `StopFailed`) sin filtrar paths ni output crudo.

## 6. CloudflareQuickTunnel adapter

Implementa `TunnelProvider` sobre `supervisor` + `log` + `resolver`:

- `start(origin)`:
  1. `resolver.resolve()` -> `PathBuf` del binario.
  2. `supervisor::spawn(bin, origin)` con argv explícitos
     `["tunnel", "--url", origin.as_str(), "--no-autoupdate", "--loglevel", "info"]`
     y `env_clear` + `PATH`/`HOME` mínimos.
  3. Leer líneas (stdout+stderr, lossy UTF-8) y buscar `PublicBaseUrl`.
  4. Primer match válido -> `Running { base_url }`, latch.
  5. Exit antes de URL -> `Failed`. Timeout -> kill + `StartupTimeout`.
- `stop()`: signal graceful -> espera -> kill controlado -> reap. Siempre
  recolecta el hijo (sin zombis/huérfanos); `Drop` mata si sigue vivo.

## 7. LocalOrigin model

`http://127.0.0.1:<port>/`, port `1..=65535`. Parse estricto idéntico en rigor a
`LoopbackUrl`: rechaza `0.0.0.0`, IPs LAN, `localhost`, IPv6, path/query,
puertos fuera de rango y caracteres no numéricos. Construcción:
`LocalOrigin::from_port(u16)` o `LocalOrigin::parse(&str)`. El
`PublicationManager` lo deriva de `publisher.local_url()` (capacidad validada),
nunca de un `String` arbitrario del usuario.

## 8. PublicBaseUrl model

`https://<host>.trycloudflare.com/`. Validación: esquema `https` exacto, host en
ASCII `[a-z0-9-]+(.trycloudflare.com)` con al menos un label antes del sufijo,
sin port/userinfo/path/query/fragment. Método `join(route) -> String` que produce
`https://<host>/<route>/`. Rechaza `http://`, otros hosts, URLs con path, y
cualquier inyección.

## 9. Process supervision strategy

`std::process::Command` con argv explícitos (nunca shell). Un hilo lector por
pipe alimenta un `sync_channel` acotado; `start` sondea con timeout:

- detecta URL (exitoso);
- detecta `try_wait()` != None (exit antes de URL -> `ProcessExited`);
- timeout -> kill.

El contrato del supervisor es **portable** y no modela señales Unix hacia el
dominio. El port expone `start()`, `request_stop()`, `wait()`, `force_kill()`.
La implementación Fedora usa internamente SIGTERM/SIGKILL (`nix`) y `try_wait`
por `shutdown_timeout` (p.ej. 5s) con `kill()` final y `wait()` reap; un
`ProcessGuard` asegura kill+reap en `Drop` (sin huérfanos). Windows implementará
el mismo contrato con su mecanismo de terminación (deferido, no en M4).

## 10. URL discovery strategy

Extracción defensiva y testeada por línea (stdout+stderr, decodificación lossy;
UTF-8 malformado -> reemplazo y continuar). Por cada línea, candidatos
`https://...` validados por `PublicBaseUrl`; se toma el primer match y se
retiene. Rechaza: `http://`, host sin sufijo `trycloudflare.com`, URLs con path,
y URLs múltiples no válidas (se ignora lo que no valida).

Alternativa estructurada documentada (no implementada en M4): endpoint Prometheus
`--metrics` con la métrica `cloudflared_tunnel_quick_tunnel_hostname`. Se deja
como hardening futuro si el parsing resultara frágil en la versión testeada.

## 11. cloudflared version strategy

- Registrar **minimum supported + tested version** (no "latest" implícito).
- Invocación con `--no-autoupdate` para deshabilitar el auto-update de cloudflared.
- `BinaryResolver` devuelve el path: desarrollo = búsqueda en `PATH`; packaging
  futuro = path sidecar versionado. No hay downloader/updater en M4.
- El smoke test registra `cloudflared --version`; el manifiesto
  `config/cloudflared-version.env` documenta la versión testeada en Fedora 44.
- El supervisor no valida versión en runtime (MVP); sólo documenta/registra.

## 12. Lifecycle

Estado persistente vs runtime:

| Persistent | Runtime only |
| --- | --- |
| Visibilidad; ruta durable; último snapshot | `TunnelState`, base URL, PID, handles, timestamp de startup |

Tras reinicio todo es `Stopped`/Local; nunca auto-publicar ni auto-iniciar túnel.

Transiciones (un solo publisher + un solo túnel):

| Evento | Comportamiento |
| --- | --- |
| 0 publicados | publisher Stopped, tunnel Stopped |
| Publish A | prepare -> start publisher -> register A -> start tunnel -> base URL |
| Publish B | prepare -> reuse publisher -> register B -> reuse tunnel |
| Unpublish A (queda B) | unregister A; tunnel y publisher siguen |
| Unpublish último | unregister -> stop tunnel -> stop publisher |
| Republish A | snapshot update; misma ruta; sin tocar túnel |

Orden: arranque = publisher antes que tunnel; parada = tunnel antes que publisher.

## 13. PublicationManager integration

Extender `PublicationManager` con un genérico `T: TunnelProvider` (default
`NoopTunnel` para retro-compatibilidad M3), `tunnel: Mutex<T>`.

- `publish` (primer proyecto): tras `register` exitoso, si `published` estaba
  vacío, `origin = LocalOrigin::from_port(publisher.local_url().port())` y
  `tunnel.start(origin)`. Si falla -> `PublicationError::TunnelStart`, rollback
  de runtime (unregister + stop publisher, snapshot en disco preservado) y error.
- `publish` (proyecto adicional o update): reusa túnel; no reinicia.
- `unpublish` (último): `tunnel.stop()` y luego `publisher.stop()`. Fallo de stop
  -> `stop_failed` para reintento (patrón M3), sin romper B en curso.
- `Publication` gana `public_url: Option<String>`; `list_published()` y
  `endpoint()` exponen base pública cuando el túnel corre.
- `recover()` no auto-publica ni auto-inicia túnel; reintenta stops pendientes.

## 14. Failure semantics (MVP)

Política: fallar la operación, preservar snapshot local, un reintento controlado
en la siguiente acción del usuario. Sin daemon de reconexión.

1. Publisher arranca, cloudflared falla -> `TunnelStart`; rollback runtime; snapshot intacto.
2. cloudflared arranca pero no se puede parsear URL -> timeout -> `TunnelStart`; idem.
3. cloudflared sale inesperado -> `Failed`; sin auto-restart; próxima acción reintenta una vez.
4. startup timeout -> `TunnelStart`; kill; snapshot intacto.
5. shutdown timeout -> SIGKILL final; `StopFailed` recuperable; no huérfano.
6. unpublish durante Starting -> serializado por lifecycle lock (espera).
7. segundo Publish durante Starting -> serializado; reusa túnel ya Running.
8. crash transitorio con proyectos publicados -> `Failed`; próxima acción reintenta.
9. red no disponible -> start falla/sale -> `TunnelStart`.
10. Cloudflare no disponible -> timeout/exit -> `TunnelStart`.

## 15. Concurrent behavior

El `lifecycle` lock de M3 serializa start/register/stop de publisher y las
transiciones de túnel. Preparación A/B puede ser paralela; start/stop del túnel
se serializan. Publish A + Unpublish A se serializan en orden de llegada. El
arranque bloqueante del túnel ocurre bajo el lifecycle lock (ventana documentada
acotada por el timeout de startup).

## 16. Security model

Invariantes M1/M2/M3 se mantienen. M4 añade:

- Origin sólo `127.0.0.1:<port>` (LocalOrigin validado; nunca arbitrario/0.0.0.0/LAN/hostname).
- argv explícitos (sin shell) -> inyección de argumentos imposible (el origin ya
  está validado numéricamente).
- Binario resuelto por `BinaryResolver` (fuente confiable), no por input de usuario.
- `PublicBaseUrl` sólo `https://*.trycloudflare.com`; rechaza no-HTTPS y hosts foráneos.
- `env_clear` + PATH/HOME mínimos -> sin leaks de secrets/env; Quick Tunnel no
  requiere credenciales.
- Logs sin env completo, paths sensibles ni futuras credenciales.

Tests: origin arbitrario rechazado; origin no-loopback rechazado; inyección de
argumentos imposible; URL pública no-HTTPS rechazada; logs fake maliciosos
rechazados; sin leak de env/secrets; cleanup de hijo (sin huérfanos).

## 17. Logging / observability

Líneas internas seguras y acotadas:

```
[tunnel] starting origin=127.0.0.1:<port>
[tunnel] ready base_url=https://<host>.trycloudflare.com
[tunnel] exited code=<n>
[tunnel] stopped
```

Nunca: env completo, filesystem sensible, credenciales. Sin framework de logging
nuevo; emisión estructurada mínima.

## 18. Deterministic fake strategy

Dos niveles offline/deterministic, validando responsabilidades distintas:

A. `FakeTunnel` / `FakeTunnelProvider` **in-memory** (sin subprocess): prueba
   integración con `PublicationManager`, lifecycle, reuso, propagación de URL
   pública, failure semantics y concurrencia.

B. `fake_cloudflared` **ejecutable** (bin de test, `src/bin/`), controlado por
   `FAKE_CLOUDFLARED_MODE`: prueba el adapter/supervisor real — argv, spawn,
   stdout/stderr, URL discovery, logs malformados, timeout, exit temprano,
   shutdown, forced kill, flooding y cleanup. Invocado vía
   `env!("CARGO_BIN_EXE_fake-cloudflared")`. Cero red, cero Cloudflare.

## 19. Optional real smoke strategy

`scripts/smoke-cloudflare` (manual, Fedora-only, NO en `verify`):

1. localizar `cloudflared` en PATH (si falta: skip + reporte).
2. iniciar `LocalPublisher` y publicar contenido temporal.
3. `CloudflareQuickTunnel.start(origin)` -> URL trycloudflare.
4. request real (`reqwest`/curl) -> verificar contenido.
5. stop tunnel -> verificar cleanup del hijo.
6. limpieza siempre (trap), incluso ante fallo.

## 20. Tests

| Nivel | Cobertura |
| --- | --- |
| Unit | LocalOrigin (1); PublicBaseUrl parser (2); transiciones de estado (3); argv (4); timeout model (5) |
| Integración fake | start exitoso (6); extracción URL (7); múltiples líneas (8); URL malformada (9); exit antes de ready (10); timeout (11); stop limpio (12); kill fallback (13); start repetido (14); stop repetido (15); reuso de túnel (16); crash activo (17); stdout/stderr (18) |
| Lifecycle | Publish A inicia publisher+tunnel (19); Publish B reusa (20); Unpublish A mantiene B+tunnel (21); Unpublish último para tunnel+publisher (22); fallo de tunnel -> no "Publicado" (23); update no reinicia túnel (24); Publish A/B concurrente (25); Publish durante Starting (26) |
| Seguridad | origin arbitrario (27); no-loopback (28); inyección argv (29); URL no-HTTPS (30); logs maliciosos (31); sin leak env (32); cleanup hijo (33) |
| Real (opcional) | round-trip trycloudflare (34) |

## 21. Task breakdown

| # | Task | Nivel | Depende de | Worktree | Ownership |
| --- | --- | --- | --- | --- | --- |
| 0 | Diseño/ADR approval | HIGH_ARCHITECTURE | — | — | Codex (yo) + Human |
| 1 | `project-tunnel` scaffold + LocalOrigin/PublicBaseUrl/TunnelState + parsers + unit tests | MEDIUM | 0 | `m4/tunnel-models` | `crates/project-tunnel/**` |
| 2 | Process supervisor + fake binary + integración | HIGH_CODING | 1 | `m4/supervisor` | supervisor.rs, bin, tests |
| 3 | `CloudflareQuickTunnel` + URL extraction + env isolation + estado | MEDIUM_HIGH | 2 | `m4/cloudflare-adapter` | port.rs, cloudflare.rs, log.rs |
| 4 | `PublicationManager` + `TunnelProvider` + lifecycle + failure + public_url | HIGH_CODING | 3 | `m4/manager-integration` | `crates/project-publication/**`, `project-tunnel/src/fake.rs` |
| 5 | Lifecycle/security integration tests | MEDIUM | 4 | `m4/lifecycle-tests` | tests de publication + tunnel |
| 6 | Gate/docs/verify + smoke + ADR | HIGH_ARCHITECTURE | 5 | main (integration) | docs, scripts/verify, smoke |

## 22. Reasoning level por tarea

1 MEDIUM · 2 HIGH_CODING · 3 MEDIUM_HIGH · 4 HIGH_CODING · 5 MEDIUM · 6 HIGH_ARCHITECTURE.

## 23. Worktrees propuestos

`../ai-publisher-m4-tunnel-models`, `-supervisor`, `-cloudflare-adapter`,
`-manager-integration`, `-lifecycle-tests` (+ review worktrees por tarea). El
integration checkout (main) es Codex-only.

## 24. Model allocation

| Task | Author | Reviewer |
| --- | --- | --- |
| 1 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 2 | Cursor Grok 4.6 medium | OpenCode Go Kimi K2.7 Code |
| 3 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash |
| 4 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash |
| 5 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 6 | Codex (DeepSeek V4 Pro, lead) | DeepSeek V4 Flash (independiente) |

Fallbacks según `docs/AGENT_POLICY.md`; OpenCode Go nunca usa GPT/Grok. Todo
worker cumple `MODEL_REQUESTED == MODEL_ACTUAL` vía `scripts/agent-launch`.

## 25. Author / reviewer

Author != reviewer, preferentemente cross-family (DeepSeek <-> Grok/Kimi). El
lead sólo integra commits revisados y corre `./scripts/verify` tras cada batch.

## 26. Riesgos / deuda

- Parsing de URL de logs es un punto de acoplamiento con el formato de
  cloudflared; mitigado con `PublicBaseUrl` estricto + fake tests + alternativa
  `--metrics` documentada como hardening futuro.
- Shutdown graceful depende de `nix` (señales) en unix; Windows se difiere.
- Arranque bloqueante del túnel bajo lifecycle lock acota concurrencia (ventana
  = timeout de startup); aceptable para MVP single-user.
- Sin auto-restart: un crash transitorio deja proyectos "no públicamente
  alcanzables" hasta la próxima acción; un daemon de reconexión se difiere.
- Interferencia de `~/.cloudflared/config.yaml` del usuario con Quick Tunnel:
  detectar y mapear a un error claro; mitigación adicional en packaging.
- Una base URL efímera por sesión: los links dejan de funcionar al cerrar (ya
  es expectativa de producto documentada en CODEX_HANDOFF).

## 27. Definition of Done M4

- [ ] ADR-0005/design aceptado antes de código.
- [ ] Un solo publisher + un solo Quick Tunnel por sesión; rutas M3 estables.
- [ ] "Publicado" sólo si local register Y túnel (primer proyecto) tuvieron éxito.
- [ ] Start = publisher->tunnel; Stop = tunnel->publisher; sin túnel a origin muerto.
- [ ] Origin loopback-only, argv explícitos, URL https://*.trycloudflare.com only,
      env aislado, sin credenciales, sin huérfanos.
- [ ] Tests determinísticos offline (unit + fake process + lifecycle + seguridad)
      y smoke real opcional; M1/M2/M3 verdes.
- [ ] `./scripts/verify`, `git diff --check`, review de seguridad independiente,
      handoff evidence.
- [ ] Sin dependencias M4 prohibidas (OpenCode/IA/QR/UI/Tauri/packaging/Named Tunnel/DNS).

## 28. scripts/verify incremental

M4 conserva M3 y añade suites locales nombradas (offline):

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked -p project-tunnel --test tunnel_models
cargo test --locked -p project-tunnel --test tunnel_supervisor
cargo test --locked -p project-tunnel --test tunnel_cloudflare
cargo test --locked -p project-publication --test publication_tunnel
# + suites M3 existentes (publication_lifecycle/security, publisher_http/security, migration, lifecycle)
git diff --check
```

El smoke real (`scripts/smoke-cloudflare`) es manual/opcional y NUNCA parte de
`verify`. `verify` sigue determinista y offline.

## 29. Explícitamente M5+

OpenCode AgentEngine / IA / provider+model UX / QR / frontend / Tauri UI /
packaging / sidecars versionados y updater / Named Tunnel / account provisioning /
login / DNS / Windows build. M5 es OpenCode Adapter y no altera visibilidad ni
snapshots.
