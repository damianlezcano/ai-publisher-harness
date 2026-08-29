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
