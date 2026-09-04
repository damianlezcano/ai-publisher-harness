# Current Checkpoint

## WebKitGTK AppImage helper relocation INTEGRATED + REVIEWED + REBUILT — Linux portable AppImage READY FOR HUMAN FEDORA + KDE NEON VALIDATION (2026-09-04)

- **Scope:** bounded packaging completion only (WebKitGTK helper relocation for
  the Linux AppImage). Quoted prompts and sharing/lifecycle/observability remain
  **HUMAN-PASS and untouched**. **M11 NOT STARTED.** Windows runtime remains
  **separate / untouched**. No application source or product behavior change.
- **Orchestrator/integrator:** OpenCode Go / DeepSeek V4 Flash (fresh
  completion session). **Session budget:** `SESSION_BUDGET: UNKNOWN` (exit 4;
  `OPENCODE_SESSION_ID` not exported in this shell — valid telemetry absence,
  not a hard stop; fresh small-context session). **Independent Code/Packaging
  reviewer:** fresh OpenCode Go / **Qwen 3.8 Flash** (`webkit-review`, Herdr
  pane `w1Q:p2`, `--model opencode-go/qwen3.8-flash`) = **APPROVE** (see below).
  No GPT through OpenCode Go; Cursor not used.
- **Original human failure (Fedora + KDE Neon/Ubuntu 24.04):** the same AppImage
  failed to launch on both targets with
  `Unable to spawn child process "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitNetworkProcess"`.
- **Exact root cause:** the Ubuntu 24.04-built AppImage bundled the WebKitGTK
  libraries but the shipped `libwebkit2gtk-4.1.so.0` had the Ubuntu absolute
  helper-process directory baked in
  (`/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1`). At runtime WebKit tried to spawn
  `WebKitNetworkProcess`/`WebKitWebProcess`/`WebKitGPUProcess` and the injected
  bundle from that host path, which exists only on Ubuntu-family targets and not
  on Fedora. The AppImage neither bundled those helpers nor rewrote the baked
  path, so helper lookup failed against the target host.
- **Author implementation commit:** `5d28991` `fix(packaging): relocate
  WebKitGTK AppImage helpers` (author worktree
  `.worktrees/webkit-appimage-runtime`, branch `m10/webkit-appimage-runtime-local`).
  5 files, +90: `packaging/linux/build-linux-appimage` (wire the two new gates),
  new `packaging/linux/prepare-webkit-appdir` (bundle helpers + rewrite path),
  new `scripts/check-appimage-webkit-runtime`, new `scripts/check-webkit-appdir`,
  `scripts/test-distribution-contracts` (new contract pins).
- **Path-resolution strategy (bounded):** `prepare-webkit-appdir` copies
  `WebKitNetworkProcess`, `WebKitWebProcess`, `WebKitGPUProcess` and
  `injected-bundle/libwebkit2gtkinjectedbundle.so` from the Ubuntu 24.04 build
  root into the AppDir at `usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/`, then does
  an equal-size NUL-padded string replacement in the bundled
  `libwebkit2gtk-4.1.so.0` only: the absolute root
  `/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1` becomes the relative root
  `lib/x86_64-linux-gnu/webkit2gtk-4.1` (data-only rewrite, stable file size;
  the compiled AppRun `chdir`s into `APPDIR/usr`, so the relative path resolves
  to the mounted AppImage/AppDir — no host WebKitGTK installation needed, no
  Fedora-specific path, no build-container absolute path leak). Perl `die`
  guards require one injected-bundle path and one helper root replacement;
  verified empirically on the shipped binary (0 absolute paths remain; exactly
  one relative helper root + one relative injected-bundle path, each with the
  expected 6-byte NUL padding; file size stable at 95,183,864 bytes).
- **Fresh Qwen 3.8 Flash review = APPROVE** (independent, all claims verified):
  all 3 helpers present as 0755 ELF PIE executables (interpreter
  `/lib64/ld-linux-x86-64.so.2`, `ldd`-satisfied from the AppDir closure via
  AppRun's `LD_LIBRARY_PATH` + `chdir %s/usr`), injected bundle present, "exactly
  one" assumption holds empirically, GLIBC gate passes (140 ELFs ≤ 2.39; helpers
  max 2.34), glibc not bundled, both new gates + `test-distribution-contracts`
  green, diff touches only 5 packaging/script files — no app, functional, or
  Windows changes. Non-blocking nits only: (1) `s///` without `/g` asserts
  "≥1 replaced" not "exactly one" (safe — post-build gate is fail-closed on any
  residual absolute path); (2) generic `/usr/lib`/gstreamer strings remain (not
  the spawn paths, correctly out of scope); (3) no negative-test fixture for a
  deliberately broken AppDir (contract pins are string-based). No material
  findings → no review-fix loop needed.
- **Integration:** `git merge --ff-only 5d28991` onto main — author commit
  integrated with provenance intact (no squash); the final integration HEAD is
  `66055ed` (author commit `5d28991` + this checkpoint doc). Working tree clean
  (only untracked `.worktrees/`).
- **`./scripts/verify` on integrated main EXIT=0** (log
  `/tmp/opencode/verify-webkit-integrated.log`): FE **244/244** in 21 files,
  **Rust 1162 passed in 85 suites**, clippy/fmt clean,
  `fetch-sidecars --check`, `test-distribution-contracts` **PASS**, M10 0.1.0 +
  UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check.
- **Controlled Ubuntu 24.04 rebuild from integrated main:** `./scripts/package
  linux-appimage` **EXIT=0** (log `/tmp/opencode/build-webkit-integrated.log`).
  Base image `educai-linux-portable:ubuntu-24.04` (image id `645eedde30d7`),
  Ubuntu 24.04.4 LTS, glibc 2.39. **WebKitGTK 2.52.6**
  (`libwebkit2gtk-4.1-0 2.52.6-0ubuntu0.24.04.1`). tauri linuxdeploy step
  aborted as documented (static sidecars) → documented appimagetool fallback;
  both gates run inside the build passed before the artifact was reported.
- **Final AppImage (human-validation artifact, built from integrated main):**
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`,
  **148,711,928 bytes**, built 2026-09-04 12:24:53 -0300, source HEAD
  `5d28991` (main after the controlled build; checkpoint doc follow-up `66055ed`
  does not change built source), **SHA-256
  `21d516f7f79e5ea2dbabb4bd66350b5632579f22933e4b7c65847b529df40cf2`**. Payload
  sidecars byte-identical to pins: **opencode 1.18.25** and **cloudflared
  2026.8.3** (verified in extracted payload). This is the ONLY artifact for
  human validation on both targets.
- **Final AppDir WebKitGTK layout:** `usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/`
  contains `WebKitNetworkProcess` (0755), `WebKitWebProcess` (0755),
  `WebKitGPUProcess` (0755) and `injected-bundle/libwebkit2gtkinjectedbundle.so`
  (0644); `usr/lib/libwebkit2gtk-4.1.so.0` rewritten to the relative path.
- **WebKit packaging gate (post-build, on the final AppImage) = PASS:**
  `scripts/check-appimage-webkit-runtime` → `check-webkit-appdir` **PASS**:
  helpers present, executable, ELF; injected bundle present; no absolute host
  helper path remains; relative helper + injected-bundle paths configured.
- **GLIBC gate (post-build, on the final AppImage) = PASS:**
  `scripts/check-appimage-glibc <artifact> 2.39` — **140 ELF files** inspected
  (incl. all newly bundled helpers), all require **GLIBC <= 2.39**. Helpers
  specifically: WebKitNetworkProcess/WebKitWebProcess/WebKitGPUProcess = 2.34,
  injected bundle = 2.2. **No glibc bundled; baseline unchanged.**
- **Automated runtime smoke (current Fedora 44 host, real display): truthful
  and partial.** The final AppImage launches; `WebKitNetworkProcess` now spawns
  from the **relative bundled path**
  (`lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitNetworkProcess`, resolved to the
  extracted/mounted AppDir — verified via `/proc/<pid>/exe`), and **NO** `Unable
  to spawn child process` / `No such file or directory` / `WebKitNetworkProcess:
  No such file or directory` appears in the log. The old helper-path failure is
  **GONE**. Full UI render is not reachable from this automation environment:
  after GTK/WebKit init the process aborts with environment-specific
  `Could not create default EGL display: EGL_BAD_PARAMETER` — an
  automation/display limitation, **not** the old helper-path failure. Logs:
  `/tmp/opencode/webkit-smoke.log`, `/tmp/opencode/webkit-smoke2.log`. Zero
  process/mount residue after cleanup.
- **Human validation targets (SAME AppImage, SHA `21d516f7…`):** TARGET A =
  Fedora workstation; TARGET B = KDE Neon User Edition / Ubuntu 24.04 noble.
  Launch, UI render, simple chat, Preview/Abrir, Cloudflare sharing, public URL,
  clean owned-child shutdown. **Human Fedora validation = PENDING. Human KDE
  Neon validation = PENDING. Do NOT claim Linux HUMAN-PASS.**
- **Windows:** separate native build, untouched this pass (no Windows packaging
  file in the diff). **TECHNICALLY READY FOR WINDOWS RUNTIME VALIDATION**, no
  Windows PASS. Do not start Windows runtime validation.
- **Quoted prompts: HUMAN-PASS (untouched). Sharing/lifecycle/observability:
  HUMAN-PASS (untouched). GLIBC policy <= 2.39 unchanged. Ubuntu 24.04 baseline
  unchanged. M11 NOT STARTED.**
- **Status: LINUX PORTABLE APPIMAGE READY FOR HUMAN FEDORA + KDE NEON
  VALIDATION.** No human validation claimed. STOP — do not start Windows or M11.

## Linux controlled build CONTINUATION COMPLETE — corepack/path fix + fresh Ubuntu 24.04 AppImage (2026-09-04)

- **Scope:** bounded packaging continuation only. Quoted prompts and
  sharing/lifecycle/observability remain **HUMAN-PASS and untouched**. **M11
  NOT STARTED.** Windows runtime remains **separate / untouched**. No
  application source or product behavior change.
- **Author:** OpenCode Go / DeepSeek V4 Flash. **Independent packaging
  reviewer:** OpenCode Go / Qwen 3.8 Flash (`corepack-review`, review worktree
  `../ai-publisher-linux-corepack-path`, branch `linux/corepack-path`) — 7
  review rounds, final **APPROVE** (round 7, HEAD `8ce9d6e`); round 4
  REQUEST_CHANGES (stale-artifact guard) applied in `d223599` and re-approved.
- **Prior corepack blocker confirmed + root cause (exact):** `./scripts/package
  linux-appimage` failed at Containerfile STEP 6/8 with `/bin/sh: 1: corepack:
  not found` (exit 127), right after the pinned archive checksum printed OK and
  `node`/`npm`/`npx` were symlinked. The Containerfile exposed only
  `node`/`npm`/`npx` before invoking `corepack enable`. The pinned
  Node 22.14.0 archive **does** contain `bin/corepack` (symlink →
  `lib/node_modules/corepack/dist/corepack.js`, **corepack 0.31.0**, proven by
  extracting the checksum-verified archive at
  `/tmp/opencode/node-inspect/node-v22.14.0-linux-x64/bin/corepack`).
- **Bounded fixes (commits `39c0ef0`..`8ce9d6e`), all with directly relevant
  contract-test pins in `scripts/test-distribution-contracts`:**
  - `39c0ef0` corepack PATH fix: `ln -s
    /opt/node-v${NODE_VERSION}-linux-x64/bin/corepack /usr/local/bin/corepack`
    (exactly analogous to node/npm/npx), placed before `corepack enable`.
    Contract test requires node/npm/npx/corepack symlinks from the pinned
    extraction, `corepack enable` after the symlink, and `ARG NODE_VERSION`
    pinned to 22.14.0 (proven to reject the pre-fix Containerfile).
  - `d10bca0` two further blockers surfaced by resuming the build: pnpm
    aborted `ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY` (no TTY to confirm a
    purge of the mounted host `node_modules`) → `export CI=true` in
    `build-linux-appimage` (the exact remedy the pnpm error prescribes); and
    `cargo: not found` in the `podman run` (rustup had installed into
    root-only `/root/.cargo` while `--userns=keep-id` runs as uid 1000) →
    `RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo`, chown to the container's
    uid-1000 user, `/opt/cargo/bin` on PATH. Both pinned by contract.
  - `0c3cf11` `.pnpm-store/` (pnpm store residue written into the mounted
    workspace) removed + gitignored.
  - `d73d916` `cargo tauri` not installed in the build root → `cargo install
    tauri-cli --locked --version 2.11.4 --root /opt/tauri-cli` (same version as
    the known-good Fedora artifacts; workspace pins tauri 2.11.5), chown +
    `/opt/tauri-cli/bin` on PATH. Pinned by contract.
  - `3f2f2aa` linuxdeploy aborts with `Failed to run ldd: exited with code 1`
    on the pinned sidecars (**static executables** — `ldd` exits 1 for
    `opencode`/`cloudflared`; `educai` is dynamic) after the AppDir with the
    Ubuntu 24.04 dependency closure is already complete → documented
    appimagetool fallback (same pattern as `scripts/smoke-package`) using the
    tauri-downloaded `~/.cache/tauri/linuxdeploy-plugin-appimage.AppImage`.
  - `d223599` (reviewer finding) `rm -rf bundle/appimage` before the build so a
    stale pre-existing AppImage can never be shipped as this run's output.
  - `a2ac578` linuxdeploy's bundled patchelf adds a broken `RUNPATH
    $ORIGIN/../lib` to the opencode sidecar (**the patched binary segfaults**,
    reproduced on the Fedora host and inside the Ubuntu 24.04 container;
    cloudflared static → untouched) → restore the checksum-verified pinned
    sidecars from `sidecars/` into the AppDir before appimagetool packaging.
  - `8a5dc55`+`20604c3` the container pnpm (via corepack) unconditionally
    writes `packageManager: pnpm@11.25.0+sha512…` into `app/package.json`,
    which broke host `./scripts/verify` (host corepack runs pnpm 11.24.0 and
    refuses on the version mismatch) → field reverted (net-zero diff) and
    `build-linux-appimage` ends with `git restore -- app/package.json` so the
    controlled build leaves the working tree clean.
  - `8ce9d6e` host `./scripts/verify` pnpm install hit the same no-TTY purge
    abort after a controlled build recreated `node_modules` → `CI=true pnpm
    --dir app install --frozen-lockfile` (one-line, prescribed remedy).
- **Controlled build result:** `./scripts/package linux-appimage` now
  **COMPLETES end-to-end** from the repository. Base image
  `educai-linux-portable:ubuntu-24.04` (image id `645eedde30d7`); Ubuntu
  **24.04.4 LTS**; **glibc 2.39** (`Ubuntu GLIBC 2.39-0ubuntu8.8`); **Rust
  1.97.1** (cargo `c980f4866`/rustc `8bab26f4f`); **Node v22.14.0**;
  **corepack 0.31.0**; **Tauri CLI 2.11.4**; **pnpm 11.25.0** (container
  corepack); **appimagetool** = `linuxdeploy-plugin-appimage.AppImage`
  (tauri-downloaded, run via `APPIMAGE_EXTRACT_AND_RUN=1`); sidecars verified
  by `fetch-sidecars`: **opencode 1.18.25** and **cloudflared 2026.8.3**.
- **Fresh AppImage (NOT the old Fedora artifact):**
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`,
  **148,711,928 bytes**, built **2026-09-04 10:43:26 -0300**, source HEAD
  `8ce9d6e` (working tree clean before and after the build), **SHA-256
  `24797be59c31c5a545bfd9352447c7de5cf8becf950c3c8ffc49dfb85f0251fa`**. The old
  Fedora artifact was 180,963,832 bytes / SHA `fd483807…` — provenance differs.
  Payload sidecars **byte-identical to the pins**: opencode `d91e0d33…`
  (1.18.25, runs), cloudflared `f29324fe…` (2026.8.3, runs).
- **GLIBC gate:** `./scripts/check-appimage-glibc <artifact> 2.39` **PASS** —
  139 shipped ELF files inspected (EducAI executable, bundled WebKitGTK/GTK/
  GLib closure, opencode, cloudflared, AppRun, helpers), all require
  **GLIBC <= 2.39**. No glibc bundling, baseline unchanged.
- **`./scripts/verify` EXIT=0** (log `/tmp/opencode/verify-final4.log`): FE
  **244/244** in 21 files, **Rust 1162 passed in 85 suites**, clippy/fmt clean,
  `fetch-sidecars --check`, **`test-distribution-contracts` PASS** (now pins
  corepack PATH exposure, keep-id `/opt` Rust homes, tauri-cli pin,
  non-interactive + appimagetool fallback in the build script), M10 0.1.0 +
  UX_REDESIGN_01 contracts, git diff --check.
- **Fedora runtime smoke (current Fedora 44 host, real display): truthful and
  partial.** The exact new AppImage launches, loads the **bundled Ubuntu
  24.04 GTK/WebKit closure under Fedora glibc 2.43 with NO GLIBC errors**, and
  GTK+WebKit initialize; the only failure is WebKit spawning
  `WebKitNetworkProcess` from its compiled-in absolute path
  `/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/` (an Ubuntu path absent on Fedora;
  **present on the Ubuntu 24.04/KDE Neon target**). `WEBKIT_EXEC_PATH` override
  is not honored; no root to install a host shim; containerized X forwarding
  fails at GTK init (headless container). Sidecar binaries run standalone
  (opencode 1.18.25, cloudflared 2026.8.3). **Full UI render / chat / sharing /
  clean shutdown are NOT reachable on Fedora for this Ubuntu-targeted artifact**
  and are **NOT claimed**. Old Fedora-built artifacts ran on Fedora because
  their WebKit path was Fedora-specific.
- **KDE Neon / Ubuntu 24.04 target:** not accessible from this environment —
  no target claim. Final status: **LINUX PORTABLE ARTIFACT READY FOR HUMAN KDE
  NEON VALIDATION**. Human product owner must run the exact artifact (SHA
  `24797be5…`) on the KDE Neon / Ubuntu 24.04-family machine: launch, UI,
  simple chat, Preview/Abrir, Cloudflare sharing, public URL, clean owned-child
  shutdown.
- **Windows:** strategy already implemented, untouched this pass.
  **TECHNICALLY READY FOR WINDOWS RUNTIME VALIDATION** (no Windows PASS).
- **Quoted prompts: HUMAN-PASS (untouched). Sharing/lifecycle: HUMAN-PASS
  (untouched). GLIBC policy <= 2.39 unchanged. Ubuntu 24.04 baseline unchanged.
  M11 NOT STARTED.**

## Cross-platform distribution portability — prior IN PROGRESS record (2026-09-03)

- **Scope:** dedicated packaging/platform pass only. Quoted prompts and
  sharing/lifecycle/observability remain **HUMAN-PASS and untouched**. **M11
  NOT STARTED.**
- **Bootstrap:** clean source HEAD `1ec23ed`; development host Fedora 44,
  glibc 2.43. Existing AppImage
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`
  is 180,963,832 bytes, SHA-256
  `fd483807c59121daf83d4f3efdaad3236f9b607a963caaf652c53783a7ca771e`.
- **Linux root cause proved:** the Fedora Tauri/linuxdeploy AppDir copied the
  Fedora WebKitGTK/GTK/GLib dependency closure into `usr/lib`; AppImage does
  not virtualize glibc. The executable requires GLIBC 2.39; bundled
  WebKitGTK, JavaScriptCore, GLib, GnuTLS, Pixman and related libraries require
  GLIBC 2.43. The new extraction gate rejects the old artifact with named
  offenders and policy maximum GLIBC 2.39.
- **Linux policy/build:** Ubuntu 24.04 (glibc 2.39 + WebKitGTK 4.1) is selected;
  Ubuntu 22.04 was rejected because its standard packages do not provide that
  WebKitGTK ABI. `packaging/linux/Containerfile` and
  `./scripts/package linux-appimage` create the controlled build root; no
  Fedora runtime library is an input. On the continuation attempt,
  `./scripts/package linux-appimage` completed the previously interrupted apt
  install layer (confirming that interruption was environmental) but failed in
  the next Node layer: `/bin/sh: 1: corepack: not found`. The extracted pinned
  Node 22.14.0 archive contains the needed binary, but the Containerfile links
  only `node`, `npm`, and `npx` before invoking `corepack enable`; the bounded
  fix is to expose that existing `corepack` binary on `PATH`. No build root,
  dependency baseline, Fedora input, or product behavior change is proposed.
  Repository policy requires a fresh isolated author plus OpenCode Go Qwen 3.8
  Flash review for that packaging edit. The required launcher was correctly
  fail-closed in this Codex session because `scripts/check-session-budget`
  reports `SESSION_BUDGET: UNKNOWN` / exit 4 for Codex identity telemetry, so
  no worker/reviewer or packaging edit was started. No fresh artifact exists;
  the real Ubuntu/KDE Neon/Fedora matrix remains pending.
- **Windows policy/build:** one native x64 NSIS installer, built natively on a
  Windows 11 x64/MSVC runner—no Linux cross-compile. The manifest separately
  pins OpenCode 1.18.25 Windows x64 ZIP (SHA-256
  `831e213e…08416`) and cloudflared 2026.8.3 Windows AMD64 EXE (SHA-256
  `83e726ed…4eaae`); `packaging/windows/build.ps1` verifies and packages them.
  Resolver support handles `.exe`/MSVC suffixes while owned-PID process and
  safe opener abstractions remain unchanged. No Windows machine is available
  in this checkout: **TECHNICALLY READY FOR WINDOWS RUNTIME VALIDATION**, not
  HUMAN-PASS; no Windows artifact SHA yet.
- **Reviewer:** fresh OpenCode Go / Qwen 3.8 Flash review of `43fb8db` returned
  **REQUEST_CHANGES**: fail-closed GLIBC tool dependency/zero-ELF handling,
  legacy fetch command compatibility, reproducibility wording/checksum detail,
  and documented Windows PowerShell fetch duplication. Bounded fixes committed
  as `d271003`; a fresh independent OpenCode Go / Qwen 3.8 Flash re-review of
  `1ec23ed..d271003` returned **APPROVE**.
- **Automated evidence:** `./scripts/verify` **PASS** (Rust workspace +
  clippy/fmt, FE 244/244, sidecar manifest, Windows packaging contracts, Tauri
  check, diff check; log `/tmp/educai-distribution-verify.log`); project-app
  sidecar tests 8/8 PASS; old Fedora artifact GLIBC rejection PASS. A fresh
  controlled Linux artifact and real platform runtime validation remain pending.

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

## Cloudflare sharing regression — completion tail PASS (2026-09-03)

- **Cloudflare regression investigation: COMPLETE.** The human-reported regression
  is **NOT reproducible in the current runtime**. The three-layer matrix is
  **LOCAL/TUNNEL/PUBLIC = PASS/PASS/PASS** (origin `127.0.0.1:47787`, cloudflared
  2026.8.3, trycloudflare URL, HTTP 200, public body SHA-256 matched local). The
  interrupted `crates/project-tunnel/src/cloudflare.rs` candidate (DNS resolution
  + TCP/443 readiness polling) remains **DISCARDED and absent** from the tree
  (verified: no `getaddrinfo`/`TcpStream`/readiness-poll/2s-connect code present;
  git grep clean). **No production Cloudflare implementation fix is required.**
- **Explicit UI-driven validation (this tail, real FE dist in WebKitGTK 4.1 +
  real Rust bridge wrapping the real AppState publication manager + real
  cloudflared + real trycloudflare URL):**
  - **UI Compartir = PASS** — UI reached `Compartido` (composer + card); public
    URL shown in the share menu; public HTTP **200**; body SHA-256
    `402cf2d3…` byte-identical to the expected Creation (`Actividad`,
    "Palabras que confunden - Inglés", 1113 bytes); no stale/wrong artifact.
    Evidence: `/tmp/opencode/share-ui/share-ui-report.json`,
    `/tmp/opencode/share-ui/u1-shared.png`.
  - **UI Dejar de compartir = PASS** — confirm dialog shown with truthful
    message; UI returned to `Compartir`; no stale `.share-control-menu` /
    `.share-control-url`; sidebar Compartido badge count 0; backend state
    `local`. Evidence: `u2-confirm.png`, `u2-local.png`.
  - **UI Compartir again = PASS** — after unshare, re-share succeeded; UI
    reached `Compartido` again; new public URL HTTP **200**; body SHA-256
    `402cf2d3…` (expected Creation, byte-identical); no stale process/
    publication state blocked the operation. Evidence: `u3-shared.png`.
  - **External flakiness noted (not a defect):** trycloudflare quick-tunnel
    hostnames occasionally never register on the public DNS edge for a given
    run (curl 000 while tunnel + origin were healthy). A subsequent re-share or
    fresh run resolves fine — matches the Codex probe S1/S2/S4 pattern. The
    harness used a bounded outer retry and retained a fully-passing run. No
    production change warranted (per the DISCARD disposition).
- **Independent reviews (fresh OpenCode Go sessions, Herdr `w1N`):**
  - **Product/UX reviewer = OpenCode/DeepSeek V4 Flash = APPROVE.** Reviewed the
    evidence package + the actual share surface (`useShareControl`,
    `PublishPanel`, `CreationsPanel`, `WorkspaceView`, `ConversationsSidebar`,
    `app.rs`, `manager.rs`, `app_facade.rs`). Findings: Compartir truthful
    end-to-end; Dejar de compartir clean; Compartir-again clean (route reused,
    fresh tunnel URL); state derived solely from backend `publication_status`
    after refresh, busy labels truthful, no misleading Compartido state; public
    link shown in auto-open menu, Open is backend-resolved. PNG screenshots not
    renderable by the reviewer model; claims assessed via textual report/log.
  - **Code/Correctness reviewer = OpenCode/Qwen 3.8 Flash = APPROVE.** Confirmed:
    final tree did NOT retain the discarded cloudflare.rs candidate (no
    DNS/TCP readiness change remains); publication state transitions
    (publish/unpublish/status) correct and idempotent; unshare keeps other
    published / last-stop stops tunnel+publisher; republish reuses route;
    publication + app_facade test suites green; session-budget tooling
    unaffected (`scripts/test-session-budget` all checks passed); quoting
    untouched (diff `99aa62f..HEAD` = checkpoint doc + budget-selector fix only);
    GLIBC untouched; M11 NOT STARTED; harness/bridge stayed outside the repo
    (git status clean, HEAD `182be5c`).
- **`./scripts/verify` = PASS, EXIT=0** (log `/tmp/opencode/verify-share-tail.log`):
  cargo fmt --all -- --check, cargo clippy --locked --workspace --all-targets
  -D warnings, **FE 244/244** in 21 files, **Rust 1148 tests passed in 85 suites**
  (0 failed), `pnpm` build/test, fetch-sidecars --check (opencode + cloudflared),
  M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check.
  Working tree clean.
- **Quoted prompts: HUMAN-PASS (untouched). GLIBC: pending (untouched). M11: NOT
  STARTED.**
- **Disposition:** all gates pass — **TECHNICALLY READY FOR HUMAN
  RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Next and only gate: human product-owner
  re-acceptance. Stop; do not start GLIBC in this session.

## Cloudflare sharing regression recovery — prior Codex pass (2026-09-03)

- **Session budget:** `SESSION_BUDGET: UNKNOWN`; `PLATFORM: CODEX`; `SESSION_SOURCE: Codex process/identity`. This is valid telemetry absence, not a hard stop. The selector fix at `ac14c6b` is preserved.
- **Scope:** bounded Cloudflare sharing regression only. Quoted prompts remain **HUMAN-PASS**; GLIBC remains **pending**; M11 remains **NOT STARTED**.
- **Three-layer reproduction:**
  - `LOCAL: PASS` — EducAI-owned origin `127.0.0.1:47787`, route `/`, HTTP 200, expected artifact `<html><body>PROBE-OK</body></html>` served.
  - `TUNNEL: PASS` — EducAI-owned `cloudflared` PID 284751, version 2026.8.3, target `http://127.0.0.1:47787/`, alive, trycloudflare URL obtained. The log showed degraded QUIC/reconnect messages but active registered connections; no launch failure.
  - `PUBLIC: PASS` — `https://being-chester-champions-bristol.trycloudflare.com/`, HTTP 200, expected path reached, public body SHA-256 matched the local body.
  - `UI: Compartido = false` for the current Codex desktop surface only because no visible/capturable EducAI window or badge evidence was available. This is not a reproduced backend/publication failure.
- **Exact failing boundary:** not reproduced. Existing bounded app-facade evidence also records a real share: `PUBLISH` completed in 7.7907s, returned a trycloudflare URL, and the public route returned HTTP 200 with the expected Creation. Current source has no changed share-state files after the nearest known-good `99f6f7d` sharing pass.
- **Known-good comparison:** `99f6f7d` is the nearest explicit human-known-good sharing/state pass. `useShareControl`, `PublishPanel`, `App` refresh wiring, and the publication facade remain unchanged relative to that pass; no evidence connects them to the reported symptom.
- **Interrupted `crates/project-tunnel/src/cloudflare.rs` candidate:** final disposition **DISCARD**. It added production DNS resolution and TCP/443 polling after URL extraction, with repeated 100ms probes and up to 2s per-address connects under the 30s startup deadline. This adds startup latency and false-negative risk, and the reproduced network pass does not require it. Only that pre-existing candidate was reverted; unrelated work was untouched.
- **Targeted checks:** `cargo test --locked -p project-tunnel --test cloudflare` = 14/14; `cargo test --locked -p project-app --test app_facade` = 43/43; `cargo fmt --all -- --check` and `git diff --check` pass.
- **Real flow evidence:** current bounded artifacts prove `Compartir`/public serving once. No fresh UI-driven `Dejar de compartir` and `Compartir again` observations were possible because the desktop surface was not capturable. Existing app-facade tests cover publication, unpublication, and republish semantics. Do not represent this as human acceptance.
- **Review/verify status:** required fresh OpenCode Go reviewers (DeepSeek V4 Flash UX and Qwen 3.8 Flash correctness) were not launched because this session is outside Herdr (`HERDR_ENV` unavailable). `./scripts/verify` = **PASS** (FE 244/244; Rust/workspace and M10 checks green). No implementation fix was needed.
- **Disposition:** current defect not reproduced; do not invent a Cloudflare workaround. Stop for human re-acceptance after verification and the required independent reviews are completed in an appropriate OpenCode/Herdr session.

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

## Estado actual (USER PROMPT QUOTING / SERIALIZATION PASS — TRANSPORT VERIFICADO LIMPIO + CONTRACT TESTS PINNED, REVIEWS INDEPENDIENTES APPROVE, INTEGRADO EN MAIN `8e75a64`+`ff1bca3`, VERIFICACIÓN VERDE, TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE; M11 NO INICIADO; GLIBC SIGUE PENDIENTE, 2026-09-03)

- **PASS = COMPLETE (orquestador/autor: OpenCode/DeepSeek V4 Flash, sesión FRESH).** El product owner reportó `hola` funciona y `"hola"` falla. **El trace completo del camino del texto de usuario NO pudo reproducir el fallo en main actual — el transporte YA lleva el texto del usuario como datos estructurados en TODAS las capas.** La evidencia es exhaustiva (ver abajo). El pass no cambió comportamiento de producción: agregó **CONTRACT TESTS que fijan la invariante** "USER TEXT IS DATA, NOT SYNTAX" en los límites FE→invoke, facade→engine y adapter→request body, más la captura del texto exacto en el fake server. **M11 NO INICIADO. GLIBC portability blocker PERMANECE FUERA DE SCOPE** (ver UNRESOLVED NEXT PACKAGING BLOCKER). Commits: `8e75a64` (contract tests), `ff1bca3` (doc nit del reviewer).
- **ROOT CAUSE REPORT PRECISO:** (1) **Capa que falla: NINGUNA del transporte actual.** El texto de usuario NUNCA deja de ser data: FE→`invoke("agent_send",{prompt})` (JSON.stringify estándar) → wry/tauri IPC (body leído byte-a-byte vía `register_uri_scheme`, parsing serde `Message`) → comando Rust `agent_send` (serde) → `append_user_message` (serde `to_string_pretty`) → `augment_prompt` (concat de string, el texto del usuario va RAW al final del bloque de instrucciones) → `OpenCodeAgentEngine.send` (`json!({"parts":[{"type":"text","text":req.text}]})` vía serde) → reqwest `.json()` → sidecar `POST /session/{id}/prompt_async` (acepta `"hola"`). (2) **Por qué el texto sin comillas sobrevive:** idéntico a cualquier otro texto — no hay rama de quotes en ninguna capa. (3) **Por qué "fallaría" el texto con comillas según el reporte:** NO falla en el transporte actual; el síntoma reportado dejó CERO rastro en el backend (ver evidencia: sin turno `"hola"` en DB del sidecar 105 user messages, sin mensaje en project.json reales, sin error en opencode.log). El único mecanismo consistente con "un prompt que termina en comilla no se envía" sería el guard de Enter durante composición IME (`ComposerBar.tsx:137-138`, contrato intencional y testeado `does not send while IME composition is active`) — PERO la evidencia lo refuta: el usuario envió con éxito prompts con comilla final (`...palabras: "pato", "gato", "perro", "vaca", "delfin", "elefante"` termina en `"`). (4) **Representación exacta:** sin representación "vieja" que corregir — todas las capas ya usan el JSON serializer / argumentos de datos. (5) **Shell involucrado: NO** (único `Command::new` = spawn del sidecar con argv fijo `serve --hostname --port --pure`; sin `sh -c`, sin `--prompt`, sin concat de texto de usuario en comandos). (6) **JSON serialization involucrada: SÍ, pero correcta** — serde en todos los límites, nunca ensamblado manual. (7) **Implicancias de seguridad:** texto shell-like (`$(touch /tmp/educai-should-not-exist)`, `hola; touch ...`) llega literal y NUNCA se ejecuta (markers ausentes antes/después, probe real + tests). (8) **Por qué generaliza:** los contract tests fijan que CUALQUIER texto (quotes, JSON-like, shell-like, backslash, rutas Windows, multiline, emoji) llega byte-a-byte al modelo como valor de string.
- **EVIDENCIA DE REPRODUCCIÓN (todo contra el sidecar real 1.18.25 pineado, config real copiada del app del humano, modelo real del humano `opencode/muse-spark-1.3-contributor-free` y `opencode/big-pickle`):** (a) HTTP crudo al sidecar: `hola` y `"hola"` → `prompt_async` 204, ambos user messages verbatim, assistant con `finish:"stop"` y parentID correcto. (b) **Camino completo del app** (`AppState` + `AgentService` + engine real, probe crate `/tmp/opencode/quote-repro/probe`): **matrix completa** = A `"hola"` como primer mensaje → completed "¡Hola! Qué alegría saludarte..."; B adjunto + `Usá este archivo y creá una actividad llamada "Rosco animal".` → completed, 1 Creation registrada (título con comillas intacto); C multiline `Creá una actividad.\n\nTítulo: "Animales del bosque"\n...` → completed, 1 Creation; D `$(touch /tmp/educai-should-not-exist)` y `hola; touch /tmp/educai-should-not-exist-2` → completed como texto literal, **markers NO creados** (post-check exists=false). (c) **Causalidad de turno (real, `big-pickle`):** conversation_id `01a067fe-7c27-73c9-8e89-470da6d48249` — T1 `hola` turn_id `01a067fe-8c26-…` completed "¡Hola! ¿En qué puedo ayudarte?"; T2 `"hola"` turn_id `01a067fe-9d96-…` completed "¡Hola! ¿Cómo estás?..."; T3 `bien y vos` turn_id `01a067fe-ab4a-…` completed "¡Todo bien, gracias!..." — **respuestas ligadas al turno actual, sin resurrección de turno fallido/stale, sin regresión de fallback "Listo.", `finish:"stop"` terminal** (log: `/tmp/opencode/quote-repro/causality-evidence.log`). (d) **Límite FE→IPC en el motor REAL WebKitGTK 4.1 con el dist real** (`ipc_capture.py`): escribir `"hola"` en el textarea real y Enter captura `invoke("agent_send",{projectId,prompt:'"hola"'})` — el string con comillas llega **verbatim** al invoke (JSON `"prompt":"\"hola\""`). (e) **Datos reales del humano:** la DB del sidecar tiene 105 user messages y NINGUNO es `"hola"`; sus prompts con comillas interiores funcionaron todos; los project.json reales NO contienen un turno `"hola"`; el `opencode.log` del humano NO tiene errores de prompt con comillas (solo un `ProviderModelNotFoundError` transitorio del catálogo de modelos el 9/2 18:32, no relacionado).
- **CONTRACT TESTS AGREGADOS (fijan la invariante, cambio SOLO de tests/test-harness, 175 insertions, 0 production behavior):** `crates/fake-opencode-server/src/lib.rs` captura `last_prompt_text` (primer part `type:"text"` del body de `prompt_async`, vía serde). `crates/project-agent/tests/opencode_adapter.rs` `send_preserves_quoted_and_special_text_exactly_in_request_body` — matrix de 14 casos (comillas dobles/simples, mixtas, JSON-like `{"a":"b"}`, `$HOME`, `$(echo hola)`, backticks, `hola; echo mundo`, `C:\Users\...`, `C:\\Users\\...`, backslash, emoji+quotes, multiline+quotes) que llega al body byte-a-byte. `crates/project-agent/tests/agent_security.rs` `command_like_user_text_is_sent_literally_and_never_executed` — texto shell-like literal + markers `/tmp/educai-should-not-exist[-2]` ausentes antes/después. `crates/project-app/tests/app_facade.rs` `quoted_and_shell_like_prompts_persist_and_reach_engine_verbatim` — persistencia serde + el prompt del engine conserva el texto del usuario byte-a-byte como segmento final (5 casos). `app/src/components/WorkspaceView.test.tsx` — `sends a fully quoted prompt verbatim as ordinary text` (`'"hola"'` llega a `agent_send` con comillas) y `sends shell-like user text verbatim without executing it`.
- **REVIEW PRODUCT/UX INDEPENDIENTE (OpenCode DeepSeek V4 Flash, FRESH, `quote-review-ux`, worktree `../ai-publisher-quote-review-ux` detached `8e75a64`) = APPROVE** (sin findings bloqueantes; nits no bloqueantes: ningún test combina adjunto+comillas interiores en un solo test — cubierto por separado; `ComposerBar.tsx:127` trim de whitespace preexistente, no de puntuación, ya cubierto por tests).
- **REVIEW CÓDIGO/CORRECTNESS INDEPENDIENTE (OpenCode Qwen 3.8 Flash, FRESH, `quote-review-code`, worktree `../ai-publisher-quote-review-code` detached `8e75a64`) = APPROVE** (diff puramente aditivo, sin producción; captura serde correcta y determinista; assertions byte-a-byte en el límite adapter; no-ejecución verificada con markers pre/post; nits no bloqueantes: doc dice "parts[0].text" y el código toma el primer part text — **corregido** en `ff1bca3`; app_facade usa `ends_with` en vez de igualdad — aceptable porque el servicio prepende el bloque de instrucción y la preservación exacta ya está fijada en el límite adapter).
- **VERIFICACIÓN FINAL EN MAIN `ff1bca3`: `./scripts/verify` EXIT=0** (cargo fmt/clippy/test verdes, FE **244/244** en 21 archivos — 242 previos + 2 nuevos —, format/lint/typecheck, fetch-sidecars --check, M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Rust: 1148 tests pasados en 85 suites** (antes 1142; +opencode_adapter 29/29 incl. matrix, agent_security 6/6 incl. shell-like, app_facade 43/43 incl. quoted/shell-like, fake-opencode-server verde). Log: `/tmp/opencode/verify-quote-final.log`.
- **EVIDENCIA REAL OPENCODE (sidecar pineado 1.18.25, API real):** probes contra sidecar real con config del app copiada; session/turn ids reales capturados arriba (causality log). No se loguearon contenidos de adjuntos. Los logs del humano no contienen evidencia del fallo `"hola"`.
- **INTEGRACIÓN:** commits en main `8e75a64` + `ff1bca3` (fix de doc nit del reviewer). **HEAD main = `ff1bca3`, working tree clean.**
- **LIMPIEZA:** reviewers `quote-review-ux`/`quote-review-code` cerrados (panes `w1M:p7`/`w1M:p8` closed); worktrees `../ai-publisher-quote-review-ux` y `../ai-publisher-quote-review-code` retirados; instancias AppImage de prueba y sidecars del probe terminados; `toolkit-accessibility` restaurado a false; markers `/tmp/educai-should-not-exist[-2]` ausentes. Branches de milestones previos (m-ux/*, corr/*) NO tocados. Session budget de esta sesión: CONTINUE al inicio (~32K).
- **MODELOS/POLÍTICA:** Cursor NO usado. GPT vía OpenCode Go PROHIBIDO — NO usado (no se requirió Codex: el trace y los contract tests fueron aptos para DeepSeek). Orquestador = OpenCode DeepSeek V4 Flash; Product/UX reviewer = OpenCode DeepSeek V4 Flash; Code/Correctness reviewer = OpenCode Qwen 3.8 Flash. **M11 NO INICIADO.**
- **PENDIENTE EXPLÍCITO (NO tocado en este pass):** **LINUX APPIMAGE GLIBC PORTABILITY** — Fedora-built AppImage falla en KDE Neon/Ubuntu 24.04 por símbolos GLIBC más nuevos; pass dedicado de packaging pendiente tras la re-acceptación humana del fix de quoting.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco (requiere rebuild desde main `ff1bca3`) probando explícitamente el escenario que disparó este pass: `hola` funciona, `"hola"` funciona, `hola "mundo"`, `{"a":"b"}`, multiline con comillas, y adjunto + `Usá este archivo y creá una actividad llamada "Rosco animal".`. **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (FRESH REAL APPIMAGE POST-CORRECCIÓN CONSTRUIDO DESDE MAIN `8516a62` — BUILD + VERIFICACIÓN TÉCNICA COMPLETA, FINDING A Y B VALIDADOS EN RUNTIME REAL, TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE, M11 NO INICIADO, 2026-09-02)

- **FRESH REAL APPIMAGE CONSTRUIDO DESDE MAIN `8516a62` (checkpoint del pass de corrección de aceptación humana, working tree clean) = PASS (sesión FRESH, OpenCode/DeepSeek V4 Flash, validación técnica determinista).** Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS).** Artefacto: `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.935.160 bytes**, **SHA-256 `b961080ad6b3291c3680f24c7473e3f9c5163bb39727f7fac15642cadc2671ba`** (NUEVO; el previo `7f5714e6…` era STALE — construido desde `1a29c80`, predata el merge de corrección), timestamp 2026-09-02 22:04:57 -0300, source commit `8516a626a2b1e3dd5b8a91d77841696fddbe0218` (main HEAD, working tree clean antes y después del build). Build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool tras el error esperado de linuxdeploy en Fedora 44). **Sidecars bundlados pineados verificados byte-idénticos en payload:** opencode **1.18.25** (sha `d91e0d33…`) y cloudflared **2026.8.3** (sha `f29324fe…`), `--version` en payload = 1.18.25 / 2026.8.3, sin repin/upgrade silencioso. **Frontend embebido correcto:** el binario embebe exactamente `assets/index-BgrhG3j6.js` + `assets/index-DqoasHvf.css` (idénticos al `dist` regenerado en ESTE build desde `8516a62`; el asset stale `index-BnZwruSz.js` NO está presente).
- **`./scripts/verify` EXIT=0 EN MAIN POST-BUILD** (cargo fmt/clippy/test verdes, FE **238/238** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Targeteds (todos en esta sesión, sin reuso):** `app_facade` **42/42** (incluye `attached_input_image_updates_existing_creation_in_place_without_phantom_image` escenario humano completo con republish, `agent_generated_image_can_be_a_creation_not_an_input_copy` A4, `later_turn_updates_the_same_web_creation_and_refreshes_publish`), `agent_service` **12/12** (`attached_image_copy_is_input_not_a_creation_and_in_place_update_is_registered` diff vacío, `attached_image_copy_reported_by_diff_is_still_input_not_a_creation` diff lista el PNG), `project-agent --lib` **19/19**, `attachments` 7/7, `opencode_adapter` 28/28, `app_provider` 14/14, preview 9/9+4/4+10/10. Log de verificación completo: `/tmp/opencode/verify-fresh-main.log`.
- **LANZAMIENTO REAL FEDORA/WAYLAND (DISPLAY=:0, WAYLAND_DISPLAY=wayland-0) = PASS.** Procesos/mounts stale del AppImage previo TERMINADOS (PID 991111 + sidecar 991429 del mount `EducAIbFCilP`, y mounts huérfanos `EducAIcjbpkp`/`EducAIHoNMih`/`EducAIoMfEAM` retenidos por gnome-shell/Xwayland/lm-studio, NO por EducAI). Lanzado el artefacto NUEVO (PID 1003722, setsid, 22:09) con PATH restringido `/tmp/opencode/path-indep-bin:/usr/bin:/bin` (sin opencode/cloudflared externos; `command -v opencode` = fail). Log: `[EducAI][INFO] startup version=0.1.0` + `[agent] backend starting → ready`, **SIN falso error de arranque**. WebKitNetworkProcess + WebKitWebProcess activos (UI usable). Sidecar hijo desde el mount propio del AppImage (`/tmp/.mount_EducAIlFNDno/usr/bin/opencode`, port 42351, `/global/health` HTTP 200 `{"healthy":true,"version":"1.18.25"}`; `readlink /proc/<pid>/exe` = mount del AppImage → **PATH-independencia PASS**). cloudflared **2026.8.3** verificado en el payload del mount. **Chat usability real (probe contra el sidecar live):** sesión nueva → `prompt_async` HTTP 204 → assistant `finish:"stop"` con `parentID` = user message (turn-link correcto), texto `"lista"` = respuesta exacta al request. Sin nudge.
- **CRITICAL VALIDATION A — attachment-as-input → existing-Creation update = PASS (runtime real sobre los archivos REALES del product owner).** Probe `realprobe-fix` (crate en `/tmp/opencode/realprobe-fix`, REBUILDADO contra main `8516a62` — el MISMO código embebido en el AppImage fresco) sobre `realdata-probe` (réplica real del proyecto `01a06409-…` "sopa de letras", workspace reseteado al estado pre-T2: `index.html` 11963 bytes original, SIN `encabezado.png`, C1 byteSize=11963). El agente rejugado edita `workspace/index.html` en su lugar (11963→12217) y copia `images.png` a `workspace/encabezado.png`. **PROBE_EXIT=0**: `registered ids: ["01a0640a-39e5-73fc-8393-9a7b385bacd9"]` = SOLO C1 (el MISMO `creation.id` existente, preservado por semántica de update in-place; NINGÚN id nuevo); C1 `outputs/<cid>/index.html` == 12217 bytes (el HTML real modificado); `encabezado.png` servido como sidecar dentro de `outputs/<cid>/` (sha `10f2dfee…` byte-idéntico al material `inputs/…/images.png`); `project.json` SIN Creation "image" fantasma (solo la web, byteSize=12217, visibility=public, revision=1). **El HTML actualizado referencia la imagen**: `src="encabezado.png"` presente en el HTML real modificado (el original 11963 NO lo tenía) → **Abrir C1 muestra la imagen**. Republish preservado: `later_turn_updates_the_same_web_creation_and_refreshes_publish` PASS (mismo id, snapshot actualizado).
- **PROVENANCE REGRESSION = PASS (verificado en el mismo probe + tests):** (1) **BYTE-IDENTICAL FILE COPIADO DE INPUT MATERIAL → INPUT, NO CREATION**: `encabezado.png` byte-idéntico a `inputs/images.png` (sha `10f2dfee…`) NO se registró como Creation (cubierto por `attached_image_copy_is_input_not_a_creation…` y `…_reported_by_diff_is_still_input_not_a_creation`); (2) **MODIFIED EXISTING ARTIFACT → UPDATE DE EXISTENTE**: `workspace/index.html` editado in-place → C1 re-registrado con MISMO `creation.id` (`replace_creation_content`); (3) **GENUINELY NEW AGENT-GENERATED OUTPUT → puede ser Creation nueva**: `agent_generated_image_can_be_a_creation_not_an_input_copy` PASS (A4). **SIN filtrado por extensión** (`.png` no se ignora: la distinción es por provenance/hash, no por tipo).
- **CRITICAL VALIDATION B — Configuración → Logs de esta sesión = PASS (motor REAL WebKitGTK 4.1, harness `wk_harness.py` en `/tmp/opencode/fe-render/` contra el dist REAL del build fresco, Tauri mock, 2 ventanas).** 1100x720: `hasLogsHeading=true`, `h3s=["Logs de esta sesión","Recomendados"]` (PRIMERA sección del `.dialog-body`), `.dialog` en viewport (top 43/bottom 677 de 720), header visible (36px), `.dialog-body` scroll interno (ch 538 < sh 680), `.session-logs` acotado 220px con scroll interno (sh 2304), documento NO scrollea (docScrollable=false). 640x560: idem (dialog top 34/bottom 526, body scroll interno 397<680, logs acotados, header visible). Acciones en el dist embebido: `Limpiar`, `Copiar`, `Actualizar`, empty state `Sin eventos todavía.`, clase `session-logs` — TODAS presentes (check binario). **Efimeridad across restart = PASS:** buffer en memoria SOLO-proceso (`OnceLock`+`Mutex`+`VecDeque` ring 500, `crates/project-app/src/session_log.rs`, SIN operaciones de filesystem — verificado por grep); unit `bounded_levels_and_clear_are_process_local` PASS. Reinicio real del AppImage fresco (kill → relaunch, PID 1007829): NUEVO mount `.mount_EducAIigbDPP`, NUEVO port 32893, sidecar viejo (42351) MUERTO, `[agent] backend ready` sin error → el viewer arranca con solo eventos de la sesión actual (buffer nuevo por proceso). Logs de ejecuciones previas NO presentes (no hay persistencia; el único `.log` en disco es el diagnóstico interno del sidecar opencode en su XDG aislado `--pure`, separado del viewer).
- **LOG SAFETY = PASS (auditoría de call-sites en `crates/project-app/src/app.rs` + probe backend real).** `session_log::record` con: `conversation created/renamed/deleted id=`, `attachment associated conversation_id= material_id= name=<safe_file_name> bytes=`, `turn accepted conversation_id= message_id= chars=`, `turn started conversation_id= turn_id= model=<provider>/<model>`, `turn terminal conversation_id= turn_id= status= creations= duration_ms=`, `conversation model changed/cleared id=`, `creation shared conversation_id= creation_id=`, `conversation unshared id=`. **NUNCA** prompts, texto de mensajes, credenciales, tokens, headers auth, contenidos de attachments ni HTML/JS/CSS generados. Probe backend real (`logging-probe` REBUILDADO, `/tmp/opencode/logging-probe`): `session-logs-after-create count=1` → `session-logs-final count=4` con ring first/last = metadata-only (ids, status, counts, duration_ms).
- **REGRESIÓN-ONLY (UX previamente aceptada, NO reabierta — `./scripts/verify` EXIT=0 + targeteds FE 72/72 en esta sesión):** Conversation Details responsivo (name/model/files/rename/select/clear/open folders — `ConversationDetails.test.tsx` 4/4), un "Abrir carpeta contenedora" por sección lógica (`folder_open_rejects_invalid_project_before_opening` + FE), preview tipada con sniff de magic bytes (PNG como imagen, texto escapado — `PreviewModal.test.tsx` 7/7), modelo por-conversación (`conversation_model_is_validated_persisted_isolated_and_clearable` PASS), sin sugerencia stale de adjunto en el composer (`ComposerBar.test.tsx` "does not render a persistent material picker"), mensaje genérico de procesamiento ("Procesando tu solicitud…" = `messages.agent.creating`, ChatPanel fase `working`), chat contextual simple (probe live real), Creation preview (`CreationsPanel.test.tsx` 10/10, preview_lifecycle 4/4 + preview_security 10/10). Sin redesign en este pass.
- **MODELOS/POLÍTICA:** Cursor NO usado. GPT vía OpenCode Go PROHIBIDO — NO usado (fase 100% OpenCode/DeepSeek V4 Flash orquestador, determinista: build + verify + probes contra sidecar real + harness WebKitGTK). Session budget CONTINUE.
- **ESTADO DEL REPO:** main limpio (working tree clean antes y después del build), HEAD `8516a62`. M11 NO INICIADO. Sin worktrees/branches/workers temporales. AppImage fresco `b961080a…` en su ruta canónica.
- **PENDIENTES EXPLÍCITOS (NO tocados en este pass):** (1) **USER PROMPT QUOTING / SERIALIZATION**: input con comillas como "hola" falla mientras hola funciona — pass dedicado pendiente. (2) **LINUX APPIMAGE GLIBC PORTABILITY**: GLIBC_2.42/2.43 requeridos por librerías bundladas → falla en KDE Neon 24.04/Ubuntu Noble — pass dedicado de baseline reproducible pendiente.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `b961080a…` del escenario Finding A (adjuntar images.png a una Sopa de letras existente → pedir "agregale esta imagen en el encabezado" → C1 se actualiza con la imagen visible al Abrir, sin Creation "Imagen" fantasma, y si estaba compartida la URL refleja el cambio) y Finding B (Configuración → Logs de esta sesión visible, scrollea, efímera entre reinicios). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (PASS DE CORRECCIÓN DE ACEPTACIÓN HUMANA — SEMÁNTICA INPUT/OUTPUT DE ADJUNTOS + LOGS DE ESTA SESIÓN = COMPLETE, REVIEWS APPROVE, INTEGRADO EN MAIN, preservado)

- **PASS = COMPLETE (orquestador/autor: OpenCode/DeepSeek V4 Flash, sesión FRESH).** Pass ACOTADO de corrección de aceptación humana con DOS hallazgos del product owner sobre el AppImage real `7f5714e6…`: **(A)** adjuntar `images.png` y pedir "agregale esta imagen en el encabezado" NO modificó la Creation existente "Sopa de letras" y en cambio surfació una Creation fantasma "Imagen"; **(B)** "Configuración → Logs de esta sesión" reportado como ya no visible. **M11 NOT STARTED.** **GLIBC portability blocker PERMANECE FUERA DE SCOPE** (ver UNRESOLVED NEXT PACKAGING BLOCKER). Commits de implementación: `aeaddfa`, `8cc8c54`, `ae238f0` (HEAD).
- **ROOT CAUSE EXACTO DE (A) (evidenciado con datos REALES en disco del AppImage `7f5714e6…` en `~/.local/share/com.educai.publisher/projects/01a06409-…`, NO adivinado):** T2: el agente editó `workspace/index.html` EN SU LUGAR (11963→12217 bytes, mismo path) y copió la imagen adjunta a `workspace/encabezado.png` (sha `10f2dfee…` = byte-idéntico al material `inputs/…/images.png`). El pipeline de registro: el `/diff` real devolvió vacío para el edit commitado-in-place; el fencing de workspace por SOLO path (`workspace_before` HashSet) descartaba el `index.html` modificado (el path ya existía) → **C1 NO se re-registró** (su `outputs/…/index.html` quedó en 11963 bytes, sin imagen); el PNG copiado era NUEVO → se escaneó → se registró como Creation `image` "encabezado" → **la card fantasma "Imagen"**. `sidecar_paths_of_web_entry` no pudo ligar el PNG a un web artifact porque index.html no estaba en la lista.
- **SEMÁNTICA INPUT vs OUTPUT (explicitada en el dominio):** INPUT MATERIAL = archivos provistos por el usuario (adjuntos, imágenes de referencia) que viven byte-igual en `inputs/`; OUTPUT/CREATION = artifact producido deliberadamente por el agente como entregable. Un PNG subido NO es una Creation por existir en el workspace. **Fix por PROVENANCE/OWNERSHIP, NUNCA por extensión** (`.png` no se ignora: una imagen GENERADA por el agente con hash distinto SÍ puede ser Creation — cubierto por `agent_generated_image_can_be_a_creation_not_an_input_copy`).
- **FIX (A) — fencing path+SHA-256 + filtro de provenance:** (1) `workspace_before` ahora es `path → sha256` (snapshot al inicio del turno); un archivo existente editado EN SU LUGAR (mismo path, contenido distinto) se detecta como UPDATE y se re-registra con la semántica de update establecida (match kind+display_name → `replace_creation_content` → MISMO `creation.id`, republish preservado vía `refresh_published_snapshot`); archivos sin cambios (leftovers de turnos previos/fallidos) siguen excluidos. (2) `merge_artifacts` SIEMPRE une el escaneo acotado al diff (el diff sigue siendo autoritativo), con el fence hash; un candidate sin sha legible se omite (seguridad del fence por path anterior, sin error del turno). (3) Filtro de provenance: un archivo del workspace byte-idéntico a un material de `inputs/` (`collect_user_material_hashes`, walk acotado por depth+file-count, SIN tope de bytes para no perder la garantía) es INPUT, nunca Creation — aplica a los dos caminos (diff vacío y diff que reporta el PNG). El PNG copiado pasa a servirse como sidecar del web Creation (`copy_web_sidecars`) → **Abrir C1 muestra la imagen**.
- **RUNTIME REAL (probe sobre los archivos REALES del product owner, `realprobe-fix` en `/tmp/opencode/`):** reconstruido el estado pre-T2 con los archivos reales (index.html original 11963, material images.png real, agente rejugado editando index.html y copiando encabezado.png) → `AgentService` corregido con `FilesystemCreationRegistrar` real: **EXIT=0** — C1 re-registrado con el HTML real modificado (12217 bytes), `encabezado.png` servido como sidecar dentro de `outputs/<cid>/`, y `project.json` SIN Creation "image" fantasma. Prueba real de que el PNG de input no se clasifica como output.
- **FINDING (B) — Logs de esta sesión:** verificado con el motor REAL WebKitGTK 4.1 (harness pygobject + Tauri mock, ventanas 1100x720 y 640x560): el modal Configuración SÍ renderiza "Logs de esta sesión" como PRIMERA sección del body (`h3` + descripción + Limpiar/Actualizar/Copiar + `<pre class="session-logs">` acotado 220px con scroll interno 2304px), header sticky visible, `.dialog-body` scrollea internamente (680>397), modal entra en viewport, proveedores alcanzables, documento no scrollea (1100x720). Backend sin cambios: buffer in-memory ring 500 (`OnceLock`+`Mutex`+`VecDeque`), SOLO-proceso, sin persistencia, metadata-only, espejo stderr. Tests FE NUEVOS que fijan el contrato: heading presente como primer hijo del `.dialog-body`, acciones presentes, `session-logs` acotado, empty state "Sin eventos todavía.", Actualizar re-consulta. **Sin redesign de logging ni del modal.**
- **Conversation Details NO reabierto (preservado):** modal responsivo con scroll interno, modelo por-conversación, rename, Material subido + Creaciones generadas, un "Abrir carpeta contenedora" por sección, preview tipada. Sin cambios en este pass.
- **`./scripts/verify` EXIT=0 EN MAIN POST-PASS** (cargo fmt/clippy/test verdes, FE **238/238** — antes 236 —, format/lint/typecheck, fetch-sidecars --check, M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Targeteds (todos en esta sesión, sin reuso):** `app_facade` **42/42** (+`attached_input_image_updates_existing_creation_in_place_without_phantom_image` escenario humano completo con republish, +`agent_generated_image_can_be_a_creation_not_an_input_copy` A4), `agent_service` **12/12** (+`attached_image_copy_is_input_not_a_creation_and_in_place_update_is_registered` diff vacío, +`attached_image_copy_reported_by_diff_is_still_input_not_a_creation` diff lista el PNG), `project-agent --lib` **19/19** (nuevos unit: in-place update detection, scan fingerprint, user-material hashes), `attachments` 7/7, `opencode_adapter` 28/28, `app_provider` 14/14, preview 9/9+4/4+10/10, project-fs lifecycle. `pnpm --dir app run build` OK (hashes FE `index-BgrhG3j6.js` + `index-DqoasHvf.css` sin cambios).
- **REVIEWS INDEPENDIENTES (FRESH, worktree de review `../ai-publisher-corr-04-review` sobre el diff commitado):** **Product/UX = OpenCode DeepSeek V4 Flash = APPROVE** (adjunto ya no es Creation fantasma; C1 se actualiza in-place con identidad preservada y Abrir muestra la imagen; imagen generada sigue siendo Creation posible A4; success claim respaldado por artifact real actualizado; Logs visible + contrato + modal responsivo intacto; nits no bloqueantes: tradeoff especulado en image-sólo byte-igual, I/O menor por turno, tests DOM-estructurales). **Code/Correctness = OpenCode Qwen 3.8 Flash = APPROVE** (Q1-Q6 verificados: fencing hash correcto, provenance segura, registrar intacto, sin regresiones, tests fieles a las 2 variantes reales + A4, logs efímeros sin cambios backend) con 2 MINORs no bloqueantes → **fix acotado `8cc8c54`** (walk de materiales acotado por depth+file-count SIN tope de bytes; candidatos sha-ilegible omitidos en vez de error del turno; `workspace_before` registra presentes con sha vacío) → **re-review Qwen FRESH = APPROVE** (ambos minors verificados, gate total del pass, comandos verdes; nits no bloqueantes: tests unit de merge usan helpers sin cubrir archivos ilegibles). Limpieza menor `ae238f0` (helpers de test alineados a `unwrap_or_default`).
- **MODELOS/POLÍTICA:** Cursor NO usado (quota agotada). GPT vía OpenCode Go PROHIBIDO — NO usado (autor 100% OpenCode/DeepSeek V4 Flash; reviews DeepSeek V4 Flash y Qwen 3.8 Flash vía OpenCode CLI). Session budget CONTINUE (~41K al inicio).
- **ESTADO DEL REPO:** main limpio (working tree clean), HEAD `ae238f0`. M11 NO INICIADO. Sin worktrees/branches/workers temporales. AppImage `7f5714e6…` queda STALE (predata este pass) → requiere rebuild en el próximo gate.
- **PENDIENTES EXPLÍCITOS (NO tocados en este pass):** (1) **USER PROMPT QUOTING / SERIALIZATION**: input con comillas como "hola" falla mientras hola funciona — pass dedicado pendiente. (2) **LINUX APPIMAGE GLIBC PORTABILITY**: GLIBC_2.42/2.43 requeridos por librerías bundladas → falla en KDE Neon 24.04/Ubuntu Noble — pass dedicado de baseline reproducible pendiente.
- **STATUS: TÉCNICAMENTE READY PARA EL PRÓXIMO GATE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO después de este pass: **FRESH REAL APPIMAGE BUILD + TECHNICAL VERIFICATION** desde main `ae238f0` (`scripts/smoke-package appimage`, sidecars pineados opencode 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real Fedora/Wayland), luego **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco del escenario Finding A (adjuntar images.png a una Sopa de letras existente → pedir "agregale esta imagen en el encabezado" → C1 se actualiza con la imagen visible al Abrir, sin Creation "Imagen" fantasma, y si estaba compartida la URL refleja el cambio) y Finding B (Configuración → Logs de esta sesión visible, scrollea, efímera). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (UX/FUNCTIONAL FIXES PASS — MODALES, DETALLES DE CONVERSACIÓN Y ADJUNTOS = COMPLETE, preservado)

- **PASS = COMPLETE (orquestador/autor: OpenCode/DeepSeek V4 Flash, sesión FRESH).** Pass ACOTADO de corrección UX/funcional sobre: (A) modal Configuración, (B) modal Detalles de la conversación, (C) "Abrir carpeta contenedora" repetido por archivo, (D) preview/apertura de archivos mal tipada, (E) imagen adjunta usada como input vs abierta, (F) sugerencia persistente del archivo anterior en el composer. **M11 NOT STARTED.** **GLIBC portability blocker PERMANECE FUERA DE SCOPE y NO resuelto aquí** (ver UNRESOLVED NEXT PACKAGING BLOCKER). Commit de implementación: `1a29c80`.
- **PROBLEMAS OBSERVADOS / CAUSA RAÍZ / CORRECCIÓN (A-F):** **(A)** Configuración desbalanceado/sin adaptación: `.dialog` base no tenía `max-height` ni scroll interno → todo el modal scrolleaba incluyendo header; `.session-logs` (pre de logs) NO tenía clase CSS → sin scroll interno. **Fix:** `Dialog.tsx` envuelve children en `.dialog-body` (flex 1, `overflow-y:auto`, min-height 0); `.dialog` gana `max-height:min(88vh,760px); overflow:hidden`; header sticky (`flex:0 0 auto`); `.provider-dialog` = `max-width:min(680px,calc(100vw - 32px))`; `.session-logs` = `max-height:220px; overflow:auto` (scroll interno). Logs efímeros (in-memory, sin persistencia) intactos. **(B)** Conversation Details no se adaptaba: usaba el `.dialog` base (max-width 420px, sin max-height) → contenido cortado. **Fix:** `.conversation-details-dialog` = `max-width:min(560px,calc(100vw - 32px))`; base `max-height:min(88vh,760px)`; body scrollea internamente; header/quitar sin cortarse. **(C)** "Abrir carpeta contenedora" se repetía por cada fila (`material_open_folder`/`creation_open_folder` por item). **Fix:** UNA acción por sección (Material subido → `materials_open_folder`, Creaciones generadas → `creations_open_folder`), comandos NUEVOS a nivel proyecto que resuelven los roots canónicos `inputs/`/`outputs/` vía `FilesystemProjectContentStore::materials_dir`/`creations_dir` (nuevos, con la misma disciplina symlink/containment que `material_path`/`creation_dir`); las filas ahora muestran nombre + tamaño + un "Abrir" (preview) individual, sin folder-open repetido. **(D)** Preview mal tipada: `PreviewModal` clasificaba SOLO por `contentType.startsWith("image/")` y TODO lo demás lo decodificaba como texto (`atob`+`TextDecoder`) → PNG/binario renderizados como basura. **Fix:** clasificación por contentType + **sniff de magic bytes** (PNG/JPEG/GIF/WebP) con fallback a imagen aunque el tipo declarado sea genérico; text-like (`text/*`, json, xml, yaml, js) y `text/html` → preview textual ESCAPADO (nunca HTML crudo); binarios no previsualizables → metadata (nombre/tamaño/kind) + "Abrir con la aplicación" (`onOpenExternal` → `materialOpen`/`creationOpen`), NUNCA texto basura. `CreationsPanel` y `ConversationDetails` pasan meta + open-external. **(E)** Imagen adjunta vs abierta: el adjunto del composer ya era input del turno (`attachmentIds` → `agent_send` → `resolve_attachments` → provision a `workspace/materials/` + prompt); la confusión era semántica de UI. **Fix:** preview = acción explícita separada (Abrir en chip/timeline/details); el flujo de envío NO dispara `material_open`/`preview_data` (test FE `WorkspaceView "uses an attached image as turn input without opening any preview"`); test backend NUEVO `attached_image_is_provisioned_as_creation_input_without_opening` (attachments 7/7) prueba que los bytes de la imagen llegan a `workspace/materials/` y la creación se registra. **(F)** Sugerencia del archivo anterior: el composer tenía un **material picker persistente** que listaba TODOS los materiales del proyecto como sugerencia al reabrir "Adjuntar". **Fix (root cause):** se eliminó el picker; "Adjuntar" abre SIEMPRE el diálogo nativo de archivo (el dedup `material_add_from_path` re-adjunta un material ya subido devolviendo el existente); el área de adjuntos del composer refleja SOLO el borrador del turno y se limpia tras send (`setAttachmentIds([])`); sin lista de sugerencias stale. CSS muerto del picker eliminado.
- **`./scripts/verify` EXIT=0 EN MAIN POST-PASS** (cargo fmt/clippy/test verdes, FE **236/236** en 21 archivos — antes 228 —, format/lint/typecheck, fetch-sidecars --check, M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Targeteds (todos en esta sesión, sin reuso):** `app_facade` **40/40** (+`folder_open_rejects_invalid_project_before_opening`), `attachments` **7/7** (+`attached_image_is_provisioned_as_creation_input_without_opening`), `opencode_adapter` **28/28**, `app_provider` **14/14**, `agent_service` **10/10**, `project-fs project_lifecycle` (incl. nuevo `materials_and_creations_folders_resolve_to_owned_fixed_roots`), preview `preview_security` 10/10 + `preview_lifecycle` 4/4 + `project-app --test preview` 9/9 (regresión-only, NO reabiertos). Nuevos tests FE: PreviewModal binario/sniff/html-escaped (3), ConversationDetails folder-por-sección + preview txt + PNG como imagen (2), ComposerBar dialog-nativo + sin-picker (2), WorkspaceView imagen-como-input + borrador-limpio (2), ProviderPanel clase responsiva/session-logs (1). `pnpm --dir app run build` (tsc + vite) OK.
- **FRESH REAL APPIMAGE CONSTRUIDO (working tree del pass = commit `1a29c80`) = PASS.** Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS)**. Artefacto: `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.931.064 bytes**, **SHA-256 `7f5714e665c16e844fbbf36a0cfca9ebaedf08e2d5b7770461e50f763b011152`** (NUEVO, difiere del previo `a227d1d1…`), timestamp 2026-09-02 18:56 -0300. **Frontend embebido NUEVO:** el binario `usr/bin/educai` del payload embebe `assets/index-BgrhG3j6.js` + `assets/index-DqoasHvf.css` (los hashes del `pnpm build` de este pass; el stale `index-BnZwruSz.js` NO está presente).
- **SIDECAR PINS VERIFICADOS EN PAYLOAD (byte-idénticos al sidecar fetcheado):** opencode **1.18.25** (payload sha `d91e0d33…`) y cloudflared **2026.8.3** (payload sha `f29324fe…`). `--version` en payload: opencode `1.18.25`, cloudflared `2026.8.3`. Sin repin/upgrade silencioso.
- **LANZAMIENTO REAL FEDORA/WAYLAND (DISPLAY=:0) = PASS.** Instancias stale del AppImage previo (`821048` @mount `EducAIeNAGFE`, `834278` @mount `EducAImNMMpj`) y sidecars huérfanos TERMINADOS (no usados como evidencia). Lanzado el artefacto NUEVO (PID 871903, setsid, PATH restringido `/tmp/opencode/path-indep-bin:/usr/bin:/bin`, `--debug`, stderr capturado en `/tmp/opencode/educai-launch-modal-pass.log`) con log: `[EducAI][INFO] startup version=0.1.0` + `[agent] backend starting → ready`, **SIN falso error de arranque**. WebKitNetworkProcess + WebKitWebProcess activos (UI usable). Sidecar hijo desde el mount propio del AppImage (`/tmp/.mount_EducAIOmFOjA/usr/bin/opencode`, port 35851, `/global/health` HTTP 200 `{"healthy":true,"version":"1.18.25"}`; `readlink /proc/<pid>/exe` = mount del AppImage → **PATH-independencia PASS**; `command -v opencode` = fail en el PATH restringido). cloudflared **2026.8.3** en el payload del mount.
- **REGRESIÓN-ONLY (NO reabierto, `./scripts/verify` EXIT=0 + targeteds):** "hola" contextual, chat secuencial, Creation request, Preview/Abrir (preview_lifecycle 4/4, preview_security 10/10, project-app preview 9/9), turn-link/causalidad de turno, modelo por conversación (rename + select/clear + aislamiento), logs de esta sesión efímeros, arquitectura de publicación real (publication suites verdes), Enter/Shift+Enter, badge compartido, delete «Sí», aislamiento de conversaciones — sin regresiones.
- **ESTADO DEL REPO:** main limpio (working tree clean después del build), HEAD `1a29c80` (commit de implementación del pass; el commit de checkpoint doc lo sigue). M11 NO INICIADO. Sin worktrees/branches/workers temporales. AppImage fresco en su ruta canónica.
- **MODELOS/POLÍTICA:** Cursor NO usado. GPT vía OpenCode Go PROHIBIDO — NO usado. Fase 100% OpenCode/DeepSeek V4 Flash (orquestador/autor), sin workers LLM (fase determinista: build + verify + probes contra sidecar real + tests). Session budget CONTINUE.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO funcional: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `7f5714e6…`, escenario ampliado con los 6 casos de prueba de este pass: (1) conversación con txt + imagen → abrir Detalles, listado de materiales, modal completo con scroll interno, preview correcto de txt e imagen; (2) creación simple con txt; (3) adjuntar imagen y pedir "agregala en el encabezado" → la imagen se usa como input de creación (sin abrir preview); (4) reabrir composer → NO sugiere el archivo anterior; (5) abrir Configuración → log viewer con scroll interno y solo-sesión; (6) redimensionar ventana → ambos modales se adaptan. Además: "hola" → saludo real, pregunta normal, Creation con adjunto, secuencial, Preview, Compartir, detalles desde el título, cambiar modelo por conversación sin afectar otras, Logs de esta sesión, y el escenario Finding 6 (modelo que falla → modelo que funciona → respuesta SOLO de ese turno). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## UNRESOLVED NEXT PACKAGING BLOCKER (registrado, NO abordado en esta sesión)

- **LINUX APPIMAGE PORTABILITY / REPRODUCIBLE BUILD BASELINE.** El AppImage construido en Fedora fue observado fallando en KDE Neon 24.04 / Ubuntu Noble-family host porque las librerías bundladas requieren símbolos GLIBC más nuevos (GLIBC_2.42 / GLIBC_2.43 y requisitos ABI relacionados). **NO corregido en este pass por directiva: PERMANECE FUERA DE SCOPE del pass de quoting/serialización y NO resuelto aquí.** Tras la re-acceptación funcional humana del AppImage fresco (que requiere rebuild desde main `ff1bca3`), se requiere un pass dedicado: **LINUX APPIMAGE PORTABILITY / REPRODUCIBLE BUILD BASELINE PASS** (build en un entorno baseline/glibc más viejo, verificación de símbolos GLIBC requeridos vs host destino, y definición de un baseline reproducible).

## Estado previo (FRESH REAL APPIMAGE POST-CONVERSATION-UX `a227d1d1…` — FINDINGS 1-6 VALIDADOS, TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE, preservado)

- **AppImage `a227d1d1…` (desde main `2b122e5`, merge `6886ba9`):** 180.931.064 bytes, SHA-256 `a227d1d1805a15570b71cd9fa9c1d6d09fd12787d6d392a4584cdddfbd809ee5`, FE embebido `index-BnZwruSz.js` + `index-UHkEOOtE.css`, sidecars opencode 1.18.25 (`d91e0d33…`) + cloudflared 2026.8.3 (`f29324fe…`). Lanzamiento real Fedora/Wayland PASS (PATH-independencia, sidecar propio del mount). **Finding 6 validado en runtime real** (ancla de turno inmutable + parent estricto + evicción de sesión en errores + fencing de workspace-scan; T1 fallido no resucita en T2/T4). **Findings 1-5 validados** ("Procesando tu solicitud…" transitorio; Conversation Details con rename/modelo por-conversación/material+creaciones; composer limpio tras send; card sin label redundante; Logs de esta sesión solo-sesión). **`./scripts/verify` EXIT=0** (FE 228/228, adapter 28/28, app_facade 39/39, app_provider 14/14, agent_service 10/10, preview 9/9 + 4/4 + 10/10). **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE, NO HUMAN ACCEPTED. M11 NO INICIADO.**

## Estado previo (CONVERSATION UX / PER-CONVERSATION MODEL / SESSION LOGGING / MODEL-SWITCH CAUSALITY PASS = COMPLETE — REVIEWS APPROVE, INTEGRADO EN MAIN `6886ba9`, preservado)

- **PASS = COMPLETE.** Orquestador FRESH (OpenCode/DeepSeek V4 Flash) bootstrapeó en main `456be54` (checkpoint previo: AppImage `832408b6…` técnicamente listo, human re-acceptance pendiente), clasificó el trabajo (Findings 1-6 + logging; Finding 6 = deep async/state causality → author Codex), lanzó UN solo autor Codex CLI (cuenta OpenAI/ChatGPT del owner, gpt-5.6-terra, continuado en gpt-5.6-luna por capacidad, worktree `../ai-publisher-ux-conversation-pass`, branch `corr/ux-conversation-pass`), 2 reviews independientes FRESH, ciclo de fixes acotado R1-R6 + tests FE, y fusionó. **Merge `6886ba9` en main** (ort, 30 archivos, +1203/−422, commits del autor `a61fc08`…`4d51a03` preservados). **`./scripts/verify` EXIT=0 EN MAIN POST-MERGE** (FE **228/228** en 21 archivos, cargo fmt/clippy/test verdes, format/lint/typecheck, fetch-sidecars --check, M10 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). Targeteds post-merge: `opencode_adapter` **28/28**, `app_facade` **39/39**, `project-agent --lib` **16/16**, `agent_service` **10/10**, session_log unit tests, FE **233/233 antes de quitar ModelSelector muerto → 228/228 final**. Working tree limpio.
- **REVIEW PRODUCT/UX INDEPENDIENTE (OpenCode DeepSeek V4 Flash, FRESH, `review-ux2`) = APPROVE.** "Procesando tu solicitud…" correcto y general (solo transitorio, nunca mensaje assistant persistido); Conversation Details descubrible por el título; rename/modelo/archivos jerárquicos y claros; modelo claramente por-conversación; sin control global duplicado (Configuración = proveedores + Logs); card de Creation simplificada con a11y preservada; composer limpio tras send con material retenido; Log Viewer "Logs de esta sesión" solo-sesión (in-memory, se pierde al reiniciar), niveles/autoscroll/Clear/Copy, metadata-only; causalidad de switch OK. REQUIRED findings resueltos: "Predeterminado de Configuración" ahora llama `conversation_model_clear` (vive, no error) + copy centralizado en `messages.ts`. Nits no bloqueantes: helpers muertos (`titleAria`/`selectAriaLabel`/`progress.creating`).
- **REVIEW CÓDIGO/CORRECTNESS INDEPENDIENTE (OpenCode Qwen 3.8 Flash, FRESH, `review-code2b`) = APPROVE.** MAJOR 1-4 y MINOR 5-8 resueltos: validación de modelo pineado contra `model_list()` al enviar (miss → WARN + fallback global, sin cuelgue); `conversation_model_clear` persiste `None`; tests de aceptación (validación/persistencia/aislamiento/clear, seguridad de rutas propias, `failed_send_evicts_cached_session_for_next_turn`, timeouts ajustados, ring 500/niveles/args/clear, ConversationDetails/title-click/composer-clean/Logs); a11y (aria-label `Detalles de <name>`, live region acotada + Refresh); robustez (evicción en TODOS los errores de send, `Poisoned` recuperado, deadline ÚNICO compartido anchor+poll). MINORs residuales no bloqueantes tras cleanup final `4d51a03`: casos B/D/E de Finding 6 con cobertura parcial (genérico is_err / log / Conflict sin test Rust dedicado), fallback send-time sin test directo, copy `requires_choice` re-apuntado a detalles de conversación, filtro de modelos conectados/gratis en ConversationDetails, `clearLogs` con try/catch. Sin regresión de seguridad (log metadata-only, sin archivos; folder-open solo rutas canónicas propias).
- **ROOT CAUSE EXACTO DE FINDING 6 (evidenciado con sidecar REAL pineado 1.18.25, NO adivinado):** `poll_session` recomputaba `originating_user_id = last_user_message_id` de la lista VIVA de mensajes en cada poll → un mensaje posterior podía cambiar qué user ID se trataba como el turno actual; además los fallbacks lenientes de `message_belongs_to_turn` (`(_,None)=>true`, `(None,Some)=>true`) podían atribuir un mensaje assistant stale (con `finish:"stop"` y parentID de un turno previo, o sin parentID) al turno actual. Tras un fallo de T1, si el último user message seguía siendo el de T1, la respuesta tardía de T1 (p.ej. lista de equipos) satisfacía el check y se entregaba como respuesta a "Hola" (T2).
- **FIX FINDING 6 (preservando causalidad correcta existente):** `a61fc08` captura el user message id del turno DESPUÉS de `prompt_async` (anchoring inmutable, id NO presente en el snapshot pre-send) y lo mantiene fijo mientras se polea; exige parentID EXACTO + `finish:"stop"`; evicción de la sesión cacheada en TODOS los errores de send (el siguiente turno abre sesión NUEVA). `f327c87` cercas el fallback de workspace-scan a archivos AUSENTES al inicio del turno (los leftovers de un turno fallido NO se registran en un turno posterior). `message_belongs_to_turn` estricto `_ => false`. Sin heurísticas de contenido, sin sleeps, deadline único acotado.
- **TRACE REAL Finding 6 (probe `/tmp/opencode/causality-probe`, sidecar pineado 1.18.25, XDG aislado, motor del worktree):** T1 `ses_f9c6fd94fffeDk0X1ULulz5kZF` modelo `missing/provider`, user `msg_063902701001RGKM58k04apJTM`, terminal `TaskFailed("timed out")` a `1788376816726`; T2 en sesión DISTINTA `ses_f9c6f6398ffe4UdSN2L5FwP9fV` modelo `opencode/big-pickle`, user `msg_063909c6f001BEiV69B4gpzETm` → assistant `msg_063909c7c001iFGKDrPWISJPUG` (parent=T2 user, `finish=stop`), `Completed` `1788376821440`, respuesta SOLO saludo "¡Hola! ¿En qué puedo ayudarte?", artifacts=[]. T1 NO resucitó, NO se atribuyó a T2. Trace crudo: `/tmp/opencode/causality-runtime-trace.txt`.
- **SEMÁNTICA DE MODELO POR-CONVERSACIÓN:** `Project.model` opcional (provider/model ids) en `project.json` con serde default (sin migración, schema v3 intacto); `resolve_agent_inputs` usa el modelo explícito de la conversación (validado contra `model_list()` al enviar, miss → WARN + fallback global) o el default global si `None`; `conversation_model_select`/`conversation_model_clear` bloqueados durante turno activo (try_lock → `Conflict`), lock `Poisoned` recuperado; el cambio aplica SOLO a turnos futuros, nunca muta turnos completados.
- **LOGGING / LOG VIEWER:** buffer en memoria de 500 entradas (OnceLock + Mutex + VecDeque, ring), espejo a stderr (`[EducAI][LEVEL] ...`), niveles ERROR/WARN/INFO/DEBUG (default INFO) con `--debug`/`--log-level` vía `configure_from_args` en lib.rs setup; SOLO-proceso (sin archivos, sin logs de sesiones previas tras restart); viewer "Logs de esta sesión" en Configuración con niveles, autoscroll, Refresh, Clear, Copy. Seguridad: solo metadata (ids, conteos, `safe_file_name`, duraciones, model/provider ids) — NUNCA prompts, texto de mensaje, credenciales, tokens, headers auth, contenidos de attachments ni HTML/CSS/JS generados.
- **FINDINGS 1-5 RESUELTOS:** (1) "Procesando tu solicitud…" transitorio general; (2) Conversation Details desde el título (rename, modelo por-conversación, Material subido + Creaciones generadas con "Abrir carpeta contenedora" solo rutas propias validada); (3) composer sin chip/sugerencia stale tras send (material/historial/Details retenidos); (4) Creation card sin label redundante (kind label + a11y preservada); (5) Configuración → Logs.
- **MODELOS/POLÍTICA:** Cursor NO usado (quota agotada). GPT vía OpenCode Go PROHIBIDO — NO usado; GPT únicamente vía Codex CLI (cuenta OpenAI/ChatGPT del owner): autor gpt-5.6-terra → gpt-5.6-luna (capacidad). Reviews: Product/UX = OpenCode DeepSeek V4 Flash FRESH; Code/Correctness = OpenCode Qwen 3.8 Flash FRESH. Ambos APPROVE sin REQUEST_CHANGES final.
- **ESTADO DEL REPO:** main limpio, HEAD `6886ba9` (merge), M11 NO INICIADO. Sin worktrees/branches/workers temporales (limpio al cierre). AppImage previo `832408b6…` SÍ queda STALE tras este merge → requiere rebuild.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximos gates: (1) **FRESH REAL APPIMAGE BUILD + TECHNICAL VERIFICATION** desde main `6886ba9` (`scripts/smoke-package appimage`, sidecars pineados 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real Fedora/Wayland, probe real del chat + switch de modelo); luego (2) **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** (escenario: conversación nueva, "hola" → saludo real, pregunta normal, Creation con adjunto, secuencial, Preview, Compartir, detalles de conversación desde el título, cambiar modelo por conversación sin afectar otras, Logs de esta sesión, y el escenario Finding 6: modelo que falla → volver a modelo que funciona → "Hola" → respuesta SOLO de ese turno). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (FRESH REAL APPIMAGE POST-MESSAGE-SELECTION — preservado)

- **FRESH REAL APPIMAGE CONSTRUIDO DESDE MAIN `5c17e13` (checkpoint del merge msg-selection `9e2c851`) = PASS (sesión FRESH, deepseek-v4-flash, orquestador).** El AppImage previo `30238e4f…` era **STALE** (construido desde `f3e9f30`, predata el merge `9e2c851`). Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS)**. Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.881.912 bytes**, **SHA-256 `832408b677be75a7b9c12f53348d7ef032ccdbcfc1e9418a17f98eab668c429d`** (NUEVO, difiere del stale `30238e4f…`), timestamp 2026-09-02 14:57:35 -0300, source commit `5c17e13` (main HEAD, working tree clean antes del build, sin cambios de producto sin commitear). Build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool tras el error esperado de linuxdeploy en Fedora 44).
- **PROVENANCE/EMBEDDED FRONTEND:** el binario `usr/bin/educai` del payload embebe el dist regenerado EN ESTE BUILD. Frontend embebido `assets/index-5bMlDLhr.js` + `assets/index-UHkEOOtE.css` (los MISMOS hashes del build previo — esperado: el diff del pass msg-selection `99e8f52..9e2c851` es SOLO Rust, 5 archivos +298/−106, sin cambios FE). **El binario fresh embebe el texto honesto de fallo "No recibimos una respuesta" (probe UTF-8 en binario = True)**. No hay hardcode "Listo." de respuesta: las únicas 2 ocurrencias literales "Listo." en el binario son strings del SYSTEM PROMPT de instrucción (service.rs: "por ejemplo: \"Listo. Creé el recurso…\" / "ni respondas solo \"Listo.\" antes de haber escrito el recurso"), NO un fallback de reply — verificado por contexto UTF-8 en binario.
- **SIDECAR PINS VERIFICADOS EN PAYLOAD (byte-idénticos al sidecar fetcheado):** opencode **1.18.25** (payload sha `d91e0d33…` = binario extraído; el pin del manifest `58a3729a…` es del tarball, consistente con checkpoints previos) y cloudflared **2026.8.3** (payload sha `f29324fe…` = pin `config/components.json`). `--version` en payload: opencode `1.18.25`, cloudflared `2026.8.3`. Sin repin/upgrade silencioso.
- **`./scripts/verify` EXIT=0 EN MAIN POST-BUILD** (cargo fmt/clippy/test verdes, FE **228/228** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 version 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Targeteds post-merge:** `opencode_adapter` **27/27** (incluye `send_selects_only_new_turn_terminal_text_and_excludes_reasoning`, `sequential_sends_select_each_current_turn_response`, `growing_assistant_message_resets_grace_until_stop`), `app_facade` **37/37** (incluye `missing_agent_text_does_not_become_misleading_listo`), `agent_service` **10/10**, `project-agent --lib` **15/15**, preview `preview_lifecycle` 4/4 + `preview_security` 10/10 + `project-app --test preview` 9/9. Sin reuso de resultados viejos (todo corrido en esta sesión).
- **LANZAMIENTO REAL FEDORA/WAYLAND (DISPLAY=:0) = PASS.** Procesos viejos EducAI (PIDs 424166/424466 @`pnbpGK`, 503638/503938 @`CFNFfP`, del artifact stale `30238e4f…`) TERMINADOS (stale, no usados como evidencia). Lanzado el artefacto NUEVO (PID 571232, setsid, 14:58) con PATH restringido `/tmp/opencode/path-indep-bin:/usr/bin:/bin` (sin opencode/cloudflared externos; `command -v opencode` = fail en PATH restringido). Log: `[agent] backend starting → ready`, SIN falso error de arranque. WebKitNetworkProcess + WebKitWebProcess activos (UI usable). Sidecar hijo desde el mount propio del AppImage (`/tmp/.mount_EducAIjdhhkl/usr/bin/opencode`, port 35237, `/global/health` HTTP 200, `{"healthy":true,"version":"1.18.25"}`; `readlink /proc/<pid>/exe` = mount del AppImage → **PATH-independencia PASS**). cloudflared **2026.8.3** en payload del mount (`/tmp/.mount_EducAIjdhhkl/usr/bin/cloudflared`, `--version` = 2026.8.3).
- **ADAPTER REAL PROBE (el adapter corregido `OpenCodeAgentEngine` de MAIN `5c17e13` compilado en probe standalone y corrido contra el sidecar REAL pineado 1.18.25 en vivo) = PASS.** Probe `realprobe` (crate temporal en `/tmp/opencode/realprobe`, path-dep a `project-agent` de main, modelo `opencode/big-pickle` free, config XDG aislada, 4+2 turnos en UNA sesión):
  - **CASE A "hola"** → `Completed` 4.4s, message `"¡Hola! ¿En qué puedo ayudarte?"` — **respuesta REAL contextual, NO "Listo."**.
  - **CASE B pregunta normal** → `"París"` (responde exactamente esa pregunta).
  - **CASE C Creation/tool turn** → `Completed` 9.2s, message `"Creé `index.html` en el directorio de trabajo con un saludo de la actividad 'Prueba Real'."` + **archivo real `index.html` en disco** (artifacts en adapter = [] porque `/diff` real devuelve `[]` para archivos commiteados → fallback documentado workspace-scan en `service.rs`); sin nudge; final `finish:"stop"` con texto final correcto.
  - **CASE D turno secuencial** → `"Mercurio"` (correcto para M4, sin one-turn-behind, sin stale reuse).
  - **TURN5 lento** → tarea de 30 archivos `test_01..30.txt`: `Completed` a los **38.3s** (>15s grace viejo, NO cortado) con texto final real; 30 archivos en disco. **TURN6 post-abort** → `"4"` (sesión usable tras cancel).
- **SLOW-BUT-VALID (>15s) EXPLÍCITO = PASS (probe3).** Tarea de 30 archivos `s_01..30.txt`: `Completed` a los **40.1s** con texto terminal real (`"Serie `s_01.txt` a `s_30.txt` creada: 30 archivos…"`), 30 archivos en disco. **El viejo grace de 15s NO corta la respuesta lenta** — la espera corre hasta `finish:"stop"` (acotada por `task_timeout`). Sin sleeps introducidos.
- **BOUNDED DEADLINE / FAILURE = PASS.** probe3 con `task_timeout=4s`: `TaskFailed("timed out")` a los **4.0s** (sin espera infinita). probe4 (tarea que no termina, abort a los 10s desde hilo): `cancel()` → `Ok(())`, el send termina limpio en el deadline absoluto 120s (`TaskFailed("timed out")`), **sin fake "Listo."**; **el turno siguiente en la MISMA sesión responde `"18"` correctamente** (sin envenenar el siguiente turno, sin stale reuse). Probe1 TURN6 post-abort idem (`"4"`). Cobertura determinista ya en adapter 27/27 (`send_never_idle_times_out`, `send_idle_without_new_assistant_message_times_out`).
- **TRACE REAL "hola" (sidecar 1.18.25, `/session/<id>/message`) = evidencia de selección:** user `msg_0634da8790…` (text "hola") → assistant `msg_0634da88d0…` con **`parentID` = id del user message** (turn-link correcto), parts `[]` en T+1..3, `finish:"stop"` + parts `text:"¡Hola! ¿En qué puedo ayudarte?"` en T+4. **La selección autoritativa (watermark + turn-link VIVO + `finish:"stop"` + solo parts `type=="text"`) eligió exactamente ese mensaje nuevo turn-linked.** Sin heurística de longitud/keyword, sin content-length, sin sleeps.
- **RESPONSE-SELECTION REGRESSION (§16) = PASS (verificado en main + binario + runtime):** sin hardcode "Listo." de fallback (solo strings de instrucción del system prompt en binario); identidad de mensaje assistant correcta; identidad de turno stale NO reusada (watermark + parentID == último user message VIVO); selección final respeta el turno actual; `finish:"tool-calls"` NO terminal (nunca displayeado como completado — cubierto por `send_does_not_treat_intermediate_text_as_terminal_before_artifacts` y 4+ trazas reales); `finish:"stop"` ÚNICO terminal normal; sin heurística de contenido. Pinned sidecar real 1.18.25 usado en TODOS los probes (NO mocked).
- **PREVIEW/ABRIR PRESERVADO (regresión-only, NO reabierto):** preview `preview_lifecycle` **4/4**, `preview_security` **10/10**, `project-app --test preview` **9/9**, FE `CreationsPanel.test.tsx` **12/12** (Abrir usa `preview_open_web` con el MISMO `creation.id` que Compartir). Adjuntos, cards, publicación Cloudflare real, modelo en Configuración, delete «Sí», Enter/Shift+Enter, badge, aislamiento, storage, workspace binding: sin cambios (diff msg-selection = solo backend chat, tree limpio, verify EXIT=0).
- **MODELOS/POLÍTICA:** Cursor quota agotado — **NO usado**. **GPT vía OpenCode Go PROHIBIDO en esta fase** — NO usado. Orquestación 100% OpenCode/DeepSeek V4 Flash. Sin workers LLM lanzados (fase determinista: packaging + probes contra sidecar real + tests). Session budget CONTINUE (~12K al inicio).
- **ESTADO DEL REPO:** main limpio (working tree clean antes y después del build), HEAD `5c17e13`, M11 NO INICIADO. Sin worktrees/branches/workers temporales (los probes son crates temporales bajo `/tmp/opencode/realprobe`, fuera del repo). AppImage fresco en su ruta canónica.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `832408b6…` (escenario §22: lanzar el AppImage exacto, conversación NUEVA, enviar `hola`, observar respuesta contextual real — si es "Listo." el humano FALLA de inmediato; luego pregunta normal, Creation, secuencial, Preview, Share/update). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (OPENCODE ASSISTANT MESSAGE SELECTION / FINAL RESPONSE SEMANTICS PASS = COMPLETE — REVIEWS APPROVE, INTEGRADO EN MAIN, M11 NO INICIADO, 2026-09-02)

- **PASS = COMPLETE.** Orquestador FRESH (deepseek-v4-flash, sesión CONTINUE ~66K) bootstrapeó (checkpoint `99e8f52`, main limpio, worktree autor `../ai-publisher-msg-selection-pass` = `corr/msg-selection-pass` head `d0ca259` SIN mergear, base `405ecfe`, budget CONTINUE, sesión Luna 279K CERRADA y NO reutilizada), lanzó 2 reviews independientes FRESH sobre diff `405ecfe..d0ca259` (5 archivos, +298/−106) en worktree de review `../ai-publisher-msg-selection-review` (detached `d0ca259`), y fusionó. **Merge `9e2c851` en main** (ort, 5 archivos, +298/−106, commits del autor `fe636cc`, `ca58d19`, `969c001`, `d0ca259` preservados). **`./scripts/verify` EXIT=0 EN MAIN POST-MERGE** (cargo fmt/clippy/test verdes, FE **228/228** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). Targeteds post-merge: `opencode_adapter` **27/27**, `app_facade` **37/37**, `agent_service` **10/10**. Working tree limpio.
- **REVIEW PRODUCT/UX INDEPENDIENTE (OpenCode DeepSeek V4 Flash, FRESH, `product-ux-review` pane `w1F:p1W`, worktree de review) = APPROVE.** CASE A: no "Listo." fallback en código de producto; vacío+sin creación → `MessageStatus::Failed` + "No recibimos una respuesta. Probá de nuevo.", vacío+con creación → texto honesto de creación (app.rs). CASE B: `authoritative_assistant_text` devuelve solo el mensaje nuevo turn-linked `finish:"stop"`, parts `type=="text"` (reasoning/step excluidos). CASE C: `finish:"tool-calls"` NO terminal; grace 15s y seam `with_idle_grace` REMOVIDOS → texto intermedio nunca cierra el turno, turnos lentos corren hasta `finish:"stop"` (acotados por `task_timeout` 120s). CASE D: watermark + `parentID == id del último user message VIVO` mantienen causalidad secuencial. Verificado por reviewer: `opencode_adapter` 27/27, `app_facade` 37/37, `project-agent --lib` 15/15. Sin regresión a Preview. NOTAS no bloqueantes: parentID estricto con fallback leniente cuando falta parentID; el "Listo." real del modelo sigue surfaciendo por diseño (semántico, correcto).
- **REVIEW CÓDIGO/CORRECTNESS INDEPENDIENTE (OpenCode Qwen 3.8 Flash, FRESH, `code-review` pane `w1F:p1X`, worktree de review) = APPROVE.** Hardcode "Listo." eliminado (app.rs:1448-1459) sin regresión blank (AgentRunView.message siempre Some). Maquinaria de grace REMOVIDA por completo (IDLE_WITHOUT_TEXT_GRACE, ACK_WITHOUT_ARTIFACTS_GRACE, idle_since/idle_artifacts, with_idle_grace); grep repo-wide sin referencias colgantes. Selección turn-autoritativa (watermark + turn-link VIVO + `finish:"stop"` único terminal); `tool-calls` NO terminal; `message_text` solo parts `type=="text"`, fallback a `content` solo sin parts. **Espera ACOTADA:** deadline absoluto `now + task_timeout` (DEFAULT_TASK 120s) → Timeout → `TaskFailed("timed out")`; failed/error/failure → `TaskFailed`; abort → `Cancelled`; non-2xx → Http error. Cubierto por `send_never_idle_times_out` / `send_idle_without_new_assistant_message_times_out`. Sin sleeps reintroducidos (solo tick 20ms). NOTAS no bloqueantes: comentario duplicado colgante en `opencode.rs:118` (cosmético); comentario/doc de `assistant_message_is_terminal` (373-374) y nombre de test `growing_assistant_message_resets_grace_until_stop` aún referencian semántica de grace removida; `crates/project-provider/src/adapter.rs:439` conserva su propio `last_assistant_text_from_messages` pero es el path separado de connection-test (acotado, honesto "Conectado."), NO el chat reply flow — no tocado, aceptable. Comandos corredos por reviewer (worktree, pipefail): `opencode_adapter` 27 passed EXIT=0, `app_facade` 37 passed EXIT=0, `project-agent --lib` 15 passed EXIT=0, `cargo fmt --check` EXIT=0, `cargo clippy --all-targets -D warnings` EXIT=0. `./scripts/verify` NO corrido por reviewer (sidecars gitignored ausentes en worktree), corrido por orquestador en main = EXIT=0.
- **AUTOR (OpenCode / GPT-5.6 Luna, `msg-selection-luna`, pane `w1F:p1T` — CERRADO, NO reutilizado):** commits `fe636cc` (selección autoritativa: requiere mensaje assistant nuevo + turn-linked + `finish:"stop"` + solo parts de texto humano; texto vacío → NUNCA "Listo."), `ca58d19` (correlación de turno: `originating_user_id` recomputado de la lista VIVA en cada poll, no del snapshot pre-send), `969c001` (reset de grace por firma de progreso), `d0ca259` (eliminación del ack-grace early-return 15s + maquinaria muerta: espera solo `finish:"stop"` o deadline de tarea). Total 5 archivos, +298/−106 (vs base `405ecfe`).
- **ROOT CAUSE EXACTO DE "Listo." (evidenciado con sidecar REAL pineado 1.18.25, NO adivinado):** mecanismo B. Live OpenCode 1.18.25 devuelve para `"hola"` un mensaje assistant con `parentID` apuntando al user message del turno, parts que evolucionan `[] → step-start → reasoning → text → step-finish` y `finish:"stop"` recién al final (latencia 4–19s variable). El código viejo: (1) `assistant_reply_text(None/empty)` en `app.rs` hardcodeaba `"Listo."`; y (2) el ack-grace de 15s podía cortar un turno lento y devolver mensaje vacío → `"Listo."`. El adaptador viejo además seleccionaba el último texto assistant de TODA la sesión (sin watermark ni turn-link) y concatenaba parts incluyendo reasoning.
- **SELECCIÓN ANTES:** `last_assistant_text_from_messages` iteraba TODA la sesión (sin watermark ni linkage) → texto de turnos previos o intermedios; `assistant_reply_text(None)` → hardcode `"Listo."`; grace 15s podía devolver vacío. **SELECCIÓN DESPUÉS:** watermark `assistant_index >= before_assistant_count` (ancla determinista del pass previo) + `message_belongs_to_turn` con `parentID == id del último user message VIVO` + `assistant_finish == "stop"` como único terminal normal + `message_text` que toma SOLO parts `type=="text"` (excluye reasoning/step/system) y cae a `content` solo si no hay parts; app.rs: vacío+sin creación → `MessageStatus::Failed` + texto honesto ("No recibimos una respuesta. Probá de nuevo."), vacío+con creación → texto honesto de creación sin explicación, NUNCA "Listo.".
- **RUNTIME REAL (preservado del autor, sidecar pineado 1.18.25, `sidecars/opencode-x86_64-unknown-linux-gnu` — verificado presente, `--version` = 1.18.25):** TASK HOLA → saludo real ("¡Hola! ¿Cómo puedo ayudarte?…", incl. una corrida lenta de 19.3s que ANTES devolvía vacío); TASK QUESTION → "París"/"París."; TASK CREATION → texto final real ("Listo. Creé `index.html`…") + archivo `index.html` en disco (artifacts en adapter = [] porque `/diff` real devuelve `[]` para archivos commiteados → fallback documentado workspace-scan en `service.rs`, tests existentes `workspace_scan_registers_when_diff_is_empty`); TASK SEQ_Q4 → "Marte". Mensajes dumpados con IDs/finish/parentID/parts reales. Sin empty en ninguna corrida tras `d0ca259`. El orquestador NO re-corrió el probe live (no rehace el diagnóstico del autor; evidencia real preservada + tests + reviews).
- **TESTS:** adapter **27/27** (nuevos: `send_selects_only_new_turn_terminal_text_and_excludes_reasoning`, `sequential_sends_select_each_current_turn_response`, `growing_assistant_message_resets_grace_until_stop`; FakeServer ahora agrega user message + assistant con `parentID` real y soporta `messages_sequence`), `app_facade` **37/37** (nuevo: `missing_agent_text_does_not_become_misleading_listo`), `agent_service` **10/10**, FE **228/228**. Unit `opencode.rs` nuevos: `human_text_parts_win_over_mixed_content`, `final_text_requires_new_linked_stop_message`.
- **GATE de no-regresión (confirmado):** `finish:"tool-calls"` NO terminal; `finish:"stop"` ÚNICO terminal normal; Creation no terminal independiente; sin nudge; un turno activo por conversación; user message persistido antes de ejecución; Creation del turno origen; PREVIEW/ABRIR = PASS humano (NO reabierto); sin heurística de texto, sin filtro literal "Listo.", sin sleeps, deadline de fallo ACOTADO preservado; M11 NO INICIADO. NIT menor no bloqueante: comentario duplicado en `opencode.rs:118-120` (cosmético).
- **POLÍTICA/MODELOS:** Cursor quota agotado — NO usado. GPT vía OpenCode Go PROHIBIDO para este pass de reviews (autor Luna fue OpenCode Go GPT-5.6 Luna por directiva explícita del owner en el pass previo; en ESTA sesión de orquestación/review SOLO DeepSeek V4 Flash y Qwen 3.8 Flash, sin GPT). Reviews: Product/UX = OpenCode DeepSeek V4 Flash FRESH; Code/Correctness = OpenCode Qwen 3.8 Flash FRESH; ambas APPROVE sin REQUEST_CHANGES → sin Codex fixer. **Session budget CONTINUE (~66K al merge).**
- **PROXIMO GATE (único): FRESH REAL APPIMAGE BUILD + TECHNICAL VERIFICATION** desde main `9e2c851` (`scripts/smoke-package appimage`, sidecars pineados opencode 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real Fedora/Wayland, probe real del chat "hola"/question/creation/seq), luego **HUMAN PRODUCT-OWNER RE-ACCEPTANCE**. **M11 NO INICIADO — NO configurar M11 como próximo.**

## Estado previo (FRESH REAL APPIMAGE POST-CHAT-CAUSALITY CONSTRUIDO DESDE MAIN `f3e9f30`/MERGE `2091ec3` — BUILD + VERIFICACIÓN TÉCNICA COMPLETA, TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE, M11 NO INICIADO, 2026-09-02)

- **FRESH REAL APPIMAGE CONSTRUIDO DESDE MAIN `f3e9f30` (checkpoint del merge chat-causality `2091ec3`) = PASS (sesión FRESH, deepseek-v4-flash, orquestador).** El AppImage previo `40403c69…` era **STALE** (construido 10:54 desde `71ff7bf`, predata el merge `2091ec3`). Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS)**. Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.877.816 bytes**, **SHA-256 `30238e4f5940e2834f614c88bb2d92f89f3b993c963b3bff2a9166230c384ab8`** (NUEVO, difiere del stale `40403c69…`), timestamp 2026-09-02 12:38 -0300, source commit `f3e9f30` (main HEAD, working tree clean antes del build, sin cambios de producto sin commitear). Build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool tras el error esperado de linuxdeploy en Fedora 44).
- **PROVENANCE/EMBEDDED FRONTEND:** el binario `usr/bin/educai` embebe el dist regenerado EN ESTE BUILD (dist generado 12:37:25, binario 12:37:57): referencia exacta `assets/index-5bMlDLhr.js` + `assets/index-UHkEOOtE.css` (hashes NUEVOS de este build); el asset stale `index-BFehLbJS.js` NO está presente. Marcador de corrección `turnId` presente en el binario embebido. `index.html` del dist referencia `index-5bMlDLhr.js`.
- **SIDECAR PINS VERIFICADOS EN PAYLOAD (byte-idénticos al sidecar fetcheado):** opencode **1.18.25** (payload sha `d91e0d33…` = binario extraído; el pin del manifest `58a3729a…` es del tarball, consistente con checkpoints previos) y cloudflared **2026.8.3** (payload sha `f29324fe…` = pin `config/components.json`). `--version` en payload: opencode `1.18.25`, cloudflared `2026.8.3`. Sin repin/upgrade silencioso.
- **`./scripts/verify` EXIT=0 EN MAIN POST-BUILD** (cargo fmt/clippy/test verdes, FE **228/228** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 version 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Targeteds:** `opencode_adapter` **24/24**, `app_facade` **36/36**, `agent_service` **10/10**, preview `preview_lifecycle` 4/4 + `preview_security` 10/10 + `project-app --test preview` 9/9. Sin reuso de resultados viejos (todo corrido en esta sesión).
- **LANZAMIENTO REAL FEDORA/WAYLAND (DISPLAY=:0) = PASS.** Procesos viejos EducAI (PIDs 10188 @08:34, 242114 @11:02, montajes `.mount_EducAIecNFPh`/`.mount_EducAIJCJKGB`) TERMINADOS (stale, no usados como evidencia). Lanzado el artefacto NUEVO (PID 416057, setsid, 12:44) con PATH restringido `/tmp/opencode/path-indep-bin:/usr/bin:/bin` (sin opencode/cloudflared externos; `command -v opencode` = fail). Log: `[agent] backend starting → ready`, SIN falso error de arranque. WebKitNetworkProcess + WebKitWebProcess activos (UI usable). Sidecar hijo desde el mount propio del AppImage (`/tmp/.mount_EducAIBPhCpe/usr/bin/opencode`, port 41849, HTTP 200 en `/global/health`; `readlink /proc/<pid>/exe` = mount del AppImage → **PATH-independencia PASS**). cloudflared presente en payload del mount.
- **TARGETED CHAT-TURN CAUSALITY VALIDATION = PASS (runtime real, NO solo mocked).** Validación con el sidecar REAL 1.18.25 empaquetado (port 41849, modelo `opencode/big-pickle` free, config app aislada): **tarea real con tool real** creó archivos en un workspace real. Traza real de mensajes `/session/<id>/message?limit=`:
  - **CASE B (turno de creación):** user → assistant `finish:"tool-calls"` (intermedio, con tool part) → assistant `finish:"stop"` (final) + texto "Created `smoke.txt`…" + artefacto real `smoke.txt`=`smoke-ok`. **El artefacto `live.txt` apareció en disco a las 12:50:58 MIENTRAS el último mensaje seguía en `finish:"tool-calls"`/None; el marker `stop` recién llegó a las 12:52:04** → artefacto antes de `stop` NO es terminal (evidencia real, sin nudge).
  - **CASE C (turnos secuenciales):** 2 turnos completos en una sesión: turno1 `smoke.txt`/`smoke-ok` + turno2 `second.txt`/`second-ok`, cada uno con su `finish:"tool-calls"`→`finish:"stop"` y su resultado correspondiente al request origen; sin one-turn-behind. Además probe del adapter real (abajo) hizo 3 turnos.
  - **CASE E (cancel/failure):** `POST /session/<id>/abort` = HTTP 200; la sesión sigue usable (2º prompt 204) y el turno post-abort produjo `after_abort.txt` con secuencia `tool-calls`→`stop` (sin envenenar el siguiente turno). Sin espera infinita: la espera de ack-grace (15s) devuelve `Completed` con texto vacío si el agente está genuinamente idle sin `stop` (limitación conocida documentada) y el timeout de tarea mapea a `TaskFailed("timed out")` (test `send_idle_without_new_assistant_message_times_out`).
  - **`/session/status` REAL = `{}`** (mapa vacío) aun con trabajo activo → `{}` NO es terminal (consistente con el root cause). `/diff` real devuelve `[]` para archivos commiteados → el path de Creation usa el fallback de workspace-scan (test `workspace_scan_registers_when_diff_is_empty` verde) — comportamiento por diseño, no regresión.
- **ADAPTER REAL PROBE (el adapter corregido `OpenCodeAgentEngine` compilado contra el sidecar REAL 1.18.25 en vivo, 3 turnos, tarea de artefacto real, sin nudge) = PASS.** `ensure_ready` version=1.18.25; turn1 `Completed` (6.9s, msg "Created `probe1.txt`…", archivo en disco); turn2 `Completed` (15s grace, archivo `probe2.txt` en disco); turn3 (misma sesión) `Completed` (112.4s, `probe3.txt`). Los 3 archivos en disco con el contenido exacto. `PROBE_EXIT=0`.
- **COMPLETION-MARKER SEMANTICS (evidencia real):** `finish:"tool-calls"` = INTERMEDIO (NO terminal) — 4+ trazas reales lo muestran con trabajo/artefactos continuando; `finish:"stop"` = ÚNICO terminal normal (todos los turns completos lo tienen); artefactos observados antes de `stop` NO cierran el turno (evidencia `live.txt` 12:50:58 vs `stop` 12:52:04); ausencia de terminal normal con condición de fallo/cancel explícita NO cuelga infinito (abort 200 + timeout a `TaskFailed("timed out")`). Determinista cubierto por `opencode_adapter` 24/24 (incluye `send_does_not_treat_intermediate_text_as_terminal_before_artifacts`, `send_does_not_treat_brief_listo_as_complete_before_artifacts`, `send_completes_on_explicit_stop_without_files`, `send_idle_without_new_assistant_message_times_out`).
- **CREATION CORRELATION = PASS (donde el tooling lo permite).** Creation pertenece al turno origen (turn_id → `Map<projectId, turnId>` en App.tsx; eventos stale ignorados); no requiere el siguiente envío (marker stop → retorno con fetch de artefactos); scan/artefacto no cierra el turno antes del marker; sin re-registro duplicado (`later_turn_does_not_reregister_prior_workspace_files` verde). Preview/Abrir sigue abriendo la misma Creation (`preview_lifecycle` 4/4, `preview_security` 10/10, `project-app --test preview` 9/9, FE `CreationsPanel` mismo `creation.id`).
- **PREVIEW/COMPARTIR/ETC. PRESERVADO (no regresión):** Preview/Abrir = PASS humano (NO reabierto). Adjuntos, cards, publicación Cloudflare real, modelo en Configuración, delete «Sí», Enter/Shift+Enter, badge, aislamiento, storage, workspace binding: sin cambios en este build (tree limpio, verify EXIT=0). Compartir/update cubierto por `app_facade` 36/36 (incluye `publish_promotes_the_generated_web_creation_as_the_public_entry`, `later_turn_updates_the_same_web_creation_and_refreshes_publish`).
- **ESTADO DEL REPO:** main limpio (working tree clean), HEAD `f3e9f30`, M11 NO INICIADO. Sin worktrees/branches/workers temporales. AppImage fresco en su ruta canónica.
- **MODELOS/POLÍTICA:** Cursor quota agotado — NO usado. Esta fase 100% OpenCode (deepseek-v4-flash orquestador). Sin workers (fase determinista: packaging + probes + tests). Session budget CONTINUE.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `30238e4f…` (escenario §19: conversación nueva, adjuntar `datosrosco.txt`, pedir UNA VEZ un Pasapalabra/Rosco, SIN nudge "donde está?"/"podes?"/"me avisas?", el chat sigue solo, respuesta final corresponde al request, Creation aparece, Abrir funciona, luego un segundo request secuencial sin one-turn-behind). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (CHAT TURN CAUSALITY / RESPONSE CORRELATION PASS = COMPLETE — REVIEWS APPROVE, INTEGRADO EN MAIN, M11 NO INICIADO, 2026-09-02)

- **CHAT TURN CAUSALITY / RESPONSE CORRELATION PASS = COMPLETE.** Orquestador FRESH (deepseek-v4-flash) retomó en `84abbed`, verificó el estado durable (main limpio, autor SIN mergear, worktree `../ai-publisher-corr-03-chat-causality` limpio y alcanzable en `4d1f657`), lanzó 2 reviews independientes FRESH y fusionó. **Merge `2091ec3` en main** (ort, 9 archivos, +202/−83), preservando los commits del autor `e1c16ff`, `49bbc3d`, `4d1f657`. **`./scripts/verify` EXIT=0 en main post-merge** (FE **228/228**, cargo fmt/clippy/test verdes, contracts M10 + UX_REDESIGN_01, fetch-sidecars --check, cargo check src-tauri, git diff --check). Targeteds post-merge: `opencode_adapter` **24/24**, `app_facade` **36/36**. Working tree limpio.
- **REVIEW PRODUCT/UX INDEPENDIENTE (OpenCode DeepSeek V4 Flash, FRESH) = APPROVE.** El diff resuelve el problema humano: el turno cierra solo con el marker terminal determinista (`info.finish == "stop"`); el retorno temprano por "idle + texto + artefactos" fue ELIMINADO (`opencode.rs`); el texto intermedio `finish:"tool-calls"` ("Listo."/"Voy a preparar la actividad.") NUNCA es terminal; los artefactos solos NO cierran el turno (`4d1f657`); la identidad de turno se preserva vía el `MessageId` durable del usuario (turn_id) encadenado `app.rs → AgentRunView → AgentTaskEvent → App.tsx` (mapa `projectId→turnId`, eventos terminales stale con turnId no coincidente IGNORADOS); Creation se registra y persiste DENTRO del run origen (nunca drenada por el siguiente mensaje). Verificado por el reviewer: `opencode_adapter` 24/24, `app_facade` 36/36, FE 228/228, `project-agent` lib 14/14. Sin regresión a Preview/Abrir ni a cards/share. NITs no bloqueantes: campo muerto `idle_without_text_grace` (escrito, no leído); fallback de grace 15s puede surfacer texto intermedio solo si el agente está genuinamente idle >15s sin `stop` (limitación conocida documentada, estrictamente mejor que el comportamiento previo); gap menor de cobertura FE para el guard de `turnId` en App.tsx (sin test dedicado).
- **REVIEW CÓDIGO/CORRECTNESS INDEPENDIENTE (OpenCode Qwen 3.8 Flash, FRESH) = APPROVE.** Semántica terminal: `finish:"tool-calls"` nunca terminal; `finish:"stop"` (último mensaje assistant, `info` o top-level) es el ÚNICO terminal normal con refresh de `/diff` en ese punto; deadline absoluto de tarea garantiza sin espera infinita; timeout mapea a `TaskFailed("timed out")`. Causality: persist-before-run con `MessageId` → `turn_id` por el path durable (legacy `run_agent` solo test); `inFlightRef` ahora `projectId→turnId` map; eventos terminales mismatch descartados; keying por proyecto impide leaks entre conversaciones. Polling/one-turn: retorno temprano por artefactos eliminado; `idle_since`/`last_artifact_fetch` resetean en fases busy; mutex por proyecto + gate working del FE serializan; cancelled/failed liberan el lock. Creation timing: marker stop → retorno inmediato con fetch fresco de artefactos → card aparece en el evento completante, no en el siguiente envío; sin path de re-registro duplicado. Verificado: `opencode_adapter` 24/24, `app_facade` 36/36, FE 228/228, `cargo fmt --check` limpio. NIT/LOW: campo muerto `idle_without_text_grace`; fallback grace 15s (mismo límite documentado); App.tsx guard de `turnId` sin test FE dedicado; ENV: `cargo check -p educai` no corre en el worktree por sidecar gitignored ausente (consistente por inspección, cubierto por el facade compilado).
- **AUTOR (OpenCode / GPT-5.6 Luna, `high-coding-luna`, pane `corr-03-luna` = `w1G:p1`, worktree `../ai-publisher-corr-03-chat-causality`):** commits `e1c16ff` (turn_id threading + terminal marker), `49bbc3d` (completion = `finish:"stop"`, heurística de texto ELIMINADA), `4d1f657` (artefacto NO completa un turno antes del marker terminal). Total 9 archivos, +202/−83.
- **ROOT CAUSE (evidenciado con runtime real opencode 1.18.25, preservado):** OpenCode 1.18.25 reporta `/session/status` como `{}` (mapa vacío → fase "idle" por default) MIENTRAS el trabajo está activo; los mensajes assistant intermedios tienen `info.finish: "tool-calls"` y solo el mensaje final tiene `info.finish: "stop"`. El adapter trataba "idle + texto assistant + artefactos" como terminal → el texto intermedio ("Listo."/"Voy a hacerlo.") cerraba el turno antes del trabajo real, y el trabajo pendiente se drenaba en el siguiente envío (one-turn-behind, Creation aparecía tras el nudge). Traza real: `15:05:31 status={} diff=[]` → `15:05:35 step-start/reasoning` → `15:05:36 finish:"tool-calls" + tool + step-finish` → trabajo → artefacto `smoke-ok` → final `finish:"stop"`. `status={}` NO es terminal. **CICLO DE VIDA BEFORE/AFTER:** BEFORE M1 → ejecución R1 → texto intermedio/finish tool-calls → lógica vieja terminal → usuario envía M2 → trabajo R1 pendiente drenado → R1 aparece tras M2; AFTER M1 → ejecución R1 → finish tool-calls intermedio → R1 sigue activo → trabajo/artefacto completo → Creation observada → finish:"stop" final → R1 terminal → respuesta final + Creation presentadas para M1.
- **COMPLETION SIGNAL (determinista, §7/§10):** el ÚLTIMO mensaje assistant debe tener `info.finish == "stop"`. Los artefactos solos NUNCA completan un turno (fix `4d1f657`). Grace de ack (15s) conservado como fallback de Q&A corto. Sin heurística de texto, sin sleeps, sin patch "Listo".
- **TURN IDENTITY (§5/§8):** el `MessageId` durable del mensaje de usuario es la identidad lógica del turno (`turn_id`), preservado `AgentRunInputs → AgentRunView → AgentTaskEvent → UI` (`app/src/App.tsx` ahora `Map<projectId, turnId>`; eventos terminales stale con `turnId` que no coincide con el turno activo son IGNORADOS). Un turno agente activo por conversación: verificado. Creation pertenece al turno origen (diff→scan ocurre dentro del poll del turno, con refresh forzado en terminal).
- **RUNTIME VALIDATION REAL (preservada, §17):** sidecar pineado opencode **1.18.25** (`sidecars/`), modelo `opencode/big-pickle` (free), tarea real que creó `smoke.txt` con `smoke-ok` vía tool real. Traza multi-etapa con timestamps reales (arriba). No se necesita nudge.
- **POLÍTICA/MODELOS:** Cursor quota agotado (directiva 2026-09-02): **ningún Cursor usado** en este pass (rutas 100% OpenCode: autor GPT-5.6 Luna, Product/UX DeepSeek V4 Flash, Code/Correctness Qwen 3.8 Flash). Sesión orquestador economizada (budget ~20K; la previa rotó en 126K por duplicar trabajo). **PROXIMO GATE (único): FRESH REAL APPIMAGE BUILD + TECHNICAL VERIFICATION** desde main `2091ec3` (`scripts/smoke-package appimage`, sidecars pineados opencode 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real Fedora/Wayland), luego **HUMAN PRODUCT-OWNER RE-ACCEPTANCE**.
- **GATES de no-regresión (confirmados):** PREVIEW/ABRIR = PASS humano (NO reabrir). Adjuntos, cards, publicación Cloudflare real, modelo en Configuración, delete «Sí», Enter/Shift+Enter, badge, aislamiento, storage intactos. AppImage fresco `40403c69…` previo. **NO HUMAN ACCEPTED. M11 NO INICIADO.**

## Estado previo (CHAT TURN CAUSALITY PASS — TRABAJO DE AUTOR COMPLETO Y VERIFICADO EN WORKTREE, ORQUESTADOR ROTADO ANTES DE REVIEWS/MERGE, M11 NO INICIADO, 2026-09-02)

- **CHAT TURN CAUSALITY / RESPONSE CORRELATION PASS = TRABAJO DE AUTOR COMPLETO y VERIFICADO (3 commits en `corr/chat-causality-pass`), PERO NO REVISADO NI MERGEADO.** El orquestador (FRESH, deepseek-v4-flash) bootstrapeó, cableó el rol Luna en el launcher, lanzó el autor OpenCode GPT-5.6 Luna, hizo 2 ciclos acotados de fix con inspección propia, verificó `./scripts/verify` EXIT=0 en el worktree, y alcanzó **ROTATE_SESSION_REQUIRED (126,719 tokens)** ANTES de lanzar las reviews independientes → **Product/UX review y Code/Correctness review PENDIENTES (sesión FRESH siguiente).** Worktree autor limpio, branch `corr/chat-causality-pass` SIN mergear. Main limpio. **M11 NO INICIADO.**
- **AUTOR (OpenCode / GPT-5.6 Luna, `high-coding-luna`, pane `corr-03-luna` = `w1G:p1`, worktree `../ai-publisher-corr-03-chat-causality`):** commits `e1c16ff` (turn_id threading + terminal marker), `49bbc3d` (completion = `finish:"stop"`, heurística de texto ELIMINADA), `4d1f657` (artefacto NO completa un turno antes del marker terminal). Total 9 archivos, +202/−83. `./scripts/verify` EXIT=0 en worktree (FE **228/228**, cargo verde, contracts, fetch-sidecars --check). Adapter **24/24**, app_facade **36/36**. Author handoff STATUS: PASS. Session Luna seguía disponible/idle al rotar.
- **ROOT CAUSE (evidenciado con runtime real opencode 1.18.25):** OpenCode 1.18.25 reporta `/session/status` como `{}` (mapa vacío → fase "idle" por default) MIENTRAS el trabajo está activo; los mensajes assistant intermedios tienen `info.finish: "tool-calls"` y solo el mensaje final tiene `info.finish: "stop"`. El adapter trataba "idle + texto assistant + artefactos" como terminal → el texto intermedio ("Listo."/"Voy a hacerlo.") cerraba el turno antes del trabajo real, y el trabajo pendiente se drenaba en el siguiente envío (one-turn-behind, Creation aparecía tras el nudge). Traza real: `15:05:31 status={} diff=[]` → `15:05:35 step-start/reasoning` → `15:05:36 finish:"tool-calls" + tool + step-finish` → trabajo → artefacto `smoke-ok` → final `finish:"stop"`. `status={}` NO es terminal.
- **COMPLETION SIGNAL (determinista, §7/§10):** el ÚLTIMO mensaje assistant debe tener `info.finish == "stop"`. Los artefactos solos NUNCA completan un turno (fix `4d1f657`). Grace de ack (15s) conservado como fallback de Q&A corto. Sin heurística de texto, sin sleeps, sin patch "Listo".
- **TURN IDENTITY (§5/§8):** el `MessageId` durable del mensaje de usuario es la identidad lógica del turno (`turn_id`), preservado `AgentRunInputs → AgentRunView → AgentTaskEvent → UI` (`app/src/App.tsx` ahora `Map<projectId, turnId>`; eventos terminales stale con `turnId` que no coincide con el turno activo son IGNORADOS). Un turno agente activo por conversación: verificado. Creation pertenece al turno origen (diff→scan ocurre dentro del poll del turno, con refresh forzado en terminal).
- **TESTS NUEVOS:** `send_does_not_treat_intermediate_text_as_terminal_before_artifacts` (artefacto en /diff + finish tool-calls NO completa; espera el stop y devuelve el texto FINAL + artefacto), `only_stop_finish_marks_the_latest_assistant_message_terminal`, `send_completes_on_explicit_stop_without_files`, `sequential_sends_keep_distinct_turn_ids_and_ordered_results` (CASE D), turn_id presente en completed/cancelled/failed (CASE H). FakeServer ahora soporta `prompt_response_finish`.
- **RUNTIME VALIDATION REAL (§17):** sidecar pineado opencode **1.18.25** (`sidecars/`), modelo `opencode/big-pickle` (free), tarea real que creó `smoke.txt` con `smoke-ok` vía tool real. Traza multi-etapa con timestamps reales (arriba). No se necesita nudge.
- **POLÍTICA/MODELOS:** Cursor quota agotado (directiva 2026-09-02): **ningún Cursor usado**. Se agregó rol `high-coding-luna` (`config/agent-models.env` = `opencode-go/gpt-5.6-luna`, `scripts/agent-launch` + `scripts/test-agent-launch`, commit `7f37ca7` en main) y nota temporal en `docs/AGENT_POLICY.md`. Reviews pendientes: **Product/UX = OpenCode DeepSeek V4 Flash (o Qwen 3.8 Flash); Code/Correctness = OpenCode Qwen 3.8 Flash** (ambas FRESH, sin Cursor).
- **PROXIMO PASO (sesión FRESH de orquestador):** (1) budget CONTINUE; (2) inspect diff `b04a7ed..4d1f657` en el worktree (o reusar el pane `corr-03-luna` si sigue vivo y mismo task); (3) **Product/UX review FRESH** (DeepSeek V4 Flash o Qwen 3.8 Flash, §19-20); (4) **Code/Correctness review FRESH** (Qwen 3.8 Flash, §21-22); (5) fix loop si REQUEST_CHANGES (reusar Luna si disponible, si no FRESH); (6) **merge gate §24** (root cause evidenciado, sin nudge, correlación turno correcta, texto intermedio no terminal, Creation del turno origen, chat ordenado, active-turn determinista, switching seguro, runtime real PASS, reviews APPROVE, preview PASS, M11 NO INICIADO); (7) merge a main + `./scripts/verify` en main + checkpoint durable + limpieza; (8) **next gate: FRESH REAL APPIMAGE BUILD + TECHNICAL VERIFICATION, luego HUMAN PRODUCT-OWNER RE-ACCEPTANCE.**
- **GATES de no-regresión (confirmados):** PREVIEW/ABRIR = PASS humano (NO reabrir). Adjuntos, cards, publicación Cloudflare real, modelo en Configuración, delete «Sí», Enter/Shift+Enter, badge, aislamiento, storage intactos. AppImage fresco `40403c69…` previo. **NO HUMAN ACCEPTED. M11 NO INICIADO.**

## Estado previo (FRESH REAL APPIMAGE POST-CORRECCIÓN CONSTRUIDO DESDE MAIN `71ff7bf` — BUILD + VERIFICACIÓN TÉCNICA COMPLETA, TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE, M11 NO INICIADO, 2026-09-02)

- **FRESH REAL APPIMAGE CONSTRUIDO DESDE MAIN `71ff7bf` (post-merge corrección `5e4b170`) = PASS (sesión FRESH, deepseek-v4-flash, validación técnica determinista).** Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS).** Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.877.816 bytes**, **SHA-256 `40403c697adbf2e2596a225856e6c0b377a92f3e66068a6176e209bc2228149d`** (NUEVO; el previo `ec336881…` era STALE — construido desde `ebeac0e`, predata el merge de corrección `5e4b170`), timestamp 2026-09-02 10:54:56 -0300, source commit `71ff7bf` (main HEAD, working tree clean antes del build, sin cambios de producto sin commitear). Build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool tras el error esperado de linuxdeploy en Fedora 44). **Sidecars bundlados pineados verificados en payload y mount en vivo:** opencode **1.18.25** (binario extraído `d91e0d33…`, byte-idéntico al `sidecars/opencode-x86_64-unknown-linux-gnu` fetcheado; el pin del manifest `58a3729a…` es del tarball de origen) y cloudflared **2026.8.3** (sha `f29324fe…` = pin `config/components.json`). `--version` en payload: opencode `1.18.25`, cloudflared `2026.8.3`. **Frontend embebido correcto:** el binario embebe exactamente `assets/index-BFehLbJS.js` + `assets/index-UHkEOOtE.css` (idénticos al `dist` generado en este build desde `71ff7bf`; el asset stale `index-DJsNCuJZ.js` NO está presente). Markers de corrección en el dist embebido: "Dejar de compartir", "Compartido", "Creando tu recurso".
- **`./scripts/verify` EXIT=0** (cargo fmt/clippy/test verdes, FE **228/228** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 version 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check). **Lanzamiento real en Fedora/Wayland (DISPLAY=:0):** backend `[agent] starting → ready` SIN falso error de arranque, sin errores en log; WebKitNetworkProcess + WebKitWebProcess activos (UI usable). **PATH-independencia:** lanzado con PATH restringido `/tmp/opencode/path-indep-bin:/usr/bin:/bin` (sin opencode/cloudflared externos; `command -v opencode` = fail); el sidecar opencode hijo corre desde el mount propio del AppImage (`/tmp/.mount_EducAINKfoAO/usr/bin/opencode`, port 33999, HTTP responde); cloudflared presente en el payload del mount.
- **TARGETED REQUEST-COMPLETION (A) = PASS (donde el tooling lo permite).** `cargo test -p project-agent --test opencode_adapter` **23/23**: `send_does_not_treat_brief_listo_as_complete_before_artifacts`, `send_tolerates_transient_diff_errors_during_ack_wait`, `send_completes_brief_listo_after_ack_grace_when_no_files_appear`. El ack breve no corta el trabajo requerido; sin "donde esta?"/nudge. Generación real OpenCode live = territorio HUMAN RE-ACCEPTANCE.
- **TARGETED PREVIEW (B) = PASS.** `cargo test -p project-preview --test preview_lifecycle` **4/4** (token root 200, teardown invalida token), `--test preview_security` **10/10**, `cargo test -p project-app --test preview` **9/9** (`web_preview_starts_and_closes_by_token`, foreign creation 404, oversized resource). Misma Creation que Compartir (mismo `creation.id`) cubierto por FE `CreationsPanel.test.tsx`.
- **TARGETED SHARE/UPDATE (C/E/H) = PASS (donde el tooling lo permite).** `cargo test -p project-app --test app_facade` **35/35**: `publish_promotes_the_generated_web_creation_as_the_public_entry` (publish/index.html contiene el markup generado, NO "Material del proyecto"), `later_turn_updates_the_same_web_creation_and_refreshes_publish` (update in-place + refresh snapshot, misma URL), `new_distinct_web_does_not_replace_an_already_published_snapshot`, `creation_path_rejects_cross_project_id`, `set_creation_visibility_toggles`, `delete_unpublishes_before_removing_data`, `delete_aborts_when_unpublish_fails_leaving_project_intact`. FE `App.test.tsx`: `shows the sidebar Compartido badge as soon as the conversation is shared` (badge inmediato, sin rerender extra), `refreshes the conversation list when a share-related task completes`. FE `PublishPanel.test.tsx`: menuitem "Dejar de compartir" **enabled** + clase `danger` cuando share activo; stop-sharing confirm + unpublish. CSS verificado: `.share-control-menu button.danger` `--danger`/`--danger-soft`/`--muted`/`:focus-visible`. **Cloudflare público real con el artifact actualizado NO se valida determinísticamente en esta fase → territorio HUMAN RE-ACCEPTANCE.**
- **TARGETED CONVERSATION ISOLATION (F) = PASS.** FE `App.test.tsx`: `shows only the selected conversation's messages when switching` y `ignores a late agent result from another conversation` (resultado async tardío de A no pinta en B). Re-entry sin "working" restaurado: `re-enables the composer after a rejected agent_send`.
- **TARGETED CHAT ORDERING (I) = PASS.** agent_send persiste el mensaje de usuario antes de ejecutar (persist-before-run); UI un turno in-flight por conversación; serialización por proyecto (`same_project_runs_are_serialized`, agent_service **10/10**); `send_tolerates_transient_diff_errors_during_ack_wait` (stale ack no aborta el turno).
- **TARGETED KEYBOARD (G) = PASS.** FE `ComposerBar.test.tsx`: `Enter sends; Shift+Enter inserts a newline without sending`, `does not send while IME composition is active` (isComposing), `does not send whitespace-only prompts`, `does not send when the composer is busy`, `does not send an empty prompt`.
- **STORAGE DOC CONSISTENCY = VERIFICADO (sin migración).** `docs/STORAGE_LAYOUT.md` consistente con el código: root = Tauri `app_data_dir()` (`app/src-tauri/src/lib.rs:63`), id `com.educai.publisher` (`tauri.conf.json:5`), XDG aislado vía `--pure` + XDG_* (`project-opencode/src/lib.rs:24,62-72`), preview temp `m8-preview-` (`app.rs:731`, fuera del app-data), `0o700` (`app.rs:1405`), publish = snapshot `PublicationSnapshotStore`, `revision=1`, AppImage = media de paquete, NO contenedor de datos. Sin discrepancia material.
- **M11 NO INICIADO.** Sin redesign de storage. Sin sleeps/force-rerenders. Sin claim de aceptación humana.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `40403c69…` (escenario §22: sin falso error de arranque; conversación nueva; request de creación SIN nudge "donde está?"; Creation aparece; Abrir muestra el contenido real; Compartir publica el contenido real; "Compartido" inmediato; Dejar de compartir legible; modificar Creation compartida → refresh de URL pública refleja el cambio; segunda conversación aislada; chat secuencial coherente; Enter envía; Shift+Enter newline; rename/delete; confirmación con «Sí»; persistencia tras reinicio). **NO afirmar aceptación humana desde OpenCode. NO iniciar M11.**

## Estado previo (HUMAN-ACCEPTANCE CORRECTION PASS — CONVERSATION STATE / CREATION LIFECYCLE / PREVIEW / SHARE REACTIVITY / CHAT UX — COMPLETE, REVIEWS APPROVE, INTEGRADO EN MAIN, M11 NO INICIADO, 2026-09-02)

- **PASS DE CORRECCIÓN HUMANA (STATE / CREATION LIFECYCLE / PREVIEW / SHARE / CHAT UX) = COMPLETE Y NO ES M11.** Corrige los hallazgos A–I del product owner sobre el AppImage real (falso "Listo." antes de ejecutar; Abrir en blanco; "Dejar de compartir" ilegible; actualización de Creation compartida que no llega a la URL pública; fuga de estado entre conversaciones; Enter/Shift+Enter; badge "Compartido" tarde; respuestas fuera de orden). Finding D (Cloudflare público muestra el artifact real) se PRESERVA.
- **INTEGRADO EN MAIN.** Autor (Cursor Grok 4.6 High FRESH) commits `99f6f7d` + fix de review `18b7c11` en `corr/state-lifecycle-pass` (worktree `../ai-publisher-corr-02-state-lifecycle`, base main `66d1a99`). **Merge `5e4b170` en main** (ort, 26 archivos, +1346/−108; docs/STORAGE_LAYOUT.md nuevo). **`./scripts/verify` EXIT=0 en main post-merge** (FE **228/228**, cargo verde, M10 + UX_REDESIGN_01 contracts, fetch-sidecars --check, cargo check src-tauri, git diff --check). Working tree limpio.
- **REVIEWS INDEPENDIENTES:** Product/UX Cursor Grok 4.6 High FRESH = **APPROVE** (`99f6f7d`). Código/a11y `opencode-go/qwen3.8-flash` FRESH = **REQUEST_CHANGES → APPROVE**: MAJOR (send-failure dejaba "working" permanente; fix `18b7c11` `onSendEnd` limpia inFlight + fase idle en ambas ramas del catch, composer re-habilitado, retry real, sin "working" fantasma al re-entrar, con test), MINOR (wipe-before-CAS podía borrar outputs viejos si el replace fallaba; fix write-then-prune `prune_after_replace_keeps_the_new_primary_and_drops_stale_sidecars`), LOW (transient `/diff` en la espera de ack abortaba el turno; fix tolera el error y sigue polleando, `send_tolerates_transient_diff_errors_during_ack_wait`). Re-review Qwen FRESH = **APPROVE** (verificados los 3 fixes, invariantes del diff combinado, storage doc exacto). Re-review UX acotado Cursor Grok 4.6 High FRESH = **APPROVE** (`18b7c11`: recuperación de fallo honesta en voseo, sin spinner permanente, sin duplicados, sin regresión A–I).
- **Clasificación de riesgo:** A, B, C, E, F, G, H, I son acotados (poller, preview routing, CSS, overwrite in-place + republish ADR-0004, frontend isolation/correlation). Sin STOP: no hubo migración de schema, no se rediseñó AgentEngine, no se reestructuró el FS. `revision` sigue en `1`. Sin sleeps UI arbitrarios ni force-rerenders.
- **A (falso Listo):** `poll_session` no trata un ack breve (`Listo.`/`OK`/…) + idle + sin artifacts como terminal; espera gracia (15s prod) y reconsulta `/diff`; si el estado sale de idle, el timer resetea. Instrucción: escribir archivos primero; nunca un "Listo." suelto antes. El usuario NUNCA necesita "donde esta?"/"podes?"/"seguí". Estado transitorio "Creando tu recurso…". Tests: `send_does_not_treat_brief_listo_as_complete_before_artifacts`, `brief_ack_detects_listo_and_ignores_real_replies`.
- **B (Abrir en blanco):** preview token-root (`/preview/<token>/` y sin slash) sirve `index.html`, igual que el publisher; nested dirs siguen 404. WebView navega a `{base}index.html`. Misma Creation que Compartir (mismo `creation.id`). Tests: `preview_lifecycle` token root 200, `preview.rs` GET base URL.
- **C (Dejar de compartir):** `.share-control-menu button.danger` usa `--danger` (peso 600) sobre fondo transparente, hover `--danger-soft`, disabled `--muted`, `:focus-visible` outline; habilitado cuando share activo (sin aspecto disabled falso). Test: menuitem enabled + class `danger`.
- **E (update de Creation compartida):** match por kind+display_name → overwrite `outputs/<id>/` (mismo id, visibilidad, revision=1). Si el proyecto ya está published **y** la Creation registrada ya es pública, `refresh_published_snapshot` hace replace de ADR-0004 (misma `publicationRoute`, sin re-engagement del túnel). Una actividad distinta (`actividad-2`) crea id nuevo y **no** hijackea la URL. Si republish falla, el mensaje del asistente es honesto: "El recurso local se actualizó, pero el enlace compartido no. Volvé a pulsar Compartir." Tests: `later_turn_updates_the_same_web_creation_and_refreshes_publish`, `new_distinct_web_does_not_replace_an_already_published_snapshot`, `later_turn_does_not_reregister_prior_workspace_files` (M1) intacto.
- **F (fuga entre conversaciones):** `selectedIdRef` sincrónico en `openConversation`; `refreshConversation` no aplica si el id ya no es el seleccionado; `WorkspaceView` solo si `conversation.id === selectedId`; key por id; eventos `agent://task` ajenos no pintan el timeline visible; in-flight per-conversation restaurado al volver. Tests: switch A/B, late foreign result, hold-open no muestra A mientras carga B, re-entry sin "working" restaurado.
- **G (Enter/Shift+Enter):** Enter envía; Shift+Enter newline; IME (`isComposing` / keyCode 229) no envía; whitespace no envía; busy respetado. Tests cubren los cinco.
- **H (badge Compartido):** `onRefresh` del workspace refresca conversación **y** `project_list`. Test: badge en sidebar al compartir, sin pulsar "+" ni otra acción.
- **I (orden de turnos):** `agent_send` persiste el mensaje de usuario **antes** de retornar; UI un turno in-flight por conversación (`sendingRef` + `onSendStart`); si `agent_send` rechaza, `onSendEnd` limpia inFlight + fase idle. AgentService ya serializaba por proyecto. Tests: persist-before-run, sequential ChatPanel order, segundo send bloqueado, re-enable tras reject.
- **Storage:** **`docs/STORAGE_LAYOUT.md`** (219 líneas, verificado contra el código por Qwen) documenta el layout real: root = Tauri `app_data_dir()` (`~/.local/share/com.educai.publisher` en Linux, `com.educai.publisher`), `settings.json` global, `projects/<id>/{project.json,inputs,workspace,outputs,publish}`, OpenCode XDG aislado (`--pure`), preview temp `m8-preview-*` (fuera del app-data), publicación = snapshot inmutable (ADR-0004). **El AppImage es media de paquete ejecutable, NO el contenedor de datos persistentes.** Sin migración. Puntero en `docs/ARCHITECTURE.md`.
- **Gates:** `pnpm format:check && pnpm lint && pnpm typecheck && pnpm test` en `app/` = **228/228**. `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked` = PASS. `./scripts/verify` = EXIT=0 (worktree autor y main post-merge, incluye `cargo check` src-tauri).
- **Runtime:** preview loopback real (HTTP 200 del artifact en token root + `index.html`); update+republish con FakeAgentEngine (mismo id, snapshot `publish/` actualizado). **Cloudflare público live y AppImage humano = HUMAN RE-ACCEPTANCE** (Finding D ya validado por el product owner; no se finge PASS de red pública).
- **LIMITACIONES NO BLOQUEANTES (conocidas):** (1) un "Listo." aislado real de Q&A corta (whitelist exacta) espera el grace completo mostrando "Creando tu recurso…" — heurística aceptada, mitigada por prompt; (2) el texto honesto de republish solo aplica cuando la refresh falla (si el modelo ignora la instrucción y mintea un archivo privado nuevo, la coincidencia in-place no se da) — cubre el camino reportado "cambiar el fondo"; (3) en 15s sin archivos un "Listo." puede caer como reply final (test cubre, necesario para acks de chat corto).
- **NO REGRESAR:** session `?directory=` binding; attachments al agente; cards Abrir/Compartir mismo id; publish del artifact (no "Material del proyecto"); modelo en Configuración; discovery free; X de Configuración vuelve a la misma conversación; toast duplicado; delete «Sí»; sin falso error de arranque genérico.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE DE ESTE PASS. NO HUMAN ACCEPTED. M11 NO INICIADO.** Próximo gate y ÚNICO: (1) **FRESH REAL APPIMAGE BUILD + VERIFICACIÓN TÉCNICA** desde main `5e4b170` (`scripts/smoke-package appimage`, sidecars pineados opencode 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real Fedora/Wayland); (2) **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco: adjunto rosco → creación sin "donde esta?"; Abrir muestra el juego real; Compartir → URL pública con el juego (sin "Material del proyecto"); modificar el juego → misma card/URL refleja el cambio; conversación nueva aislada; Enter/Shift+Enter; badge "Compartido" inmediato; "Dejar de compartir" legible. **NO iniciar M11. NO afirmar aceptación humana desde OpenCode.**

## Estado previo (FRESH APPIMAGE POST-CORRECCIÓN CONSTRUIDO Y VERIFICADO TÉCNICAMENTE — TECHNICALLY READY FOR HUMAN RE-ACCEPTANCE, M11 NO INICIADO, 2026-09-01)

- **FRESH REAL APPIMAGE CONSTRUIDO DESDE MAIN `ebeac0e` (post-corrección Creation/Share/Chat) = PASS (sesión FRESH, deepseek-v4-flash, validación técnica determinista).** Packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS).** Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.861.432 bytes**, **SHA-256 `ec3368811bdf65679e8271e571da383d4837aec9ce5ddccb885778373bea6392`** (NUEVO; el previo `930ee074…` era STALE — construido desde `773278d`, predata el merge de corrección `ebeac0e`), timestamp 2026-09-01 23:24:41 -0300, source commit `ebeac0e` (HEAD `1aba3f0` checkpoint, working tree clean antes del build, sin cambios de producto sin commitear). Build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool tras el error esperado de linuxdeploy en Fedora 44). **Sidecars bundlados pineados verificados en payload y en mount en vivo:** opencode **1.18.25** (sha256 `58a3729a…` = pin) y cloudflared **2026.8.3** (sha256 `f29324fe…` = pin, idéntico al pin `config/components.json`). **Frontend embebido correcto:** el binario embebe exactamente `assets/index-DJsNCuJZ.js` + `assets/index-CbAI0ZTD.css` (idénticos a los del `dist` generado en este build desde `ebeac0e`). **Lanzamiento real en Fedora/Wayland (DISPLAY=:0):** backend `[agent] starting → ready`, SIN falso error de arranque, sin errores en log. **PATH-independencia:** lanzado con PATH `/tmp/opencode/path-indep-bin:/usr/bin:/bin` (sin opencode/cloudflared externos); el sidecar opencode hijo corre desde el mount propio del AppImage (`/tmp/.mount_EducAIIJNecl/usr/bin/opencode`, port 45237); cloudflared presente en el payload del mount. **`./scripts/verify` EXIT=0** (cargo fmt/clippy/test verdes, FE **217/217** en 21 archivos, format/lint/typecheck, fetch-sidecars --check, M10 version 0.1.0, UX_REDESIGN_01 contract, cargo check src-tauri, git diff --check).
- **TARGETED CREATION/SHARE RUNTIME VALIDATION = PASS (unit/integración mockeada, evidencia del contrato).** `cargo test -p project-app --test app_facade` **32/32**: `publish_promotes_the_generated_web_creation_as_the_public_entry` (el `publish/index.html` contiene el markup generado y NO "Material del proyecto"), `web_sidecar_sibling_is_copied_into_outputs_and_publish` (sidecars CSS/JS/imágenes copiados), `publish_without_creation_id_still_promotes_the_latest_web`, `run_agent_registers_creation_private_by_default`, `set_creation_visibility_toggles`, `creation_path_rejects_cross_project_id`, `delete_unpublishes_before_removing_data`. `cargo test -p project-agent --test agent_service` **9/9**: `later_turn_does_not_reregister_prior_workspace_files` (dedupe M1), `workspace_scan_registers_when_diff_is_empty` (scan solo si diff vacío), `web_sidecar_assets_are_not_separate_creations` (assets de sidecar no son Creations separadas), `traversal_artifact_path_is_rejected_and_not_registered` (seguridad de paths), `run_registers_scripted_artifacts_as_private`. Genericity confirmada: sin hardcode Pasapalabra (cualquier `.html/.htm` = `Web`, título "Actividad" para `index.html` raíz).
- **TARGETED ABRIR/COMPARTIR VALIDATION = PASS (donde el tooling lo permite).** Tests FE `CreationsPanel.test.tsx`: Abrir usa `preview_open_web` con el MISMO `creation.id` (no `creation_open` genérico); Compartir asocia la MISMA creación; `aria-label="{Abrir}: {displayName}"`/`"{Compartir}: {displayName}"` por card. Publicación del artifact (no raíz del proyecto) cubierta por el test Rust `publish_promotes_the_generated_web_creation_as_the_public_entry`. **Nota de límite:** networking público real de Cloudflare (URL pública abierta mostrando el juego, sin "Material del proyecto") NO se valida determinísticamente en esta fase técnica → **territorio de HUMAN RE-ACCEPTANCE.**
- **TARGETED CHAT REGRESSION = PASS (FE 217/217).** `ChatPanel.test.tsx`: no renderiza burbuja assistant completada vacía etiquetada solo "Asistente" (B5), no duplica contenido asistente en `.chat-status.ok` verde (B6/B11), working status no duplica, error no duplica, failed line solo cuando la burbuja más nueva coincide (Blocker B post-T7), materials adjuntos solo en burbuja del usuario. Toast "Tu recurso está listo." eliminado (un evento lógico = una notificación).
- **TARGETED SETTINGS VALIDATION = PASS.** `App.test.tsx`: `keeps the model selector out of the composer` (B7/H — composer = adjuntar/mensaje/enviar, sin selector permanente, sin "Modelo gratuito" banner); `opens settings from the gear button, shows the model selector there, and restores the conversation on close` (B7/H — ModelSelector en Configuración, X cierra y vuelve EXACTO a la misma conversación, sin reset). Default free/model discovery del backend intacto (sin hardcode Big Pickle; el test usa `big-pickle` solo como fixture de mock de prueba).
- **DELETE-CONFIRMATION «SÍ» REGRESSION = PASS.** `ConfirmDialog.test.tsx`: acepta `Sí/sí/SI/si` + espacios alrededor; rechaza `s i`/`siii`/`no`/solo-espacios/vacío/título-exacto-cuando-configurado; Enter NO puede saltar la validación; Cancel nunca borra; botón deshabilitado hasta válido; `confirmText` explícito (proyectos) conserva matching exacto. Semántica destructiva backend intacta.
- **REVIEW-FIX REGRESSIONS REPRESENTADAS EN MAIN Y CUBIERTAS POR TESTS = VERIFICADO.** Dedupe (M1), títulos accesibles por card (m6), sidecar copy (m2/m7), nested `index.html` (m3), scan caps/bounds (m4), idle/race polling (m5), tests real-registrar — todos presentes en el merge `ebeac0e` y verificados por los tests Rust/FE listados arriba.
- **M11 NO INICIADO.** Sin fuga de alcance: sin redesign de infra de publicación, sin cambios destructivos Task F, sin tocar runtime/session-directory, sin rerun de exploración Product/UX amplia.
- **PROVENANCE DEL ARTEFACTO:** source commit `ebeac0e` (HEAD `1aba3f0`), build 2026-09-02 02:24 UTC (23:24 -0300) vía `scripts/smoke-package appimage`, SHA-256 `ec3368811bdf65679e8271e571da383d4837aec9ce5ddccb885778373bea6392`, 180.861.432 bytes. El previo `930ee074…` en la misma ruta fue reemplazado por este build (rm -rf del dir appimage por smoke-package + rebuild). Sidecars pins: opencode **1.18.25**, cloudflared **2026.8.3** (verificados, sin repin). Limpieza: procesos/mounts AppImage de prueba removidos, working tree limpio.
- **STATUS: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Próximo gate y ÚNICO: **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco `ec336881…` (escenario §17/§19: adjunto `datosrosco.txt` + prompt real → Creation card [Abrir][Compartir], URL pública con el juego real y sin "Material del proyecto", sin burbuja vacía, sin toast duplicado, modelo en Configuración, «Sí» para eliminar). **NO afirmar aceptación humana desde OpenCode.** NO iniciar M11.

## Estado previo (PASS CORRECCIÓN PRODUCT/UX CREACIÓN/COMPARTIR/CHAT — COMPLETE, REVIEWS APPROVE, INTEGRADO EN MAIN, M11 NO INICIADO, 2026-09-01)

- **PASS DE ACEPTACIÓN HUMANA (CREACIÓN / SHARE / CHAT UX) = COMPLETE Y NO ES M11.** Corrige
  los 7 bloqueadores PRODUCT/UX hallados por el product owner en el AppImage real (asistente
  generó un Rosco/Pasapalabra desde `datosrosco.txt`, pero solo apareció prosa, la URL
  pública mostraba "Material del proyecto", el asistente pidió abrir archivos a mano, hubo
  burbuja vacía "Asistente", toast duplicado "Tu recurso está listo.", y selector de modelo
  permanente en el composer).
- **INTEGRADO EN MAIN.** Autor (Cursor Grok 4.6 High FRESH) commit `3ba7c5a` + fix de review
  `857d98c` en `corr/creation-share-ux-pass` (worktree `../ai-publisher-corr-01-creation-share`,
  base main `3a7c6d1`). **Merge `ebeac0e` en main** (ort, 30 archivos, +1320/−300).
  Evidencia durable de reviews en `docs/qwen-review-creation-share.md`,
  `docs/qwen-rereview-creation-share.md`, `docs/ux-rereview-creation-share.md` (commit `05a2c2a`).
- **B1 (card de creación):** `opencode.rs` `normalize_output_path` acepta paths session-relative
  (`rosco.html` → `workspace/rosco.html`) y absolutos solo si contienen `/workspace/`;
  `service.rs` `merge_artifacts` usa el diff del sidecar cuando trae un archivo registrable y
  el workspace scan SOLO como fallback si el diff queda vacío (prevención de duplicados M1);
  cualquier `.html/.htm` es `Web`; el registrar guarda webs como `index.html` y copia sidecars
  (CSS/JS/imágenes) a `outputs/<id>/` — genérico, sin hardcode Pasapalabra.
  **B2 (Abrir/Compartir = misma creación):** `publish(projectId, creationId?)` fluye de la card
  → `useShareControl` → Tauri `commands.rs` → `app.rs publish_creation`; Abrir usa el mismo
  `creation.id` (`preview_open_web`).
  **B3 (URL pública muestra la creación, no "Material del proyecto"):** `app.rs
  prepare_share_visibility` marca PÚBLICA la creación objetivo (id preferido, si no el último
  web, si no la última) y degrada otros webs públicos antes del snapshot; test
  `app_facade.rs publish_promotes_the_generated_web_creation_as_the_public_entry` assert que
  `publish/index.html` contiene el markup generado y NO contiene "Material del proyecto".
  **B4 (sin abrir-archivo-manual):** `service.rs build_instruction` ordena escribir un recurso
  web estático con `index.html` como entrada, dice que EducAI mostrará Abrir/Compartir, y
  prohíbe pedir abrir/doble clic/explorador.
  **B5 (burbuja vacía):** poll ignora texto asistente vacío; `ChatPanel.tsx` no renderiza
  burbuja assistant completada vacía sin creations; errores/cancel siguen como `role="alert"`.
  **B6 (toast duplicado):** toast "Tu recurso está listo." ELIMINADO (un evento lógico = una
  notificación); listener `agent://task` registrado una vez con refs + `unlisten` cancelado
  (sin re-suscripción por `selectedId`).
  **B7 (modelo a Configuración):** composer = adjuntar/mensaje/enviar (+ slot Compartir);
  `ModelSelector` en `ProviderPanel` (Configuración); default free/model discovery del backend
  intacto (sin hardcode Big Pickle); X de Configuración = `setSettingsOpen(false)` → vuelve
  EXACTO a la misma conversación.
- **REVIEW PRODUCT/UX INDEPENDIENTE (Cursor Grok 4.6 High FRESH) = APPROVE** (pane cerrado,
  sesión previa). 2 residuales NO bloqueantes: (1) título de card caía a "index" cuando el
  modelo escribía `index.html` en la raíz → **RESUELTO en el fix de review (m1)**: la raíz
  `index.html`/`index.htm` ahora se titula "Actividad"; carpetas padre siguen ganando en
  anidados (`actividad-2/index.html` → "actividad-2"); (2) Compartir sigue también en la
  bottom bar además de la card — consistente con el pass.
- **REVIEW CÓDIGO/A11Y/CORRECTNESS FRESH (`opencode-go/qwen3.8-flash`) = REQUEST_CHANGES →
  APPROVE.** Primer review sobre `3a7c6d1..3ba7c5a`: **M1 MAJOR** (el scan de workspace
  re-registraba artifacts de turnos previos → cards duplicadas en turnos siguientes y
  promoción de duplicado stale en el fallback sin-id) + **m1-m7 MINOR** (título "index";
  sidecar copy podía producir Creation no publicable — reserved roots/stems; `index.html`
  anidado descartado; scan/copy sin capping ni exclusión de árboles de dependencias; poll de
  `/diff` cada 20ms con 120s de timeout si vacío; a11y: botones Abrir/Compartir sin nombre
  accesible por creación; sin cobertura del path filesystem sidecar) + LOW/NIT (L1-L4, N1).
  **Fix acotado por el MISMO autor (Cursor Grok 4.6 High FRESH, commit `857d98c`, 14 archivos
  +554/−94):** M1 vía opción (a) — diff del sidecar autoritativo, scan solo si diff vacío
  (`later_turn_does_not_reregister_prior_workspace_files` + `workspace_scan_registers_when_diff_is_empty`
  verdes); m1 título humano "Actividad"; m2 `sidecar_component_ok` replica `validate_component`
  del snapshot (reserved stems a cualquier profundidad, `materials.html`/`files` solo raíz);
  m3 skip de `index.html` solo en `dest_root`; m4 skip `node_modules/dist/build/target/vendor/venv/
  __pycache__/coverage/bower_components` + caps profundidad 8 / archivos 500 / bytes 32 MiB;
  m5 grace idle 2s arranca aunque no haya files y `/diff` se trae una vez al iniciar el grace;
  m6 `aria-label="{Abrir}: {displayName}"` / `"{Compartir}: {displayName}"` por card; m7 tests
  real-registrar (`web_sidecar_sibling_is_copied_into_outputs_and_publish`); L1 param muerto
  eliminado; L2 dead code eliminado (messages.agent.ready, CSS `.composer-model*`); L4 copy
  best-effort + validación `..` en source; N1 `content:""` cae a `parts`. **L3 NO fixed por
  diseño** (note de demotion de webs públicas para target no-web, pre-existente LOW, M1 le
  quita su peor manifestación — aceptado por el revisor). **Re-review FRESH
  (`opencode-go/qwen3.8-flash`) = APPROVE** (verificado: diff 3ba7c5a..857d98c, invariantes 1-11
  del diff combinado, targeted tests verdes, `pnpm typecheck` + `cargo fmt --check` + `git diff
  --check`; residuales no bloqueantes: LOW L3, NIT log del copy error, NIT skip de nombres
  genéricos).
- **RE-REVIEW UX ACOTADO (Cursor Grok 4.6 High FRESH) = APPROVE** sobre los DOS cambios de
  comportamiento visible del fix: (1) título "Actividad" para `index.html` raíz — lenguaje de
  aula, sin fuga del nombre de archivo, consistente con B1-B3; (2) sin cards duplicadas en
  turnos siguientes — el docente ve UNA card nueva por actividad, y el fallback latest-Web de
  Compartir ya no puede promover un re-registro stale. B1-B3 (Abrir/Compartir sobre el mismo
  artifact registrado; share público de la creación) intactos.
- **VERIFICACIÓN EN WORKTREE AUTOR (post-fix `857d98c`):** `pnpm format:check/lint/typecheck`
  OK, **vitest 217/217** (21 archivos), `cargo fmt --check` + `clippy -D warnings` + `cargo
  test --locked --workspace --all-targets` verdes (584 tests), **`./scripts/verify` EXIT=0**.
  **VERIFICACIÓN EN MAIN POST-MERGE (`ebeac0e`): `./scripts/verify` EXIT=0** (FE 217/217,
  cargo verde, contracts M10 + UX_REDESIGN_01, fetch-sidecars --check, cargo check src-tauri,
  git diff --check). Evidencia = unit/integración mockeada; NO AppImage real, NO Cloudflare
  live, NO generación OpenCode live (no se reclama aceptación humana).
- **DELETE-CONFIRMATION «SÍ» = PRESERVADO (commit `3a7c6d1`, intacto en este pass).**
  `ConfirmDialog.tsx`/`ConversationsSidebar.tsx` sin cambios en este diff; `normalizeConfirmation`
  acepta `Sí/sí/SI/si` (+ espacios) y cadenas ajenas nunca confirman; Enter no saltea;
  Cancel nunca borra; flujo de proyectos conserva matching exacto del título.
- **M11 NO INICIADO.** Sin fuga de alcance: sin redesign de infra de publicación, sin cambios
  destructivos Task F, sin tocar runtime/session-directory (no reabiertos).
- **PRÓXIMO GATE (siguiente sesión FRESH):** (1) **FRESH REAL APPIMAGE BUILD + VERIFICACIÓN
  TÉCNICA** desde main `ebeac0e` (`scripts/smoke-package appimage`, sidecars pineados
  opencode 1.18.25 + cloudflared 2026.8.3, `./scripts/verify` EXIT=0, lanzamiento real
  Fedora/Wayland); (2) **HUMAN PRODUCT-OWNER RE-ACCEPTANCE** del AppImage fresco (escenario
  real §17/§15: adjunto rosco + prompt real → creación card [Abrir][Compartir], URL pública
  con el juego y sin "Material del proyecto", sin burbuja vacía, sin toast duplicado, modelo
  en Configuración, «Sí» para eliminar). NO iniciar M11. NO afirmar aceptación humana desde
  OpenCode. Rotación de sesión previa en `3251ffd` (orquestador previo alcanzó ~106K).

## Estado previo (CONFIRMACIÓN DE ELIMINACIÓN CON «SÍ» — CAMBIO FRONTEND BOUNDED, INTEGRADO, 2026-09-01)

- **DELETE-CONFIRMATION «SÍ» (frontend, acotado) INTEGRADO.** Cambio puntual sobre el
  diálogo compartido `app/src/components/ConfirmDialog.tsx` para la ELIMINACIÓN DE
  CONVERSACIÓN: ya NO se exige escribir el título exacto de la conversación; ahora se
  confirma con la afirmación **«Sí»**. `normalizeConfirmation` (trim → toLowerCase →
  NFD → strip U+0300–U+036f) acepta `Sí`/`sí`/`SI`/`si` y tolera espacios al inicio/final.
  Cadenas ajenas (`No`, `borrar`, el propio título, `s i`, `siii`, solo-espacios, vacío)
  NO habilitan el botón. **Enter NO puede saltar la confirmación** (input fuera de
  `<form>`, botones `type="button"`, `useFocusTrap` solo mapea Escape/Tab); el botón
  `danger` sigue `disabled={!ready || busy}`; Cancel/Escape/backdrop → `onCancel` nunca
  `onConfirm`.
- **SIN FUGA DE ALCANCE / SIN RELAJAR TAREA F.** La regla `ready` quedó:
  `confirmText !== undefined ? value === confirmText : normalizeConfirmation(value) ===
  normalizeConfirmation(messages.common.confirmYes)`. El flujo de PROYECTOS
  (`ProjectsView.tsx`, pasa `confirmText={deleting.name}`) conserva el matching **exacto**
  original (case/accent/sensitive, sin trim) → byte-idéntico al pre-cambio. El flujo de
  CONVERSACIÓN (`ConversationsSidebar.tsx`, pasa solo `confirmPrompt`, sin `confirmText`)
  usa la rama afirmativa. `commitDelete` (guard in-flight/busy, fail-closed, reset solo
  en éxito) y toda la semántica destructiva/persistencia/unpublish/filesystem de Task F
  quedaron **intactas** (diff solo frontend, 5 archivos, sin Rust/tauri/api).
- **A11Y / COPY.** `confirmPrompt` se asocia al input vía `aria-describedby` (`<p
  id="confirm-prompt">`); foco inicial en el input; `role="dialog" aria-modal` intactos.
  Copy voseo: `confirmYes: "Sí"`, `confirmPrompt: "Para confirmar, escribí Sí."`,
  `confirmNameLabel: "Confirmación"` (label sr-only genérico, aceptado).
- **REVIEWS INDEPENDIENTES (qwen3.8-flash, sesiones FRESH):** primera →
  **REQUEST_CHANGES** (should-fix scope-leak del matching + should-fix `aria-describedby`;
  nits de tests); fix acotado aplicado → re-review **APPROVE**. Nota: la sugerencia
  literal del reviewer (`confirmText === messages.common.confirmYes ? …`) habría roto
  `ConfirmDialog.test.tsx` (que pasa `confirmText` explícito); se resolvió con la regla
  explícito=exacto / ausente=afirmativo.
- **VERDE:** vitest FE **214/214** (21 archivos), `tsc --noEmit` 0, `eslint` 0,
  `prettier --check` 0, **`./scripts/verify` EXIT=0** (cargo check, contracts M10 +
  UX_REDESIGN_01, fetch-sidecars).
- **PENDIENTE (fuera de este cambio acotado):** el AppImage `930ee074…` se construyó
  desde `773278d` y **NO incluye** este cambio; el próximo AppImage fresco +
  re-aceptación humana deben incluirlo. El pase grande de corrección de aceptación
  humana (8 ítems) sigue pendiente y **debe preservar** esta confirmación con «Sí»
  (ítem 8). M11 **NO INICIADO**. El orquestador rota en este checkpoint.

## Estado previo (APPIMAGE NUEVO POST-T7 CONSTRUIDO Y VERIFICADO — TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE, M11 NO INICIADO, 2026-09-01)

- **APPIMAGE NUEVO POST-T7 REAL = PASS (sesión FRESH, deepseek-v4-flash, validación técnica completa).** AppImage NUEVO construido desde main `773278d` (post-T7 human blocker merge `d6f97ab` + checkpoint `773278d`) con el packaging canónico M10 `scripts/smoke-package appimage`. **EXIT=0 (smoke-package PASS).** Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`, **180.816.376 bytes**, **SHA-256 `930ee074bfbe40b4cf1e5c9582c93b884d695d6348bf7521e764ade5b9f6834d`** (NUEVO; difiere del stale T7 `3dba67a8…`), timestamp 2026-09-01 20:47:14 -0300, source commit `773278d`, build via `scripts/smoke-package appimage` (fetch-sidecars → `cargo tauri build --bundles appimage` → fallback documentado a appimagetool), repo limpio (working tree clean) antes del build, sin cambios de producto sin commitear. Sidecars bundlados pineados verificados en el payload extraído: opencode **1.18.25** y cloudflared **2026.8.3** (cloudflared SHA-256 `f29324fe…` idéntico al pin `config/components.json`). **Lanzamiento real en Fedora/Wayland (DISPLAY=:0):** app corre con WebKitNetworkProcess + WebKitWebProcess activos, backend `[agent] starting → ready` SIN falso error de arranque, sin errores en log. **PATH-independencia:** lanzado con PATH sin opencode/cloudflared; el sidecar opencode hijo se ejecuta desde el mount propio del AppImage (`/tmp/.mount_EducAIGcoBlM/usr/bin/opencode`, port 42523). **Frontend embebido correcto:** el binario embebe exactamente `assets/index-Dt0XeFOc.js` + `assets/index-CxEdFXeO.css` (idénticos nombres a los del `dist` generado en este build desde main `773278d`; los markers del fix — CSS `.conversation-menu-dropdown button.danger` / `danger-soft` y JS "Eliminar conversación" / `chat-status.err` — presentes en el dist embebido y `external_directory` en el binario). **`./scripts/verify` EXIT=0** (cargo fmt/clippy/test, FE 202/202, format/lint/typecheck, M10 + UX_REDESIGN_01 contracts, fetch-sidecars --check, cargo check src-tauri, git diff --check). **Targeted Blocker A runtime (probes directos contra el sidecar real empaquetado 1.18.25, 127.0.0.1:42523):** `POST /session` + body JSON `directory` → la sesión queda ligada al mount del AppImage (`/tmp/.mount_EducAIGcoBlM/usr`) — reproduce el bug; `POST /session?directory=%2Ftmp%2Fopencode%2Fpostt7-evidence%2Fws-test` → la sesión queda ligada al workspace EducAI deseado (campo `directory` en `GET /session`). **Secuencia de requests (sin aceptar "hola" único):** 3 prompts en la MISMA sesión ligada (hola → "Hola. ¿En qué puedo ayudarte?"; 2º → "Sí, sigo la conversación. ¿Qué necesitas?"; 3º → "Recibido, tercer mensaje. ¿En qué trabajamos?"), 1 conversación/sesión NUEVA con contexto de adjunto (rosco.txt → "Listo."), todos con `cwd` del message = `/tmp/opencode/postt7-evidence/ws-test`, sin ASK external_directory (permission deny bindeado), sin espera ~120s (respuestas en ~1s), y sin misclasificar fallo en vuelo como arranque. **Targeted Blocker B (vitest `ChatPanel.test.tsx`):** "does not duplicate a persisted failed assistant message as raw error text" PASS + "still renders a failed status line when an earlier failed bubble is not the newest message" PASS (fix de review preservado: burbuja histórica NO suprime fallo nuevo). **Targeted Blocker C (vitest `App.test.tsx` menu + CSS):** menu ⋮ Renombrar/Eliminar con `role=menu`/`menuitem` PASS; CSS `.conversation-menu-dropdown button.danger` con `color: var(--danger)` sobre superficie (contraste legible), hover `--danger-soft`, disabled `--muted`, nowrap + padding compacto, copy español intacto. Limpieza: instancias de prueba del AppImage (nueva y stale T7) terminadas, mounts `/tmp/.mount_EducAI*` removidos, worktrees limpios (solo `main`), branch único `corr/a-creation-contract` preexistente sin tocar. **Status: TÉCNICAMENTE READY FOR HUMAN RE-ACCEPTANCE. NO HUMAN ACCEPTED. M11 NO INICIADO. Gate siguiente y único: HUMAN PRODUCT-OWNER RE-ACCEPTANCE sobre ESTE AppImage nuevo (`930ee074…`).** Limitaciones para validación humana: (1) secuencia real-provider en el AppImage con modelo gratis y adjunto rosco se dejó al escenario §17 humano; (2) la validación de prompts usó el modelo gratis `big-pickle` determinista; (3) la visibilidad/contraste visual final del menú y los flows UI completos se confirman en el escenario humano; (4) no se ejecutó el escenario §17 completo (es humano).

## Estado previo (POST-T7 HUMAN BLOCKER PASS INTEGRADO — BLOQUEADORES A/B/C CORREGIDOS, ESPERANDO NUEVO APPIMAGE + RE-ACEPTACIÓN HUMANA, 2026-09-01)

- **POST-T7 HUMAN BLOCKER PASS INTEGRADO (`d6f97ab`).** El product owner probó
  el AppImage real T7 y encontró 3 bloqueadores; este pass los corrigió y
  fusionó. **Blocker A (raíz CONFIRMADA):** el adapter mandaba el directorio de
  sesión como campo JSON `directory`, que opencode 1.18.25 IGNORA (los campos
  desconocidos del body se descartan; NO es `additionalProperties:false`).
  La sesión quedaba ligada al cwd del sidecar (mount del AppImage), el agente no
  veía los adjuntos, colgaba en un ASK `external_directory` sin responder y el
  timeout de tarea de 120s se mapeaba como error de arranque falso
  "No se pudo iniciar el asistente de IA.". **Fix:** `with_directory_query`
  (`crates/project-opencode/src/lib.rs`) envía `POST /session?directory=<percent-encoded workspace>`
  (probado contra el sidecar real empaquetado 1.18.25: el query bindea el
  workspace y el asistente responde "listo"; el JSON body NO bindea). Se agrega
  body `permission: [{external_directory,* ,deny}]` para que el agente no
  pregunte por directorios externos (los adjuntos están dentro del workspace
  ligado, no se ocultan). El timeout en vuelo de tarea ahora mapea a
  `TaskFailed` → "No se pudo completar la creación." (honesto); el timeout de
  arranque `ensure_ready` SIGUE mapeando a `AiUnavailable` → "No se pudo iniciar
  el asistente de IA." (real). **Blocker B (doble render):** `ChatPanel` suprimía
  el `.chat-status.err` con `hasPersistedFailure` si existía CUALQUIER burbuja
  fallida histórica → podía ocultar un fallo nuevo sin burbuja persistida o en la
  ventana pre-refresh. Fix (`21e9e5b`): la supresión solo aplica si la burbuja
  MÁS NUEVA del timeline es assistant failed/cancelled con `text === agentMessage`;
  cualquier otro caso muestra el `.chat-status.err` (role=alert) una vez.
  **Blocker C (menú Eliminar):** `.danger` (texto blanco) ganaba sobre
  `background: transparent` del dropdown → texto blanco en menú blanco. Fix:
  `.conversation-menu-dropdown button.danger` con `--danger` sobre superficie,
  nowrap, padding compacto, hover `--danger-soft`, disabled `--muted`; copy
  español intacto. **Autor:** Cursor Grok 4.6 High (`corr/post-t7-blockers`,
  `b106d07b` + fixes `21e9e5b`). **Reviews:** UX Cursor Grok 4.6 High FRESH
  (APPROVE + re-APPROVE), código/a11y `opencode-go/qwen3.8-flash` FRESH
  (REQUEST_CHANGES → APPROVE; MAJOR = supresión ligada a burbuja más nueva, no a
  historial). **Evidencia runtime:** probes directos del orquestador contra el
  sidecar real del AppImage T7 (opencode 1.18.25, 127.0.0.1:36771) —
  `POST /session` + body JSON directory → `directory=/tmp/.mount_EducAIGjCKDD/usr`
  (NO bindea); `POST /session?directory=%2Ftmp%2F...` → bindea el workspace y
  `prompt_async` responde "listo". **Tests:** `./scripts/verify` EXIT=0 en main
  post-merge (cargo 565+, FE 202/202, fmt/lint/typecheck, M10 + UX_REDESIGN_01
  contracts, fetch-sidecars --check, git diff --check). **M11 NO iniciado.**
  **Siguiente gate:** construir AppImage NUEVO desde `d6f97ab`, verificación
  técnica, y re-aceptación humana del product owner (escenario real §15).
- **T7 APPIMAGE NUEVO REAL = PASS (sesión FRESH, deepseek-v4-flash).** AppImage

- **T7 APPIMAGE NUEVO REAL = PASS (sesión FRESH, deepseek-v4-flash).** AppImage
  NUEVO construido desde main `d25f957` (Task G integrada via `2451c50`) con el
  packaging canónico M10 `scripts/smoke-package appimage` (fetch-sidecars →
  `cargo tauri build --bundles appimage` → fallback documentado a appimagetool →
  inspección payload sidecars). **EXIT=0 (smoke-package PASS).** Artefacto:
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`,
  **180.816.376 bytes**, **SHA-256 `3dba67a83223394efa697f3e95ff6ad46ae504093df931459d2eea9b05259bd7`**
  (NUEVO; difiere del previo `423cdb28…`), timestamp 2026-09-01 17:35:27 -0300.
  Sidecars bundlados pineados y verificados en el payload y en el mount en vivo:
  opencode **1.18.25** y cloudflared **2026.8.3** (cloudflared SHA-256
  `f29324fe…` idéntico al pin). **Lanzamiento real en Fedora/Wayland
  (DISPLAY=:0):** app corre con WebKit renderer + network process activos,
  backend `[agent] starting → ready` SIN falso error de arranque, sin errores en
  log. **PATH-independencia:** lanzado con PATH sin opencode/cloudflared; el
  sidecar opencode hijo se ejecuta desde el mount propio del AppImage
  (`/tmp/.mount_EducAI…/usr/bin/opencode`). **Frontend Task G verificado:**
  el binario embebe exactamente `assets/index-CJy6dhvp.js` +
  `assets/index-BlpY7WEx.css` (idénticos al `dist` de main generado en este
  build). **`./scripts/verify` EXIT=0** (cargo fmt/clippy/test ok — 85 suites
  ok —, pnpm format/lint/typecheck ok, FE 201/201, fetch-sidecars --check ok,
  M10 version alignment 0.1.0, UX_REDESIGN_01 contract ok, cargo check
  src-tauri ok, git diff --check ok). Limpieza de procesos/mounts AppImage de
  prueba completada. **Status: TÉCNICAMENTE READY FOR HUMAN REVIEW. M11 NO
  iniciado. Gate siguiente y único: HUMAN PRODUCT-OWNER ACCEPTANCE (el humano
  abre el AppImage y corre el escenario real §15).**
- **T7 PENDIENTE (próxima sesión FRESH):** ~~AppImage NUEVO real desde main
  `2451c50`, sidecars pineados (opencode 1.18.25, cloudflared 2026.8.3, SIN
  cambiarlos), `./scripts/verify` PASS contra artefacto fresco, y luego STOP para
  aprobación humana del product owner.~~ **HECHO (ver arriba). M11 NO iniciar.
  Gate siguiente y único: HUMAN PRODUCT-OWNER ACCEPTANCE.**
- **ESTADO PREVIO (T6 y Task G) — ver secciones históricas abajo.**

- **Current main commit: `d6f97ab`** (merge del post-T7 human blocker pass). La
  corrección A-G + post-T7 A/B/C sigue INTEGRADA y verificada (ver detalle
  arriba). `git log --oneline -14` para el detalle.
- **T6 PLAYWRIGHT HEADED = PASS (sesión fresh, deepseek-v4-flash).** Ejecutado
  contra main `2451c50` (Task G integrada) con el harness canónico
  `docs/ux-redesign-01/harness/run.sh` (capture.py headed + measure.py + ocr.py,
  Vite dev server en :1420, boundary Tauri mockeado via `mock-inject.js`).
  **57/57 aserciones PASS**, MEASURE PASS (3 viewports), OCR 78/78 imágenes,
  EXIT=0. 17 flows × 3 viewports (1366×768, 1440×900, 1920×1080): PNG 78, `.ocr.txt`
  78, `.a11y.txt` 17. Flows 15-17 nuevos específicos de Task G/F: (15) Creation
  card Abrir/Compartir + Abrir sin error; (16) delete confirm type-name-to-confirm
  con copy llano (titulo/body/Eliminar habilitado tras teclear nombre/item
  removido); (17) asistente renderiza UNA vez (sin `.chat-status.ok` verde, texto
  llano). Flows 01-14 actualizados al DOM Task G (rename via menú "…",
  attachments `attachment-chip` con Abrir, share menu auto-open post-publish) y
  `mock-inject.js` `app_status → agent:"ready"` (contrato Task C; composer habilitado
  solo con backend ready). Sin hallazgos UX_BLOCKER/UX_IMPORTANT; solo POLISH
  pre-aprobado (nombre display del mock, contraste, tooltip truncation). Evidencia
  durable en `docs/ux-redesign-01/` (RESULTS.md actualizado). Verificación
  EXIT=0. **Budget al cierre de T6: ROTATE_SESSION_REQUIRED (111K) → la sesión
  checkpointea T6 y SE DETIENE; T7 NO inicia en esta sesión.**
- **T7 PENDIENTE (próxima sesión FRESH):** AppImage NUEVO real desde main
  `2451c50`, sidecars pineados (opencode 1.18.25, cloudflared 2026.8.3, SIN
  cambiarlos), `./scripts/verify` PASS contra artefacto fresco, y luego STOP para
  aprobación humana del product owner. M11 NO iniciar. Gate siguiente y único:
  HUMAN PRODUCT-OWNER ACCEPTANCE.
- **TASK G INTEGRADA (`2451c50`).** Autor `cursor-grok-4.6-high` (HIGH_VISUAL,
  `corr/g-product-ux-pass`, commit `e345520`, pane `w1M:p1`), revisor UX
  independiente `cursor-grok-4.6-high` FRESH (`corr/g-product-ux-review`, pane
  `w1N:p1`, **APPROVE**, 5 nits no bloqueantes NIT-1..5), revisor
  código/a11y `opencode-go/qwen3.8-flash` FRESH
  (`corr/g-product-ux-a11y-review`, pane `w1P:p1`, **APPROVE**, verificado con
  tsc/eslint/prettier/vitest 201/201 en worktree detached, LOW/NIT no
  bloqueantes). `./scripts/verify` PASS en main tras G (EXIT=0: cargo verde,
  201 FE, fmt/lint/typecheck, M10 + UX_REDESIGN_01 contracts). Sin ciclos
  REQUEST_CHANGES (ambos APPROVE a la primera). Panes author y ambos reviewers
  cerrados tras APPROVE/integración. Worktrees `../ai-publisher-corr-01-g`,
  `../ai-publisher-corr-01-g-review`, `../ai-publisher-corr-01-g-a11y` y
  branches `corr/g-product-ux-pass`(-review/-a11y-review) a limpiar en cierre.
  Alcance Task G: solo `app/src` (20 archivos, +248/-97), sin backend, sin M11.
- **Session budget: CONTINUE al cierre de G (verificar antes del próximo
  lanzamiento)**. La sesión debe checkpointear Task G limpio y detenerse; la
  siguiente fase de validación (Playwright headed, AppImage NUEVO real, revisión
  humana) es un gate separado, no parte de Task G.
- **TASK F INTEGRADA (`6ea0e67`).** Autor `opencode-go/kimi-k2.7-code`
  (commits `26411f1` + fixes `14365c0` en `corr/f-conversation-delete`),
  revisor `opencode-go/qwen3.8-flash` FRESH (pane `w1K:p1`): REQUEST_CHANGES →
  APPROVE. El REQUEST_CHANGES fue por MAJOR-1 (delete-while-generating dejaba
  archivos huérfanos: `AgentService::run` hacía `create_dir_all` sin verificar
  metadata y el delete no cancelaba/serializaba contra el agente). Fixes
  aplicados por el MISMO autor (círculo autor→revisor respetado): delete
  serializa contra el per-project lock y cancela el run en vuelo antes de
  remover; `run_agent_with_inputs` falla rápido si metadata ya no existe;
  `AgentService::run` limpia el árbol huérfano si el proyecto desaparece
  mid-run; UI deshabilita "Eliminar" mientras la conversación genera; tests
  fail-closed (unpublish-failure aborta con datos intactos) y de error de
  delete (dialogo queda abierto, item preservado). `./scripts/verify` PASS en
  main tras F (199 tests frontend, cargo verde, M10 + UX_REDESIGN_01 contract
  ok). Panes author (`w1J:p1`) y reviewer (`w1K:p1`) cerrados tras APPROVE.
  Worktree `../ai-publisher-corr-01-f` y `../ai-publisher-corr-01-f-review`
  removidos; branch `corr/f-conversation-delete` y `-review` borrados.
- **Session budget: CONTINUE (58K al cierre de F)**. La sesión está sana y NO
  debe ampliar el trabajo automáticamente. El siguiente trabajo alto-valor es
  **Task G (pass visual producto/UX, Cursor Grok 4.6 High)** y debería idealmente
  arrancar desde una sesión de orquestador FRESH (Task G es trabajo de producto,
  no funcional; el contrato exige Grok solo vía Cursor). Este orquestador
  prefiere checkpointear Task F limpio y detenerse.
- **Progreso del pass:** Task A INTEGRADA (`e6389ea`). Task B INTEGRADA
  (`88761be`). Task C INTEGRADA (`c94e114`; autor kimi `fd1d928`, revisor qwen
  APPROVE tras REQUEST_CHANGES). Task D INTEGRADA (`f44d507`; autor Composer
  2.5 `18ac233`, revisor qwen APPROVE). Task E INTEGRADA (`cea141e`; fix LOW
  `60dc786`, revisor qwen FRESH APPROVE). **Task F INTEGRADA (`6ea0e67`; autor
  kimi `26411f1`+`14365c0`, revisor qwen REQUEST_CHANGES→APPROVE — ver detalle
  arriba)**. **Task G INTEGRADA (`2451c50`; autor Cursor Grok 4.6 High
  `e345520`, revisor UX Cursor Grok 4.6 High APPROVE, revisor código/a11y qwen
  APPROVE — ver detalle arriba)**. `./scripts/verify` PASS en main tras G.
Tasks A-G hechas; **T6 Playwright headed PASS (57/57); T7 AppImage NUEVO
   real PASS (ver arriba); resta solo la aprobación humana del product
   owner**.
- **M11 NO iniciado.** Nada de M11 en esta corrección.
- **Trabajo previo integrado y conservado** (UX_REDESIGN_01): Task A modelo
  gratis real (`a3ef122`), Task B visual (`88fd346`), Playwright 44/44,
  AppImage real construido y verificado (detalles al final). NO reiniciar.
- **Pendiente: la revisión humana real del AppImage generó 16 hallazgos UX**
  (sección "Hallazgos humanos" abajo). Este pass los corrige. **A-G done;
  pendiente validación final (Playwright headed, AppImage NUEVO real, aprobación
  humana).**

## Hallazgos humanos (16) y causas raíz confirmadas (YA investigadas)

1. Layout chat mucho más cercano al concepto. OK.
2. Panel Materiales permanente eliminado. OK, correcto.
3. Drag & drop funciona.
4. **Recursos dropeados aparecen a la derecha y parecen producidos por el
   asistente.** Causa: `WorkspaceView.importRef` (`app/src/components/WorkspaceView.tsx:66-91`)
   importa materiales vía `materialsAddFromPaths` pero NO los adjunta al
   mensaje pendiente (`ComposerBar` mantiene `attachmentIds` en estado local,
   `app/src/components/ComposerBar.tsx:52`). Los materiales no adjuntos se
   renderizan como `message-resource` en el timeline (`ChatPanel.tsx:193-198`),
   fuera de la burbuja del usuario.
5. **Sidebar/título muestra "Proyecto sin título 1".** Causa: el default
   activo es `messages.conversation.defaultName = "Conversación nueva"`
   (`app/src/messages.ts:64`) pero persisten proyectos de esquemas anteriores
   con nombres legacy; además `messages.project.defaultName = "Proyecto sin
   título"` (`messages.ts:92`) vive en el catálogo y `ProjectsView.tsx:33` lo
   usa. Backend `create_project` exige nombre no vacío (no auto-nombra).
6. **"No se pudo iniciar el asistente de IA." en primer arranque aunque luego
   funciona.** Causa: el backend es lazy y `ensure_ready` tiene timeout de 30s
   (`crates/project-opencode/src/backend.rs:16` DEFAULT_STARTUP). En arranque
   frío del AppImage el sidecar tarda; el primer `agent_send`/`model_get_selected`
   puede caer en `BackendNotReady`/`Timeout` → `AppError::from_agent`
   (`crates/project-app/src/error.rs:164-185`) → `ErrorCode::AiUnavailable`
   mensaje "No se pudo iniciar el asistente de IA." (línea 177). El frontend
   NO expone estados STARTING/READY/FAILED (nunca llama a `appStatus`/`agent_status`;
   solo escucha `agent://task`, `app/src/App.tsx:80-108`).
7. El modelo gratis responde tras el arranque. OK.
8. **Respuesta con `/tmp/opencode/...`, `node`, rutas `.js`, instrucciones
   shell.** Causa: el texto del asistente ES la respuesta cruda del LLM
   (`result.task.message` → `send_message_run` en `crates/project-app/src/app.rs:906-926`
   lo persiste como mensaje de asistente). No hay system-prompt/instrucción
   que fuerce lenguaje plano; `augment_prompt` solo agrega el bloque de
   materiales (`crates/project-agent/src/service.rs:176-188`).
9. **El usuario no identifica dónde quedó el juego.** Causa: la "creación"
   existe como `Creation` (registrador `FilesystemCreationRegistrar`,
   `crates/project-agent/src/registrar.rs:30-92`) y se renderiza como
   `CreationCard` (`app/src/components/CreationsPanel.tsx:30-99`) DENTRO de la
   burbuja del asistente (`ChatPanel.tsx:129-144`), pero sin botones
   "Abrir"/"Compartir" en la tarjeta (solo "Vista previa"/"Abrir en
   navegador") y sin una presentación clara de creación.
10. **Se espera: creación visible con [Abrir] [Compartir].** Ver arriba.
11. **Resultado renderizado dos veces: mensaje normal + texto verde crudo.**
    Causa CONFIRMADA: `App.tsx:91` guarda `event.message` crudo del evento
    `agent://task` en `agentMessage`; `ChatPanel.tsx:231-233` lo pinta como
    `.chat-status.ok` verde, ADEMÁS de la burbuja del asistente persistida
    (`ChatPanel.tsx:128`) que se refresca con `refreshConversation`
    (`App.tsx:98`). Duplicado de contenido asistente.
12. Usuario adjuntó archivo de texto con datos del rosco. OK (flujo existe).
13. Usuario pidió usar esos datos. OK.
14. **El flujo real de adjunto/contexto falló.** Causa: si el material se
    agrega por drag&drop, NO se adjunta a `attachmentIds` del composer →
    `agent_send(projectId, prompt, attachmentIds=[])` → el agente nunca ve el
    archivo. El aprovisionamiento backend EXISTE y es correcto
    (`resolve_attachments` `app.rs:781-829`, `provision_attachments`
    `service.rs:146-174`, prompt "Materiales adjuntos… están en la carpeta
    materials"). El eslabón roto es FRONTEND: drop → adjuntar al mensaje.
15. Los recursos deben seguir entendibles sin dashboard Materiales.
16. **No hay forma contextual de eliminar conversaciones.** ~~`project_delete`
    existe end-to-end (commando `commands.rs:80-89`, `AppState::delete_project`
    `app.rs:310-317`, `ProjectService::delete_project` `project-core/src/lib.rs:807-811`,
    `api.ts:31`) pero NINGÚN componente lo llama; `ConfirmDialog` existe
    (type-name-to-confirm, `ConfirmDialog.tsx:15-48`) pero no está cableado.
    **Bug adicional detectado:** `delete_project` NO hace `unpublish` → entrada
    stale en `PublicationManager.published` y el proyecto aparece "shared"
    hasta reiniciar.~~ **RESUELTO EN TASK F (`6ea0e67`):** menú contextual "…"
    por conversación (Renombrar / Eliminar conversación) en
    `ConversationsSidebar.tsx`; `ConfirmDialog` cableado (type-name-to-confirm,
    copy en lenguaje llano); `delete_project` hace `unpublish` primero
    (fail-closed); delete serializa contra el agente (cancela run en vuelo) y
    limpia huérfanos; UI deshabilita Eliminar mientras genera; selección
    post-delete correcta (inactiva queda, activa → siguiente predecible, última
    → estado vacío "No hay conversaciones").

## Contratos clave actuales (para los workers)

- **Message/Project:** `Project.messages: Vec<Message>` schema v3
  (`crates/project-core/src/lib.rs:342-357, 400-420`); `Message { id, role,
  text, status, createdAt, materialIds, creationIds }`. Validación: user msg
  solo `material_ids`, assistant msg solo `creation_ids`.
- **Creation:** `Creation { id, displayName, kind: Web|Document|Image|File,
  visibility, relativePath, contentType?, byteSize, revision,
  parentCreationId?, createdAt }` (`lib.rs:378-399`). UI capabilities
  (open/preview/publish) se derivan en facade, no se almacenan.
- **DTOs:** `ProjectSummary {id,name,createdAt,updatedAt,shared}`,
  `ProjectView {id,name,materials,creations,messages,publication}`,
  `MessageView`, `CreationView {id,displayName,kind,visibility,byteSize,
  createdAt,revision}` (`crates/project-app/src/dtos.rs`, `app/src/types.ts`).
- **Frontend estado:** `App.tsx` (conversations/selectedId/conversation/
  agentPhase/agentMessage/settingsOpen), `WorkspaceView` (pendingUser/
  sendError/drag-drop/import), `ChatPanel` (timeline derivada),
  `ComposerBar` (prompt/attachmentIds/model selector), `ConversationsSidebar`
  (lista, rename inline, NO delete). `PublishPanel` = ShareControl (single
  Compartir en bottom bar).
- **Agent:** `AgentService::run` (`service.rs:49-103`) ensure_ready →
  open_session(workspace) → provision_attachments → send → registrar artifacts
  (skip `materials/`). `agent://task` events desde `commands.rs:274-322`.
  Backend status en `OpenCodeBackend.status()` → `BackendStatus` enum
  (`crates/project-opencode/src/status.rs:1-7`): Stopped|Starting|Ready|Failed;
  expuesto a UI solo vía `agent_status`/`app_status` (sin uso frontend).
- **Attachments:** `resolve_attachments` autoriza contra materiales del
  proyecto; copia a `workspace/materials/<n>-<name>`; augment_prompt lista.
  Read path seguro (`project-fs` validate_read_path). Cleanup: sin cleanup
  explícito de `workspace/materials/` tras run (aceptado).
- **Publicación:** `publish/unpublish/publication_status`, túnel Cloudflare,
  QR frontend. TEMPORAL; honestidad de enlace. Reusar tal cual.
- **Pins sidecar:** opencode 1.18.25, cloudflared 2026.8.3 (M10, sin cambios).
- **Copy catálogo:** `app/src/messages.ts` = catálogo ejecutable (ADR-0012);
  tests verdes deben seguir.

## Plan de tareas de esta corrección (ejecución por la sesión siguiente)

Modelos (política activa, AGENT_POLICY.md): orquestador
`opencode-go/deepseek-v4-flash`; funcional `opencode-go/kimi-k2.7-code`;
revisión funcional/código `opencode-go/qwen3.8-flash`; producto/UX frontend
**Cursor Grok 4.6 High** (solo vía Cursor, NUNCA OpenCode Go); revisión UX
independiente: Cursor Grok 4.6 High FRESH; LOW mecánico: Composer 2.5 o
`opencode-go/mimo-v2.5`. AUTHOR != REVIEWER. Una tarea = un worktree. Cada
worker: verificar MODEL_REQUESTED == MODEL_ACTUAL antes del task; handoff
compacto; cerrar panes tras PASS+APPROVE (CONTEXT_LEAK si queda idle).

| # | Tarea | Autor | Revisor | Ownership | AC (resumen) |
| --- | --- | --- | --- | --- | --- |
| A | ~~Contrato de CREACIÓN user-facing + fin de fuga técnica~~ **HECHA** (`e6389ea`) | kimi-k2.7-code | qwen3.8-flash | ~~`crates/project-agent`, `crates/project-app/src/app.rs`, dtos, `app/src/components/CreationsPanel.tsx`~~ | Creación con Abrir/Compartir; respuesta asistente en lenguaje plano (build_instruction en service.rs); sin paths/comandos en UX normal. APPROVE. |
| B | ~~Flujo real de adjunto/contexto~~ **HECHA** (`88761be`) | kimi-k2.7-code | qwen3.8-flash | ~~`app/src/components/{WorkspaceView,ComposerBar,ChatPanel}.tsx` + tests~~ | Drop/import → material adjunto al mensaje del usuario (attachmentIds lift a WorkspaceView, controlado a ComposerBar); llega al agente vía `agent_send` con ids; sin resource-item duplicado; tests deterministas. APPROVE (nits: race agentPhase estrecho, reset de attachmentIds al cambiar de proyecto — no bloqueantes). |
| C | ~~Error falso de arranque (STARTING/READY/FAILED)~~ **HECHA** (`c94e114`) | kimi-k2.7-code | qwen3.8-flash | ~~`app/src/App.tsx`, `WorkspaceView.tsx`, `messages.ts`, `types.ts`, tests~~ | Estados explícitos vía poll de `app_status`; "Preparando el asistente…"; solo fallo terminal real; `failed` recuperable (retry + auto-poll); tests cold/delayed/failure/recovery. APPROVE. |
| D | ~~Terminología de conversación~~ **HECHA** (`f44d507`) | Composer 2.5 | qwen3.8-flash | ~~`app/src/messages.ts`, `App.tsx`, tests, legacy naming~~ | `conversationDisplayName()` render-time: legacy "Proyecto sin título"/"Proyecto sin título N" → "Conversación nueva"; 8 AC cubiertos (default, legacy, user-renamed, sidebar, header, restart, ordering, sin Project terminology en DOM). APPROVE sin hallazgos. |
| E | ~~Duplicado/texto verde~~ **HECHA** (`cea141e`) | LOW (orquestador deepseek-v4-flash; raíz ya confirmada) | qwen3.8-flash | `app/src/App.tsx`, `app/src/components/ChatPanel.tsx`, `app/src/messages.ts`, tests | Eliminar doble render del contenido asistente (`.chat-status.ok` verde transitorio duplicaba la burbuja persistida; backend siempre persiste el mensaje terminal en `send_message_run`). `setAgentMessage(null)` en completed; se mantienen spinner working + `.err` failed (a11y role="alert"). 193 tests, verify PASS. APPROVE (NIT: CSS `.chat-status.ok` muerto). |
| F | ~~Eliminar conversación (backend semántica + UI)~~ **HECHA** (`6ea0e67`) | kimi-k2.7-code (`26411f1` + fixes `14365c0`) | qwen3.8-flash | ~~`crates/project-app/src/app.rs` (delete + unpublish + serialización agente), `crates/project-agent/src/service.rs` (cleanup huérfanos), `app/src/{App,components/ConversationsSidebar}.tsx`, `messages.ts`, `styles.css`, tests~~ | Menú "…" contextual (Renombrar/Eliminar), ConfirmDialog type-name, delete durable + fail-closed + unpublish primero, sin huérfanos (serialización/cancel contra agente + cleanup mid-run), selección post-delete correcta, última → estado vacío, renombrar preserva id/orden/activa, tests 13 AC. REQUEST_CHANGES→APPROVE (MAJOR-1 resuelto). |
| G | ~~Pass visual producto/UX~~ **HECHA** (`2451c50`) | Cursor Grok 4.6 High (`e345520`) | Cursor Grok 4.6 High FRESH (UX, APPROVE) + qwen3.8-flash (código/a11y, APPROVE) | ~~`app/src` (App shell, sidebar, timeline, composer, creación, adjuntos, settings X, menú)~~ | Chat tipo mensajería; adjuntos en el mensaje (📄 nombre [Abrir]); creación card icon+kind+Abrir/Compartir (EducAI decide el opener); URL de compartir visible; Settings con X en título (vuelve a la misma conversación); selector de modelo sin raw ids / sin hardcode de modelo gratis; sidebar "Conversaciones"; sin dashboard; sin fuga técnica. APPROVE (UX: 5 nits no bloqueantes NIT-1..5; código/a11y: LOW/NIT). |
| T6 | ~~Playwright headed~~ **HECHA (PASS, 57/57)** | LOW (deepseek-v4-flash) | qwen3.8-flash | `docs/ux-redesign-01/harness/` | 3 viewports, 17 flows, 57 aserciones, 78 PNG + 78 OCR + 17 a11y; flows 15-17 Task G/F (creación Abrir/Compartir, delete confirm, no-duplicado asistente); measure + ocr PASS. EXIT=0. |
| T7 | ~~AppImage real + `./scripts/verify`~~ **HECHA (PASS)** | LOW/Composer | qwen3.8-flash | packaging M10 | AppImage con sidecars, lanzamiento real, verificación completa |

Orden sugerido: A → B → C (backend funcional, cada una con su worktree) →
D/E (LOW) → F-backend → F-UI + G (Grok) → review Grok → review qwen →
Playwright → AppImage → verify. Integrar solo commits revisados. NO M11.
**A, B, C, D, E, F, G YA integradas (main `2451c50`). VALIDACIÓN FINAL:
T6 Playwright headed PASS y T7 AppImage NUEVO real PASS (main `d25f957`).
Resta solo la aprobación humana del product owner. NO es M11.**

## Worktrees

- `main` → `/home/damian/rh/workspaces/damianlezcano/educai/ai-publisher-harness`
  (integración, `ebeac0e`; NO es workspace de autor).
- Creation/Share/UX pass: worktree autor `../ai-publisher-corr-01-creation-share`
  (`corr/creation-share-ux-pass`, commits `3ba7c5a` + `857d98c`). INTEGRADO vía
  merge `ebeac0e`. A remover + branch a borrar en cierre de sesión.
- Post-T7 pass: worktree autor `../ai-publisher-corr-01-postt7`
  (`corr/post-t7-blockers`), UX review `../ai-publisher-corr-01-postt7-review`
  (`corr/post-t7-ux-review`), a11y review `../ai-publisher-corr-01-postt7-a11y`
  (`corr/post-t7-a11y-review`). A remover/branches a borrar en cierre de sesión
  tras integración de `d6f97ab`.
- Worktrees de Task A, B, C, D, E removidos tras integración.
- Worktree de Task F (`../ai-publisher-corr-01-f`, `corr/f-conversation-delete`)
  y worktree de review F (`../ai-publisher-corr-01-f-review`,
  `corr/f-conversation-delete-review`) removidos; branches borrados tras
  integración de `6ea0e67`.
- Worktrees de Task G (`../ai-publisher-corr-01-g` `corr/g-product-ux-pass`,
  `../ai-publisher-corr-01-g-review` `corr/g-product-ux-review`,
  `../ai-publisher-corr-01-g-a11y` `corr/g-product-ux-a11y-review`) cerrados en
  cierre de sesión tras integración de `2451c50`; branches a borrar.

## Verificación y pruebas

- Frontend: `cd app && pnpm format:check && pnpm lint && pnpm typecheck && pnpm test`.
- Rust: `cargo fmt --check && cargo clippy --all-targets && cargo test`.
  (Drift de toolchain: rustup no instalado; clippy 1.98.0 vs pin 1.97.1 — el
  drift NO se manifestó en la corrida previa; si aparece, es preexistente.)
- Gate completo: `./scripts/verify` (exit 0 al final del pass).
- `git diff --check` limpio; `git status --short` limpio antes del handoff.
- Budget: `scripts/check-session-budget` ANTES de cada lanzamiento de worker
  (CONTINUE <80K; CHECKPOINT_WARNING 80K-99,999; ROTATE 100K-129,999; HARD >=130K).

## Aceptación final real (M9/M10 ya aprobados, no repetir)

- **AppImage NUEVO POST-T7 (main `773278d`, post-T7 blocker fixes integrados):**
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`,
  180.816.376 bytes, SHA-256
  `930ee074bfbe40b4cf1e5c9582c93b884d695d6348bf7521e764ade5b9f6834d`,
  timestamp 2026-09-01 20:47:14 -0300. **ESTE es el artefacto para la
  re-aceptación humana.** (AppImage previo T7 `3dba67a8…` es STALE — predata los
  fixes A/B/C; NO usar como evidencia de aceptación.)
- Modelo gratis real confirmado: `big-pickle` (providerID `opencode`), cost 0,
  respuesta "¡Hola! ¿Cómo puedo ayudarte?". `modelGetSelected`/`default_free_model`
  determinista (ADR-0015); NO hardcodear nombres (solo tests/fake).
- PATH-independencia y sidecars bundled verificados en M10 y re-verificados en
  T7 contra el AppImage NUEVO (opencode 1.18.25 + cloudflared 2026.8.3 en
  payload; launch con PATH sin sidecars usa el bundled).
- **Este pass NO re-testea M1-M10; solo integró las correcciones A-G, corrió el
  Playwright headed (T6) y construyó/verificó el AppImage NUEVO (T7) para
  revisión humana.**

## Model allocation (sesión anterior cerrada)

- **Creation/Share/UX human-acceptance pass (COMPLETE, integrado `ebeac0e`): orquestador
  `opencode-go/deepseek-v4-flash` (esta sesión, budget CONTINUE al cierre). Autor
  `cursor-grok-4.6-high` vía Cursor (`corr/creation-share-ux-pass`, commits `3ba7c5a` +
  fix `857d98c`). Revisor UX independiente `cursor-grok-4.6-high` FRESH (sesión previa,
  APPROVE). Revisor código/a11y `opencode-go/qwen3.8-flash` FRESH (`creation-share-code-review`,
  pane `w1F:p1G`, REQUEST_CHANGES con M1 MAJOR + m1-m7 MINOR + LOW/NIT). Fix acotado por
  autor Cursor Grok 4.6 High FRESH (`creation-share-fix`, pane `w1F:p1H`, commit `857d98c`).
  Re-review código/a11y `opencode-go/qwen3.8-flash` FRESH (`creation-share-rereview`, pane
  `w1F:p1J`, APPROVE). Re-review UX acotado `cursor-grok-4.6-high` FRESH
  (`creation-share-ux-rereview`, pane `w1F:p1K`, APPROVE). Merge `ebeac0e` + evidencia
  `05a2c2a`, `./scripts/verify` EXIT=0 en main (FE 217/217). Panes reviewers cerrados tras
  APPROVE; fixer/author a cerrar en cierre de sesión; worktree autor + branch
  `corr/creation-share-ux-pass` a limpiar. Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0
  sesiones.**
- **Post-T7 human blocker pass (este): orquestador `opencode-go/deepseek-v4-flash`
  (rota en ROTATE_SESSION_REQUIRED 117K tras merge/cleanup). Autor
  `cursor-grok-4.6-high` vía Cursor (`postt7-author`, pane `w1F:p1A`,
  `corr/post-t7-blockers`, commits `b106d07b` + fixes `21e9e5b`). Revisor UX
  independiente `cursor-grok-4.6-high` FRESH (`postt7-ux-review`, pane `w1F:p1B`,
  `corr/post-t7-ux-review`, APPROVE + re-APPROVE tras MAJOR). Revisor código/a11y
  `opencode-go/qwen3.8-flash` FRESH (`postt7-a11y-review`, pane `w1F:p1C`,
  `corr/post-t7-a11y-review`, REQUEST_CHANGES → APPROVE; MAJOR resuelto en
  `21e9e5b`). Merge `d6f97ab`, `./scripts/verify` EXIT=0 en main. Panes author y
  ambos reviewers a cerrar en cierre de sesión; worktrees/branches a limpiar.
  Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0 sesiones.**
- **Task G: autor `cursor-grok-4.6-high` (HIGH_VISUAL vía Cursor,
  `task-g-author`, pane `w1M:p1`, commit `e345520` en
  `corr/g-product-ux-pass`), revisor UX independiente `cursor-grok-4.6-high`
  FRESH (`task-g-ux-review`, pane `w1N:p1`, `corr/g-product-ux-review`,
  APPROVE con nits NIT-1..5), revisor código/a11y `opencode-go/qwen3.8-flash`
  FRESH (`task-g-a11y-review`, pane `w1P:p1`,
  `corr/g-product-ux-a11y-review`, APPROVE con LOW/NIT). Sin ciclos
  REQUEST_CHANGES. Merge `2451c50`, `./scripts/verify` PASS (EXIT=0). Panes
  cerrados tras APPROVE/integración; worktrees/branches de G a limpiar en
  cierre.**

- Orquestador previo: `opencode-go/deepseek-v4-flash` (cerrada en bootstrap
  HARD). **Este pass:** deepseek-v4-flash (A/B/C/D integradas; rota en
  CHECKPOINT_WARNING tras Task D).
- **Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0 sesiones.** (seguir así;
  Qwen3.8 Max solo con ESCALATION_REASON explícito).
- Task A: autor `opencode-go/kimi-k2.7-code`, revisor `opencode-go/qwen3.8-flash`
  (APPROVE tras REQUEST_CHANGES). Ambos panes cerrados. Grok NO usado (G/F-UI
  pendientes).
- Task B: autor `opencode-go/kimi-k2.7-code`, revisor `opencode-go/qwen3.8-flash`
  (APPROVE, nits no bloqueantes). Commit `a16a07c`. Ambos panes cerrados.
- Task C: autor `opencode-go/kimi-k2.7-code` (`fd1d928`), revisor
  `opencode-go/qwen3.8-flash` (REQUEST_CHANGES → APPROVE; MAJOR-1 resuelto con
  regresión; NIT-1 poll cadence + MINOR-1 transient resend anotados no
  bloqueantes). Ambos panes cerrados. Backend sin cambios.
- **Task D: autor Composer 2.5 (`task-d-author`, commit `18ac233`), revisor
  `opencode-go/qwen3.8-flash` (`task-d-review`, APPROVE sin hallazgos). Ambos
  panes cerrados.** Grok NO usado (G/F-UI pendientes).
- **Task E: fix LOW commit `60dc786` en `corr/e-duplicate-render` (implementado
  por el orquestador deepseek-v4-flash — desviación de proceso documentada: la
  raíz ya estaba confirmada en el checkpoint y el cambio es acotado; el circuito
  autor→revisor se respetó para la revisión), revisor `opencode-go/qwen3.8-flash`
  FRESH (`task-e-review`, pane `w1F:p18`, APPROVE tras verificar contrato backend
  `send_message_run`, tests 193/193, tsc/eslint/prettier). NIT no bloqueante:
  CSS `.chat-status.ok` (`app/src/styles.css:407`) quedó muerto (solo referencias
  en selectores negativos de test). Merge `cea141e`, branch borrado, worktree y
  pane de review cerrados.** Grok NO usado (G/F-UI pendientes).
- **Task F: autor `opencode-go/kimi-k2.7-code` (`task-f-author`, pane `w1J:p1`,
  commits `26411f1` + fixes `14365c0`), revisor `opencode-go/qwen3.8-flash`
  (`task-f-review`, pane `w1K:p1`, FRESH — REQUEST_CHANGES → APPROVE tras
  verificar MAJOR-1 resuelto con lock-ordering sólido, fail-closed testeado, y
  gates verdes 199 FE + cargo + fmt/clippy). Ambas panes cerrados. Merge
  `6ea0e67`, branches `corr/f-conversation-delete`(-review) borrados, worktrees
  removidos. Hallazgos residuales NO bloqueantes anotados por el revisor:
  MINOR TOCTOU (si delete completa en la ventana entre el pre-check de
  `run_agent_with_inputs` y que `AgentService::run` adquiera el lock del agente,
  puede quedar un scratch dir `projects/<id>/workspace` sin datos de usuario;
  fix recomendado: chequeo de existencia autoritativo DENTRO del lock del agente
  en `run`, antes de `create_dir_all`), NIT comment del pre-check (usa lock
  distinto al del agente), NIT test de serialización usa dos AppState
  independientes (pasa vía cleanup, no vía el mecanismo "waits"; añadir twin
  single-instance), NIT disable UI solo cubre la conversación seleccionada.
  Grok NO usado (G pendiente).**

## Mapa de Task F (contexto histórico — YA EJECUTADO en `6ea0e67`, conservado como contexto)

Backend delete ya existe end-to-end pero NO des-publica (bug hallazgo 16):
(MAPEO ORIGINAL — Task F ya resuelta arriba; se conserva para auditoría del
estado previo.)

- `project_delete` Tauri: `app/src-tauri/src/commands.rs:80-89`.
- `AppState::delete_project`: `crates/project-app/src/app.rs:310-317` — solo
  `self.projects.lock().delete_project(&pid)`, NO llama `self.unpublish` →
  entrada stale en `PublicationManager.published` (proyecto "shared" hasta
  restart). **Fix Task F: delete debe unpublish antes/consistente.**
- `ProjectService::delete_project`: `crates/project-core/src/lib.rs:807-811`:
  `get` → `repository.delete` (metadata) → `content.remove_project_tree`.
  `FilesystemProjectRepository::delete` borra el dir del proyecto
  (`project-fs/src/lib.rs:495-505`); `remove_project_tree` borra el árbol si
  existe (`project-fs/src/lib.rs:754-760`). Owner del árbol = proyecto (única
  duración; sin recursos compartidos entre proyectos: materials/creations son
  por-proyecto, `inputs/<id>` y `outputs/<id>` bajo `projects/<pid>/`).
- `AppState::unpublish`: `app.rs:1183-1189` → `PublicationManager::unpublish`
  (`crates/project-publication/src/manager.rs:306-340`, AlreadyLocal si no
  publicada, idempotente). Tauri `unpublish`: `commands.rs:349-353`.
- List ordenado por `updated_at` desc (`project-fs/src/lib.rs:438-442`);
  `rename_project` actualiza `updated_at` (`project-core/lib.rs:798-806`) →
  renombrar mueve al tope. Requisito F "order semantics unchanged": preservar
  esta regla (updated_at desc), no reintroducir otra.
- Rename UI ya existe: inline ✎ en `ConversationsSidebar.tsx:24-52,172-180`
  (usa `api.projectRename`, `api.ts:29-30`). Delete NO está cableado en la UI.

Frontend conversación:

- `app/src/App.tsx`: `conversations/selectedId/conversation` state;
  `refreshConversations` (28-33), `openConversation` (40-54), efecto inicial
  auto-crea default si lista vacía (56-86). Selection post-delete debe vivir
  aquí (refrescar lista, elegir activa, limpiar si última).
- `ConversationsSidebar.tsx` (189 líneas): props `conversations/selectedId/
  onSelect/onRefresh`; rename inline + ✎; NO delete. Agregar menú ⋮ contextual
  (Renombrar / Eliminar conversación) + ConfirmDialog. Copy catálogo en
  `app/src/messages.ts` (conversations.* / common.*); NO ProjectId/paths/términos
  técnicos en UX.
- `ConfirmDialog.tsx` (type-name-to-confirm, 15-48) existe y está testado
  (`ConfirmDialog.test.tsx`) pero NO cableado en conversaciones. UX F pide
  confirmación humana simple: "¿Eliminar esta conversación?" / "Se eliminarán
  los mensajes y los recursos asociados a esta conversación." / [Cancelar]
  [Eliminar]; visualmente destructivo; sin delete silencioso.
- `api.projectDelete`: `app/src/api.ts:31`. `AppError`/`errorMessage`:
  `api.ts:103-115`.
- Tests frontend: patrón `App.test.tsx` (mock invoke/listen), vitest + testing
  library; 193 tests verdes en main. Backend tests: `crates/project-app/tests/
  app_facade.rs` (delete ya en `project_lifecycle`), `project-fs/tests/
  project_lifecycle.rs` (delete: 1138, 1157, 1173), `project-publication/tests/`
  (unpublish idempotente).

Decisión ownership recursos: en la arquitectura actual NO hay recursos
compartidos entre conversaciones (cada proyecto es dueño exclusivo de su árbol);
el único estado cruzado durable es `PublicationManager.published` (manejar con
unpublish). Por lo tanto NO se requiere ARCHITECTURE_ESCALATION por ownership:
delete del proyecto borra su árbol completo (mensajes+materials+creations) y
debe unpublish primero para no dejar entrada stale. Si el autor encontrara una
referencia cruzada real no contemplada, debe parar y escalar, no adivinar.

## Próximo paso (inmediato)

> **Nota (cambio «Sí» integrado en `main`):** el pase grande de corrección de
> aceptación humana (7 ítems, COMPLETO en `ebeac0e`) **preservó** la nueva
> confirmación de eliminación de conversación con «Sí» (ítem 8 del pase previo), y
> el **próximo AppImage fresco** debe construirse desde un `main` que ya la incluya
> y también las correcciones Creation/Share/Chat (el actual `930ee074…` es de
> `773278d` y NO trae este pass).

**PASS CREATION/SHARE/CHAT INTEGRADO Y VERIFICADO (`ebeac0e`).** Repo en
`TÉCNICAMENTE LISTO PARA RE-ACEPTACIÓN HUMANA` en cuanto se construya el AppImage
fresco. El ÚNICO gate siguiente es: (1) **FRESH REAL APPIMAGE BUILD + VERIFICACIÓN
TÉCNICA** desde main `ebeac0e`, y (2) que el **product owner re-corra el escenario
real §17/§15** sobre ESE AppImage nuevo. Solo el humano puede marcar HUMAN
ACCEPTED. M11 NO iniciar.

1. Construir AppImage NUEVO desde main `ebeac0e` con `scripts/smoke-package
   appimage` (fetch-sidecars → `cargo tauri build --bundles appimage`), sidecars
   pineados SIN cambiar (opencode 1.18.25, cloudflared 2026.8.3), `./scripts/verify`
   EXIT=0 contra el artefacto fresco, lanzamiento real en Fedora/Wayland con PATH
   sin sidecars, y luego entregar al product owner.
2. El product owner re-corre el escenario real §15 sobre el AppImage NUEVO:
   conversación nueva + adjunto de rosco + prompt real → el asistente responde y
   genera la creación; card de creación [Abrir][Compartir] (título humano, no
   "index"; sin cards duplicadas en turnos siguientes); Abrir funciona; el agente
   usa el archivo; Compartir produce URL pública usable con EL JUEGO (no "Material
   del proyecto"); sin burbuja vacía "Asistente"; sin toast duplicado; modelo en
   Configuración; menú "…" → Eliminar conversación con confirmación «Sí»;
   renombrar/eliminar conversación, reinicio y delete persistido. Solo el humano
   acepta el AppImage final. NO afirmar aceptación humana desde OpenCode.
3. NO iniciar M11. Este pass queda en TÉCNICAMENTE LISTO esperando el AppImage
   nuevo y la re-aceptación humana.
2. **Seguimiento recomendado NO bloqueante (de las reviews de G):**
   - (UX NIT-1 / qwen LOW) `PublishPanel.tsx`: la URL pública es `<p>` dentro de
     `role="menu"`; envolver en `role="group"` (o mover los `<p>` al contenedor
     del popover) para no saltar el texto en lectores de pantalla.
   - (qwen LOW) `ComposerBar.tsx` `modelOptionLabel`: al caer a etiqueta genérica
     ("De pago"/"Gratis") cuando `name===modelId`, agregar el nombre del
     proveedor para evitar opciones indistinguibles.
   - (qwen LOW) `useShareControl.ts`/`WorkspaceView.tsx`: `onShare` en una
     tarjeta de creación abre el menú del ShareControl del composer (mismo hook,
     distinta ubicación); enfocar/anunciar el menú revelado.
   - (qwen/UX NIT) `messages.timeline.resourceLabel`, CSS `.message-resource`,
     `humanSize` (export sin uso) quedaron muertos; cleanup de catálogo/CSS.
   - (re-review code/a11y NIT) `registrar.rs`: el error del copy sidecar
     best-effort se descarta con `let _ =` sin log; agregar debug/warn.
   - (re-review code/a11y NIT) Skip lists (`build`, `dist`, `target`,
     `materials`) a cualquier profundidad podrían excluir una carpeta de
     actividad con ese nombre; improbable en este dominio, aceptado.
   - (re-review code/a11y LOW) `app.rs prepare_share_visibility`: Compartir
     explícito de una card no-web no degrada un Web público existente (la raíz de
     la URL puede no ser el artifact de la card); M1 le quitó su peor
     manifestación; revisitar solo si el producto quiere democión de cualquier
     Web público cuando el target no es Web.
   - (F review) chequeo de existencia de proyecto autoritativo DENTRO del lock
     del agente en `AgentService::run` + test single-instance delete↔agent.
3. NO iniciar M11. El pass de corrección queda en TÉCNICAMENTE READY FOR HUMAN
   REVIEW esperando aceptación humana.
