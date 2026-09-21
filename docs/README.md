# Documentation map

This index points to canonical documents. It does not replace architecture,
security, UX, or Harness policy.

## Current (normative)

| Area | Path |
| --- | --- |
| Product, UX, security policy | [`product/`](product/) — `PRODUCT.md`, `UX.md`, `SECURITY.md` (keep distinct) |
| System / Knowledge / storage architecture | [`architecture/`](architecture/) |
| Executable architecture contracts | [`architecture-contracts/`](architecture-contracts/) |
| ADRs | [`decisions/`](decisions/) |
| Harness methodology, policy, lifecycle, verification | [`engineering/`](engineering/) |
| Platform and distribution | [`distribution/`](distribution/) |
| Current operational snapshot | [`CURRENT_CHECKPOINT.md`](CURRENT_CHECKPOINT.md) |

Start from the repository authority order in [`CODEX_HANDOFF.md`](../CODEX_HANDOFF.md) and [`AGENTS.md`](../AGENTS.md). Role prompts live in [`prompts/`](../prompts/). Runtime routing is [`RUNTIME.md`](../RUNTIME.md).

## Historical (not current authority)

| Area | Path | Retention |
| --- | --- | --- |
| Milestone designs M1–M10 and index | [`history/milestones/`](history/milestones/) | Keep as implementation traceability. M11 remains a future task, not approved work. |
| Audits and review reports | [`history/reviews/`](history/reviews/) | Keep findings; do not treat as live architecture. |
| UX redesign / release-gate reports and evidence | [`history/ux/`](history/ux/) | Keep report + evidence trees intact. |
| Dated technical snapshots | [`checkpoints/`](checkpoints/) | Historical only; see [`checkpoints/README.md`](checkpoints/README.md). |

`tasks/done/` is the requirement lifecycle record. It is not a documentation archive.

See also [`history/README.md`](history/README.md).
