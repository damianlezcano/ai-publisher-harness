# REQ-WIN-001: Compilación, empaquetado y validación integral nativa en Windows

- Status: `DONE` — all Windows, supported-Linux, and Python-enabled
  distribution-contract gates are PASS; final independent closure review is
  PASS.
- Planning baseline SHA: `4ba4ad3139cd61ac993dfc6fe7980651b95db165`.
- Human gate: `RELEASE_HUMAN` — required on a real Windows 11 x64 PC, with
  machine and artifact evidence recorded before closure.
- Execution roles: the Orchestrator assigns bounded authors and an independent
  Reviewer only after activation. Routing follows `RUNTIME.md`.

## Historical evidence and current gap

Windows is a supported distribution target: native x64 NSIS built on Windows
with MSVC, not Fedora cross-compilation. The repository records a historical
Windows HUMAN-PASS on 2026-09-05 for the then-released feature set: native
build/install/update, launch, normal and quoted chat, attachment/model
interpretation, Creation/Preview/Abrir, cloudflared Quick Tunnel/public URL,
stop sharing, clean owned-child shutdown, and persistence through restart and
reinstall/update.

The following historical fixes/proofs must be preserved and revalidated as
relevant; they are not open defects to reimplement blindly:

- OpenCode and cloudflared child environments forward the parent `SYSTEMROOT`
  on Windows while retaining deliberate minimal environment isolation.
- project creation uses a Windows-aware parent-directory durability boundary;
  staged publication regular files are opened write-capably for `sync_all`;
  canonical/verbatim Windows project paths retain the owning-project check.
- the Windows `build.ps1` checksum-verifies/extracts pinned OpenCode and
  cloudflared sidecars, produces NSIS, and historical artifacts launched with
  healthy owned sidecars and no black consoles/residue.

That pass is not proof of the current full product. Since then, Knowledge and
its native ONNX Runtime payload were integrated. Current distribution policy
states that Windows ONNX DLL metadata is pinned but native Knowledge runtime
validation remains pending. `packaging/windows/build.ps1` currently skips the
`onnxruntime` component, which is evidence for a packaging/runtime validation
gap rather than authorization to assume a fix. Checkpoints after Knowledge
work repeatedly state “Windows Knowledge remains NOT YET HUMAN-VALIDATED.”

## Objective

On a real supported Windows 11 x64 PC, establish whether the current source
can produce, install, launch, and exercise a trustworthy NSIS distribution of
EducAI end to end. Correct only failures reproduced by this validation, with
bounded task contracts and normal architecture/security gates; do not treat a
successful `cargo check` as release validation.

## Scope

- Preflight the actual Windows machine/toolchain and repository baseline:
  Windows version/architecture, MSVC/Windows SDK, Rust, Node/Corepack/pnpm,
  Tauri CLI, available disk/network needed for pinned downloads, and clean
  source/artifact provenance.
- Build the native NSIS installer using `packaging/windows/build.ps1`; capture
  source SHA, command, tool versions, installer path, size, SHA-256, manifest
  component versions/checksums, and all build failures.
- Inspect the built installer/install directory for EducAI and required pinned
  sidecars; determine from evidence whether the currently declared Windows
  ONNX Runtime payload is packaged and loadable. If it is absent or invalid,
  treat that as a validation failure requiring a separately bounded fix, not a
  silent omission.
- Install (and, when safe, update/reinstall) the NSIS artifact on the target
  PC. Validate first launch, no unexpected consoles, owned OpenCode/cloudflared
  lifecycle, and no residue after normal close.
- Run a real functional smoke using a configured provider/account only with
  the human operator’s authorization: create projects/conversations, ordinary
  conversation including a quoted prompt, attachments/materials, Creation,
  Preview/Abrir, local/public sharing, public URL, Stop sharing, and restart
  persistence.
- Validate Knowledge with an appropriate local material set: accepted import,
  indexing/status, local semantic runtime/model availability or its truthful
  degraded/offline behavior, a grounded Knowledge interaction, and restart
  persistence. The run must distinguish a model-first-use download issue from
  an installed ONNX DLL/sidecar packaging issue.
- Recheck Windows filesystem/path behavior that these flows exercise: app-data
  project storage, atomic write/reopen, publication snapshot, Windows path
  separators/canonicalization, and no exposure outside explicit publish roots.
- Record failures with minimal safe diagnostics and create fix tasks only for
  actual failures. Re-run the affected build, installer, smoke, security, and
  human checks after each fix.

Probable implementation/validation surfaces include `packaging/windows/`,
`config/components.json`, `app/src-tauri/`, sidecar/runtime resolution in
`crates/project-app/`, process/tunnel adapters, project filesystem publication
code, and focused tests. Exact ownership is selected only after a failure is
classified; this requirement does not pre-authorize changes to all of them.

## Validation-only versus fix-if-failure

Validation-only unless a reproducible failure occurs:

- native build and NSIS artifact provenance;
- install/update, launch, sidecar lifecycle, real product smoke, public share
  and stop-share, persistence/restart, and human observation;
- confirmation of previously fixed `SYSTEMROOT`, project-storage, publication
  durability, and no-console behavior;
- Windows-specific test limitations already documented historically. They must
  be recorded as limitations only after comparison with the current baseline,
  never waved away as new failures.

Fix-if-failure only:

- Windows packaging or checksum/payload extraction discrepancies, including
  ONNX Runtime DLL presence/co-location/loadability;
- native compilation/lint/test regressions that block the supported artifact;
- sidecar startup/lifecycle failures, filesystem/path/persistence regressions,
  publication/security failures, or a real functional smoke failure.

Every fix needs a new or reworked bounded task contract, focused regression,
independent review, and repeat of the affected human scenario. A validation
task must not silently implement a product change.

## Non-goals

- No Fedora-to-Windows cross-compilation, macOS work, auto-update, signing, or
  M11 component-update/rollback design.
- No architecture redesign, provider/onboarding change, model change, or
  replacement of pinned sidecars/components to mask a failure.
- No claim that a Linux gate or a Rust-only `cargo check` validates Windows.
- No removal/weakening of Unix fixture tests merely because their assumptions
  fail on Windows; classify and repair test-harness debt separately if needed.
- No collection of credentials, prompts, raw message/material content, paths,
  or provider payloads in the evidence.

## Architecture and security impact

- Affected contracts: `AC-013` (corpus privacy) and the publication security
  invariants in `docs/product/SECURITY.md`; preserve, do not modify them.
- Relevant decisions: ADR-0001 (native shell/sidecars), ADR-0013 (pinned,
  checksum-gated sidecars), and ADR-0016 (Windows ONNX DLL/runtime contract).
- An observed failure that requires changing a protected Architecture Contract,
  publication isolation, or a protected authority stops as
  `ARCHITECTURE_CHANGE_REQUIRED: AC-XXX`.

## Acceptance criteria

1. A real Windows 11 x64 machine record identifies OS/build/architecture,
   hardware-relevant facts, toolchain versions, operator/date/timezone, source
   SHA, command outputs, installer size/SHA-256, and component pin evidence.
2. `packaging/windows/build.ps1` produces a native NSIS installer from the
   recorded source SHA, with checksum-gated components; its payload contents
   match the declared Windows distribution contract or each discrepancy is a
   reproduced, tracked failure.
3. Installer install/update and launch work on the target machine. EducAI,
   OpenCode, cloudflared, and any required Knowledge runtime payload have the
   expected ownership/lifecycle behavior; close leaves no EducAI-owned sidecar
   residue and no unexpected child console.
4. A human validates creation of a project/conversation, ordinary and quoted
   conversation, attachments/materials, Creation, Preview/Abrir, publish,
   actual public URL, Stop sharing, and restart persistence.
5. A human validates the current Knowledge distribution state: local runtime
   payload/model behavior, material import/index status, a Knowledge turn or a
   truthful documented degraded state, and persistence/restart. A successful
   non-Knowledge 2026-09-05 flow cannot satisfy this criterion by itself.
6. Project/publication filesystem behavior preserves local data and never
   serves inputs/workspace or another project; Windows paths do not bypass
   containment/snapshot protections.
7. Focused native tests and applicable formatting/lint/type checks pass, or
   every documented pre-existing Windows-only fixture limitation is reproduced
   against the baseline and separated from product verdicts. The Linux general
   gate is run in its supported environment; its result is recorded as
   complementary, not a substitute for this gate.
8. All real failures are either fixed and revalidated through the same scenario
   or remain explicit blockers. Independent `PASS` plus `RELEASE_HUMAN` are
   recorded before closure.

## Verification plan and expected evidence

On Windows: run the native packaging command, selected Rust/frontend checks,
focused platform tests, installer inspection/install/update, process/lifecycle
checks, and the complete human smoke. Record command/result, exact artifact,
safe screenshots/video or operator checklist, public URL result without
retaining user content, restart observations, and before/after process state.

On the supported Linux development/CI environment: run the repository’s
applicable formatting, architecture, distribution-contract, and
`CI=true ./scripts/verify` gates against the integrated source. Do not run the
Linux Bash/WebKit gate on Windows as if it certified Windows.

## Closure record

- Windows human gates: `PASS`, including the installed ONNX Runtime semantic
  Knowledge flow and post-restart persistence, as recorded in TASK-WIN-1A,
  TASK-WIN-2, and TASK-WIN-3.
- Supported-Linux gates: `./scripts/test-distribution-contracts` — `PASS`;
  `CI=true ./scripts/verify` — `PASS`.
- Final independent closure review: `PASS` by an independent frontier Reviewer
  in a separate read-only checkout. It confirmed AC-013 preservation, all
  acceptance criteria, and no remaining real blocker. Its observations are
  non-blocking closure-record hygiene and a future Linux packaging watch item.
- `RELEASE_HUMAN`: `PASS`.

## Planned task contracts and sequence

1. `TASK-WIN-1` — machine preflight, native build, installer/payload inventory,
   and classification of any failure; no speculative product fixes.
2. `TASK-WIN-2` — installed runtime and functional smoke through sidecars,
   projects, conversation, materials, Creation, preview, publication, stop,
   filesystem, and restart; create a rework task only for a real failure.
3. `TASK-WIN-3` — Knowledge-specific Windows payload/runtime and human
   validation evidence, including release-human checklist and review handoff.

Only after `REQ-METRICS-001` is selected/completed as appropriate may the
Orchestrator move this directory to `tasks/active/REQ-WIN-001/`, refresh its
base SHA, assign independent ownership, and execute these contracts.

## Open questions

- Whether the current `build.ps1` omission of `onnxruntime` is still intentional
  technical debt or a current defect; only native payload inspection and a
  Knowledge runtime test can answer it.
- Which supported Windows PC and authorized provider/model will be used for the
  human scenario, and whether its network permits the pinned sidecar and
  first-use model downloads.
- Whether the current native Windows test suite retains any historical
  Unix-only fixture limitations; these need fresh baseline comparison, not
  assumption.
