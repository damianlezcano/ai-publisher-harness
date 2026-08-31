# M10 Packaging — Design

Status: **Approved** (architecture accepted by the human owner; no
implementation yet). ADR-0013 is Accepted. This document is the durable design
handoff for the M10 implementation session (a fresh
`opencode-go/deepseek-v4-flash` orchestrator).

## 1. Exact M10 objective (from repository)

Canonical M10 definition, reconciled across the source-of-truth documents:

- `docs/MILESTONES.md` — `M10 Packaging`.
- `docs/CURRENT_CHECKPOINT.md` — `M10 (packaging): no iniciado` and
  `M10 = Packaging (Linux AppImage/RPM; sidecars OpenCode/cloudflared). No iniciar.`
- `docs/M9_DESIGN.md` §2/§36 — `M10 = Packaging. Linux AppImage then RPM;
  bundle/manage OpenCode and cloudflared as pinned sidecars; native CI for future
  Windows.`
- `docs/PLATFORM_POLICY.md` — target Linux distribution artifacts in this order:
  AppImage, then RPM; Windows deferred to a future native-CI milestone.

**Objective:** Produce the first installable Linux x86_64 artifacts (AppImage,
then RPM) for EducAI, bundling pinned, checksum-verified OpenCode and
`cloudflared` sidecars so a non-technical user can install and run the app
without installing prerequisites. Windows/macOS remain deferred.

> Reconciliation note: `CODEX_HANDOFF.md` (the original M0 handoff) lists
> "Windows, macOS, Linux builds" as the eventual M10 goal, but
> `PLATFORM_POLICY.md` and `M9_DESIGN.md` (the current, active scope) narrow the
> milestone to Linux-first with Windows/macOS deferred. This is a documented
> refinement, not a conflict. macOS/Windows are explicitly deferred below and
> re-confirmed in the DoD for human sign-off.

## 2. Executive summary

M10 makes the M1-M9 product installable and self-contained without changing any
product, domain, or security-invariant behavior. It is a **packaging +
infrastructure** milestone with three additive changes:

1. A Tauri-free **sidecar resolution** function (`project-app/src/sidecar.rs`)
   that locates the bundled `opencode`/`cloudflared` binaries relative to the
   installed app, falling back to `PATH` for development.
2. A **component pin manifest** (`config/components.json`) plus a
   checksum-gated **`scripts/fetch-sidecars`** fetcher that provisions the
   pinned binaries into a gitignored `sidecars/` directory.
3. **Tauri bundle configuration** (`targets = ["appimage","rpm"]`,
   `bundle.externalBin`, version `0.1.0`) plus the shell wiring that feeds the
   resolved sidecar paths into `AppConfig`.

The decisive architectural result: **zero changes to project-core/project-fs,
AgentEngine, PublicationManager, the provider/credential domain, publisher,
tunnel, or preview servers; zero new Tauri commands, capabilities, or windows.**
The only new trust boundary is the provenance of the bundled third-party
binaries, which is controlled by the pinned manifest + SHA-256 verification
(ADR-0013). `scripts/verify` remains offline and deterministic; the actual
bundle build and sidecar round-trip are a manual `scripts/smoke-package`.

## 3. M9 / M10 / next-milestone boundary

| Milestone | Owns | Excludes |
| --- | --- | --- |
| M9 | Education UX polish (CLOSED) | packaging, sidecars, component updates |
| M10 | Linux AppImage + RPM packaging; pinned, checksummed OpenCode + cloudflared sidecars; shell sidecar resolution; version 0.1.0; packaging smoke script | component update/rollback tooling (M11), Windows/macOS artifacts, auto-update, code signing, production CSP tightening, runtime download |
| M11 | Component updates: extend the pin manifest into a versioned, rollback-capable update flow; native Windows CI | UX features; consumes M10's packaged surface + manifest |

M10 **consumes** the M9-polished UI as the packaged surface and the M7/M8
OpenCode + tunnel/credential architecture unchanged. It begins, but does not
complete, the "pinned, tested, compatible versions + rollback" theme: M10
establishes the initial pin and checksum contract; M11 builds update/rollback
on top.

## 4. User journeys / operational flows

1. **Download and run (AppImage).** A teacher downloads one `.AppImage`, marks it
   executable, and runs it. No terminal, no prerequisites, no OpenCode or
   Cloudflare install. First launch shows the M9 first-run guide.
2. **Install (RPM).** A Fedora user installs the `.rpm` via the graphical
   software center; the app appears in the app menu; OpenCode/cloudflared are
   installed alongside (or bundled) with no extra steps.
3. **Ask the AI.** The bundled, pinned `opencode` is used automatically; the user
   never sees its name, path, or version.
4. **Share.** The bundled, pinned `cloudflared` powers the Quick Tunnel; the user
   sees only "Compartir → Compartiendo… → Compartido" with the link/QR.
5. **No network, no sidecar.** If a sidecar is missing at runtime (dev build
   without a fetched bundle), the existing lazy failure UX applies unchanged:
   `ai_unavailable` ("El asistente no pudo iniciarse." → Reintentar) and
   `publish_failed` ("No pudimos compartir en este momento." → Reintentar). The
   app never crashes at startup for a missing sidecar.

## 5. Architecture changes

All additive; no existing boundary is redesigned (see ADR-0013 for rationale).

- `project-app/src/sidecar.rs` (NEW, Tauri-free): `SidecarLocation { Bundled
  (PathBuf) | OnPath(String) }` and `resolve_sidecar(name, install_dir,
  path_var)`, plus an env-override entry point. Resolution order:
  `EDUCAI_SIDECAR_DIR` → install dir (`<name>`, `<name>-<triple>`) → `PATH`.
- `project-app/src/app.rs` (EXTEND): `AppConfig` fields are unchanged; a new
  small helper maps resolved locations onto `opencode_binary` (absolute path or
  bare name) and `cloudflared_binary` (`Some(abs)` or `None`). No behavior change
  to `OpenCodeBackend`/`FixedBinaryResolver`/`PathBinaryResolver`.
- `app/src-tauri/src/lib.rs` (EXTEND): `build_state` computes the install dir
  from `std::env::current_exe()` and populates `AppConfig` from
  `resolve_sidecar`. Dev fallback preserved.
- `app/src-tauri/tauri.conf.json` (EXTEND): version `0.1.0`,
  `bundle.targets = ["appimage","rpm"]`, `bundle.externalBin` referencing the two
  sidecars, optional `category`/`shortDescription` for RPM metadata.
- `config/components.json` (NEW): pinned component manifest (schema version 1).
- `scripts/fetch-sidecars` (NEW): fetch + SHA-256 verify + install; `--check`
  offline mode.
- `.gitignore` (EXTEND): add `sidecars/`.

Dependency direction is unchanged: `UI → project-app facade → adapters`; sidecar
resolution is infrastructure, not domain (ADR-0001).

## 6. Module / API changes

```
crates/project-app/src/sidecar.rs      NEW   resolve_sidecar + SidecarLocation (+ unit tests)
crates/project-app/src/app.rs          EXTEND map resolved locations into AppConfig (no signature change)
crates/project-app/src/lib.rs          EXTEND re-export sidecar module
app/src-tauri/src/lib.rs               EXTEND build_state: resolve + inject sidecar paths
app/src-tauri/tauri.conf.json          EXTEND version 0.1.0, targets, externalBin, bundle metadata
app/src-tauri/Cargo.toml               EXTEND version 0.1.0
crates/project-app/Cargo.toml          EXTEND version 0.0.0 -> 0.1.0 (APP_VERSION becomes 0.1.0)
config/components.json                 NEW   pinned manifest (opencode + cloudflared)
scripts/fetch-sidecars                 NEW   fetch/verify/install + --check
scripts/smoke-package                  NEW   manual packaging smoke
scripts/verify                         EXTEND M10 gate + fetch-sidecars --check
docs/VERIFY.md                         UPDATE M10 section + gate
.gitignore                             EXTEND sidecars/
```

No changes to `project-core`, `project-fs`, `project-agent`, `project-opencode`,
`project-tunnel`, `project-publication`, `project-publisher`, `project-preview`,
`project-provider`, or the frontend `app/src`. No new Tauri command, capability,
or window.

## 7. Security implications

No existing security invariant is weakened. One new trust boundary is
introduced and explicitly controlled:

- **New boundary — bundled sidecar provenance.** `opencode` and `cloudflared`
  become part of the app's TCB (run with user privileges, read workspace, reach
  network). Mitigations (ADR-0013): official-source-only downloads, pinned
  versions, SHA-256 verification that fails closed and deletes partials,
  committed manifest, and human-approved versions. No `latest` tracking.
- **No capability/command/window changes.** The frontend never receives a
  sidecar path; resolution stays in the Rust shell.
- **Existing child-process isolation unchanged.** `build_env`/supervisor still
  clear the environment, bind loopback-only, and pass no shell.
- **Credential invariant (#8) unchanged.** Sidecars/versions are not project
  data and never enter project files, logs, URLs, or bundles.
- **Deferred (explicit):** production CSP hardening (`csp: null` today) and code
  signing are out of M10 scope; flagged as debt, not silently changed.

## 8. Failure / recovery semantics

- **Missing sidecar at runtime:** unchanged lazy failure — first agent use →
  `ai_unavailable`; first publish → `publish_failed`; both map to the M9
  guidance with Reintentar. No startup crash.
- **Checksum mismatch during fetch:** `scripts/fetch-sidecars` fails closed,
  deletes the partial artifact, and exits non-zero with a clear message; the
  operator fixes/approves the manifest before a bundle is produced.
- **Bundle build failure (manual smoke):** reported by `scripts/smoke-package`
  as FAIL; never affects `verify`.
- **Wrong-arch sidecar:** resolution checks executability; a non-executable or
  wrong-arch binary falls through to `PATH`/`BinaryNotFound`, surfacing the same
  friendly error path.

## 9. UX implications

None for end users. The product vocabulary (Proyectos, Materiales, Creaciones,
IA, Compartir) and the M9 copy are untouched. OpenCode, cloudflared, paths, and
versions remain hidden. The only user-visible difference is that the app is
now installable as a single artifact with no prerequisites.

## 10. Portability implications

- Linux x86_64 only for M10 (AppImage + RPM), matching PLATFORM_POLICY.
- The manifest is `platform`-keyed (`linux-x86_64`) so M11 and future
  Windows/macOS builds add parallel entries without schema change.
- `resolve_sidecar` is Tauri-free and platform-portable by construction (the
  shell supplies the install dir); no Unix-specific code is added to the core.
- The target-triple-suffixed fallback keeps the resolver tolerant of Tauri
  bundle naming differences across versions.

## 11. Deterministic test strategy

All offline; no network, no bundle build, no sidecar download:

- **`resolve_sidecar` unit tests** (`project-app`): with a temp dir containing a
  chmod `+x` file, assert `Bundled`; with an empty install dir and a `PATH`
  containing the name, assert `OnPath`; with neither, assert `OnPath(name)`
  fallback; with `EDUCAI_SIDECAR_DIR` set, assert env override wins; non-executable
  file is skipped.
- **`fetch-sidecars --check`:** validates the manifest JSON shape, `schemaVersion`,
  per-component `name/platform/version/source/sha256`, and that every `sha256`
  is 64 lowercase hex. Offline; wired into `verify`.
- **Version alignment check (in verify):** assert `tauri.conf.json` version and
  the `educai`/`project-app` crate versions all read `0.1.0` (simple string
  checks, deterministic).
- **Existing suites unchanged:** full `cargo test --workspace` (webkit-free),
  `pnpm` suite, and `cargo check` for `src-tauri` all continue to pass.

## 12. Optional / manual smoke strategy

`scripts/smoke-package` (Fedora, graphical session, SKIP when webkit/tauri
tooling absent; exit 3), never part of `verify`:

1. `./scripts/fetch-sidecars` (real download + checksum verify).
2. `cargo tauri build --manifest-path app/src-tauri/Cargo.toml --bundles appimage`
   (then `rpm`); verify both artifacts exist.
3. Confirm both sidecars are present inside the AppImage payload (e.g.
   `--appimage-extract` inspection) at the expected `usr/bin/` location.
4. Launch the AppImage; complete the M9 demo flow (create → ask IA → share →
   QR) to prove the bundled OpenCode + cloudflared work end-to-end.
5. Verify a missing-sidecar dev build still surfaces the friendly
   `ai_unavailable`/`publish_failed` guidance (not a crash).

Real external-service/network behavior remains manual, matching the existing
smoke pattern.

## 13. ADR(s)

- **ADR-0013** — Sidecar resolution, pinning, and checksum verification
  (Accepted). Governs T1-T5.

No security-invariant ADR is required (no invariant is weakened; the new
provenance boundary is controlled by ADR-0013 itself).

## 14. Task breakdown

| # | Task | Level | Depends | Worktree | Ownership |
| --- | --- | --- | --- | --- | --- |
| 0 | Design + ADR-0013 | HIGH_ARCHITECTURE | — | — | V4 Pro (this session) + Human |
| 1 | Sidecar resolution (`resolve_sidecar`) + tests | MEDIUM | 0 | `m10/sidecar` | crates/project-app/src/sidecar.rs, crates/project-app/tests/sidecar.rs, lib.rs re-export |
| 2 | Tauri shell wiring + bundle config + version 0.1.0 | MEDIUM | 1 | `m10/tauri-bundle` | app/src-tauri/src/lib.rs, tauri.conf.json, app/src-tauri/Cargo.toml, crates/project-app/Cargo.toml |
| 3 | Component manifest + fetch-sidecars (+ `--check`) + .gitignore | MEDIUM | 0 | `m10/components` | config/components.json, scripts/fetch-sidecars, .gitignore |
| 4 | verify gate + docs (VERIFY.md M10, version check) | MEDIUM | 1, 3 | `m10/verify` | scripts/verify, docs/VERIFY.md |
| 5 | smoke-package script | LOW | 2, 3 | `m10/smoke-package` | scripts/smoke-package |
| 6 | Integration + gate + checkpoint (DoD) | MEDIUM | 4, 5 | main | docs, checkpoint, final verify |

## 15. Dependencies between tasks

```
T0 ──┬── T1 ── T2 ──┬── T5
     └── T3 ────────┴── T4 ── T6
```

- T1 and T3 are independent after T0 (paths `sidecars/opencode` and
  `sidecars/cloudflared` are fixed by this design, so T2's `externalBin` and
  T3's fetch output cannot drift).
- T2 depends on T1 (uses `resolve_sidecar`).
- T4 depends on T1 (gate tests the resolver) and T3 (gate calls
  `fetch-sidecars --check` and discriminates on `config/components.json`).
- T5 depends on T2 + T3 (needs bundle config and fetched binaries).
- T6 integrates and gates after T4 + T5.

## 16. Reasoning level per task

0 HIGH_ARCHITECTURE · 1 MEDIUM · 2 MEDIUM · 3 MEDIUM · 4 MEDIUM · 5 LOW ·
6 MEDIUM.

## 17. Proposed worktrees

`../ai-publisher-m10-sidecar`, `-tauri-bundle`, `-components`, `-verify`,
`-smoke-package` (+ review worktrees per task). Integration checkout (`main`)
is lead-only. Shared-file contention is avoided by ownership boundaries:
T1 owns `project-app` sidecar files, T2 owns `src-tauri` + version bumps, T3
owns `config/` + `scripts/fetch-sidecars` + `.gitignore`, T4 owns `scripts/verify`
+ `docs/VERIFY.md`. `scripts/verify` is touched only by T4 (and T6's gate).

## 18. Implementation model allocation

Orchestrator: `opencode-go/deepseek-v4-flash` (fresh session after approval).
`MODEL_REQUESTED == MODEL_ACTUAL` enforced via `scripts/agent-launch`.

## 19. Author / reviewer allocation

| Task | Author | Reviewer |
| --- | --- | --- |
| 1 | OpenCode Go DeepSeek V4 Flash | OpenCode Go Qwen3.8 Max |
| 2 | OpenCode Go DeepSeek V4 Flash | Cursor Composer 2.5 |
| 3 | OpenCode Go DeepSeek V4 Flash | OpenCode Go Qwen3.8 Max |
| 4 | Cursor Composer 2.5 | OpenCode Go DeepSeek V4 Flash |
| 5 | Cursor Composer 2.5 | OpenCode Go DeepSeek V4 Flash |
| 6 | OpenCode Go DeepSeek V4 Flash (lead) | OpenCode Go Qwen3.8 Max |

Author ≠ reviewer, cross-family where practical. T1 and T3 (resolution + fetch/
checksum) get a second-family reviewer (Qwen) because they carry the new
provenance trust boundary. No task here alters a security invariant, so no
independent security-review task is required; ADR-0013 is the controlling
record.

## 20. Risks / debt

- **Exact sidecar versions/SHA-256** are not fabricated in this design; T3 fills
  them from official release channels and the human owner approves before
  fetch/bundle. Risk: wrong checksum or URL → fail-closed fetch catches it.
- **Tauri externalBin naming drift** (target-triple suffix) across Tauri
  versions: mitigated by the `<name>-<triple>` fallback in `resolve_sidecar`.
- **Fedora bundling tooling** (`rpmbuild`, `appimagetool`) differs from the dev
  environment: contained to `scripts/smoke-package` prerequisites, never in
  `verify`.
- **Two Cargo.lock files** (root + `src-tauri`): version bumps touch both;
  T2 must update `Cargo.lock` accordingly and re-run `cargo check`.
- **CSP still `null`** and **no code signing**: deferred hardening, documented
  debt, not changed silently.
- **Supply-chain surface** of bundled binaries: controlled by ADR-0013; M11
  adds rotation/rollback.

## 21. Definition of Done M10

- [ ] ADR-0013 accepted and the exact pinned versions in `config/components.json` human-approved before fetch.
- [ ] `resolve_sidecar` in `project-app` with offline tests (bundled / PATH / env-override / non-executable / suffix fallback).
- [ ] `src-tauri` wires resolved sidecar paths into `AppConfig`; dev `PATH` fallback preserved; no new command/capability/window.
- [ ] `tauri.conf.json`: version `0.1.0`, `targets = ["appimage","rpm"]`, `externalBin` for both sidecars; crate versions aligned to `0.1.0`.
- [ ] `config/components.json` schema-valid; `scripts/fetch-sidecars` fetches, verifies SHA-256, fails closed, `--check` is offline.
- [ ] `sidecars/` gitignored; committed repo contains no bundled binary or secret.
- [ ] `scripts/verify` adds the M10 gate (`fetch-sidecars --check`, version alignment, discriminates on `config/components.json`) and prints `verify: M10 contract passed`.
- [ ] `./scripts/verify`, `git diff --check`, all Rust + frontend tests green; no security invariant regressed.
- [ ] `scripts/smoke-package` produced and manually validated once (AppImage + RPM built; sidecars present; end-to-end share works).
- [ ] Windows/macOS, auto-update, signing, and production CSP explicitly remain deferred.

## 22. scripts/verify incremental plan

Additions (all offline) to `scripts/verify`:

```bash
# M10: offline sidecar/component checks
./scripts/fetch-sidecars --check          # manifest shape + checksum format
# version alignment: tauri.conf.json + educai + project-app all report 0.1.0
# (simple rg/string assertions, deterministic)

# final gate discriminates on the M10 manifest:
if [[ -f config/components.json ]]; then
  printf 'verify: M10 contract passed\n'
elif [[ -f app/src/messages.ts && -f app/src/guidance.ts ]]; then
  printf 'verify: M9 contract passed\n'
# ... existing chain ...
```

`docs/VERIFY.md` gains a short M10 section documenting that M10 is packaging +
infrastructure, that verify stays offline (never builds the bundle or downloads
sidecars), and that the gate discriminates on `config/components.json`.

## 23. Explicit next-milestone scope

M11 = Component Updates: extend `config/components.json` into a versioned,
rollback-capable component update flow for app/OpenCode/cloudflared (pinned
compatibility manifest, safe in-app or scripted update, rollback on failure),
and begin native Windows CI on a Windows runner (no Fedora cross-compilation).
M10 does **not** begin update/rollback, auto-update, code signing, Windows/macOS
artifacts, or production CSP hardening.
