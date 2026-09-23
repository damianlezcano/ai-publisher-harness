# TASK-WIN-2: Smoke funcional del instalador Windows y persistencia

- Requirement: `REQ-WIN-001`
- Base SHA: `4eafcd7fdb90eed6d7ab44219189e80d82a4abe9`
- Status: `PASS`
- Human gate: `TARGETED_HUMAN`
- Author / reviewer: assigned after TASK-WIN-1 / independent reviewer assigned before review

## Scope

- Objective: on the designated real Windows PC, validate the installed NSIS
  artifact through launch, owned sidecars, project/conversation, attachments,
  Creation, preview, publication/public URL/stop, filesystem safety, and
  restart/update persistence.
- Owned paths: evidence under `tasks/active/REQ-WIN-001/` after selection.
- Allowed paths: installed test artifact and project data created specifically
  for this validation; safe read-only process/artifact inspection.
- Prohibited paths: arbitrary user data, credentials, unrelated projects,
  product code unless a reproduced failure gets a separate rework contract,
  and the assignment’s named local out-of-scope files.
- Non-goals: automatic test account provisioning or treating a failed public
  network as a code issue without diagnosis.

## Architecture impact

- Affected contracts: `AC-013` and publication security invariants.
- Required ADR/change control: `None` for validation; failure fixes follow
  normal change control.

## Acceptance and verification

- Acceptance criteria: satisfy REQ-WIN-001 criteria 3, 4, and 6 with a
  human-run, safe evidence checklist, including clean shutdown and restart.
- Focused commands: native process/lifecycle checks, relevant Rust/frontend
  tests, installer install/update commands, and inspection of test data only.
- General gate: `CI=true ./scripts/verify` in Linux after source changes.

## Handoff

- Implementation result: current installed-app human observation passes launch,
  normal chat, Creation, and Preview; no product code changed outside the
  reviewed Windows ONNX packaging rework.
- Verification evidence: final closure still needs an exact-current-artifact
  operator record for quoted conversation, publication/public URL and Stop,
  clean sidecar/no-console shutdown, restart persistence, and filesystem
  publication isolation. The historical 2026-09-05 Windows distribution pass
  documents those unchanged surfaces but is not silently substituted for this
  requirement's fresh evidence.
- Reviewer verdict: pending.
- Rework history: none.
- Final Windows human gate: `PASS`. The operator confirmed the current
  installed application's publication lifecycle: the active publication was
  reachable at its public URL; after Stop / dejar de compartir, that URL no
  longer served the resource and returned Cloudflare `502 Host Error`,
  consistent with the owned origin/tunnel being stopped. This is expected
  teardown behavior, not a publication-isolation failure. Together with the
  final Windows validation already recorded for launch, chat, Creation,
  Preview, and Knowledge, the operator declares the Windows Human Gates
  complete. No user content, URL, project path, or credentials are recorded.
- Independent review: `REWORK` for the full task record, not for a product
  defect. It confirms the publication observation is PASS, but requires
  explicit current-artifact observations for quoted chat, clean owned-sidecar
  and no-console shutdown, restart persistence, and publication/filesystem
  isolation before criteria 3, 4, and 6 can be approved.
- Final human evidence: the operator closed EducAI completely and verified in
  Task Manager that no EducAI, cloudflared, or OpenCode process associated with
  the session remained. EducAI then reopened correctly. The public URL served
  the published activity while sharing was active; no navigation or exposure
  to other files, conversations, or local content was observed. After Stop /
  dejar de compartir, the URL returned Cloudflare `502 Host Error`, consistent
  with the stopped owned origin/tunnel. This supplies the explicit clean-close,
  restart, and publication-isolation observations previously requested. No
  URL, local path, content, or credential is recorded.
- Independent re-review: `REWORK` for evidence completeness only; it found no
  Windows product defect. The new record satisfies clean lifecycle, restart,
  publication access/Stop, isolation, and Knowledge persistence. Before this
  task can pass, record on the current artifact: (1) one quoted-conversation
  result, and (2) install/update plus explicit observation that no unexpected
  child console appears while the application/sidecars run. Historical passes
  are not substituted silently for these current-artifact observables.
- Install/update and console evidence: the operator completed the current
  installer/update successfully and observed no unexpected console while the
  application was active. The update left two EducAI instances open; the
  operator explicitly accepts this as multi-instance behavior, not a product
  defect. No product change is requested or made. This closes the current
  install/update and no-unexpected-console observation; only the quoted
  conversation remains pending for this Windows task.
- Independent re-review: `PASS` for criterion 3. It found no Windows defect:
  current install/update succeeds, no unexpected console appeared while
  active, and prior clean-close evidence establishes no owned-sidecar residue.
  The two EducAI instances are accepted multi-instance behavior; no protected
  contract or requirement mandates single-instance operation. Quoted
  conversation remains the sole pending Windows observable.
- Quoted-conversation evidence: the operator sent a quoted-text prompt.
  EducAI processed it and responded normally, with no parsing error,
  execution behavior, unexpected console, or other anomaly. This closes the
  final Windows observable for criterion 4 without retaining prompt or
  conversation content.
- Final independent review: `PASS`. It confirmed all Windows criteria 3, 4,
  and 6 observations, including the content-free quoted turn. No Windows
  product defect is reproduced. Only the supported-Linux general gate and the
  Python-enabled distribution-contract gate remain before requirement closure.
