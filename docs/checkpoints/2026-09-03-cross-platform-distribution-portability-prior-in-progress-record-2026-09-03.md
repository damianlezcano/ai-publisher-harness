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
