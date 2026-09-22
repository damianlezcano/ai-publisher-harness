# TASK-WIN-3: Knowledge en Windows y evidencia de gate release

- Requirement: `REQ-WIN-001`
- Base SHA: `4eafcd7fdb90eed6d7ab44219189e80d82a4abe9`
- Status: `PASS`
- Human gate: `RELEASE_HUMAN`
- Author / reviewer: assigned after TASK-WIN-2 / independent reviewer assigned before review

## Scope

- Objective: prove or accurately block the current Windows Knowledge
  distribution contract: ONNX DLL payload/loading, first-use model path,
  material/import/index behavior, grounded Knowledge interaction or truthful
  degraded state, restart, and release-human evidence.
- Owned paths: evidence under `tasks/active/REQ-WIN-001/` after selection;
  a separate bounded fix contract if a reproducible payload/runtime defect is
  found.
- Allowed paths: designated Windows test artifact/data, relevant Knowledge
  distribution tests and safe diagnostics.
- Prohibited paths: raw material/prompt/provider content in evidence, model or
  component version substitution, architecture changes, and unrelated code.
- Non-goals: declaring prior non-Knowledge Windows HUMAN-PASS sufficient;
  bypassing local-only/offline degradation semantics.

## Architecture impact

- Affected contracts: `AC-013` (privacy) and current Knowledge invariants;
  preserve them.
- Required ADR/change control: ADR-0016 governs runtime/payload behavior. Any
  protected-contract change stops with `ARCHITECTURE_CHANGE_REQUIRED: AC-XXX`.

## Acceptance and verification

- Acceptance criteria: satisfy REQ-WIN-001 criteria 1, 2, 5, 7, and 8; retain
  only sanitized machine/artifact/operator evidence; obtain independent review
  and `RELEASE_HUMAN` before requirement closure.
- Focused commands: native payload/load and focused Knowledge tests, relevant
  installer/runtime observations, native format/lint/type checks; Linux
  architecture/general gate after integrated source changes.
- General gate: `CI=true ./scripts/verify` in its supported Linux environment.

## Handoff

- Implementation result: human validation on the real Windows 11 x64 installation confirms ordinary chat, Creation, preview, installed-app launch, accepted import of three materials, and a grounded Knowledge response. It also reproduces semantic-Knowledge preparation in truthful degraded mode, not a successful semantic-runtime run; no product code or user data was changed during diagnosis.
- Verification evidence: sanitized read-only ledger inspection for the validation project's latest accepted-import operation: `state=completed`, `agent_state=completed`, `total=3`, `copied=3`, `lexical_completed=3`, `failed=1`, `chunks_total=0`, `embeddings_total=0`, `embedding_completed=0`, `embeddings_created=0`, `embeddings_reused=0`, and no bound embedding generation. The operation has exactly three accepted materials and all are `material_index_state=ready`; no summary operation/failure class exists for the turn. Therefore `failed=1` did not come from a failed current material, lexical indexing, or recovered summary attempt. The installed app-data model-generation directory, ONNX model, and tokenizer are all absent; the provider-load path classifies that condition as `model_not_installed`, then falls back to lexical Knowledge. This explains the observed `3 de 3` ready plus `Archivos con error: 1`: it is a real semantic-preparation failure caused by the missing first-use model, while the displayed label is too broad because it calls the operation-wide counter a file error. No raw material bodies, names, prompts, paths, credentials, or provider payloads were read or recorded. This does not satisfy the required native semantic-runtime/model validation; the Human Gate remains unapproved. The separate `scripts/test-distribution-contracts` Python prerequisite remains unrelated.
- First-use bootstrap: the repository's explicit supported installer `cargo run -p project-knowledge --example model_install`, scoped with `EDUCAI_K2_APP_DATA` equal to the application's existing app-data root, completed without manual cache copy or product-code change. Its `ModelManager::install_if_missing()` downloaded the immutable HTTPS revision to private staging, verified all six manifest artifacts by length and SHA-256, then atomically published the generation. A sanitized verification found all six `size_matches=true` and `sha_matches=true`.
- Native semantic smoke: `cargo run -p project-knowledge --example real_inference`, scoped to that verified model root and the Windows packaged `onnxruntime.dll`, passed. It loaded the local provider and emitted a normalized 384-dimensional query vector; all five bounded passage/subdivision sequences returned `output_shape=1x384`; final provider health was `healthy`. This is native Windows runtime evidence only, not a substitute for the required installed-app human reimport/semantic-response evidence.
- Remaining human evidence: repeat the three-material scenario in the installed application now that the supported model generation exists; record only sanitized counters showing `chunks_total > 0`, `embedding_completed > 0`, and a bound embedding generation, then confirm the Knowledge response is semantic. The Computer Use service was unavailable in this session, so no app UI interaction or material inspection was attempted.
- Human gate evidence (final semantic run): the real Windows installed app
  reported `Archivos listos=3/3`, `Archivos con error=0`, and
  `Preparacion=3/3`. It reported `Fragmentos procesados=3,128`,
  `Embeddings completados=3,128/3,128`, and `Embeddings creados=3,128`.
  The response displayed `Knowledge usado`; accumulated detail reported one
  Knowledge-used response and three materials used. This establishes a
  semantic, fully indexed run with no material/import error. No raw material
  body, filename, prompt, path, credential, or provider payload is retained.
  `RELEASE_HUMAN=PASS` for the Knowledge-specific scenario.
- Reviewer verdict: `REWORK` pending review of the recorded final evidence and
  the remaining requirement-level gates; no product defect was identified.
- Final independent review: `PASS` for this Knowledge-specific human gate. It
  confirmed that the final semantic counters supersede the earlier documented
  `model_not_installed` degraded run. It also confirmed that restart
  persistence remains a separate requirement-level gate rather than an
  unreported success.
- Native regression evidence after the final human result: with scoped model
  and packaged-runtime environment variables,
  `cargo test -p project-knowledge --test embedding_runtime` passed (`1/1`,
  7.91 seconds) on Windows. The real provider gate verifies the local model,
  ONNX loading, 384 dimensions, finite L2-normalized vectors, batch ordering,
  batching boundaries, and session health. `cargo fmt --all -- --check`,
  PowerShell parsing of `packaging/windows/build.ps1`, Tauri JSON parsing,
  `bash -n ./scripts/test-distribution-contracts`, and `git diff --check` also
  passed. The full distribution-contract script remains pending a real Python
  runtime, and the Linux general gate remains pending a supported Linux host.
- Rework history: none.
- Independent review: `REWORK` for requirement closure only. The semantic
  Knowledge Human Gate remains PASS, but an explicit Knowledge restart and
  persistence observation is still required by criterion 5. It must not be
  relabeled a non-Windows-only pending gate.
- Final human persistence evidence: after a complete EducAI close and clean
  process check, the operator reopened the application. The conversation with
  its three materials remained available; without attaching them again, a new
  Knowledge query responded correctly. This supplies the explicit post-restart
  Knowledge persistence observation required by criterion 5. No conversation,
  material, prompt, filename, path, credential, or provider payload is
  retained.
- Independent re-review: `PASS`. It confirmed the post-restart conversation
  and new no-reattach Knowledge response satisfy criterion 5; no Windows
  Knowledge defect is reproduced. Remaining requirement evidence is confined
  to TASK-WIN-2's quoted-conversation/install-update/no-console observations
  and the separate Linux/Python gates.
