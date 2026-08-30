# ADR-0008: AI provider onboarding via OpenCode integration API

- Status: Accepted

## Context

M5 isolated a product-managed `opencode serve` from the developer's global
OpenCode configuration, and M6 delivered the first usable desktop shell. M7 must
let a non-technical user connect an AI provider (OpenAI, Gemini, DeepSeek, and
others) without ever seeing OpenCode IDs, config files, environment variables,
JSON, or shell. The security invariant (SECURITY.md #8) forbids credentials from
ever appearing in project files, logs, URLs, or exported bundles.

The installed OpenCode (1.18.25) exposes a v2 **integrations** HTTP API over the
same loopback server M5 already drives: per-provider auth methods (`key`,
`oauth`, `env`), one-way credential storage, OAuth/device/browser flows, a
credential list with opaque IDs (no read-back), and model discovery. This means
OpenCode already *is* the provider integration layer and credential store.

## Decision

M7 delegates provider authentication, credential ownership, and model discovery
to OpenCode's integration API. A new `project-provider` crate defines a
`ProviderConnector` port (provider list/detail, connect API key, OAuth
begin/poll/complete/cancel, disconnect, model list, connection test) whose single
adapter `OpenCodeProviderConnector` drives the OpenCode server endpoints. No
second provider SDK and no app-owned credential store are introduced.

### Credential ownership and storage boundary

OpenCode owns credentials. The durable store is OpenCode's `auth.json` (0600)
inside the M5-isolated `XDG_DATA_HOME` (`<app-data>/opencode/data/opencode/auth.json`).
The app never persists a credential itself. The credential-domain contract is the
`ProviderConnector` port; the app holds only opaque credential references
(`ConnectionView { id, label }`).

We do **not** introduce an OS keyring (Secret Service / DPAPI) in M7. OpenCode
already implements OAuth/device handshakes and token refresh, and already stores
secrets with 0600 in the isolated data dir. A parallel keyring would create two
sources of truth and force re-injection of secrets into OpenCode, undoing M5's
isolation. A future OS-keyring-backed store can be inserted behind the same port
only if the product ever stops delegating to OpenCode or requires at-rest
encryption beyond 0600.

### One-way credential flow

The frontend submits a credential exactly once (`provider_connect_key`) or, for
OAuth, never handles a secret at all (`provider_oauth_begin` → show URL/instructions
→ poll). There is no `get_secret`/read-back command; OpenCode exposes no
`GET /api/credential/{id}`. The secret is typed as a redaction-safe `SecretString`
and is dropped immediately after the connect request.

### Shared backend and restart semantics

The agent engine and the provider connector share one `opencode serve` process.
Process ownership moves from `OpenCodeAgentEngine` into a shared
`OpenCodeBackend` (new `project-opencode` crate); the `AgentEngine` port and
`AgentService` semantics are unchanged (mechanical refactor). Any credential
mutation (connect key, OAuth complete, disconnect) triggers a backend restart so
no stale session can use a removed credential; model selection applies per prompt
without restart.

## Consequences

- The domain (`project-core`) and the publisher/tunnel remain independent of
  OpenCode; `project-provider` is an adapter behind `ProviderConnector`.
- Credentials cannot leak into project files, logs, URLs, or bundles by
  construction: they live only in OpenCode's isolated `auth.json` and flow
  one-way through the loopback HTTP API.
- The provider UX is provider-generic: 212 integrations are available, with a
  curated "featured" subset (OpenAI, Google, DeepSeek, Anthropic, and the free
  `opencode` tier) highlighted; the backend hardcodes none of their mechanics.
- OAuth/device flows (OpenAI ChatGPT, OpenCode Console) work without the app
  implementing any provider SDK. API-key providers (Google/Gemini, DeepSeek,
  Anthropic) use `connect/key`. The `env` method is intentionally not offered.
- At-rest protection is filesystem permissions, not encryption; accepted for the
  MVP and documented as a future hardening path.
- A connection test requires a minimal real model call (no validation endpoint
  exists), which is user-initiated and may consume a fraction of a cent.

## Alternatives considered

### App-owned OS-keyring credential store, re-injected into OpenCode

Rejected: duplicates OpenCode's OAuth/token-refresh/SDK responsibilities,
creates two sources of truth, and requires re-injecting secrets (env/connect)
on every launch, weakening the M5 isolation.

### Drive `opencode auth login` CLI subprocess

Rejected: the CLI is interactive and would require terminal emulation; the
server HTTP API is the stable, non-interactive contract we already use.

### Direct OpenAI/Gemini/DeepSeek SDK integrations in our app

Rejected: violates "OpenCode remains the provider integration layer" and
"no direct provider integrations in the MVP"; would make us a second provider
SDK framework and force us to reimplement OAuth.

### Read credentials back for display/editing

Rejected: OpenCode exposes no read-back, and displaying a secret violates the
"configured/not-configured only" contract and SECURITY.md #8.
