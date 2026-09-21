## WINDOWS OPENCODE ENVIRONMENT FIX PASS — missing SYSTEMROOT forwarded to the OpenCode child on Windows only; regression + native NSIS rebuild + silent install + real runtime proof = ALL PASS — READY TO RESUME WINDOWS HUMAN RUNTIME VALIDATION (2026-09-04)

- **Scope:** bounded Windows-only runtime fix + regression + rebuild + install +
  runtime proof. **Proven root cause (do NOT reopen):** EducAI's process
  supervisor `env_clear()`s and reconstructs a minimal child environment that
  omitted `SYSTEMROOT` on Windows; controlled differential experiments showed
  removing only SYSTEMROOT from an inherited env makes `opencode serve` exit
  immediately `0xC0000409` with no listener. Exact fix: on Windows only, forward
  the parent `SYSTEMROOT` value into the reconstructed OpenCode child env after
  clearing. No redesign of process supervision, no OpenCode version change, no
  sidecar packaging change, no M11, no Knowledge.
- **Author implementation commit:** `6337c02` `fix(windows): preserve SYSTEMROOT
  for OpenCode child` — `crates/project-opencode/src/lib.rs` only: `build_env`
  now extends the managed key set with `windows_systemroot_env()`; `#[cfg(windows)]`
  forwards `std::env::var("SYSTEMROOT")` (parent value, never a hardcoded
  `C:\Windows`), `#[cfg(not(windows))]` forwards nothing (Linux unchanged).
  Env isolation preserved: `ChildGuard::spawn` still `.env_clear()`s and applies
  exactly the managed keys (PATH/HOME/XDG_* + SYSTEMROOT on Windows); no broad
  environment inheritance; no secret leakage; lifecycle/ownership untouched.
  In-file regression test `build_env_preserves_systemroot_only_on_windows`
  asserts the exact managed key set, SYSTEMROOT forwarded verbatim from parent
  on Windows, and SYSTEMROOT absent on non-Windows targets.
- **Tests run on native Windows 11 x64/MSVC (cargo 1.97.1, pnpm 11.25.0,
  tauri-cli 2.11.4):** `cargo test -p project-opencode --all-targets` 5/5 PASS
  (incl. new regression); `-p project-agent --test agent_security` 6/6 PASS;
  `-p project-provider --test provider_security` 12/12 PASS; full workspace
  `cargo test --workspace --all-targets` green except **pre-existing Windows
  test-harness failures, proven NOT regressions** (verified identical at parent
  `bcc47a4` in a scratch worktree): `project-process` supervisor 3 failures all
  `assert!(process_exists(pid))` where the helper hardcodes Linux `/proc/{pid}`
  (always false on Windows), and 5 timing/HTTP failures in
  `project-agent --test opencode_adapter` that fail identically at the parent
  commit. `cargo fmt --all -- --check` PASS; `cargo clippy --workspace
  --all-targets -D warnings` blocked only by a **pre-existing Windows dead-code
  error** in `project-tunnel/tests/tunnel_security.rs` (`process_exists` is used
  only from a `#[cfg(unix)]` test — zero diff in this pass). `git diff --check`
  clean. `./scripts/verify` (bash + Linux packaging gates) intentionally not run
  on Windows per policy.
- **NSIS rebuild PASS:** `powershell -ExecutionPolicy Bypass -File
  .\packaging\windows\build.ps1` exit 0. Artifact
  `app/src-tauri/target/release/bundle/nsis/EducAI_0.1.0_x64-setup.exe`,
  **59,259,829 bytes**, **SHA-256
  `260119C3615490E602ED0EB6A2AE47B36400A8EBBC0774F0AC5ABC0A5E112740`**. Sidecars
  byte-identical to manifest pins (opencode 1.18.25, cloudflared 2026.8.3);
  build-only `packageManager` write-back in `app/package.json` reverted
  (working tree clean at `6337c02`).
- **Install / runtime proof (same Windows 11 machine):** silent NSIS
  install (`/S`) exit 0, installed `educai.exe` updated, bundled sidecars
  verified. Launched `educai.exe --debug` → session log: `[agent] backend
  starting` → `[agent] backend spawned pid=10976 port=50675` → `[agent] backend
  ready version=1.18.25 pid=10976`. Child `opencode.exe` pid 10976 alive,
  ParentProcessId = educai pid 13456 (ownership OK), listener
  `127.0.0.1:50675` LISTENING (OwningProcess 10976), `GET /global/health`
  → **HTTP 200** `{"healthy":true,"version":"1.18.25"}`. **No 0xC0000409** in the
  session log. WM_CLOSE on the EducAI window → `[EducAI][INFO] app shutdown
  requested` → `[agent] backend stopped pid=10976` → `app shutdown complete`;
  both educai and opencode exited, **zero residue**. No NEW later-stage blocker
  surfaced.
- **SECONDARY OBSERVABILITY GAP — DEFERRED:** in-app Log Viewer still exposes
  only startup information while debug stderr carries `[agent] backend
  starting/spawned/exited`; intentionally NOT fixed in this pass.
- **Quoted prompts / attachments / Creations / sharing / provider logic / UI /
  Linux packaging / sidecar pins / build.ps1 / Tauri bundle config / M11 /
  Knowledge: untouched.**
- **Status: WINDOWS OPENCODE ENVIRONMENT FIX PASS — READY TO RESUME WINDOWS
  HUMAN RUNTIME VALIDATION.** No human acceptance claimed.
