# REQ-DOCS-001: Repository documentation taxonomy and entry-point cleanup

- Status: `IN_REVIEW`
- Planning baseline SHA: `5664e44` (`feat(harness): formalize repository workflow and architecture contracts`)
- Human gate: `TARGETED_HUMAN` — the owner approves the resulting documentation map and entry-point wording before closure.
- Execution roles: strong Orchestrator; cheap, bounded documentation workers; strong independent Reviewer. Concrete routing must follow `RUNTIME.md` and `config/agent-models.env`; this contract names no model.
- Reviewer requirement: independent from every task author; inspect the exact integrated diff, migration ledger, references report, and real gate output. `PASS` is required before the human gate.

## Objective

Make `docs/` navigable by separating current canonical documentation from historical designs, reviews, evidence, and checkpoints, while retaining technical traceability and repairing every repository reference. Clarify the purpose of the repository entry points and add a brief root README repository map.

## Non-goals

- No product, runtime, UI, crate, app, packaging, sidecar, or behavior change.
- Do not use `tasks/done/` as a document archive; it remains the lifecycle record for completed requirements.
- Do not change an Architecture Contract invariant, ADR decision, security invariant, or user-facing product policy. If a wording change would alter one, stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-XXX`.
- Do not delete source content. `DELETE` is not authorized in this requirement; a source path disappears only as the completed half of a tracked `MOVE`, after content preservation and reference repair.
- Do not read or modify `opencode.json`, `a.zip`, `crates/project-app/examples/knowledge_grounding_ab_live.rs`, or `crates/project-app/examples/scratch_ab_live.rs`.

## Target taxonomy

The taxonomy deliberately has one shallow current area and two explicit historical areas. Do not add category directories beyond those below without an Orchestrator decision recorded in the migration ledger.

```text
docs/
  README.md                         # documentation map and authority navigation
  product/                          # current product, UX, and security policy
  architecture/                     # current system, Knowledge, storage architecture
  architecture-contracts/           # protected executable invariant contracts
  decisions/                        # ADRs
  engineering/                      # current Harness, lifecycle, verification, workflow policy
  distribution/                     # current platform and distribution policy
  CURRENT_CHECKPOINT.md             # concise current operational technical snapshot
  checkpoints/                      # immutable dated technical snapshots only
  history/
    milestones/                     # M1..M10 historical designs and milestone index
    reviews/                        # historical audits and review reports
    ux/                             # historical UX design/gate reports and their evidence
```

`docs/README.md` is a pointer/index, not a second architecture or policy document. `docs/history/README.md` and `docs/checkpoints/README.md` must state their retention rules. Directory `tasks/done/` remains outside this taxonomy.

## Classification and migration decision matrix

The executor must create a machine-reviewable migration ledger before moving files. It must confirm each source, destination, action, content-preservation method, and all inbound references. This matrix is the required starting decision; a conflict with actual content stops for the Orchestrator rather than being silently redesigned.

| Current path/group | Decision | Destination / responsibility |
| --- | --- | --- |
| `docs/PRODUCT.md`, `docs/UX.md`, `docs/SECURITY.md` | MOVE | `docs/product/`; current normative product, UX, security policy. Keep distinct; no merge. |
| `docs/ARCHITECTURE.md`, `docs/KNOWLEDGE_ARCHITECTURE.md`, `docs/STORAGE_LAYOUT.md` | MOVE | `docs/architecture/`; current implementation/design truth. Keep distinct; update protected-path documentation and gates as necessary without changing invariants. |
| `docs/architecture-contracts/` | KEEP | Existing path. Protected contracts and manifest remain together. |
| `docs/decisions/` | KEEP | Existing ADR path and numbering remain immutable. |
| `docs/AGENT_POLICY.md`, `docs/HARNESS_ENGINEERING.md`, `docs/REQUIREMENTS.md`, `docs/TESTING.md`, `docs/VERIFY.md`, `docs/DEFINITION_OF_DONE.md`, `docs/MULTI_AGENT_WORKFLOW.md`, `docs/WORKTREES.md` | MOVE | `docs/engineering/`; current Harness methodology, policy, lifecycle, test strategy, executable verification contract, closure criteria, delegation and checkout operation. Keep distinct with explicit cross-references. |
| `docs/PLATFORM_POLICY.md`, `docs/DISTRIBUTION.md` | MOVE | `docs/distribution/`; current platform support and packaging/release policy. Keep distinct. |
| `docs/MILESTONES.md` | RENAME + HISTORY | `docs/history/milestones/README.md`; historical milestone map/index, linking only to retained historical designs and future M11 note where relevant. |
| `docs/M1_DESIGN.md` … `docs/M10_DESIGN.md` | MOVE + HISTORY | `docs/history/milestones/`; preserve filenames unless a ledger-supported rename improves collision-free clarity. They are historical design/implementation traceability, not current authority. |
| `docs/HARNESS_REVIEW.md` | MOVE + HISTORY | `docs/history/reviews/harness-m0-review.md`; historical M0 review evidence. |
| `docs/ARCHITECTURE_AUDIT_CONVERSATION_OPENCODE.md` | MOVE + HISTORY | `docs/history/reviews/architecture-audit-conversation-opencode.md`; read-only audit, not a current architecture authority. Add a dated/context note if needed; do not overwrite its findings. |
| `docs/qwen-review-creation-share.md`, `docs/qwen-rereview-creation-share.md`, `docs/ux-rereview-creation-share.md` | MOVE + HISTORY | `docs/history/reviews/`; retain review/rereview provenance and link it from any related historical UX report only when a real relation exists. |
| `docs/UX_REDESIGN_01_DESIGN.md` | MOVE + HISTORY | `docs/history/ux/ux-redesign-01/design.md`; completed/implemented design traceability, not current UX authority. |
| `docs/ux-redesign-01/` | MOVE + HISTORY | `docs/history/ux/ux-redesign-01/evidence/`; retain `RESULTS.md`, harness scripts, reviews, screenshots, OCR, and a11y evidence as one intact tree. Update `scripts/verify` paths if it validates this evidence. |
| `docs/UX_RELEASE_GATE_01.md` | MOVE + HISTORY | `docs/history/ux/ux-release-gate-01/report.md`; approval/gate evidence, not current UX policy. |
| `docs/ux-release-gate-01/` | MOVE + HISTORY | `docs/history/ux/ux-release-gate-01/evidence/`; retain evidence tree intact. |
| `docs/CURRENT_CHECKPOINT.md` | KEEP + SPLIT | Retain this exact canonical current-handoff path, but reduce it to an explicitly dated current operational snapshot. Move superseded dated entries verbatim into dated files under `docs/checkpoints/`; add a checkpoint index and preserve links/provenance. This resolves its current accumulated-history contradiction with `AGENT_POLICY`. |
| `docs/checkpoints/` (currently empty) | KEEP + POPULATE | Historical technical snapshots only, named/date-indexed and never treated as requirement evidence. |
| `docs/README.md`, `docs/history/README.md`, `docs/checkpoints/README.md` | CREATE | Pointers/indexes only. They must explain current versus history/checkpoints and point to the authoritative documents without duplicating policy. |
| Root `README.md` | KEEP + UPDATE | Add a concise repository structure map and update moved documentation links. Do not make it a duplicate architecture document. |
| `START_CODEX.txt` | KEEP + REDUCE TO POINTER | Bootstrap-only: direct a new session to the authority order, `CODEX_HANDOFF.md`, `AGENTS.md`, selected requirement, and `RUNTIME.md` when routing is needed. No durable state, policies, or full duplicated lists. |
| `CODEX_HANDOFF.md` | KEEP + UPDATE | Durable continuation context: functional baseline/current protected boundaries, authority ordering, and handoff expectations. It may link to the current checkpoint; it is not a full checkpoint archive. |
| `AGENTS.md` | KEEP + UPDATE | Concise repository-wide agent operating rules, role limits, authority precedence, and lifecycle constraints. It points to detailed methodology/policy/workflow rather than restating them. |
| `RUNTIME.md` | KEEP | Operational role routing and human-gate semantics only; no enduring policy/model duplication. Update paths only. |
| `prompts/` | KEEP + UPDATE REFERENCES ONLY | Durable per-role prompt contracts. Keep role-specific instructions short; do not duplicate AGENTS or policy. |
| `skills/` | KEEP + UPDATE REFERENCES ONLY | Skill-specific operating aids; no taxonomy move in this requirement unless a link must be repaired. |
| `scripts/` | KEEP + UPDATE REFERENCES ONLY | Gates may change only to track moved docs or validate reference integrity. No functional product/runtime behavior. |
| `tasks/` and `tasks/done/` | KEEP | Requirement lifecycle records only. No historical documentation move or archive is permitted. |
| Any listed source | DELETE | Not authorized. No content deletion decision exists in this requirement. |

## Authority and duplication boundaries

The future change must preserve these responsibilities rather than merge documents indiscriminately:

| Surface | Sole responsibility after cleanup |
| --- | --- |
| `README.md` | Short repository entry map: product, Harness, documentation, task lifecycle, build/distribution, generated/local artifacts. |
| `START_CODEX.txt` | Extremely short new-session bootstrap/pointer. |
| `CODEX_HANDOFF.md` | Durable project continuation context and protected current-state summary. |
| `AGENTS.md` | Mandatory repository-wide agent rules and authority precedence. |
| `docs/engineering/HARNESS_ENGINEERING.md` | Canonical Harness methodology (prompt/context/harness/evaluation/loop/graph engineering). |
| `docs/engineering/AGENT_POLICY.md` | Changeable cost, reliability, session, and routing policy; model specifics remain governed by runtime config. |
| `docs/engineering/MULTI_AGENT_WORKFLOW.md` | Herdr/worktree delegation procedure and independent-review mechanics. |
| `prompts/orchestrator.md`, `worker.md`, `reviewer.md` | Minimal role-local execution contracts; no policy duplication. |
| `docs/engineering/TESTING.md` | Test strategy, levels, fixtures, and behavior-specific evidence. |
| `docs/engineering/VERIFY.md` | Executable final-gate contract and milestone command wiring. |
| `docs/engineering/DEFINITION_OF_DONE.md` | Work closure checklist and handoff/review conditions. |
| `docs/CURRENT_CHECKPOINT.md` | One current technical/operational snapshot for continuation. |
| `docs/checkpoints/` | Historical dated snapshots; no current-state authority. |

## Acceptance criteria

1. The final `docs/` tree matches the target taxonomy or an explicitly approved simpler equivalent, with a readable docs index.
2. Every relevant existing document/directory has a recorded `KEEP`, `MOVE`, `RENAME`, `MERGE`, `POINTER`, `HISTORY`, or `DELETE` decision; no `DELETE` is executed.
3. Current policy/specification and historical design/review/evidence are visibly separated; `tasks/done/` is never used as a document repository.
4. M1–M10 and the milestone index are coherently retained as history without broken links; M11 remains a future task concept, not approved work.
5. UX redesign/release-gate reports, their artifact trees, architecture audit, and review reports remain traceable as history/evidence.
6. `README.md` contains a concise “Repository structure” section covering `app/`, `crates/`, `docs/`, `tasks/`, `prompts/`, `skills/`, `scripts/`, `config/`, `packaging/`, `sidecars/`, `components/`, and other material root directories, separated into PRODUCT, HARNESS, DOCUMENTATION, TASK LIFECYCLE, BUILD/DISTRIBUTION, and GENERATED/LOCAL ARTIFACTS.
7. `START_CODEX.txt`, `CODEX_HANDOFF.md`, `AGENTS.md`, `RUNTIME.md`, policy/workflow/methodology documents, and role prompts have the declared non-overlapping responsibilities.
8. TESTING, VERIFY, and DEFINITION OF DONE stay distinct and cross-reference rather than duplicate; CURRENT_CHECKPOINT is current while `docs/checkpoints/` is historical.
9. Before and after every move/rename, the implementation inventories and repairs Markdown links, scripts, `rg`/literal path checks, README, AGENTS, CODEX_HANDOFF, START_CODEX, task records, Architecture Contracts/manifest, skills, and prompts. No tracked old path remains except intentional documented historical prose.
10. Product/runtime behavior is unchanged; protected architecture/security/UX content and ADR decisions are not semantically altered.
11. `./scripts/architecture-verify`, `CI=true ./scripts/verify`, `cargo fmt --all -- --check`, and `git diff --check` pass, along with a documented Markdown/reference-integrity check.
12. An independent Reviewer returns `PASS`; then the declared targeted human approval is recorded before the Orchestrator moves the requirement to Done.

## Required future verification

```bash
git status --short
git diff --check
cargo fmt --all -- --check
./scripts/architecture-verify
CI=true ./scripts/verify
# plus a documented repository-wide moved-path / Markdown-link integrity check
```

The future reference-integrity task must first use `rg` to create an old-path inventory, then rerun the same inventory after moves. It must validate relative Markdown targets (including fragments when a verifier supports them), `scripts/verify` required-document paths, `scripts/architecture-verify` references, and `git ls-files` content preservation. No network-dependent checker is required.

## Execution sequence and handoff

Move this directory as a whole to `tasks/active/REQ-DOCS-001/` only when selected. The Orchestrator declares exact author/reviewer identities, worktrees, current base SHA, and any necessary non-overlapping refinements. Execute `TASK-D1` first, then D2/D3/D4 in isolated ownership, then D5, then the Orchestrator-led D6 verification/review handoff. A `REWORK` repeats the relevant task and verification before a new independent review.
