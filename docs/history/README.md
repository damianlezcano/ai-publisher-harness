# Historical documentation

Files under `docs/history/` are retained for technical traceability: completed
milestone designs, audits, review reports, and UX gate evidence.

They are **not** current product, architecture, security, UX, or Harness
authority. If a historical document disagrees with a file under `docs/product/`,
`docs/architecture/`, `docs/engineering/`, `docs/distribution/`,
`docs/architecture-contracts/`, or `docs/decisions/`, the current document wins.

## Retention

- Keep content; do not delete source history to tidy the tree.
- Repair links when paths move; do not rewrite findings.
- Do not relocate these files into `tasks/done/`.

## Layout

- [`milestones/`](milestones/) — M1–M10 designs and the milestone index. M11 is only a future concept.
- [`reviews/`](reviews/) — harness/architecture/product review artifacts.
- [`ux/`](ux/) — UX redesign and release-gate reports plus their evidence trees.

Dated operational snapshots live in [`docs/checkpoints/`](../checkpoints/), not here.
