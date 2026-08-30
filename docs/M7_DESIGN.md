# M7 AI Provider Onboarding Design

Status: Approved and implemented. ADR-0008 and ADR-0009 are Accepted. Closed on 2026-08-30; `./scripts/verify` reports `M7 contract passed` and `git diff --check` is clean.

## 1. Executive summary

M7 removes the last technical prerequisite the non-technical user currently
faces: the assumption that OpenCode/provider credentials already exist. It
introduces a "Conectá tu IA" surface (provider list, authentication, connection
test, simplified model choice) with zero exposure of OpenCode IDs, config files,
environment variables, JSON, endpoints, or shell.

The decisive architectural finding is that the installed OpenCode (1.18.25)
already owns a complete provider integration layer over its loopback HTTP
server: a v2 **integrations** API with per-provider **auth methods** (`key`,
`oauth`, `env`), one-way credential storage, OAuth/device/browser flows, a
credential list with opaque IDs, and model discovery. M7 therefore **delegates
credential ownership to OpenCode** rather than building a second provider SDK or
a parallel keyring. Our app becomes a thin, secure conductor over OpenCode's
native API; it never persists a credential itself and never reads one back.

Security invariant #8 (credentials never in project files/logs/URLs/bundles) is
preserved by construction: credentials flow once from the frontend into
OpenCode's isolated `auth.json` (0600, app-managed data dir) over loopback HTTP,
and the frontend only ever receives `connected = true` plus an opaque label.

## 2. M6 / M7 / M8 boundary

| Milestone | Owns | Excludes |
| --- | --- | --- |
| M6 | Desktop shell + workspace UI + publish/QR | provider onboarding, clipboard paste, embedded preview |
| M7 | AI provider onboarding (providers, auth, connection test, simplified model choice, credential lifecycle) | attachments/advanced resource UX, billing, cloud sync, provider marketplace, packaging |
| M8 | Attachments / advanced resource UX (clipboard image paste, rich previews, embedded web preview) | — |

M7 does not reopen M5's `AgentEngine` contract (its port and `AgentService`
semantics stay identical). It *does* refactor the internal ownership of the
`opencode serve` process out of `OpenCodeAgentEngine` into a shared
`OpenCodeBackend` (§9, §24) so that the agent engine and the provider connector
share one backend — this is an infrastructure refactor, not a behavior change.

## 3. Actual OpenCode version / auth capability findings

Verified against the installed binary, `opencode --version` → **1.18.25**
(existing M5 supported range `>=1.18 <2` already covers it).

- `opencode auth` is an alias of `opencode providers`: `auth list` (list
  credentials), `auth login [url]` (`-p/--provider`, `-m/--method`), `auth
  logout [provider]`. The CLI is interactive; M7 does **not** drive the CLI, it
  drives the server HTTP API.
- Credentials are stored by OpenCode in `<data>/opencode/auth.json` (0600), where
  `<data>` is `XDG_DATA_HOME`. With M5's isolated XDG, this becomes
  `<app-data>/opencode/data/opencode/auth.json` — inside the managed config root,
  never the developer's `~/.local/share/opencode`.
- The server exposes a v2 **integrations** API (from `GET /doc`, OpenAPI 3.1):

| Endpoint | Purpose | Notes |
| --- | --- | --- |
| `GET /api/integration` | list integrations + methods + current connections | `IntegrationInfo{id,name,methods,connections}` |
| `POST /api/integration/{id}/connect/key` | store an API key | body `{key, label?}` → 204 |
| `POST /api/integration/{id}/connect/oauth` | begin OAuth/device flow | body `{methodID,inputs,label?}` → `IntegrationAttempt{attemptID,url,instructions,mode,time}` |
| `GET /api/integration/attempt/{attemptID}` | poll OAuth status | `pending`/`complete`/`failed`/`expired` |
| `POST /api/integration/attempt/{attemptID}/complete` | complete code-based flow | body `{code?}` → 204 |
| `DELETE /api/integration/attempt/{attemptID}` | cancel OAuth attempt | → 204 |
| `DELETE /api/credential/{credentialID}` | remove a stored credential | → 204 |
| `PATCH /api/credential/{credentialID}` | update credential | not used in MVP (delete + re-add) |
| `GET /api/model` | enabled model list | `ModelV2Info{id,providerID,family,name,cost,status,enabled,limit}` |
| `GET /config/providers` | providers + `default` map | `{providers:[], default:{providerID:modelID}}` |
| `POST /api/session/{sessionID}/model` | switch a session's model | body `{model:{providerID,modelID}}` |
| `GET /global/health` | readiness + version | `{healthy,version}` |

There is **no `GET /api/credential/{id}`** — credentials are write/delete only.
The secret is never readable back through the API. `ConnectionInfo` exposes only
`{type:"credential", id, label}`, never the secret.

- **Auth-method model** (`IntegrationMethod` union):
  - `key` → API key (optional `label`).
  - `oauth` → `{id, label, prompts?}`; `prompts` are optional labeled
    `text`/`select` inputs (e.g. an enterprise URL). OpenCode handles the
    redirect/callback itself; the app only shows the returned `url`/`instructions`
    and polls.
  - `env` → environment-variable names only. **Not offered in M7 UX** (it is the
    advanced/CI path and would leak env mechanics into the UI).
- **Providers observed** (212 integrations in the isolated catalog):
  - `openai` — methods `key`, `env`, `oauth` ×2 (`chatgpt-browser`
    "ChatGPT Pro/Plus (browser)", `chatgpt-headless` "ChatGPT Pro/Plus (headless)").
    A ChatGPT Plus/Pro **subscription is supported** via the OAuth methods.
  - `opencode` (OpenCode Zen) — `key` ("API key (service account)"), `env`,
    `oauth` ("OpenCode Console account", device flow).
  - `google` — `key` + `env` only. **Gemini is API-key only** (Google AI Studio /
    `GOOGLE_API_KEY`/`GEMINI_API_KEY`); a consumer Gemini subscription is not an
    API credential and must not be presented as one.
  - `deepseek` — `key` + `env` only. **API key required** (platform.deepseek.com).
  - `anthropic` — `key` + `env` only.
  - Every other provider — `key` + `env` only (no OAuth).
- **Free models**: the `opencode` provider ships zero-credential models
  (`*-free`, e.g. `nemotron-3.5-lightning-free`, `mimo-v2.5-free`) with
  `apiKey:"public"` baked in and `cost: 0`. `GET /api/model` returned 30 models
  with no credential configured, all under the `opencode` provider. The provider
  default is `{ "opencode": "big-pickle" }`. This is the low-friction starting
  point (§13).
- **Model catalog source**: models.dev, cached at
  `XDG_CACHE_HOME/opencode/models.json`. The free `opencode` models are available
  without network on first run; the broader provider catalog is only refreshed
  by the CLI (`opencode models --refresh`), which M7 does not invoke (see §32).
- **No dedicated "test connection" endpoint** exists; `connect/key` stores
  without validating. A connection test therefore requires a minimal real model
  call (§14).

## 4. Provider abstraction

New Tauri-free crate **`project-provider`** owns the provider domain and a
`ProviderConnector` port. Types are OpenCode-independent so project-core and the
UI never see OpenCode concepts. The single adapter is
`OpenCodeProviderConnector`; tests use `FakeProviderConnector`.

```rust
pub trait ProviderConnector: Send + Sync {
    fn list_providers(&self) -> ProviderResult<Vec<ProviderSummary>>;
    fn provider_detail(&self, provider_id: &str) -> ProviderResult<ProviderDetail>;
    fn connect_api_key(&self, provider_id: &str, key: &SecretString, label: Option<&str>)
        -> ProviderResult<ConnectionState>;
    fn begin_oauth(&self, provider_id: &str, method_id: &str)
        -> ProviderResult<OAuthAttempt>;
    fn oauth_status(&self, attempt_id: &str) -> ProviderResult<OAuthStatus>;
    fn complete_oauth(&self, attempt_id: &str, code: Option<&str>)
        -> ProviderResult<ConnectionState>;
    fn cancel_oauth(&self, attempt_id: &str) -> ProviderResult<()>;
    fn disconnect(&self, credential_id: &str) -> ProviderResult<()>;
    fn list_models(&self) -> ProviderResult<Vec<ModelSummary>>;
    fn test_connection(&self, provider_id: &str, model_id: &str)
        -> ProviderResult<ConnectionTest>;
}
```

DTOs (camelCase, serializable):

- `ProviderSummary { id, name, auth_methods, connected, connection_label, highlighted }`
- `ProviderDetail { id, name, auth_methods, connections }`
- `AuthMethodView { kind: "api_key" | "account", method_id: Option<String>, label, prompts: Vec<AuthPrompt> }`
  (`AuthPrompt { key, message, kind: "text" | "select", options?, placeholder?, optional }`)
- `ConnectionView { id, label }` (opaque credential id + user label)
- `ModelSummary { provider_id, model_id, name, free, recommended, deprecated }`
- `OAuthAttempt { attempt_id, url, instructions, mode: "auto" | "code" }`
- `OAuthStatus { status: "pending" | "complete" | "failed" | "expired", message: Option<String> }`
- `ConnectionTest { outcome: ConnectionTestOutcome, message: String }` where
  `ConnectionTestOutcome = Connected | CredentialInvalid | ProviderUnavailable | NoCompatibleModel | NetworkError`

The secret is typed as a redaction-safe `SecretString` (newtype, no `Debug`
printing, no accidental `Clone` into logs). It is held only long enough to POST
`connect/key` and is dropped immediately.

## 5. Auth-method model

Three native methods are mapped to two user concepts:

| OpenCode method | UI concept | Action |
| --- | --- | --- |
| `key` | "Clave de acceso" (API key) | paste key → `connect/key` |
| `oauth` | "Conectá tu cuenta" | `begin_oauth` → show URL/instructions (or open browser) → poll → done |
| `env` | (hidden) | not offered; ignored in UI |

Provider cards present only the methods OpenCode reports. Featured providers are
curated by `id` with accurate, non-misleading labels (§4/§16). No provider is
hardcoded in the backend beyond a "featured" ordering hint; the backend is
provider-generic and reads the integration list at runtime.

## 6. Credential-store architecture

**Decision: OpenCode owns credential storage; the app owns no secrets.**

The `ProviderConnector` port is the credential boundary. Its single adapter
delegates to OpenCode's integration API, and the durable storage is OpenCode's
`auth.json` (0600) inside the M5-isolated `XDG_DATA_HOME`. Rationale:

- OpenCode already implements OAuth/device/browser handshakes, token refresh,
  and provider SDK details. Reimplementing these violates "OpenCode remains the
  provider integration layer" and "our app should not become a second provider
  SDK framework".
- A parallel OS keyring would create two sources of truth and force us to
  re-inject secrets into OpenCode (env/`connect` on every launch), undoing the
  isolation M5 established.
- OpenCode exposes write/delete-only credential APIs with no read-back, which is
  exactly the "opaque reference, configured/not-configured only" contract M7 needs.

We therefore do **not** introduce an OS keyring in M7. The credential-domain
contract is expressed as the `ProviderConnector` port so a future
OS-keyring-backed store (Secret Service on Fedora, Credential Manager/DPAPI on
Windows) can be inserted behind the same port without touching project-core or
the UI. That future step is only warranted if we ever stop delegating to
OpenCode or need at-rest encryption stronger than 0600 (see ADR-0008
consequences).

The only product-persisted state that M7 adds is **non-secret selection state**
(selected `provider_id`/`model_id` and a small "featured order" hint), stored in
`<app-data>/settings.json` (§23) — never credentials, never in a project.

## 7. Fedora secure-storage strategy

- Credentials live at `<app-data>/opencode/data/opencode/auth.json`, created by
  OpenCode with `0600`. `project-app` also creates `<app-data>` (and the
  `opencode` subtree) with owner-only permissions (`0700` for the dirs) before
  first spawn.
- The `opencode serve` child runs with a cleared environment (M5 `ChildGuard`)
  plus only `PATH`/`HOME` and the isolated XDG vars; no credential is ever placed
  in the environment or in argv.
- Our own `settings.json` is written atomically (temp + rename, M1 pattern) and
  contains no secret values by construction (the store API has no secret field).
- At-rest protection is filesystem permissions, not encryption. This is
  acceptable for the MVP because the secrets are (a) single-user desktop
  credentials, (b) confined to the user's own app-data dir, and (c) already the
  mechanism OpenCode itself uses on this platform. Encrypted-at-rest via Secret
  Service is documented as a future hardening ADR, not an M7 requirement.

## 8. Future Windows portability strategy

- No Windows code in M7 (per `PLATFORM_POLICY.md`). Portability is preserved by
  design, not implementation.
- The "isolated app-managed data dir" is a single concept resolved through
  Tauri's `app.path().app_data_dir()` (already used by M6's `build_state`), which
  yields the correct per-OS location. The OpenCode child receives it via the
  platform-appropriate env vars (XDG on Linux; `APPDATA`/`LOCALAPPDATA` on
  Windows) behind a small `OpenCodePaths` helper in `project-opencode`. Windows
  only needs that helper's env mapping and OpenCode's Windows binary; both are
  deferred.
- The `ProviderConnector` port and the `SecretString` boundary are platform
  neutral. A future Windows credential-store swap (Credential Manager/DPAPI)
  slots behind the port.

## 9. Isolated OpenCode config integration

M5 already isolates the backend via `XDG_*` + `--pure`. M7 extends that
isolation to credentials and models without changing the mechanism:

- **Config dir** (`XDG_CONFIG_HOME`): product permissions/agent/skills only.
  Credentials are **never** written here.
- **Data dir** (`XDG_DATA_HOME`): `auth.json` (OpenCode credential store) +
  `opencode.db` (session/state). Fully isolated from the developer's
  `~/.local/share/opencode`.
- **Cache dir** (`XDG_CACHE_HOME`): `models.json` (models.dev catalog).
- **State dir** (`XDG_STATE_HOME`): ephemeral state.

The agent engine and the provider connector must share **one** `opencode serve`
process so that (a) credentials configured through the provider connector are
visible to agent sessions, and (b) model switches apply to the same sessions. To
make this safe, the process ownership moves out of `OpenCodeAgentEngine` into a
shared `OpenCodeBackend` (§24):

```
project-opencode (NEW)
  OpenCodeBackend { binary, config_dir, port, min/max version, http client, ChildGuard }
    ensure_ready()/base_url()/get()/post()/delete()/patch()/shutdown()

project-agent (refactor, no port change)
  OpenCodeAgentEngine { Arc<OpenCodeBackend>, session map }   // AgentEngine port unchanged

project-provider (NEW)
  OpenCodeProviderConnector { Arc<OpenCodeBackend> }          // ProviderConnector port

project-app
  owns Arc<OpenCodeBackend>, passes clones to both services
```

`build_argv`/`build_env`/health/version/`Semver` move into `project-opencode`.
The `AgentEngine` trait, `AgentService`, and `AgentPrompt.model` remain
unchanged; M1-M6 tests must stay green (mechanical refactor, same gate as M5
task 1).

## 10. Provider discovery

- The backend lists integrations via `GET /api/integration` (212 providers) and
  projects them to `ProviderSummary` with a **featured** flag.
- Featured (shown first, with friendly names/descriptions): `openai` ("ChatGPT"),
  `google` ("Gemini"), `deepseek` ("DeepSeek"), `anthropic` ("Claude"), and
  `opencode` ("Gratis, sin conexión" — the free tier). This list is a UX hint in
  `settings.json`, not a backend hardcode; unknown/removed featured ids are
  silently dropped.
- Remaining providers are available under a collapsed "Otros proveedores" list
  (searchable) for power users; they are never in the default path.
- `auth_methods` are taken from the provider's `methods`, filtered to `key` and
  `oauth` (drop `env`). Connection state is derived from `connections` (empty →
  disconnected; else first credential's `label`).

## 11. Model discovery

- `GET /api/model` returns enabled models as `ModelV2Info`. M7 projects them to
  `ModelSummary { provider_id, model_id, name, free, recommended, deprecated }`.
- `free` is grounded on `cost == 0` (reliable). `recommended` is grounded on the
  provider's default (`/config/providers` `default` map) or, when absent, the
  provider's first enabled model. `deprecated` is grounded on `status ==
  "deprecated"`.
- Models are grouped by provider and only shown for (a) connected providers and
  (b) the free `opencode` provider. The full developer catalog is never dumped
  into the UI.

## 12. Simplified model-selection UX

- Default experience = **one global model choice** stored in `settings.json`
  (M7 is global; per-project override is already plumbed via
  `AgentPrompt.model` and can come later).
- The default selected model is the **free recommended model** (`opencode`
  default), so first launch works with zero configuration.
- The "Modelo" control shows, in order: `Recomendado` (provider default),
  `Gratis` models, then the user's connected providers with their models
  (`name`, not `id`). No "Fast"/"More capable" labels are offered because they
  cannot be grounded reliably from the catalog (no latency signal).
- Selection is explicit. On generation, `run_agent` passes the stored
  `ModelRef` into `AgentPrompt.model`; a changed selection applies to the next
  prompt (no backend restart needed — §16).

## 13. Free-model policy

- The `opencode` provider's free models are the zero-friction starting point and
  the **sensible default**.
- UI labels them "Gratis" and states that free availability is not a promise
  ("Puede cambiar con el tiempo"). We never guarantee permanence.
- Fallback rules when the stored model disappears (after an OpenCode update or
  a catalog change):
  1. If the provider still has a free/recommended model, select it and tell the
     user ("Este modelo ya no está disponible; usamos el recomendado.").
  2. Otherwise, surface "Este modelo ya no está disponible. Elegí otro." and
     require an explicit choice.
  - We **never silently switch provider** and **never silently switch to a paid
    model**. If the only remaining models are paid, we stop and ask.
- Free vs paid is surfaced (free models carry a "Gratis" badge; paid models show
  no cost number, only "De pago" where OpenCode's `cost` is non-zero), so a user
  always knows which class they are selecting.

## 14. Test-connection design

`test_connection(provider_id, model_id)` runs a **minimal real model call** in a
throwaway session (temp dir under the managed dir, never a user project):

1. `POST /session` with a scratch directory.
2. `POST /session/{id}/message` (or `prompt_async` + poll) with a fixed trivial
   prompt ("Respondé: ok") and the target model, no tools.
3. Discard the session and **never register creations**.

Outcome mapping (no raw provider payloads/status surfaced):

| Signal | Outcome | Message |
| --- | --- | --- |
| completed with text | `Connected` | "Conectado." |
| 401/403 from provider | `CredentialInvalid` | "Esta clave no es válida." |
| 404 / model-not-found | `NoCompatibleModel` | "Este modelo ya no está disponible." |
| timeout / 5xx / connection refused | `ProviderUnavailable` | "No pudimos conectarnos con el proveedor." |
| DNS/TCP failure | `NetworkError` | "No hay conexión con el proveedor. Revisá tu conexión." |

A test on a paid model may consume a fraction of a cent; it is user-initiated
and the prompt is minimal. This is the most reliable minimal mechanism because
`connect/key` does not validate.

## 15. Disconnect / update credential lifecycle

- **Disconnect** = `DELETE /api/credential/{credentialID}` → connection state
  becomes disconnected → backend restart (§16) drops stale sessions.
- **Update** = delete + re-add (the `PATCH` credential endpoint is not used in
  MVP). The UI presents "Conectar de nuevo" rather than an in-place edit of a
  secret we never read back.
- Revoked/invalid credentials are detected by `test_connection` (or a failed
  generation) → message "Necesitás volver a conectar tu cuenta." The credential
  remains stored until the user disconnects (OpenCode owns it; we do not
  auto-delete on a provider auth error).

## 16. OpenCode backend / session restart semantics

- **Credential mutations** (connect key, OAuth complete, disconnect) **trigger a
  backend restart** (shutdown + lazy respawn on next use). This guarantees the
  new credential/model catalog is loaded and that no stale session can use a
  removed credential. Sessions are ephemeral per-run by M5 design, so dropping
  them is safe.
- **Model selection** does **not** restart the backend: the model is applied per
  prompt (`AgentPrompt.model`) or per session (`POST /api/session/{id}/model`).
- **Provider/model catalog** is refreshed by the restart only to the extent
  OpenCode's cached catalog allows; the models.dev refresh remains out of scope
  (§32).
- On app restart: settings persist (selection), agent runtime = stopped, and
  credentials persist in the isolated `auth.json` (they survive restart; only a
  disconnect removes them).

## 17. Frontend / Tauri command surface

Narrow, capability-scoped commands added to `app/src-tauri/src/commands.rs`
(same pattern as M6; no generic shell/fs/process):

```
provider_list                 -> Vec<ProviderSummary>
provider_detail(id)           -> ProviderDetail
provider_connect_key(id, key, label?) -> ConnectionView        // key enters ONCE, never returned
provider_oauth_begin(id, method_id)    -> OAuthAttempt
provider_oauth_status(attempt_id)      -> OAuthStatus
provider_oauth_complete(attempt_id, code?) -> ConnectionView
provider_oauth_cancel(attempt_id)      -> ()
provider_disconnect(credential_id)     -> ()
provider_test_connection(id, model_id?) -> ConnectionTest
model_list                     -> Vec<ModelSummary>
model_select(provider_id, model_id)    -> ()
model_get_selected             -> ModelSummary
```

Security rules: no `get_secret`/`get_api_key`/read-back command exists; the
frontend keeps the key only in a controlled input state, sends it once, and
clears it; the OAuth `url` is opened via the backend `opener` path (the frontend
never invokes an arbitrary browser URL itself — see §17 note). Provider/model ids
from the frontend are validated against the live integration/model lists before
use; unknown ids return `not_found` and are never echoed.

Note: `provider_oauth_begin` returns the auth `url` and the frontend shows a
"Conectar" button that asks the backend to open it (`opener::open_browser`), so
arbitrary URL opening remains backend-owned.

## 18. Error UX

New `ErrorCode` variants in `project-app` map to human messages (Spanish, no raw
payloads/stack traces/HTTP codes/ids):

| Code | Message |
| --- | --- |
| `ProviderNotFound` | "Ese proveedor no está disponible." |
| `ProviderConnectFailed` | "No pudimos conectar tu cuenta." |
| `CredentialInvalid` | "Esta clave no es válida." |
| `CredentialRevoked` | "Necesitás volver a conectar tu cuenta." |
| `ProviderUnavailable` | "No pudimos conectarnos con el proveedor." |
| `ModelUnavailable` | "Este modelo ya no está disponible." |
| `NoCompatibleModel` | "No encontramos un modelo disponible para este proveedor." |
| `NetworkError` | "No hay conexión con el proveedor. Revisá tu conexión." |

`AppError::from_provider` performs the mapping; the frontend renders only
`code + message`.

## 19. Logging / redaction policy

- Provider commands log only events (`[provider] connected`, `[provider]
  disconnected`, `[provider] test started`, `[provider] test failed
  outcome=credential_invalid`) — never the key, the OAuth token, the prompt body,
  or provider response bodies.
- `SecretString` has no `Debug`/`Display` that leaks; the `connect/key` HTTP body
  is constructed directly and never logged.
- A defensive log scrubber redacts high-signal credential shapes (`sk-…`,
  `AIza…`, `gsk_…`, `Bearer …`) from any logged string as a second layer; it is
  a belt-and-suspenders measure, not the primary defense.
- `settings.json` and any future diagnostic bundle are checked to contain no
  credential values (§20).

## 20. Security threat model

Threats addressed (each has a named regression test, §26):

1. **Credential enters project metadata** — assert `project.json`/`inputs`/
   `workspace`/`outputs`/`publish` are byte-unchanged after connect/disconnect.
2. **Credential returned by a read command** — the connector has no read path;
   `provider_list`/`provider_detail` return only `{id,label}`; test asserts no
   secret in DTOs.
3. **Credential logged** — scrub test feeds secret-shaped strings through the
   logger and asserts redaction.
4. **Credential leaked to unrelated child processes** — assert the `opencode
   serve` env contains no secret and that publisher/tunnel subprocesses never
   receive it; `connect/key` is sent only to the loopback OpenCode server.
5. **Malicious provider id / model id** — ids validated against live lists;
   unknown ids → `not_found`, no path/env/command injection.
6. **Credential deletion** — disconnect removes the credential and a restart
   clears sessions; a subsequent generation with the removed provider fails
   cleanly ("Necesitás volver a conectar tu cuenta.").
7. **Provider removal invalidates sessions safely** — restart-on-mutation (§16).
8. **Cross-project independence** — credentials are app-global and never written
   into any project; assert two projects remain credential-free after a connect.
9. **Fake OpenCode config cannot escape the managed root** — assert the isolated
   `auth.json`/`models.json`/`opencode.db` live under `<app-data>/opencode`,
   never `~/.config/opencode`, `~/.local/share/opencode`, or a project dir.
10. **Frontend cannot request arbitrary secrets** — no read-back command; the
    command surface is allow-listed and capability-scoped.
11. **Diagnostics redact secrets** — assert any exported/settings bundle contains
    no `auth.json` content or credential values.

## 21. Deterministic fake strategy

- `FakeProviderConnector` (in-memory): scripted providers, auth methods,
  connections, models, and connection-test results; error injection for invalid
  key, provider unavailable, model disappearance, and expired OAuth.
- Extend the M5 test-only `fake_opencode_server` (`crates/project-agent/tests/
  support/fake_server.rs`, moved/ shared as needed) with the integration
  endpoints (`/api/integration`, `connect/key`, `connect/oauth`, `attempt`,
  `credential`, `/api/model`) and scripted responses so
  `OpenCodeProviderConnector` is tested over HTTP, offline.
- No real credentials, no Internet, no real provider in `scripts/verify`.

## 22. Optional real-provider smoke strategy

`scripts/smoke-provider` (manual, never in verify): locate `opencode`, `serve`
with isolated XDG, list integrations/models, optionally connect a throwaway free
provider, run a trivial generation on a free model, disconnect, shutdown. SKIPs
cleanly when `opencode` or a usable free model is unavailable.

## 23. Persistence model

- **Credentials**: OpenCode `auth.json` (isolated data dir), owned by OpenCode,
  never by us.
- **Selection/settings** (non-secret): `<app-data>/settings.json`, atomic
  write, schema `{ selectedModel: {providerId, modelId}?, featuredOrder: [id]? }`.
  No secrets, no per-project state.
- Projects, materials, creations, publication: unchanged (M1-M6).

## 24. Module / dependency graph

```
project-opencode (NEW)
  ├── project-process (ChildGuard)
  └── reqwest + serde/serde_json (HTTP client, version range, isolated XDG env)

project-agent (refactor)
  ├── project-opencode (OpenCodeBackend)
  ├── project-core, project-fs, project-process
  └── (AgentEngine port + AgentService unchanged)

project-provider (NEW)
  ├── project-opencode (OpenCodeBackend)
  ├── (ProviderConnector port + models + errors + ProviderService + settings + fake)

project-app (extend)
  ├── project-agent, project-provider, project-opencode (owns Arc<OpenCodeBackend>)
  └── ... existing M1-M6 wiring

app/src-tauri (extend) -> project-app
app/src (extend)       -> Tauri commands (thin client)
```

`project-core` gains no provider/agent/OpenCode dependency (unchanged). The only
crates that know OpenCode are `project-opencode`, `project-agent` (adapter), and
`project-provider` (adapter) — all behind ports.

## 25. ADR(s) proposed

- **ADR-0008 (Accepted)** — AI provider onboarding via OpenCode's integration API, with
  delegated credential ownership (no app-owned secret store, no OS keyring for
  M7), one-way frontend credential flow, and restart-on-credential-mutation.
- **ADR-0009 (Accepted)** — Simplified global model selection and the free-model default /
  never-silently-switch policy.

## 26. Tests

| Level | Coverage |
| --- | --- |
| Unit | provider/model DTO mapping; `SecretString` redaction; method mapping (key/oauth/env); outcome mapping |
| Fake connector | connect key, OAuth begin/poll/complete/cancel, disconnect, model list, test-connection outcomes, error injection |
| HTTP adapter (fake server) | integration list, connect/key 204, connect/oauth attempt, attempt poll/complete/cancel, credential delete, model list, malformed/unexpected responses, timeout |
| ProviderService | default selection, model-disappearance fallback (same provider, no paid switch), explicit-only fallback when only paid remains |
| Security (§20) | named regression tests for threats 1-11 |
| Lifecycle | restart-on-credential-mutation, sessions dropped, model switch without restart, app restart persists selection |
| Frontend | "Conectá tu IA" components, model selector, OAuth poll UX, error rendering (mocked `invoke`/`listen`) |

## 27. Task breakdown

| # | Task | Level | Depends | Worktree | Ownership |
| --- | --- | --- | --- | --- | --- |
| 0 | Design/ADR approval | HIGH_ARCHITECTURE | — | — | V4 Pro + Human |
| 1 | Extract `project-opencode` (`OpenCodeBackend`) + migrate `OpenCodeAgentEngine`; M1-M6 green | HIGH_CODING | 0 | `m7/opencode-backend` | project-opencode, project-agent |
| 2 | `project-provider`: port + models + errors + `SecretString` + `FakeProviderConnector` | MEDIUM | 1 | `m7/provider-models` | project-provider/** |
| 3 | `OpenCodeProviderConnector` adapter + extend `fake_opencode_server` integration endpoints | HIGH_CODING | 2 | `m7/provider-adapter` | adapter, fake server |
| 4 | `ProviderService`: selection/settings persistence + test-connection orchestration + restart-on-mutation | HIGH_CODING | 3 | `m7/provider-service` | ProviderService, settings |
| 5 | `project-app` wiring: share backend, provider/model facade methods + DTOs + error mapping | MEDIUM_HIGH | 4 | `m7/app-provider` | crates/project-app/** |
| 6 | Tauri commands + capabilities + state | MEDIUM | 5 | `m7/tauri-provider` | app/src-tauri/** |
| 7 | Frontend "Conectá tu IA" + model selector + tests | MEDIUM | 6 | `m7/provider-ui` | app/src provider/ |
| 8 | Security + lifecycle tests + verify + smoke | MEDIUM/HIGH | 7 | `m7/provider-tests` | tests, scripts |
| 9 | Gate/docs/ADR + verify | HIGH_ARCHITECTURE | 8 | main | docs, verify |

## 28. Reasoning level per task

1 HIGH_CODING · 2 MEDIUM · 3 HIGH_CODING · 4 HIGH_CODING · 5 MEDIUM_HIGH ·
6 MEDIUM · 7 MEDIUM · 8 MEDIUM/HIGH · 9 HIGH_ARCHITECTURE.

## 29. Proposed worktrees

`../ai-publisher-m7-opencode-backend`, `-provider-models`, `-provider-adapter`,
`-provider-service`, `-app-provider`, `-tauri-provider`, `-provider-ui`,
`-provider-tests` (+ review per task). Integration checkout (main) is lead-only.

## 30. Implementation model allocation

Implementation orchestration stays on OpenCode Go DeepSeek V4 Flash; workers use
the executable model resolver (`scripts/agent-launch`). Candidate allocation:

| Task | Author | Reviewer |
| --- | --- | --- |
| 1 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash |
| 2 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 3 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash |
| 4 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash |
| 5 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 6 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 7 | Cursor Composer 2.5 (or DeepSeek Flash) | Cursor Grok 4.6 medium |
| 8 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 9 | DeepSeek V4 Pro (lead) | DeepSeek V4 Flash |

Fallbacks per `AGENT_POLICY.md`; `MODEL_REQUESTED == MODEL_ACTUAL` enforced.

## 31. Author / reviewer

Author ≠ reviewer, cross-family when practical. Tasks 1, 3, 4 (security-adjacent:
process extraction, credential adapter, restart semantics) get an independent
security review as a second pass, per `AGENTS.md`.

## 32. Risks / debt

- **OpenCode evolves fast**: the integration API is v2 and unversioned in
  spirit; mitigated by re-deriving from `GET /doc` in contract tests (M5
  pattern) and the existing `>=1.18 <2` range check.
- **Model catalog depends on models.dev cache**: the broader provider catalog is
  refreshed only by the CLI (`opencode models --refresh`), which M7 does not
  invoke. Free models are always present; a stale catalog is a UX gap, not a
  security gap. Revisit in component-update (M11).
- **At-rest encryption**: credentials are filesystem-protected (0600), not
  encrypted. Accepted for MVP; Secret Service/DPAPI is a future hardening path
  behind the `ProviderConnector` port.
- **Test-connection costs tokens**: minimal prompt, user-initiated; accepted.
- **Restart-on-mutation latency**: a credential change drops in-flight sessions;
  acceptable and safer than stale-session risk.
- **Shared-backend extraction touches M5**: mechanical; M1-M6 must stay green
  (same gate as M5 task 1).

## 33. Definition of Done M7

- [ ] ADR-0008/0009 and this design accepted before code.
- [ ] `ProviderConnector` port stable; project-core has no provider/OpenCode dependency.
- [ ] One shared `opencode serve` backend across agent + provider; config isolation intact (XDG + `--pure`).
- [ ] Connect API key, OAuth/device flow (begin/poll/complete/cancel), disconnect, model list, and test connection work end-to-end against the real OpenCode API (manual smoke).
- [ ] Simplified global model selection with a free default; never silently switch provider or to a paid model.
- [ ] Credentials flow one-way; no read-back command; no secret in project files/logs/URLs/bundles.
- [ ] Deterministic offline tests (fake connector + fake server) + optional real smoke; M1-M6 green.
- [ ] `./scripts/verify`, `git diff --check`, independent security review, handoff.
- [ ] No M7-excluded scope (billing, accounts, cloud sync, clipboard attachments, previews, packaging/updater, Windows, marketplace).

## 34. scripts/verify incremental

M7 keeps M6 and adds offline suites named here (documented in `docs/VERIFY.md`
in the same change):

```bash
# Rust (existing + new crates)
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked -p project-opencode --all-targets
cargo test --locked -p project-provider --test provider_models
cargo test --locked -p project-provider --test provider_adapter
cargo test --locked -p project-provider --test provider_service
cargo test --locked -p project-provider --test provider_security
cargo test --locked -p project-provider --test provider_lifecycle
cargo test --locked -p project-app --all-targets
# ... M1-M6 suites unchanged ...

# Frontend (existing) + provider components
pnpm --dir app run test

# Tauri
cargo check --manifest-path app/src-tauri/Cargo.toml
git diff --check
```

Real provider smoke (`scripts/smoke-provider`) is manual/optional, never in
verify.

## 35. Explicit M8 scope

M8 = Attachments / advanced resource UX: clipboard image/screenshot paste,
richer attachments, embedded/interactive previews, resource experience polish.
Nothing in M7 implements or blocks M8; the M6/M7 boundary ("provider onboarding"
vs "attachments/preview") is preserved.
