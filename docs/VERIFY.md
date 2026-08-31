# Verification Contract

`./scripts/verify` is the final required local gate. It must be runnable from
the repository root and exit non-zero on the first class of unmet contract. It
must not require network access, credentials, a running desktop app, a tunnel,
or a real AI provider. It prints the checks it ran and remains deterministic.

The script is a thin orchestrator: it invokes repository-defined format, lint,
type, test, build, and security commands rather than duplicating their logic.
When a command is not applicable, it prints `SKIP` and explains why. A skipped
required check is a failure once that milestone introduces the relevant stack.

## Contract by milestone

| Milestone | `scripts/verify` must enforce |
| --- | --- |
| M0 | Required harness documents and skills exist; shell scripts parse; no placeholder verification contract remains |
| M1 | Harness/documentation validation; Rust format, lint, and test checks; filesystem/security integration tests for immutable inputs, atomic metadata, corrupt metadata, path boundaries, and directory separation; `git diff --check` |
| M2 | M1 checks plus `project-publisher` functional integration (`publisher_http`) and adversarial HTTP/filesystem security (`publisher_security`) suites, loopback binding assertion, and every applicable HTTP security invariant; no frontend tooling |
| M3 | M2 checks plus named publication lifecycle, snapshot-security, and schema-migration integration suites; all run locally without tunnel/UI/AI dependencies |
| M4 | M3 checks plus controlled tunnel-adapter contract tests and URL/QR behavior tests |
| M5+ | All applicable prior checks plus adapter/UI/end-to-end checks introduced by the milestone |

Any new toolchain must add its exact commands to this document and wire them
into `scripts/verify` in the same change. Release-only external smoke checks
are documented separately and never replace the local gate.

## M1 bootstrap behavior

The M1 bootstrap pins Rust in `rust-toolchain.toml` and provides the
`project-core` and `project-fs` workspace crates. Until the filesystem adapter
introduces `crates/project-fs/tests/project_lifecycle.rs`, verification prints
`SKIP` for that named integration suite; all available workspace Rust checks
remain required. Cargo commands that resolve packages use `--locked`; `cargo
fmt` does not resolve packages. M1 does not introduce or invoke Node/npm
tooling.

Frontend/package-manager checks are introduced only with a real frontend
workspace in its milestone. M1 must not install or invoke Node tooling merely
because the accepted architecture reserves TypeScript for the future UI.

## M2 publisher behavior

M2 adds Rust-only publisher checks after its crate and named integration suites
exist. The exact intended commands are recorded in `docs/M2_DESIGN.md`:
workspace format/lint/tests, `publisher_http`, `publisher_security`, and
`git diff --check`. Before M2 implementation they are documentation, not
placeholder commands: `scripts/verify` must not invoke nonexistent targets.

## M3 publication behavior

M3 adds named `project-publication` suites after that crate exists:
`publication_lifecycle`, `publication_security`, plus `project-fs`
`project_migration`. Commands match `docs/M3_DESIGN.md`. The script skips a
named suite only while its file is absent.

## M4 tunnel behavior

M4 adds named offline `project-tunnel` suites (`models`, `supervisor`,
`cloudflare`, `tunnel_security`) plus `project-publication`
`publication_tunnel`. Commands match `docs/M4_DESIGN.md`. All M4 suites are
deterministic and offline: they use the in-memory `FakeTunnel` and the
`fake_cloudflared` test executable; none contact the Internet or Cloudflare.

The real Quick Tunnel round trip is a manual, optional smoke test
(`scripts/smoke-cloudflare`, Fedora-only). It is never part of `scripts/verify`
and SKIPs cleanly when `cloudflared` is not installed; release correctness must
not depend on it.

## M5 agent behavior

M5 adds named offline suites for `project-process` (`supervisor`) and
`project-agent` (`models`, `opencode_adapter`, `agent_service`,
`agent_security`, `agent_lifecycle`). Commands match `docs/M5_DESIGN.md`. All M5
suites are deterministic and offline: they use `FakeAgentEngine`, an in-process
fake OpenCode HTTP server, and the generic `fake-process` executable; none
contact the Internet, a real OpenCode server, or an AI provider.

The real OpenCode round trip is a manual, optional smoke test
(`scripts/smoke-opencode`, Fedora-only). It is never part of `scripts/verify`
and SKIPs cleanly when `opencode` or a usable model is unavailable; release
correctness must not depend on it.

## M6 desktop + frontend behavior

M6 adds the `project-app` facade, the React/Vite/TypeScript frontend under
`app/`, and the Tauri 2 shell under `app/src-tauri`. `scripts/verify` adds:

```bash
cargo test --locked -p project-app --all-targets
pnpm --dir app install --frozen-lockfile
pnpm --dir app run format:check
pnpm --dir app run lint
pnpm --dir app run typecheck
pnpm --dir app run test
cargo check --manifest-path app/src-tauri/Cargo.toml
```

The frontend checks (format, lint, typecheck, Vitest component tests) are
deterministic and offline; component tests use mocked Tauri `invoke`/`listen`/
dialog/webview APIs and never touch OpenCode, cloudflared, or the network.
The Tauri `cargo check` requires the WebKitGTK 4.1 system packages
(`webkit2gtk4.1-devel` on Fedora); verify reports a clear failure if they are
missing. The real end-to-end desktop demo is manual (`pnpm --dir app run dev` +
`cargo tauri dev`-equivalent), never part of verify.

## M7 provider/model behavior

M7 adds the `project-opencode` crate (shared `OpenCodeBackend`) and the
`project-provider` crate (`ProviderConnector` port + `OpenCodeProviderConnector`
adapter). On top of the M6 checks, `scripts/verify` adds:

```bash
cargo test --locked -p project-opencode --all-targets
cargo test --locked -p project-provider --test provider_models
cargo test --locked -p project-provider --test provider_adapter
cargo test --locked -p project-provider --test provider_service
cargo test --locked -p project-provider --test provider_security
cargo test --locked -p project-provider --test provider_lifecycle
cargo test --locked -p project-app --all-targets
```

The provider frontend components are covered by the existing `pnpm` suite
(install, format:check, lint, typecheck, test), and the Tauri shell by
`cargo check --manifest-path app/src-tauri/Cargo.toml`. All M7 suites are
deterministic and offline: they use `FakeProviderConnector` and an in-process
fake OpenCode server; none contact the Internet, a real provider, or a real
credential. When all M7 checks pass, the final gate prints
`verify: M7 contract passed`.

The real provider round trip is a manual, optional smoke test
(`scripts/smoke-provider`, Fedora-only). It is never part of `scripts/verify`
and never replaces the local gate.

## M8 attachments / preview behavior

M8 adds the `project-preview` crate (loopback-only, token-guarded preview
server), prompt attachments in `project-agent`, and the project-app
import/preview/attachment facade. On top of the M7 checks, `scripts/verify`
adds:

```bash
cargo test --locked -p project-app --all-targets
cargo test --locked -p project-app --test materials
cargo test --locked -p project-app --test attachments
cargo test --locked -p project-app --test preview
cargo test --locked -p project-agent --test agent_attachment
cargo test --locked -p project-preview --test preview_security
cargo test --locked -p project-preview --test preview_lifecycle
```

The frontend M8 components (paste handler, attachment chips, material cards,
preview modal) are covered by the existing `pnpm` suite (install,
format:check, lint, typecheck, test), and the Tauri shell (new commands, empty
`preview.json` capability, preview window) by `cargo check --manifest-path
app/src-tauri/Cargo.toml`. All M8 suites are deterministic and offline: they use
`FakeAgentEngine`, in-memory fixtures, and loopback-only HTTP against a real
localhost preview server; none contact the Internet, an AI provider, or a
browser. The preview security suite enforces the ADR-0010 invariants
(loopback-only bind, single-use 128-bit token, read-only, containment, no
directory listing, reserved-path blocking, nosniff, teardown). When all M8
checks pass, the final gate prints `verify: M8 contract passed`.

The real desktop preview/attachment round trip is a manual, optional smoke test
(`scripts/smoke-preview`, Fedora-only, graphical session). It is never part of
`scripts/verify` and SKIPs cleanly when a desktop/webkit environment is
unavailable; real clipboard/drag behavior and the no-IPC preview window are
exercised there, never in verify.

## M10 packaging behavior

M10 is **packaging + infrastructure**: sidecar resolution, the pinned
component manifest (`config/components.json`), and Tauri bundle configuration
(`targets`, `bundle.externalBin`, version `0.1.0`). It adds no product-domain
or security-invariant change beyond the provenance boundary for bundled
third-party binaries (ADR-0013).

`scripts/verify` stays fully offline and deterministic. On top of the M9
checks, it adds:

```bash
./scripts/fetch-sidecars --check
# version alignment: tauri.conf.json + project-app + educai all report 0.1.0
TAURI_CONFIG='{"bundle":{"externalBin":null}}' cargo check --manifest-path app/src-tauri/Cargo.toml
```

`fetch-sidecars --check` validates manifest shape and SHA-256 checksum format
only; verify never downloads sidecars or builds installable bundles. Version
alignment uses simple string assertions against `tauri.conf.json` and the
relevant `Cargo.toml` files. The Tauri `cargo check` runs with `externalBin`
neutralized so the offline gate stays green without gitignored `sidecars/`
binaries on disk.

The real bundle build and sidecar round-trip is a manual, optional smoke test
(`scripts/smoke-package`, Fedora, graphical session). It is never part of
`scripts/verify` and SKIPs cleanly (exit 3) when packaging tooling is absent;
release correctness must not depend on it.

When all M10 checks pass, the final gate discriminates on the component manifest
and prints `verify: M10 contract passed`.

## M9 frontend UX polish behavior

M9 is **frontend-only** (`app/src` + docs + tests). It adds no Rust, no Tauri
command, no capability, and no security-invariant change. On top of the M8
checks, `scripts/verify` adds nothing new beyond the existing `pnpm` suite
(install, format:check, lint, typecheck, test), which now covers the M9 message
catalog (`messages.test.ts`), error guidance (`guidance.test.ts`), and the
component/UX tests (empty states, first-run, sharing/QR, keyboard, dialogs,
provider-status banner). The gate discriminates on the M9 contract files:

```bash
if [[ -f app/src/messages.ts && -f app/src/guidance.ts ]]; then
  printf 'verify: M9 contract passed\n'
```

When all M9 checks pass, the final gate prints `verify: M9 contract passed`.

M9 does not modify `app/src-tauri` capabilities or the Tauri command surface
(the T10 checklist confirms no capability/command file changed and that
`git diff --check` is clean). The desktop visual/responsive result is a manual
`scripts/smoke-ux` checklist (Fedora, graphical session), never part of verify.
