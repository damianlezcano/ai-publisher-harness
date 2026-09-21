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
