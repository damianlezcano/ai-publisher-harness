# ADR-0003: Local publisher boundary and HTTP stack

- Status: Accepted

## Context

M2 needs one local, read-only HTTP server that can expose several explicitly
registered project `publish/` roots on loopback. The server must not receive a
project root, must resist malformed URL and filesystem escape attempts, and
must remain independent of Tauri, OpenCode, Cloudflare, tunnels, and UI.

## Decision

Use a Rust `project-publisher` adapter crate built on Tokio and Axum. It owns
HTTP parsing, loopback listener lifecycle, route dispatch, and response
headers. The application-facing `LocalPublisher` port is independent of Axum,
Tokio, HTTP request types, Tauri, and project metadata.

The port registers a `PublishedProject` containing only a validated,
canonical `PublishRoot` capability and a `PublicationRoute`. It never accepts a
generic project path. A `RouteRegistry` validates and reserves route segments
before the HTTP adapter can serve them. The chosen M2 route is a normalized,
opaque lowercase ASCII route key such as `fotosintesis-a7k2`; it is stable for
the registration lifetime and unique process-wide. Friendly project names are
not authority and are not used for lookup.

M2 serves content already prepared in `publish/`; it does not copy from
`outputs/`, select creations, generate landing pages, or persist a publication
decision. It opens a dynamically assigned TCP port on `127.0.0.1` only and
exposes its loopback URL to a future tunnel adapter through `local_url()`.
MIME is determined by a controlled extension mapping with `nosniff`: PDF and
known web-image types may be inline, while office documents and unknown binary
types are downloaded. M2 does not sniff content.

## Consequences

- Axum/Tokio supply mature asynchronous HTTP and listener primitives while the
  core remains independent of the server framework. Their dependencies are
  introduced only with M2 implementation.
- Serving only a `PublishRoot` capability enforces the M1 layout boundary at
  the adapter input and keeps arbitrary project filesystem access unavailable
  to the publisher.
- Opaque ASCII route keys avoid Unicode/case/canonicalization ambiguities at
  the authorization boundary. A later UX layer may create a friendly slug plus
  random suffix, but M2 neither owns that persistence nor exposes a project
  directory listing.
- Dynamic ports avoid collisions and configuration. M2 tests consume the URL
  returned by the running publisher rather than assuming a fixed port.
- The M2 server is a technical capability. M3 owns durable publish/unpublish
  intent, route allocation policy, and application lifecycle orchestration.
- Revalidation and symlink rejection reduce filesystem races, but M2 does not
  add native-handle hardening to eliminate every TOCTOU race.

## Alternatives considered

### Raw Hyper or hand-written TCP HTTP

This minimizes framework surface but pushes parsing, method handling, request
normalization, and response correctness into product code. That is an
unfavorable security and maintenance tradeoff for a file-serving boundary.

### A synchronous small HTTP server

It can be sufficient for a prototype, but offers weaker composition with
application lifecycle and concurrent requests. It does not provide a clear
benefit over Tokio/Axum for a desktop Rust workspace.

### Tauri HTTP/plugin APIs or frontend serving

These couple the privileged server to the desktop shell or webview and make
the standalone core security boundary harder to test. They violate the
accepted dependency direction.

### Copy selected output files into `publish/` in M2

That introduces selection, preparation, and stale-content semantics before a
publication manager exists. M2 instead treats `publish/` as an explicit,
already-prepared root; a later milestone owns preparation deliberately.
