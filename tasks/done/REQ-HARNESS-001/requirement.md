# REQ-HARNESS-001: Controlled Harness restructuring

- Status: `DONE`
- Baseline SHA: `015d466` (`checkpoint: stabilize conversation knowledge and creation architecture`)
- Human gate: `NO_HUMAN` (independent review is mandatory; final human approval for closure granted)
- Author / implementation role: Orchestrator-led Harness migration; task authors are recorded in the task contracts where evidenced.
- Reviewer requirement: an independent Reviewer inspected the exact diff from `015d466`, task evidence, Architecture Contract mapping and gate results, and returned `PASS`. The reviewer identity is not recorded here.

## Objective

Establish a traceable, product-safe Harness Engineering layer around the protected
EducAI baseline: consistent entry points, role/lifecycle policy, canonical
methodology, requirements/tasks, individual Architecture Contracts, and a local
architecture gate.

## Scope

- Harness documentation, prompts, runtime/lifecycle conventions, task records,
  Architecture Contract documents/manifest, and verification scripts.
- Rework only: reconcile entry-point authority, separate current methodology
  from historical example material, complete this requirement's record,
  individualize AC-001…AC-014, improve AC-to-test traceability, and classify
  M11 as future until approved.

## Out of scope

- Product behavior, runtime code under `crates/`, provider/runtime selection,
  remote CI, CODEOWNERS, branch protection, commits, pushes, and approval of
  M11. Minimal deterministic test/assertion or test-fake/helper changes under
  `crates/` are allowed only when necessary to verify an Architecture Contract;
  they do not authorize a runtime/product behavior change.

## Owned / allowed paths

- `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `RUNTIME.md`
- `docs/HARNESS_ENGINEERING.md`, `docs/REQUIREMENTS.md`,
  `docs/architecture-contracts/`, and Harness-policy documentation
- `examples/README.md`, `prompts/`, `tasks/`, `scripts/architecture-verify`,
  `scripts/verify`

## Prohibited paths

- `app/`, runtime/product behavior under `crates/`, product
  architecture/security/UX authorities, ADRs, and the pre-existing untracked
  `a.zip`, `opencode.json`,
  `crates/project-app/examples/knowledge_grounding_ab_live.rs`, and
  `crates/project-app/examples/scratch_ab_live.rs`.

## Architecture Contracts affected

- Governance and traceability surface for `AC-001` through `AC-014`; their
  product invariants are not changed.

## Acceptance criteria

- Entry points name `docs/HARNESS_ENGINEERING.md` as the current canonical methodology; `examples/README.md` is historical only.
- Methodology retains Prompt, Context, Harness, Evaluation, Loop and Graph Engineering, is neutral to runtime providers/models, uses current commands, and labels TaskBoard historical/non-normative.
- This active requirement has reviewable lifecycle/evidence and bounded task contracts without invented approvals.
- Every AC has an individual contract document with required fields and a concrete existing test selector.
- `scripts/architecture-verify` validates manifest/document/selector traceability and runs focused offline suites.
- M11 is in `tasks/future/` because no approval evidence exists.
- `./scripts/architecture-verify`, `CI=true ./scripts/verify`, and `git diff --check` pass; final closure is authorized by the independent `PASS` and human approval.

## Verification commands

```bash
./scripts/architecture-verify
CI=true ./scripts/verify
git diff --check
git status --short
git ls-files --others --exclude-standard
git diff 015d466 --stat
git diff 015d466 --name-status
```

## Evidence and review history

- Initial migration evidence exists in the Harness documentation, prompts,
  scripts and untracked task/contract files; exact author/reviewer identities
  and independent approval were not reconstructed.
- Independent review returned `REWORK REQUIRED` with six findings: entry points,
  methodology separation, requirement lifecycle, individual AC traceability,
  architecture-gate hardening, and M11 classification.
- This rework records the corrections and final command results in
  `evidence.md`. Final independent review returned `PASS`; human approval for
  requirement closure was granted.
