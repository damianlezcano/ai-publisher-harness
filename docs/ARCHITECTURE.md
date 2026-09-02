# Architecture

## Principle
Keep product/domain logic independent of Tauri, OpenCode and Cloudflare.

Persistent on-disk layout (app data directory, conversations, materials,
workspace, Creations, preview temp, publish snapshots) is documented from
the current implementation in `docs/STORAGE_LAYOUT.md`. The AppImage is
package media, not the user-data container.

## Layers

```
UI
  -> Application Core
      -> Ports / Interfaces
          -> Adapters
```

## Core services
- ProjectManager
- MaterialManager
- CreationManager
- PublicationManager

## External adapters
- OpenCodeAgentAdapter
- FilesystemProjectStore
- LocalPublisherAdapter
- CloudflareTunnelAdapter

## Publication design
One local HTTP publisher serves zero or more published projects.

Example route table:

```
/fotosintesis-a7k2 -> <project A publish root>
/sistema-solar-k91p -> <project B publish root>
```

Cloudflare sees only one local origin, e.g. `localhost:<publisher-port>`.

M2's publisher is a read-only adapter: it receives only registered canonical
`publish/` roots and opaque route keys, never a general project root. It serves
content already prepared under `publish/`; the application decision to publish,
route persistence, and content preparation belong to M3. See proposed
ADR-0003 and `docs/M2_DESIGN.md`.

## Provider / model architecture (M7)

M7 delegates provider authentication, credential ownership, and model discovery
to OpenCode's v2 integrations API. The shared `OpenCodeBackend`
(`project-opencode`) owns the single `opencode serve` process used by both the
agent engine and the provider connector; the `AgentEngine` port and
`AgentService` semantics are unchanged by this mechanical refactor.
`ProviderConnector` (`project-provider`) is the credential-domain port whose
single adapter, `OpenCodeProviderConnector`, drives the OpenCode server.

Credentials are owned by OpenCode, one-way: the frontend submits a secret
exactly once (or never, for OAuth), OpenCode stores it in its isolated
`auth.json` (0600), and there is no read-back command and no app-owned secret
store. Model selection is one global choice with a free default (the `opencode`
tier, `cost: 0`), applied per prompt without restart; credential mutations
restart the shared backend so no stale session uses a removed credential
(ADR-0008, ADR-0009).

## Dependency rules
- UI does not invoke OpenCode/cloudflared directly.
- Core does not import Tauri APIs.
- Tunnel adapter does not understand Project objects.
- OpenCode adapter does not understand publication.
- Local publisher serves registered publish roots only.
- `project-core` has no provider/OpenCode dependency (unchanged).
- Only `project-opencode`, `project-agent`, and `project-provider` know OpenCode.
- Credentials never appear in project files, logs, URLs, or bundles (SECURITY.md #8).
