# AC-014: Project Knowledge isolation

- Status: `ACTIVE`
- Invariant: a project's Knowledge index, materials and results are isolated from every other project.
- Rationale: project ownership is a product privacy and correctness boundary.
- Required observables: Knowledge is project-local and querying one project does not surface another project's material.
- Prohibited observables: cross-project Knowledge results, state or source reuse.
- Canonical test cases: `crates/project-app/tests/knowledge.rs::knowledge_is_project_local_and_isolated`; focused command: `cargo test --locked -p project-app --test knowledge -- knowledge_is_project_local_and_isolated`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: project IDs, Knowledge store/query boundary, material indexing.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-014`; update observables, test mapping, gate and ADR before approval.
