# ADR-0006: AgentEngine boundary and OpenCode serve HTTP adapter

- Status: Accepted

## Context

M5 integrates OpenCode as the product's agent engine so a user can describe a
resource and have it generated into the project's `workspace`/`outputs`, without
a UI, drag/drop, QR, or automatic publication. OpenCode must remain invisible to
the non-technical user and stay behind an abstract `AgentEngine` port so the
domain never depends on OpenCode. Publication remains an explicit, separate
action.

## Decision

Introduce a `project-agent` crate owning the `AgentEngine` port, agent models,
a per-project session registry, a `FakeAgentEngine`, and an
`OpenCodeAgentEngine` adapter. The adapter drives a single headless
`opencode serve` process over loopback HTTP, one shared backend for all
projects, with one session per project.

### Boundary and dependency direction

```
project-agent (AgentService)
  -> AgentEngine (port) -> OpenCodeAgentEngine (adapter) -> opencode serve (loopback HTTP)
  -> project-process (generic child supervision)
  -> project-fs / project-core (project path + Creation registration)
```

`project-core` gains no OpenCode/tunnel/agent dependency. `project-agent` is
the only crate that knows OpenCode, and only inside the adapter. A generic
`project-process` crate is extracted from M4's `project-tunnel` so the `ChildGuard`
supervision pattern is reused without duplicating subprocess infrastructure;
`project-tunnel` then depends on `project-process` too.

The `project-process` extraction is a **PRE-M5 infrastructure refactor** and is
functionally equivalent: `project-tunnel` must keep the exact same external
behavior. `project-process` holds only generic subprocess infrastructure
(spawn, explicit argv, stdout/stderr capture, generic readiness plumbing, wait,
`request_stop`/`force_kill`, cleanup, exit observation) and no Cloudflare,
OpenCode, tunnel-URL, agent, project, or publication semantics. If the
extraction changes any observable M4 behavior, implementation stops until it is
resolved within this contract.

### Process supervision and binding

`opencode serve` is spawned with explicit argv and a cleared environment plus
the isolated XDG paths. It binds `127.0.0.1` only (`--hostname 127.0.0.1`,
never `--mdns`, which would bind `0.0.0.0`) on an ephemeral or chosen port.
Readiness is detected via `GET /global/health` returning `{ healthy, version }`;
the returned version is checked against a supported range (fail clearly if
incompatible). The child is stopped with the portable `request_stop`/`wait`/
`force_kill` contract inherited from M4.

### Config and session isolation

The adapter sets `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and
`XDG_STATE_HOME` to an app-managed isolated directory (plus `--pure` to skip
external plugins), so the developer's `~/.config/opencode` (permissions,
plugins, credentials) never affects product behavior. One OpenCode backend is
started lazily on first agent request and reused; one session is created per
project with its project directory as the working directory.

### Filesystem sandbox

The working directory is the project root, and the isolated config denies
access outside the project (`external_directory` deny) and denies writes to
`inputs/` and `publish/`. The backend runs with `--auto` so routine in-project
operations are auto-approved while explicitly-denied paths stay denied — the
non-technical user is never prompted for technical permissions. Exact deny
patterns are pinned against the installed config schema during implementation.

### Output registration

After a task completes, the adapter reads OpenCode's structured session diff
(`GET /session/:id/diff`) as filesystem evidence of what was created, rather
than parsing the LLM's final text. New artifacts under `outputs/` are reported
as structured `Artifact` descriptors (path, kind, name, size); the `AgentService`
registers them as `Creation` records with **private** visibility by default.
Visibility is never inferred from filenames or content; public intent is an
explicit later-layer decision.

## Consequences

- The domain and the publisher/tunnel remain independent of OpenCode.
- A future different agent engine fits behind `AgentEngine` without core change.
- Deterministic `FakeAgentEngine` + a fake `opencode serve` (HTTP) cover the
  adapter and lifecycle offline in `scripts/verify`; a real OpenCode round trip
  is a manual smoke test only.
- The `project-process` extraction is a mechanical refactor that must keep M4
  (and M1-M3) tests green.
- Provider/model/credential UX, skills, updater, and Windows sidecars are
  deferred to later milestones.

## Alternatives considered

### Embed OpenCode as a library or call its SDK directly from the core

Rejected: couples the domain to OpenCode internals and to a JS/TS SDK, violating
the dependency direction and the offline-testability of the core.

### One OpenCode process per project

Rejected: unnecessary process and resource overhead; one backend with per-project
sessions and working directories isolates correctly.

### Parse the final assistant text to detect created files

Rejected: fragile; the structured session diff and `outputs/` scan provide
filesystem evidence without trusting model prose.

### Reuse the developer's OpenCode config directly

Rejected: leaks developer permissions/plugins/credentials into product behavior;
the isolated XDG config keeps the product deterministic.
