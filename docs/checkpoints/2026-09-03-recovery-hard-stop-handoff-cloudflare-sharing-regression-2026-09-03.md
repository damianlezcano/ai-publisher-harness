## Recovery hard-stop handoff — Cloudflare sharing regression (2026-09-03)

- **SESSION MUST STOP NOW.** Bootstrap budget check returned
  `ROTATE_SESSION_REQUIRED_HARD (610877 tokens, >=130K)`. Per the active
  recovery instruction, no reproduction, code inspection, implementation,
  validation, or review was performed after that result.
- **Durable recovery:** HEAD is `99aa62f` (`docs(checkpoint): user prompt
  quoting/serialization pass ...`). The pre-existing interrupted-session
  modification remains in `crates/project-tunnel/src/cloudflare.rs`
  (`+54/-1`, uncommitted); the budget-tool correction is separate.
- **Interrupted-session classification: D — incomplete/unsafe.** The diff
  adds a production DNS+TCP readiness probe and makes the Quick Tunnel wait
  for it before setting `TunnelState::Running`. It is an unproven candidate:
  no three-layer reproduction matrix was collected, no failing boundary was
  localized, no last-known-good comparison was performed, and no targeted or
  runtime validation was run. **Do not discard, reset, or treat it as a fix**;
  retain it for a fresh, budget-compliant recovery session to inspect.
- **Three-layer matrix:** not run (mandatory hard stop at bootstrap).
  LOCAL: not tested; TUNNEL: not tested; PUBLIC: not tested; UI Compartido:
  not tested. Exact failing layer and root cause: unproven.
- **Quoted prompt = HUMAN-PASS and untouched.** **Linux AppImage GLIBC
  portability remains pending and untouched. M11 NOT STARTED.**
- **Next session:** start fresh; first recover this single diff without
  overwriting it, then execute exactly the requested local/tunnel/public
  three-layer probe before inspecting or changing the publication boundary.

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.
