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

## Dependency rules
- UI does not invoke OpenCode/cloudflared directly.
- Core does not import Tauri APIs.
- Tunnel adapter does not understand Project objects.
- OpenCode adapter does not understand publication.
- Local publisher serves registered publish roots only.
