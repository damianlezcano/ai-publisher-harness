# TASK-WIN-1A: Rework del payload ONNX Runtime en NSIS Windows

- Requirement: `REQ-WIN-001`
- Base SHA: `5ee67fc5a6520e21cb9657efea99e8212d4b103f`
- Status: `PASS`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: `Codex / independent reviewer pending`

## Scope

- Objective: correct the reproduced Windows installer defect by extracting the two already-pinned ONNX Runtime payloads, verifying their manifest metadata, and bundling them beside `educai.exe`.
- Owned paths: `packaging/windows/build.ps1`; `app/src-tauri/tauri.conf.json`; `scripts/test-distribution-contracts`; this task handoff.
- Allowed paths: generated `sidecars/` and native build artifacts for verification only.
- Prohibited paths: `config/components.json`; product/runtime Rust code; architecture contracts/ADRs; model artifacts; unrelated packaging or test changes.
- Non-goals: component/version/pin changes, runtime loading redesign, model download, Linux packaging changes, Python installation, or any Human Gate closure.

## Architecture impact

- Affected contracts: `AC-013` (preserved; no corpus, telemetry, or provider path change).
- Required ADR/change control: no ADR change. ADR-0013 checksum-gated provenance and ADR-0016 Windows co-location requirements are implemented as accepted.

## Acceptance and verification

- Acceptance criteria: the build verifies the pinned archive and both expected extracted DLL lengths/SHA-256 values; a newly generated NSIS payload installs `onnxruntime.dll` and `onnxruntime_providers_shared.dll` beside `educai.exe`; installer recipe/payload evidence records both files; existing sidecars remain checksum-gated.
- Focused commands: native `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\packaging\windows\build.ps1`; inspect generated NSIS recipe and installer/install payload; relevant distribution-contract test when its Python harness prerequisite is available; `git diff --check`.
- General gate: `CI=true ./scripts/verify` in its supported Linux environment after integration; it is complementary and not a Windows certification.

## Handoff

- Implementation result: updated the native Windows packaging path only. It no longer skips ONNX Runtime; it extracts the existing pinned ZIP, resolves the declared archive root, validates each declared payload size/SHA-256, and copies the two DLLs to the Tauri bundle resource inputs. Tauri NSIS maps those resources to the application directory beside `educai.exe`.
- Verification evidence: baseline defect: 2026-09-22 native installer recipe carried only `opencode.exe` and `cloudflared.exe`; neither ONNX Runtime DLL was present. First rework build failure was reproduced and diagnosed: the verified ONNX ZIP and both declared payloads matched their pins, but the extractor incorrectly searched `lib/...` at the temporary extraction root rather than under manifest `archiveRoot` `onnxruntime-win-x64-1.22.0`; the build therefore failed before copying a DLL. After correcting that path, `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\packaging\windows\build.ps1` succeeded. New artifact: `app/src-tauri/target/release/bundle/nsis/EducAI_0.1.0_x64-setup.exe`, 65,391,621 bytes, SHA-256 `2f5cb329fe507be9479ee168e65bf3dc75dfb18f152411a7077cc82343231ba4`, timestamp `2026-09-22 15:51:44 -03:00`. Generated `installer.nsi` has `SetOutPath $INSTDIR` followed by `File /a "/oname=onnxruntime.dll"` and `File /a "/oname=onnxruntime_providers_shared.dll"` (lines 642-643); uninstall deletes both at lines 764-765. Final sidecar payload checks: `onnxruntime.dll` is 12,418,080 bytes / `579b636403983254346a5c1d80bd28f1519cd1e284cd204f8d4ff41f8d711559`; `onnxruntime_providers_shared.dll` is 22,064 bytes / `ba00ea1ef846c9b909c7854bc56c51051a20f9773b3e1153dda118d4b85d0b93`. `powershell.exe -NoProfile` parser check of `build.ps1`, Tauri JSON parsing, Bash syntax validation of `scripts/test-distribution-contracts`, and `git diff --check` all exited `0`. `scripts/test-distribution-contracts` exited `49` before assertions because Python remains unavailable on this Windows host; this remains the separately tracked harness prerequisite limitation, not a product or installer failure. The Linux `CI=true ./scripts/verify` gate was not run on Windows and remains required in its supported environment.
- Reviewer verdict: `PASS` — independent review inspected the exact diff from `5ee67fc5a6520e21cb9657efea99e8212d4b103f`, the native artifact and generated NSIS recipe. It found the scope preserves `AC-013`; archive checksum, archive-root resolution and both extracted payload size/SHA-256 checks are present; NSIS copies both DLLs to `$INSTDIR` beside `educai.exe` and deletes them on uninstall, satisfying ADR-0016. The missing Windows Python interpreter remains informational harness-test prerequisite evidence, not an installer/product finding. Task remains `IN_REVIEW` pending its `TARGETED_HUMAN` gate.
- Rework history: created after reproduced native payload omission from TASK-WIN-1.
- Targeted human gate: `PASS`. The installed Windows application completed the
  three-material semantic flow with `3,128/3,128` completed embeddings and a
  Knowledge-used response, which proves the packaged co-located ONNX DLLs load
  through the product path. This is recorded without retaining user content.
- Final independent review: `PASS`. The reviewer confirmed the targeted human
  result closes the packaging-specific Windows load-path proof and found no
  new product, architecture, or privacy issue.
