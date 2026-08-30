# ADR-0007: Frontend framework and Tauri UI boundary

- Status: Accepted

## Context

M6 introduces the first desktop UI. ADR-0001 chose Tauri 2 as the shell with a
TypeScript web UI and deliberately left the UI framework open. The target user
is non-technical; the UI must feel like an assisted-creation tool, never an IDE
or terminal. The frontend must stay a thin, narrow client over Tauri commands;
domain logic remains in the Rust application core.

## Decision

Use **React 19 + Vite + TypeScript** for the frontend, and materialize the
Tauri 2 shell under `app/`:

```
app/
  src/                 # React frontend (TypeScript)
  src-tauri/           # Rust: thin Tauri commands + capabilities + state
```

Rationale: React is the most widely maintained and documented option, has the
largest component/testing ecosystem (React Testing Library, Vitest), excellent
Tauri compatibility (official `create-tauri-app` templates), mature TypeScript
typing, and the largest pool of future contributors. Vite provides a fast,
deterministic dev/build loop. This is the conservative choice, not a novelty.

The frontend holds only presentation state. The Rust application core
(`project-app`) remains the source of truth for projects, creations,
publication, and agent runtime. The frontend never accesses arbitrary
filesystem paths, never executes OpenCode/cloudflared, and reaches privileged
behavior only through allow-listed Tauri commands.

## Consequences

- A frontend toolchain (Node/pnpm or npm) is introduced; `scripts/verify` adds
  deterministic, offline frontend checks (format, lint, typecheck, unit/component
  tests) plus a Tauri config/build check. No Node tooling runs in M1-M5 Rust
  checks; it is gated to M6 and later.
- The Tauri command surface is a narrow, capability-scoped API (named by user
  capability, not implementation detail); there is no generic shell/filesystem/
  process command. Tauri capabilities deny unrestricted shell/fs/process access.
- Cross-platform frontend APIs are used (no hardcoded Unix paths); system-open
  goes through a backend abstraction. Windows packaging remains deferred.
- Component/design-system work is intentionally minimal; accessibility and a
  clean, education-appropriate visual baseline are prioritized over a custom
  design system.

## Alternatives considered

### Svelte 5 + Vite

Smaller runtime and less ceremony, and a first-class Tauri template. Rejected
for M6 because React's larger ecosystem and contributor pool reduce future risk
for a product with a long roadmap.

### Vue 3 + Vite

Mature and popular. Rejected on the same conservatism grounds: React is the
lower-risk default for maintainability and hiring.

### Solid

Excellent performance but a smaller ecosystem and contributor pool. Rejected as
less conservative for a long-lived product.

### Tauri without a web framework (vanilla TS)

Avoids a framework dependency but makes the workspace/chat/creations UI harder
to build, test, and maintain. Rejected.
