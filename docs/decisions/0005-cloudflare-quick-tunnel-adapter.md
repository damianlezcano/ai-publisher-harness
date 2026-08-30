# ADR-0005: Cloudflare Quick Tunnel adapter and ephemeral public exposure

- Status: Accepted

## Context

M3 delivers project-level Publish / Stop sharing against one local loopback
publisher, with durable routes like `fotosintesis-a7k2`. M4 must expose that
single publisher to the Internet through one Cloudflare Quick Tunnel so a
published project is reachable at `https://<random>.trycloudflare.com/<route>`.
The Quick Tunnel hostname is temporary and must not be persisted as durable
product state. No Cloudflare account, login, Named Tunnel, DNS, custom domain,
or credentials are involved.

## Decision

Introduce a new `project-tunnel` crate that owns a `TunnelProvider` port, a
process supervisor, and a `CloudflareQuickTunnel` adapter. It depends on no
project crate: it knows only a validated loopback origin, the tunnel process
lifecycle, and a validated public base URL.

`PublicationManager` depends on the `TunnelProvider` abstraction (not on
Cloudflare). `LocalPublisher` and `project-publisher` remain unaware of
Cloudflare. The tunnel adapter receives only a `LocalOrigin` capability derived
from the publisher's validated `LoopbackUrl`.

### Types

- `LocalOrigin`: strictly `http://127.0.0.1:<port>/`, port `1..=65535`. No
  `0.0.0.0`, LAN IP, hostname, IPv6, path, or query. Constructed from the
  publisher's `LoopbackUrl` at the boundary.
- `PublicBaseUrl`: strictly `https://<host>.trycloudflare.com/`. Single ASCII
  host label(s) before the exact `trycloudflare.com` suffix; no port, userinfo,
  path, query, or fragment.
- `TunnelState`: `Stopped | Starting | Running { base_url } | Stopping | Failed`.
  Runtime-only; never persisted in `project.json`.

### Testing strategy

Two offline, deterministic fake levels validate different responsibilities:

- An in-memory `FakeTunnel` (implementing `TunnelProvider`) tests
  `PublicationManager` integration, lifecycle, reuse, public-URL propagation,
  failure semantics, and concurrency without any subprocess.
- A `fake_cloudflared` test executable tests the real adapter/supervisor:
  argv construction, process spawning, stdout/stderr capture, URL discovery,
  malformed logs, timeout, early exit, shutdown, forced kill, output flooding,
  and child cleanup without any Internet or Cloudflare dependency.

### Process supervision

`cloudflared tunnel --url <origin>` is spawned with explicit argv (no shell),
`--no-autoupdate`, and a cleared environment plus a minimal `PATH`/`HOME`, so no
inherited secrets and no accidental reliance on user Cloudflare config. The
supervisor captures stdout/stderr, extracts the Quick Tunnel URL defensively
(strict `PublicBaseUrl` validation, first valid match latched), and detects
process exit.

The supervisor contract is **portable**, not modeled around Unix signals. The
domain and `TunnelProvider` see only `start()`, `request_stop()`, `wait()`, and
`force_kill()`. The Fedora implementation may use SIGTERM/SIGKILL internally,
but that detail never crosses the port boundary. A future Windows implementation
will satisfy the same contract with its own termination mechanism (deferred;
not implemented in M4).

### Lifecycle ordering

Start: `LocalPublisher` first, then tunnel (the tunnel points at an existing
origin). Stop: tunnel first, then `LocalPublisher` (never leave a tunnel
pointing at a dead origin). A Publish is reported successful only if both the
local registration and, for the first published project, the tunnel succeed.

## Consequences

- The tunnel abstraction is replaceable; a future Named Tunnel or other
  provider fits behind `TunnelProvider` without touching the core.
- Public reachability becomes a runtime-only session concept; a restart leaves
  everything Local and unpublished publicly, matching M3.
- Deterministic fake-process tests cover the supervisor offline; a real
  Cloudflare round trip remains a manual smoke test, never part of `verify`.
- A bundled, versioned `cloudflared` sidecar and target-specific binaries are
  deferred to packaging (M10/M11); M4 resolves the binary from `PATH` for
  development and real smoke tests.

## Alternatives considered

### Persist the Quick Tunnel URL in project metadata

Rejected: the hostname is ephemeral and may change each session; persisting it
would imply a durable external URL M4 cannot provide.

### Let PublicationManager invoke cloudflared directly

Rejected: it couples core orchestration to a specific binary and leaks
process/transport concerns past the port boundary.

### Parse the public URL only from a fixed log banner

Rejected in favor of strict `https://*.trycloudflare.com` extraction with
`PublicBaseUrl` validation, which tolerates banner-format drift and rejects
attacker-injected or non-HTTPS URLs.
