# AC-013: Corpus privacy

- Status: `ACTIVE`
- Invariant: raw corpus bodies, filesystem paths and secrets do not appear in provider-forwarded telemetry or logs.
- Rationale: local materials are sensitive and observability must remain safe.
- Required observables: privacy metrics and logs omit bodies, paths and secrets.
- Prohibited observables: raw corpus forwarding beyond bounded evidence or private data in logs/metrics.
- Canonical test cases: `crates/project-app/tests/exhaustive_rag.rs::h_privacy_metrics_and_logs_omit_bodies_paths_and_secrets`; focused command: `cargo test --locked -p project-app --test exhaustive_rag -- h_privacy_metrics_and_logs_omit_bodies_paths_and_secrets`.
- Gate: `./scripts/architecture-verify`.
- Protected/change-sensitive areas: logging, telemetry, evidence serialization, provider requests.
- Approval requirement: ADR, independent review, and explicit human approval for an invariant change.
- Change process: stop with `ARCHITECTURE_CHANGE_REQUIRED: AC-013`; update observables, test mapping, gate and ADR before approval.
