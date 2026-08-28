# ADR-0001: Desktop shell and core runtime

- Status: Accepted

## Context

The product is a local-first desktop application for Windows, macOS, and Linux.
It must keep privileged filesystem and future process control away from the UI,
remain simple to install, and later bundle OpenCode and cloudflared without
asking non-technical users to install prerequisites. The core must stay
independent of Tauri, OpenCode, and Cloudflare.

## Decision

Use **Tauri 2** as the desktop shell, with a **pure Rust Application Core** in
its own crate and a **TypeScript web UI** in a separate package. No UI framework
is selected in M1; it is not needed to implement or test the core.

The dependency direction is:

```
TypeScript UI -> typed desktop API -> Tauri command adapter -> Rust Application Core
                                                      -> ports -> adapters
```

The Rust core contains domain types, use cases, ports, and no Tauri imports.
Tauri command handlers are thin adapters that validate serialized request DTOs,
map errors to user-safe codes, and call core use cases. The UI gets an explicit,
allow-listed API surface; it never receives arbitrary filesystem access or a
generic command runner.

IPC uses Tauri command invocation with versioned, serializable request/result
DTOs. Commands are named by user capability, not implementation detail (for
example `create_project`, not `write_project_json`). Request DTOs use IDs,
validated names, and user-selected file handles/paths only at the boundary.
The command adapter resolves paths; the UI never sends relative paths into a
project tree. Events are reserved for asynchronous status updates and do not
replace command results.

Future OpenCode integration remains behind the `AgentEngine` port. The initial
preferred M5 implementation is `OpenCodeHttpAdapter`, which talks to an
`opencode serve` headless server over loopback HTTP/OpenAPI. Process
supervision and sidecar packaging sit outside the domain and may be performed
by a Rust infrastructure adapter, but the `AgentEngine` implementation
mechanism is deliberately replaceable. A future `OpenCodeEmbeddedAdapter` or a
generated client may replace the HTTP adapter without changing the Core;
`FakeAgentEngine` supports tests.

Cloudflared likewise remains behind a tunnel port, with process supervision
outside the domain. Tauri's external-binary bundle mechanism will package
pinned, target-specific sidecars. Capabilities grant only the exact sidecar and
arguments needed; JavaScript receives status and approved use-case results,
never process control or raw stdout/stderr by default.

Testing is layered: Rust unit tests for domain/use cases, filesystem integration
tests against temporary directories, command-adapter tests for DTO/error
mapping, TypeScript UI component tests, and Tauri WebDriver end-to-end tests
once a UI exists. External processes use controlled fakes in automated tests.

## Consequences

- Rust is used where durable local storage, atomic I/O, path safety, and future
  sidecar supervision matter; TypeScript is used where browser UI ergonomics
  matter. Neither language crosses into the other's responsibility by default.
- The project has Rust and Node/TypeScript toolchains. `scripts/verify` pins
  and runs Rust checks for M1; Node/frontend checks begin only when a real
  frontend workspace is introduced in its applicable milestone.
- Tauri capabilities and a narrow command layer create an auditable boundary.
- Target-specific sidecar artifacts and signing/release automation are deferred
  to M4/M10. M5 must pin and contract-test the chosen OpenCode HTTP API rather
  than binding the Core to its SDK or process details.

## Alternatives considered

### Electron with a TypeScript/Node core

Electron offers one JavaScript/TypeScript ecosystem and mature packaging, but
ships Chromium and Node, makes a privileged Node main process the natural core,
and requires continued discipline around preload/context-bridge APIs. Its
sandbox and narrow bridge can be secure, but the product's local filesystem and
future sidecar boundary are clearer with a non-Node core. It remains a viable
fallback if Tauri's platform packaging proves unacceptable.

### Tauri with all application logic in Rust, including UI

This maximizes a single native language but sacrifices the web UI ecosystem and
would make later UI work harder without improving M1's filesystem guarantees.
It is unnecessary coupling.

### Tauri with a TypeScript core in the webview

This would be fast to start but violates the required isolation: privileged
filesystem behavior would move toward the UI and be difficult to protect when
previewing untrusted content.

### Native platform UIs or Flutter

These can deliver desktop applications but either multiply platform UI work or
introduce a separate runtime and sidecar model without a decisive benefit for
this product.

## Evidence

Tauri documents target-specific external binaries and explicit sidecar
permissions in its [sidecar guide](https://v2.tauri.app/develop/sidecar/), and
capabilities for constraining frontend exposure in its
[security model](https://v2.tauri.app/security/capabilities/). Electron offers
an alternative multi-process IPC model, but its own documentation requires a
narrow context bridge and warns against exposing raw IPC:
[process model](https://www.electronjs.org/docs/latest/tutorial/process-model)
and [context isolation](https://www.electronjs.org/docs/latest/tutorial/context-isolation).
