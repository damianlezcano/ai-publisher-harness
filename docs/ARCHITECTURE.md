# Architecture

## Principle
Keep product/domain logic independent of Tauri, OpenCode and Cloudflare.

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

## Dependency rules
- UI does not invoke OpenCode/cloudflared directly.
- Core does not import Tauri APIs.
- Tunnel adapter does not understand Project objects.
- OpenCode adapter does not understand publication.
- Local publisher serves registered publish roots only.
