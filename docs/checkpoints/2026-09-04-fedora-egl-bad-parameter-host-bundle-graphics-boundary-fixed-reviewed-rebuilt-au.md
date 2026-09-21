## Fedora EGL_BAD_PARAMETER host/bundle graphics boundary FIXED + REVIEWED + REBUILT + AUTOMATED-FEDORA-SMOKED — Linux portable AppImage READY FOR HUMAN FEDORA + KDE NEON VALIDATION (2026-09-04)

- **Scope:** bounded Linux packaging completion only (Fedora EGL_BAD_PARAMETER
  host/bundle graphics boundary). Quoted prompts and
  sharing/lifecycle/observability remain **HUMAN-PASS and untouched**. **M11 NOT
  STARTED.** Windows runtime remains **separate / untouched**. No application
  source or product behavior change.
- **Orchestrator/integrator:** OpenCode Go / DeepSeek V4 Flash (fresh
  completion session). **Session budget:** `SESSION_BUDGET: UNKNOWN` (valid
  telemetry absence, not a hard stop; fresh small-context session). **Independent
  Code/Packaging reviewer (round 1):** fresh OpenCode Go / **Qwen 3.8 Flash**
  (`graphics-reviewer`, Herdr pane `w1Q:p3`, `--model opencode-go/qwen3.8-flash`)
  = **APPROVE** (all 7 review criteria PASS, empirically verified). **Independent
  Code/Packaging reviewer (round 2, fix):** fresh OpenCode Go / **Qwen 3.8
  Flash** (`graphics-reviewer2`, Herdr pane `w1Q:p4`) = **APPROVE**. No GPT
  through OpenCode Go; Cursor not used.
- **Fedora human failure (original, proven by prior Codex session):** the
  AppImage aborted on Fedora with `Could not create default EGL display:
  EGL_BAD_PARAMETER`. **Failing process:** `WebKitWebProcess`.
- **Exact root cause (authoritative, do NOT reopen):** the AppImage bundled the
  **Ubuntu 24.04** Wayland client/runtime libraries (`libwayland-client`,
  `libwayland-cursor`, `libwayland-egl`, `libwayland-server`) while the target
  host supplied the **Fedora GLVND / Mesa / GBM / DRM** graphics stack. AppRun
  prepends `usr/lib` to `LD_LIBRARY_PATH`, so the bundled Ubuntu Wayland ABI
  shadowed the host stack and Mesa's EGL rejected the mixed runtime with
  `EGL_BAD_PARAMETER`. The prior bounded disposable-AppDir experiment proved
  that removing only those four bundled `libwayland*` copies eliminated the EGL
  abort and let runtime startup proceed.
- **Author implementation commit:** `14f1202` `fix(packaging): preserve host
  Wayland graphics boundary` (author worktree `.worktrees/linux-graphics-runtime`,
  branch `m10/linux-graphics-runtime`). 5 files, +75: new
  `packaging/linux/prepare-graphics-appdir` (removes exactly the four proven
  `libwayland-client/cursor/egl/server` sonames from `usr/lib`), new
  fail-closed `scripts/check-graphics-appdir` (rejects those four sonames plus
  bundled `libEGL/libGL/libGLX/libOpenGL/libGLES*/libgbm/libdrm/lib*_mesa/
  libGLdispatch/libGLX_indirect`, `usr/lib/dri`, `usr/share/glvnd`),
  `packaging/linux/build-linux-appimage` (wire both), `scripts/
  test-distribution-contracts` (new pins), `docs/distribution/DISTRIBUTION.md` (new
  "Linux graphics-runtime boundary" section).
- **Bounded review fix (round 2):** the first controlled build surfaced one
  packaging defect: the new graphics gate extracts the packaged AppImage into a
  temp workdir via `( cd "$workdir" && "$artifact" --appimage-extract )`, but
  `$artifact` is a **relative** path, so after the subshell `cd` it failed with
  exit 127. Fixed in commit `a8326c3` `fix(packaging): gate the graphics
  boundary on the absolute artifact path` (`artifact="$(readlink -f
  "$artifact")"` before the gates + matching contract pin). `bash -n` clean,
  `./scripts/test-distribution-contracts` PASS, empirical reproduction
  confirmed by the fresh Qwen reviewer; round-2 review **APPROVE**.
- **Qwen round-1 APPROVE (14f1202, all 7 criteria PASS):** (1) graphics boundary
  removes ONLY the four proven `libwayland*` sonames — no broad deletion, all
  other bundled libs untouched; (2) host/bundle contract explicit and
  maintainable in `docs/distribution/DISTRIBUTION.md`; (3) WebKit helper packaging has ZERO
  diff (`prepare-webkit-appdir`/`check-webkit-appdir`/`check-appimage-webkit-
  runtime` untouched) — `WebKitNetworkProcess`/`WebKitWebProcess`/`WebKitGPUProcess` +
  injected bundle preserved, relative-path rewrite intact, no regression to host
  absolute Ubuntu helper paths; (4) no `WEBKIT_DISABLE_DMABUF_RENDERER` /
  `LIBGL_ALWAYS_SOFTWARE` / any MESA_/Wayland env override anywhere; (5) GLIBC
  gate `check-appimage-glibc "$artifact" 2.39` preserved verbatim, deletion-only
  change, no glibc bundling; (6) graphics gate fail-closed and wired to the
  ACTUAL assembled payload (`--appimage-extract` into a mktemp workdir with EXIT
  trap) — empirically rejects each forbidden lib, dri/, glvnd/, and missing
  AppDir/usr/lib; contract pins additive on top; (7) scope: diff touches only
  packaging/scripts/docs — no chat, prompts, attachments, Creations, sharing,
  lifecycle, Settings/UI, or Windows packaging changes.
- **Integration:** `git merge --ff-only` onto main in two fast-forward steps:
  `14f1202` then `a8326c3` — author commits integrated with provenance intact
  (no squash). **Final integrated HEAD = `a8326c3`.** Working tree clean (only
  untracked `.worktrees/`).
- **`./scripts/verify` on integrated main `a8326c3` EXIT=0** (log
  `/tmp/opencode/verify-graphics-integrated.log`): FE **244/244** in 21 files,
  **Rust 1162 passed in 85 suites**, clippy/fmt clean, `fetch-sidecars --check`,
  `test-distribution-contracts` **PASS** (incl. new graphics pins),
  M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check.
- **Controlled Ubuntu 24.04 rebuild from integrated main `a8326c3`:**
  `./scripts/package linux-appimage` **EXIT=0** (log
  `/tmp/opencode/build-graphics-integrated.log`). Base image
  `educai-linux-portable:ubuntu-24.04` (image id `645eedde30d7`), Ubuntu
  24.04.4 LTS, glibc 2.39. **WebKitGTK 2.52.6**
  (`libwebkit2gtk-4.1-0 2.52.6-0ubuntu0.24.04.1`). tauri linuxdeploy step
  aborted as documented (static sidecars) → documented appimagetool fallback;
  all three gates ran inside the build and passed before the artifact was
  reported.
- **Final AppImage (human-validation artifact, built from integrated main):**
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`,
  **148,650,488 bytes**, built 2026-09-04 14:43:52 -0300, source HEAD
  `a8326c3` (working tree clean before and after the build), **SHA-256
  `94b26f6b98fe29d4a5621cd46ba60cbb7b19691e9dcc90f6f4a853af4a2e90f0`**. Payload
  sidecars byte-identical to pins: **opencode 1.18.25** (`d91e0d33…`) and
  **cloudflared 2026.8.3** (`f29324fe…`), verified in extracted payload. This is
  the ONLY artifact for human validation on both targets.
- **Post-build gates on the final AppImage (all run against the actual
  artifact):** (1) **WebKit helper gate PASS** — `scripts/check-appimage-webkit-
  runtime` → `check-webkit-appdir` PASS: `WebKitNetworkProcess` (0755),
  `WebKitWebProcess` (0755), `WebKitGPUProcess` (0755), injected bundle present,
  no absolute host helper path remains, relative helper + injected-bundle paths
  configured (`lib/x86_64-linux-gnu/webkit2gtk-4.1`; lib size stable at
  95,183,864 bytes). (2) **Graphics-boundary gate PASS** —
  `scripts/check-graphics-appdir` PASS against the extracted payload: the four
  forbidden `libwayland-client/cursor/egl/server` sonames are ABSENT from
  `usr/lib`, and no bundled `libEGL/libGL/libGLX/libOpenGL/libGLES*/libgbm/
  libdrm/lib*_mesa/libGLdispatch/libGLX_indirect`, no `usr/lib/dri`, no
  `usr/share/glvnd` anywhere (the only `im-wayland.so` hit is a GTK input-method
  module, not a Wayland client library copy — correctly out of scope). (3)
  **GLIBC gate PASS** — `scripts/check-appimage-glibc <artifact> 2.39`: **136
  ELF files** inspected (140 prior − 4 removed wayland libs), all require
  **GLIBC <= 2.39**; helpers max 2.34; injected bundle 2.2; **no glibc
  bundled; baseline unchanged**. (4) **Diff/package integrity PASS** — git diff
  --check clean; payload sidecar SHAs byte-identical to the pins; host artifact
  SHA equals the container-reported SHA.
- **Automated Fedora smoke (current Fedora 44 host, real display):**
  `./EducAI_0.1.0_amd64.AppImage --debug` (log
  `/tmp/opencode/fedora-smoke-final.log`) — **NO EGL_BAD_PARAMETER**, **NO**
  `Unable to spawn child process` / WebKitNetworkProcess-missing error, the
  bundled backend starts (`backend spawned pid=557372` →
  `backend ready version=1.18.25`), the app runs the full 60s window without
  aborting (SIGTERM from timeout → clean `app shutdown requested (signal)` →
  `backend stopped pid=557372` → `app shutdown complete`), owned-child cleanup
  succeeds, **zero process/mount residue** after the run. `canberra-gtk-module`
  / `pk-gtk-module` load warnings classified non-fatal as before. This is an
  **automated** smoke — **NOT a human UI pass**; the product owner must still
  validate visual rendering, chat, sharing, and clean close.
- **Human validation targets (SAME AppImage, SHA `94b26f6b…`):** TARGET A =
  Fedora workstation; TARGET B = KDE Neon User Edition / Ubuntu 24.04 noble
  (same artifact, **no rebuild per target**). Launch, UI render, simple chat,
  Preview/Abrir, Cloudflare sharing, public URL, clean owned-child shutdown.
  **Human Fedora validation = PENDING. Human KDE Neon validation = PENDING. Do
  NOT claim Linux HUMAN-PASS.**
- **Windows:** separate native build, untouched this pass. **TECHNICALLY READY
  FOR WINDOWS RUNTIME VALIDATION**, no Windows PASS. Do not start Windows
  runtime validation.
- **Quoted prompts: HUMAN-PASS (untouched). Sharing/lifecycle/observability:
  HUMAN-PASS (untouched). GLIBC policy <= 2.39 unchanged. Ubuntu 24.04 baseline
  unchanged. M11 NOT STARTED.**
- **Status: LINUX PORTABLE APPIMAGE READY FOR HUMAN FEDORA + KDE NEON
  VALIDATION.** No human validation claimed. STOP — do not start Windows or M11.
