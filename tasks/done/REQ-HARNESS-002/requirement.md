# REQ-HARNESS-002: Short-intent orchestration bootstrap

- Status: `DONE`
- Baseline SHA: `5664e44` (`feat(harness): formalize repository workflow and architecture contracts`)
- Human gate: `NO_HUMAN` — owner approved closure on 2026-09-21 (no additional product/runtime observation required).
- Author / implementation role: Harness Architect/Implementer in the integration checkout (Herdr Worker panes were not assumed available for this pass)
- Reviewer requirement: independent from this author. Independent Reviewer `PASS` was obtained; owner directed closure from that PASS (including the subsequent Worker-cheap routing rework).

## Objective

Make a short user intent such as `Implementar REQ-XXXX.` a complete Orchestrator
assignment. The Harness, not the user prompt, must supply authorities, lifecycle,
task graph, Worker/Reviewer delegation, model routing, gates, and stop conditions.

## Scope

- Orchestrator execution contract, entry bootstrap, and thin pointers from
  existing Harness docs.
- Small executable helpers that resolve requirement lifecycle and Harness role
  class onto the existing `scripts/agent-launch` mapping.
- Deterministic tests/gates for the short-intent contract.

## Out of scope

- Executing `REQ-DOCS-001` or moving it out of `tasks/backlog/REQ-DOCS-001/`.
- Product behavior under `app/` or `crates/`.
- A new orchestration framework, a replacement for Herdr, or hardcoded model IDs
  in prompts/requirements.
- Reading or modifying `opencode.json`, `a.zip`,
  `crates/project-app/examples/knowledge_grounding_ab_live.rs`, or
  `crates/project-app/examples/scratch_ab_live.rs`.
- Commit, push, reset, clean, or restore.

## Owned / allowed paths

- `prompts/orchestrator.md`, `prompts/worker.md`, `prompts/reviewer.md`
- `AGENTS.md`, `START_CODEX.txt`, `CODEX_HANDOFF.md`, `RUNTIME.md`, `README.md`
- `docs/HARNESS_ENGINEERING.md`, `docs/REQUIREMENTS.md`,
  `docs/MULTI_AGENT_WORKFLOW.md`, `docs/AGENT_POLICY.md`
- `scripts/requirement-status`, `scripts/test-requirement-status`,
  `scripts/agent-class-resolve`, `scripts/test-agent-class-resolve`,
  `scripts/test-short-intent-orchestration`, `scripts/verify`
- `tasks/active/REQ-HARNESS-002/`, `tasks/TASK_CONTRACT_TEMPLATE.md` (reference only)

## Prohibited paths

- `app/`, `crates/`, `docs/architecture-contracts/`, ADRs, product
  architecture/security/UX authorities, `config/agent-models.env` model values
  (routing policy may be referenced, not rewritten with new model IDs),
  `tasks/backlog/REQ-DOCS-001/`, and the untracked files listed above.

## Architecture Contracts affected

- `None` (Harness process only). Product AC invariants are unchanged.

## Acceptance criteria

- A new Orchestrator session that has loaded the Harness treats
  `Implementar REQ-XXXX.` as sufficient intent and executes
  `prompts/orchestrator.md` without a restated user playbook.
- `prompts/orchestrator.md` is the single Orchestrator workflow authority;
  other files point to it instead of duplicating the procedure.
- `START_CODEX.txt` is bootstrap-only and does not contain the workflow.
- Lifecycle is resolved from the repository (`scripts/requirement-status`);
  FUTURE/DONE are rejected; BACKLOG is moved to ACTIVE before work; ACTIVE continues.
- Worker/Reviewer routing uses `scripts/agent-class-resolve` plus
  `scripts/agent-launch` and `config/agent-models.env`; no model ID in the
  user prompt or this requirement.
- Worker `cheap` resolves OpenCode Go first (`low` / `opencode`) and treats
  Cursor (`low` / `cursor`) as last-resort fallback only. Orchestrator remains
  `strong` and Reviewer remains `independent` per the current Harness mapping.
- Real delegation uses `scripts/agent-launch --launch` inside Herdr; missing
  Herdr is recorded, never replaced by prompt folklore.
- Independent Reviewer is mandatory; the Orchestrator must not self-review or
  auto-close. Work stops at `IN_REVIEW` / the declared human gate until those
  results are recorded.
- This requirement does not execute `REQ-DOCS-001`. A later independent
  selection of that requirement is outside this closure.
- `./scripts/architecture-verify`, `CI=true ./scripts/verify`, and
  `git diff --check` pass.

## Verification commands

```bash
./scripts/test-requirement-status
./scripts/test-agent-class-resolve
./scripts/test-short-intent-orchestration
./scripts/architecture-verify
CI=true ./scripts/verify
git diff --check
```
