# ADR-0004: Publication snapshot, durable route, and runtime state

- Status: Accepted

## Context

M3 adds project-level Publish and Stop sharing without a UI, tunnel, or public
URL. It must select only explicitly public creations, create an all-or-nothing
`publish/` snapshot, assign a friendly route, and coordinate M2's one local
publisher. M4's tunnel base URL may change each session.

## Decision

Schema version 2 persists `Creation.visibility` (`public` or `private`) and an
optional immutable project `publicationRoute`. All schema-v1 creations migrate
atomically to `private` on the first M3 mutation; route allocation occurs at
first publish and does not change on rename or republish.

M3 applies and enforces this persisted visibility only. It must never infer it
from a filename, Creation name, file type, content, keywords, or heuristic. A
Creation becomes public only through an explicit higher-layer operation. A
future OpenCode/agent layer may make that product decision, but has no M3
dependency and does not transfer its semantic responsibility to M3.

Active sharing is runtime-only. M3 persists neither a published flag, PID,
port, process handle, tunnel data, URL, nor QR. It prepares a sibling staging
tree, validates it, journals a directory swap, retains the previous snapshot,
and atomically replaces the M2 route root only after the new fixed `publish/`
root validates. M2 gains only atomic replacement of an existing route root.

## Consequences

- Durable route identity is distinct from the transient M4 URL.
- Legacy/unclassified output fails closed: it never becomes public by upgrade.
- M2 remains generic and never receives creation/project metadata.
- Only controlled `.publish-*` transient siblings are eligible for recovery
  cleanup; all source trees remain untouched.

## Alternatives considered

### Persist active publication state

It is stale after restart and implies a durable external URL that M4 cannot
provide.

### Generate a route per session

It breaks republish/rename stability and adds needless route collisions.

### Copy directly into `publish/`

It can expose a partial update and cannot preserve the previous snapshot.
