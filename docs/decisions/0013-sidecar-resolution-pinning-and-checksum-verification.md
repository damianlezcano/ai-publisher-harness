# ADR-0013: Sidecar resolution, pinning, and checksum verification

- Status: Accepted

## Context

M10 introduces the first installable Linux artifacts (AppImage, then RPM) and,
per ADR-0001 and ADR-0005, must bundle OpenCode and `cloudflared` as pinned,
target-specific sidecars instead of requiring a non-technical user to install
them from `PATH`. Today the app resolves `opencode` as a bare name (resolved by
the OS at spawn time) and `cloudflared` via `PathBinaryResolver`; neither is
bundled (ADR-0005 defers the `cloudflared` sidecar to M10/M11, ADR-0001 defers
target-specific sidecars to M10).

Bundling third-party binaries makes them part of the app's trust boundary: they
run with the user's privileges, read the project workspace, and reach the
network. The source and authenticity of those binaries therefore become a
security-invariant concern, not a convenience detail.

## Decision

1. **Runtime resolution lives in the shell, policy logic is Tauri-free.**
   Add a pure, Tauri-free `resolve_sidecar` function to the application facade
   (`project-app`, which already owns `AppConfig` and is covered by the
   webkit-free `cargo test --workspace` gate). The Tauri shell (`src-tauri`)
   computes the install directory from `std::env::current_exe()` and passes
   resolved paths into `AppConfig`. The frontend never learns a sidecar path;
   no new Tauri command or capability is introduced.

2. **Resolution order** (first hit wins):
   1. `EDUCAI_SIDECAR_DIR` environment variable, if set, for `<dir>/<name>`
      (deterministic manual/packaged-layout testing without a full bundle).
   2. The install directory (`current_exe().parent()`): `<dir>/<name>`, then the
      target-triple-suffixed `<dir>/<name>-<triple>` as a robustness fallback.
   3. `PATH` fallback (bare name) — preserves today's `cargo tauri dev`
      behavior where no sidecar is bundled.

   A located bundled binary is passed as an absolute `PathBuf`; otherwise the
   bare name is passed (opencode) or `None` (cloudflared), preserving today's
   lazy `BinaryNotFound` failure semantics at first use rather than failing the
   app at startup.

3. **Pinned component manifest.** A committed `config/components.json`
   (schema-versioned) is the single source of truth for what gets bundled:
  `name`, `platform` (`linux-x86_64` or `windows-x86_64`), pinned `version`, official `source`
   URL, and a `sha256` checksum per component. Exact versions are human-approved
   at implementation time; no `latest` tracking.

4. **Checksum-gated fetch.** A `scripts/fetch-sidecars` script downloads each
   component from its official source, verifies the pinned SHA-256, fails
   closed (and deletes the partial artifact) on any mismatch, and installs to a
   gitignored `sidecars/` directory consumed by Tauri's `bundle.externalBin`.
   Its `--check` mode is offline and deterministic (validates manifest shape and
   checksum format only) and is wired into `scripts/verify`.

5. **Packaging targets and versioning.** `bundle.targets` is set to
   `["appimage", "rpm"]` (Linux x86_64; Fedora-first per PLATFORM_POLICY).
   `bundle.externalBin` references the two sidecars. App/crate version is bumped
   to `0.1.0`. Windows uses native x64 NSIS packaging with `.exe` sidecars;
   macOS remains deferred.

## Consequences

- The bundled OpenCode and `cloudflared` binaries enter the app's TCB. Their
  provenance is controlled by the pinned manifest plus SHA-256 verification and
  official-source-only downloads; the existing child-process env isolation
  (`build_env`/supervisor clearing the environment, loopback-only argv) is
  unchanged.
- No domain, adapter, or port contract changes: `OpenCodeBackend`,
  `FixedBinaryResolver`, and `PathBinaryResolver` already accept absolute paths
  and bare names, so M10 is additive to `project-app` and `src-tauri` only.
- The manifest and `resolve_sidecar` become the foundation for M11 (component
  update and rollback), which extends the same pin/checksum contract rather than
  inventing a new one.
- `scripts/verify` stays offline: it tests the pure resolution function and the
  manifest shape, never downloads or builds a bundle. Packaging build and
  sidecar round-trip remain a manual `scripts/smoke-package`.

## Alternatives considered

### Resolve sidecars inside the OpenCode/tunnel adapters

Rejected: adapters are domain-port implementations and must not know about
installation layout; resolution is an infrastructure/shell concern (ADR-0001).
Keeping it in `project-app` keeps it Tauri-free and webkit-free-testable.

### Bundle via `tauri-plugin-shell` sidecar execution

Rejected: the app deliberately has no shell plugin and no generic process
command (M7 security posture). `bundle.externalBin` packages the binaries
without adding any shell/exec capability to the frontend.

### Download sidecars at install time / auto-update in M10

Rejected: runtime download adds a network dependency at first launch and a
larger trust surface; that is M11 (pinned update + rollback) territory. M10
ships a fixed, verified bundle only.
