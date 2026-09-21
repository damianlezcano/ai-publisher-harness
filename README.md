# EducAI

EducAI is a desktop application for non-technical users to create resources
with AI from local materials, preview them, and share a project temporarily.
The user experience deliberately hides infrastructure details such as servers,
tunnels, ports, provider credentials, and filesystem paths.

The repository contains the implemented product, its deterministic test suite,
packaging/distribution tooling, and its agent-engineering harness.

## Start here

Use this single authority order. It distinguishes product constraints from
Harness operation, task-local context, and changeable runtime configuration;
later groups never override an earlier one.

1. **Product constraints and architecture:** [current engineering handoff](CODEX_HANDOFF.md), [Product](docs/product/PRODUCT.md), [Architecture](docs/architecture/ARCHITECTURE.md), [Security](docs/product/SECURITY.md), [UX](docs/product/UX.md), then [ADRs](docs/decisions/README.md).
2. **Harness operating rules:** [AGENTS.md](AGENTS.md), [Harness Engineering methodology](docs/engineering/HARNESS_ENGINEERING.md), [Harness policy](docs/engineering/AGENT_POLICY.md), [multi-agent workflow](docs/engineering/MULTI_AGENT_WORKFLOW.md), [worktree workflow](docs/engineering/WORKTREES.md), [testing](docs/engineering/TESTING.md), [verification](docs/engineering/VERIFY.md), [definition of done](docs/engineering/DEFINITION_OF_DONE.md), [platform policy](docs/distribution/PLATFORM_POLICY.md), and [distribution](docs/distribution/DISTRIBUTION.md).
3. **Requirement and task context:** [requirements lifecycle](docs/engineering/REQUIREMENTS.md), the selected requirement and bounded task contract under `tasks/`, affected Architecture Contracts, and [current checkpoint](docs/CURRENT_CHECKPOINT.md).
4. **Runtime configuration:** `RUNTIME.md`, then `config/agent-models.env` when the assigned role needs execution routing. These are operational inputs, never product or Harness-methodology authority.

The known functional baseline is `015d466` (`main` and `origin/main`).
Verification is deterministic and local:

```bash
CI=true ./scripts/verify
```

Do not weaken the gate or alter protected product architecture merely to
accommodate harness work. See `CODEX_HANDOFF.md` for the protected boundaries
and continuation rules.

## Repository structure

### PRODUCT

- `app/` — desktop shell (Tauri/UI).
- `crates/` — application core, ports, and adapters.

### HARNESS

- `prompts/` — role-local Orchestrator / Worker / Reviewer contracts.
- `skills/` — skill-specific operating aids.
- `scripts/` — verification, launch, packaging, and other repo tools.
- `config/` — operational routing such as `agent-models.env` (not product authority).
- `AGENTS.md`, `CODEX_HANDOFF.md`, `START_CODEX.txt`, `RUNTIME.md` — agent rules, durable handoff, bootstrap pointer, and runtime routing.

### DOCUMENTATION

- `docs/` — canonical current docs plus explicit history/checkpoints. Start at [`docs/README.md`](docs/README.md).

### TASK LIFECYCLE

- `tasks/` — backlog / active / done / future requirement records (not a document archive).

### BUILD / DISTRIBUTION

- `packaging/` — package construction.
- `sidecars/` — pinned sidecar binaries and checksums.
- `components/` — bundled/component artifacts used by packaging.
- `examples/` — historical/example traces, not methodology authority.

### GENERATED / LOCAL ARTIFACTS

These are not source authority: `target/`, `.pnpm-store/`, local logs, and similar machine-local output.
