# REQ-DOCS-001 reference inventory (before moves)

- Base SHA: `5664e4495891c495ef9e5b3f40bd7b828c40e312`
- Tree: worktree `ai-publisher-req-docs-001-d1` branch `req-docs-001/d1-inventory`
- Command: `git ls-files` + literal substring scan of tracked text files
- Excluded from scan: `*.png` binaries; prohibited untracked `opencode.json`, `a.zip`, live A/B examples (not in this HEAD tree as owned edits)

## Exact path / prefix hits

| Needle | Files with hits | Total occurrences | Referring files |
| --- | --- | --- | --- |
| `docs/PRODUCT.md` | 6 | 6 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/UX_RELEASE_GATE_01.md`, `scripts/verify` |
| `docs/UX.md` | 13 | 18 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/DEFINITION_OF_DONE.md`, `docs/M9_DESIGN.md`, `docs/UX_REDESIGN_01_DESIGN.md`, `docs/UX_RELEASE_GATE_01.md`, `docs/decisions/0012-user-facing-copy-as-a-single-message-catalog.md`, `docs/qwen-rereview-creation-share.md`, `docs/qwen-review-creation-share.md`, `scripts/verify`, `skills/verify-ui/SKILL.md` |
| `docs/SECURITY.md` | 9 | 11 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/DEFINITION_OF_DONE.md`, `docs/MULTI_AGENT_WORKFLOW.md`, `docs/UX_REDESIGN_01_DESIGN.md`, `scripts/verify`, `skills/review-feature/SKILL.md` |
| `docs/ARCHITECTURE.md` | 9 | 15 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/ARCHITECTURE_AUDIT_CONVERSATION_OPENCODE.md`, `docs/CURRENT_CHECKPOINT.md`, `scripts/verify`, `tasks/done/REQ-HARNESS-001/TASK-H4-architecture-contracts.md`, `tasks/done/REQ-HIST-BASELINE/summary.md` |
| `docs/KNOWLEDGE_ARCHITECTURE.md` | 2 | 3 | `CODEX_HANDOFF.md`, `docs/CURRENT_CHECKPOINT.md` |
| `docs/STORAGE_LAYOUT.md` | 2 | 4 | `docs/ARCHITECTURE.md`, `docs/CURRENT_CHECKPOINT.md` |
| `docs/AGENT_POLICY.md` | 14 | 14 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `RUNTIME.md`, `START_CODEX.txt`, `docs/CURRENT_CHECKPOINT.md`, `docs/HARNESS_REVIEW.md`, `docs/M2_DESIGN.md`, `docs/M3_DESIGN.md`, `docs/M4_DESIGN.md`, `docs/MULTI_AGENT_WORKFLOW.md`, `docs/UX_REDESIGN_01_DESIGN.md`, `docs/UX_RELEASE_GATE_01.md`, `tasks/done/REQ-HARNESS-001/TASK-H6-controls.md` |
| `docs/HARNESS_ENGINEERING.md` | 11 | 15 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/architecture-contracts/README.md`, `examples/README.md`, `scripts/verify`, `tasks/done/REQ-HARNESS-001/TASK-H1-entry-authority.md`, `tasks/done/REQ-HARNESS-001/TASK-H2-roles-lifecycle.md`, `tasks/done/REQ-HARNESS-001/TASK-H6-controls.md`, `tasks/done/REQ-HARNESS-001/requirement.md` |
| `docs/REQUIREMENTS.md` | 9 | 9 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/HARNESS_ENGINEERING.md`, `scripts/verify`, `tasks/done/REQ-HARNESS-001/TASK-H2-roles-lifecycle.md`, `tasks/done/REQ-HARNESS-001/TASK-H3-history-backlog.md`, `tasks/done/REQ-HARNESS-001/requirement.md` |
| `docs/TESTING.md` | 8 | 9 | `CODEX_HANDOFF.md`, `README.md`, `RUNTIME.md`, `START_CODEX.txt`, `crates/project-app/tests/runtime_gate.rs`, `docs/HARNESS_REVIEW.md`, `scripts/verify`, `skills/implement-feature/SKILL.md` |
| `docs/VERIFY.md` | 11 | 16 | `CODEX_HANDOFF.md`, `README.md`, `RUNTIME.md`, `START_CODEX.txt`, `docs/HARNESS_REVIEW.md`, `docs/M10_DESIGN.md`, `docs/M2_DESIGN.md`, `docs/M7_DESIGN.md`, `docs/M8_DESIGN.md`, `docs/M9_DESIGN.md`, `scripts/verify` |
| `docs/DEFINITION_OF_DONE.md` | 5 | 5 | `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/UX_REDESIGN_01_DESIGN.md`, `scripts/verify` |
| `docs/MULTI_AGENT_WORKFLOW.md` | 6 | 7 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/HARNESS_REVIEW.md`, `scripts/verify` |
| `docs/WORKTREES.md` | 6 | 7 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/HARNESS_REVIEW.md`, `scripts/verify` |
| `docs/PLATFORM_POLICY.md` | 4 | 5 | `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/M10_DESIGN.md` |
| `docs/DISTRIBUTION.md` | 5 | 7 | `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/CURRENT_CHECKPOINT.md`, `docs/PLATFORM_POLICY.md` |
| `docs/MILESTONES.md` | 1 | 1 | `docs/M10_DESIGN.md` |
| `docs/M1_DESIGN.md` | 0 | 0 |  |
| `docs/M2_DESIGN.md` | 2 | 2 | `docs/ARCHITECTURE.md`, `docs/VERIFY.md` |
| `docs/M3_DESIGN.md` | 1 | 1 | `docs/VERIFY.md` |
| `docs/M4_DESIGN.md` | 1 | 1 | `docs/VERIFY.md` |
| `docs/M5_DESIGN.md` | 1 | 1 | `docs/VERIFY.md` |
| `docs/M6_DESIGN.md` | 0 | 0 |  |
| `docs/M7_DESIGN.md` | 0 | 0 |  |
| `docs/M8_DESIGN.md` | 0 | 0 |  |
| `docs/M9_DESIGN.md` | 2 | 3 | `docs/M10_DESIGN.md`, `docs/UX_RELEASE_GATE_01.md` |
| `docs/M10_DESIGN.md` | 1 | 1 | `tasks/future/REQ-M11-COMPONENT-UPDATES.md` |
| `docs/HARNESS_REVIEW.md` | 1 | 2 | `scripts/verify` |
| `docs/ARCHITECTURE_AUDIT_CONVERSATION_OPENCODE.md` | 1 | 1 | `docs/CURRENT_CHECKPOINT.md` |
| `docs/qwen-review-creation-share.md` | 1 | 1 | `docs/CURRENT_CHECKPOINT.md` |
| `docs/qwen-rereview-creation-share.md` | 1 | 1 | `docs/CURRENT_CHECKPOINT.md` |
| `docs/ux-rereview-creation-share.md` | 1 | 1 | `docs/CURRENT_CHECKPOINT.md` |
| `docs/UX_REDESIGN_01_DESIGN.md` | 0 | 0 |  |
| `docs/UX_RELEASE_GATE_01.md` | 2 | 2 | `docs/UX.md`, `docs/UX_REDESIGN_01_DESIGN.md` |
| `docs/CURRENT_CHECKPOINT.md` | 8 | 11 | `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/AGENT_POLICY.md`, `docs/CURRENT_CHECKPOINT.md`, `docs/M10_DESIGN.md`, `docs/UX_RELEASE_GATE_01.md`, `tasks/done/REQ-HIST-BASELINE/summary.md` |
| `docs/ux-redesign-01/` | 4 | 8 | `docs/CURRENT_CHECKPOINT.md`, `docs/UX_REDESIGN_01_DESIGN.md`, `docs/ux-redesign-01/RESULTS.md`, `scripts/verify` |
| `docs/ux-release-gate-01/` | 2 | 3 | `docs/UX_REDESIGN_01_DESIGN.md`, `docs/UX_RELEASE_GATE_01.md` |
| `docs/architecture-contracts/` | 7 | 12 | `AGENTS.md`, `scripts/verify`, `tasks/done/REQ-HARNESS-001/TASK-H4-architecture-contracts.md`, `tasks/done/REQ-HARNESS-001/TASK-H5-architecture-gate.md`, `tasks/done/REQ-HARNESS-001/TASK-H6-controls.md`, `tasks/done/REQ-HARNESS-001/evidence.md`, `tasks/done/REQ-HARNESS-001/requirement.md` |
| `docs/decisions/` | 5 | 7 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `scripts/verify` |
| `docs/checkpoints/` | 0 | 0 |  |
| `README.md` | 14 | 49 | `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `crates/project-agent/src/service.rs`, `crates/project-agent/tests/agent_service.rs`, `crates/project-app/src/app.rs`, `crates/project-app/src/creation.rs`, `crates/project-app/tests/creation_from_material.rs`, `docs/HARNESS_ENGINEERING.md`, `docs/KNOWLEDGE_ARCHITECTURE.md`, `scripts/verify`, `tasks/done/REQ-HARNESS-001/TASK-H1-entry-authority.md`, `tasks/done/REQ-HARNESS-001/TASK-H6-controls.md`, `tasks/done/REQ-HARNESS-001/requirement.md` |
| `START_CODEX.txt` | 2 | 3 | `tasks/done/REQ-HARNESS-001/TASK-H1-entry-authority.md`, `tasks/done/REQ-HARNESS-001/requirement.md` |
| `CODEX_HANDOFF.md` | 14 | 18 | `AGENTS.md`, `README.md`, `START_CODEX.txt`, `docs/DEFINITION_OF_DONE.md`, `docs/HARNESS_ENGINEERING.md`, `docs/M10_DESIGN.md`, `docs/M6_DESIGN.md`, `docs/M8_DESIGN.md`, `docs/MILESTONES.md`, `docs/UX_RELEASE_GATE_01.md`, `docs/decisions/0012-user-facing-copy-as-a-single-message-catalog.md`, `scripts/verify`, `tasks/done/REQ-HARNESS-001/TASK-H1-entry-authority.md`, `tasks/done/REQ-HARNESS-001/requirement.md` |
| `AGENTS.md` | 16 | 19 | `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/HARNESS_ENGINEERING.md`, `docs/M7_DESIGN.md`, `docs/M8_DESIGN.md`, `docs/UX_RELEASE_GATE_01.md`, `docs/decisions/0012-user-facing-copy-as-a-single-message-catalog.md`, `prompts/reviewer.md`, `prompts/worker.md`, `scripts/verify`, `skills/implement-feature/SKILL.md`, `tasks/done/REQ-HARNESS-001/TASK-H1-entry-authority.md`, `tasks/done/REQ-HARNESS-001/TASK-H2-roles-lifecycle.md`, `tasks/done/REQ-HARNESS-001/TASK-H6-controls.md`, `tasks/done/REQ-HARNESS-001/requirement.md` |
| `RUNTIME.md` | 9 | 12 | `AGENTS.md`, `CODEX_HANDOFF.md`, `README.md`, `START_CODEX.txt`, `docs/HARNESS_ENGINEERING.md`, `examples/README.md`, `prompts/orchestrator.md`, `tasks/done/REQ-HARNESS-001/TASK-H2-roles-lifecycle.md`, `tasks/done/REQ-HARNESS-001/requirement.md` |

## Relative Markdown link targets (non-http) that currently do not exist

_none found_

## scripts/verify required document paths (base tree)

- `docs/PRODUCT.md docs/ARCHITECTURE.md docs/SECURITY.md docs/UX.md \`
- `docs/DEFINITION_OF_DONE.md docs/TESTING.md docs/VERIFY.md docs/REQUIREMENTS.md \`
- `docs/WORKTREES.md docs/MULTI_AGENT_WORKFLOW.md docs/HARNESS_REVIEW.md \`
- `docs/HARNESS_ENGINEERING.md docs/architecture-contracts/README.md \`
- `docs/decisions/README.md \`
- `require_heading docs/TESTING.md '## Required test levels'`
- `require_heading docs/VERIFY.md '## Contract by milestone'`
- `require_heading docs/WORKTREES.md '## Checkout ownership'`
- `require_heading docs/MULTI_AGENT_WORKFLOW.md '## Herdr delegation'`
- `require_heading docs/HARNESS_REVIEW.md '## Resolved gaps'`
- `require_file docs/ux-redesign-01/RESULTS.md`
- `require_file docs/ux-redesign-01/harness/run.sh`
- `require_heading docs/ux-redesign-01/RESULTS.md '## Per-flow assertion matrix'`
- `require_file docs/decisions/0014-durable-conversation-history-in-project-aggregate.md`
- `require_file docs/decisions/0015-deterministic-free-model-discovery.md`
