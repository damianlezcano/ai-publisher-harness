## Runtime process lifecycle + sharing observability PASS (2026-09-03)

- **Cloudflare successful human run:** the product owner confirmed a clean manual
  share worked (`[tunnel] running` + public trycloudflare URL served the expected
  Creation) and that **Cloudflare sharing CAN work in the current AppImage**.
  Prior human testing had been **intermittent** with insufficient debug output to
  localize the boundary.
- **Process-leak human evidence:** after closing the EducAI window, owned
  processes could remain alive with identical PIDs — the EducAI AppImage process,
  bundled `opencode serve`, and bundled `cloudflared tunnel` — accumulating stale
  instances across runs. **Reproduced and fixed.**
- **Root cause (exact, evidenced):** the Tauri `educai` main process exits on
  normal window close, but the managed `Arc<AppState>` (owning `OpenCodeBackend`
  and `CloudflareQuickTunnel`) is **not dropped on that exit path**, so their
  `Drop`/`shutdown` never ran and the owned sidecars were reparented and leaked.
  Verified empirically: closing the window left `opencode serve` alive; the
  AppImage wrapper stayed because the FUSE mount stayed referenced. Component
  `Drop`/`shutdown` DOES terminate real children (probe: PASS), so the fix is the
  app-exit wiring, not the components.
- **Lifecycle correction (Part B):**
  - `AppState::shutdown()` — idempotent, bounded — stops the shared `opencode
    serve` backend (via agent engine), the local HTTP publisher, the shared
    `cloudflared` tunnel, and isolated preview servers.
  - `PublicationManager::shutdown()` — stops tunnel+publisher best-effort without
    mutating durable publication state or the published registry.
  - Tauri run loop now handles `RunEvent::ExitRequested` and `RunEvent::Exit` →
    `AppState::shutdown()` (previously relied on Drop that never ran).
  - `signal-hook` SIGTERM/SIGINT handler (async-signal-safe atomic + watcher
    thread) runs the same shutdown, so external termination (logout, task
    manager, `kill <pid>`) cannot orphan sidecars.
  - Explicitly-owned-only termination: only exact owned PIDs are signalled;
    no broad `pkill`. Cleanup is idempotent (double-fire on ExitRequested→Exit is
    safe; SIGTERM path is idempotent).
- **App-close semantics:** normal window close runs the deterministic shutdown
  (log: `app shutdown requested` → `backend stopped pid=…` → `tunnel stopped
  pid=…` → `app shutdown complete`), then the process exits. Unshare contract
  **preserved**: `Dejar de compartir` + confirm stops the tunnel only when no
  active publications remain; application exit stops it regardless.
- **OpenCode cleanup evidence (real):** start → backend serves → close → opencode
  child EXITED every cycle; SIGTERM also EXITED it.
- **Cloudflared cleanup evidence (real):** real share (real trycloudflare URL) →
  close → cloudflared EXITED; Dejar de compartir+Confirmar → `[tunnel]
  stopping/stopped` → cloudflared EXITED (manual and AppImage-verified).
- **Observability (Part A):** share pipeline is fully stage-identifiable with
  timings and pids (DEBUG level via `--debug`; failures are ERROR):
  `[share] requested conversation_id=… creation_id=…` →
  `[publish] prepared conversation_id=… route=… origin=http://127.0.0.1:PORT` →
  `[tunnel] starting origin=…` → `[tunnel] process pid=…` →
  `[tunnel] public_url=… elapsed_ms=…` → `[share] ready … elapsed_ms=…`; failures
  `[share] failed stage={local_publish_prepare|local_publish|tunnel_start|
  tunnel_stop|publisher_start|publisher_stop|unpublish}` and
  `[tunnel] failed stage={binary_resolve|spawn|process_exited|url_acquisition}`;
  backend logs spawned/ready/stopped pids + unexpected-exit status. Compatible
  with `2>&1 | tee /tmp/educai-share-debug.log`.
- **Log safety:** only ids, ports, routes, trycloudflare hostnames, pids,
  elapsed; never prompts, artifact contents, generated HTML/JS/CSS, credentials,
  Authorization headers, or full file contents. Binary paths trimmed to bare name
  (no-paths crate contract honored).
- **Instrumented real share cycles (fresh AppImage `fd483807…`, real display,
  real cloudflared 2026.8.3, real opencode 1.18.25):** several clean launch/share
  cycles; real public URLs served HTTP 200 (e.g. `https://festivals-geek-ethical-
  ste.trycloudflare.com/…`, `https://elvis-sizes-jail-closed.trycloudflare.com/
  …`). **Intermittent human symptom NOT reproduced during instrumented clean
  cycles**; the only observed anomaly was the previously documented external
  trycloudflare DNS-edge flakiness (curl 000 while tunnel+origin healthy) on some
  runs — no production change made (per DISCARD disposition). No speculative
  DNS/TCP readiness workaround introduced (verified absent).
- **Final real lifecycle cycles (after reviews/fixes, final AppImage):**
  (1) start/close without sharing → all EXITED, zero residue; (2) start/share/
  close → EducAI/opencode/cloudflared EXITED, zero residue; (3) start/share/
  unshare/close → cloudflared EXITED on unshare, all EXITED on close, zero
  residue; (4) start/share/close again → all EXITED, zero residue. SIGTERM close
  also EXITED all three with clean shutdown log. No process accumulation across
  launches (6+ AppImage cycles).
- **Independent reviews (fresh OpenCode Go sessions, Herdr `w1N`):**
  - **Product/UX = OpenCode/DeepSeek V4 Flash = APPROVE** (rationale: lifecycle
    fix real and correct; no user-visible regression; instrumentation properly
    leveled and safe; nits non-blocking: signal exit code, OS error string in
    console-only fail_detail, 100ms watcher poll, double shutdown idempotent).
  - **Code/Correctness = OpenCode/Qwen 3.8 Flash = REQUEST_CHANGES → fixes
    (`0bb0627`) → FRESH re-review = APPROVE.** Required change: hermetic
    share-stage ordering test (filter process-global session log by the test's
    unique conversation_id). Applied + nits: precise tunnel_stop/publisher_stop
    stages, `local_publish_prepare` failure stage (no dangling requested),
    BinaryNotFound path trimmed. Re-review confirmed all applied, no speculative
    readiness logic, no unsafe, suites green.
- **`./scripts/verify` EXIT=0 (final):** cargo fmt/clippy -D warnings clean, FE
  **244/244**, **Rust 1158 passed in 85 suites** (was 1148; +5 new + counts),
  fetch-sidecars --check, M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check
  src-tauri, git diff --check. Log: `/tmp/opencode/verify-lifecycle-final2.log`.
- **Commits/merge:** `829d517` (implementation), `0bb0627` (code-review fixes),
  integrated on `main` HEAD `0bb0627`, working tree clean, review panes closed.
  Fresh AppImage `fd483807…` (sha256 prefix) at canonical path.
- **Quoted prompts: HUMAN-PASS (untouched). GLIBC: pending (untouched). M11: NOT
  STARTED.**
- **Environment note:** the desktop display session had a persistent input grab
  (mutter guard window) blocking synthetic pointer/keyboard; UI interaction was
  driven via **AT-SPI accessibility actions** (real Compartir/Dejar de compartir/
  Confirmar presses) on real AppImage instances — valid, real UI events. WebKit
  paint under Xvfb was unavailable, so all in-app validation used the real
  composited display.
- **Disposition:** all gates pass — **TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE.
  NO HUMAN ACCEPTED.** Stop; do not start GLIBC.
