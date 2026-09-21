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
