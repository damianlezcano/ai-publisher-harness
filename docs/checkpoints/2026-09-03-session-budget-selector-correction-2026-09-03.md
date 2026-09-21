## Session-budget selector correction — 2026-09-03

- The budget selector defect was proven: without an explicit/current identity,
  `scripts/check-session-budget` chose the first OpenCode session listed,
  `ses_f97dd4…`, whose export ended at `610877` tokens. A fresh Codex process
  had no OpenCode identity, so this was an unrelated historical session.
- The selector is corrected and documented: explicit `--session` and
  `OPENCODE_SESSION_ID` measure that exact OpenCode session; Codex and any
  identity/telemetry gap report `SESSION_BUDGET: UNKNOWN`; cross-provider
  fallback and latest-session selection are forbidden. Thresholds are
  unchanged. Bounded tests pass.
- Cloudflare regression remains pending. The interrupted `cloudflare.rs`
  candidate remains untouched, uncommitted, incomplete, and unproven.
- Quoted prompt remains `HUMAN-PASS`; GLIBC remains pending; M11 is NOT
  STARTED.
