# TASK-WIN-1: Preflight Windows, build nativo e inventario de payload

- Requirement: `REQ-WIN-001`
- Base SHA: `4eafcd7fdb90eed6d7ab44219189e80d82a4abe9`
- Status: `PASS`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: `win1-author` / `win1-reviewer` (independent)

## Scope

- Objective: establish reproducible Windows machine/source/toolchain evidence;
  build the NSIS installer natively; inspect its checksum-pinned sidecar and
  ONNX Runtime payload state; classify failures without changing product code.
- Owned paths: evidence under `tasks/active/REQ-WIN-001/` after selection;
  temporary native build artifacts on the designated Windows machine.
- Allowed paths: `packaging/windows/`, `config/components.json`, Tauri bundle
  configuration, distribution docs, relevant scripts/tests, strictly for
  inspection or a separately assigned fix.
- Prohibited paths: product/runtime code absent a reproduced failure and new
  bounded rework contract; protected authorities; unrelated local files.
- Non-goals: Linux cross-build, silent payload substitution, or a claim of
  runtime success based solely on build success.

## Architecture impact

- Affected contracts: `AC-013` (inspection only).
- Required ADR/change control: ADR-0013/ADR-0016 constraints apply; no change
  without a separately approved failure fix.

## Acceptance and verification

- Acceptance criteria: record native environment/version/source/provenance;
  execute `packaging/windows/build.ps1`; capture artifact hash/size; compare
  installed/bundle payload to manifest; state exactly whether Windows ONNX
  runtime is present and load-testable.
- Focused commands: native PowerShell build; relevant manifest/distribution
  contract tests; selected native Rust/frontend checks; `git diff --check`.
- General gate: `CI=true ./scripts/verify` in its supported Linux environment
  after any integrated source change.

## Handoff

- Implementation result: native preflight/build completed on the designated Windows 11 x64 machine using the authorized process-only PowerShell bypass. The resulting installer is valid build evidence; payload inspection found that Windows ONNX Runtime DLLs are absent, so they are not load-testable from this artifact. No product or protected-contract changes were made.
- Verification evidence: `uname -a` reported `Linux auth.homelab 7.2.5-200.fc44.x86_64`; `git diff --check HEAD` was clean in the isolated author checkout at `a73c7f538e6538b5ee3d43cb06f2fc18ff79b4df`. Repository inspection confirmed Windows component pins for OpenCode, cloudflared, and ONNX Runtime; `packaging/windows/build.ps1` currently skips `onnxruntime`, which remains a native-validation finding rather than an authorized speculative fix.
- 2026-09-22 continuation: a real Windows 11 Home x64 machine was reached. Windows 11 Home `10.0.26200`, `AMD64`; Rust/Cargo `1.97.1`, Node `v22.14.0`, Corepack `0.31.0`, Tauri CLI `2.11.4`; Windows SDK `10.0.26100.0`; VS 2022 Build Tools `17.14.39` with MSVC `cl.exe` `14.44.35207`. Git for Windows Bash is at `C:\Program Files\Git\bin\bash.exe` but is not on `PATH`. `& 'C:\Program Files\Git\bin\bash.exe' ./scripts/requirement-status REQ-WIN-001` returned `STATE: ACTIVE`, `ACTION: CONTINUE`; `test-requirement-status`, `test-agent-class-resolve`, and Bash syntax checks over `scripts/*` passed. The scoped MSVC invocation `cmd.exe /d /c 'call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\LaunchDevCmd.bat" -arch=x64 -host_arch=x64 && powershell.exe -NoProfile -File packaging\windows\build.ps1'` loaded VS Developer Command Prompt v17.14.39, then PowerShell blocked execution of `packaging\windows\build.ps1` before its body. `pnpm.cmd` exists, while direct `pnpm` resolves to execution-policy-blocked `pnpm.ps1`; this is a second build-path portability finding once the script itself may run. No execution policy, file association, sidecar, artifact, or product source was changed.
- 2026-09-22 native build: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\packaging\windows\build.ps1` completed successfully with the bypass limited to that child process. It produced `app/src-tauri/target/release/bundle/nsis/EducAI_0.1.0_x64-setup.exe`, 62,008,887 bytes, SHA-256 `01d4a7b0f2546fb44d879cbfa9682a5513dade5b335ca6d295a0aa5f194be8a7`, timestamp `2026-09-22 15:33:04 -03:00`. Build-time sidecars are checksum-valid: cloudflared `83e726ed18ea78c5ad5213c4c3a3a27051393950d2bc8ed4de69bec12d14eaae`; the OpenCode extracted executable hash is `ef06e41a35795066e95acde276a42fbbf85d7a683c2787f6a19ed20bcde9b6ff` (the manifest pins the source ZIP archive, not the extracted executable). Generated `app/src-tauri/target/release/nsis/x64/installer.nsi` contains only `cloudflared.exe` and `opencode.exe` external payload entries (lines 643-644); the sidecars directory contains only those two files. `build.ps1` explicitly skips the `onnxruntime` manifest component. Therefore this native artifact has no `onnxruntime.dll` or `onnxruntime_providers_shared.dll`; their absence is reproduced packaging evidence, and no load test is possible until a separately authorized fix exists.
- 2026-09-22 focused checks: `& 'C:\Program Files\Git\bin\bash.exe' ./scripts/test-requirement-status` exited `0`; `git diff --check` exited `0`. `& 'C:\Program Files\Git\bin\bash.exe' ./scripts/test-distribution-contracts` exited `49` before its assertions because Git Bash invoked the Windows app-execution-alias stub for `python`/`python3`, and no Python interpreter is installed. This is a current Windows harness-test prerequisite/portability limitation, not a native installer failure; no Python, alias, or system setting was changed.
- Reviewer verdict: the payload rework is independently reviewed in
  `TASK-WIN-1A`; its targeted human semantic run confirms the previously
  absent ONNX payload is now installed and loadable. Requirement-level review
  remains pending.
- Rework history: none.
