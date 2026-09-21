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

1. **Product constraints and architecture:** [current engineering handoff](CODEX_HANDOFF.md), [Product](docs/PRODUCT.md), [Architecture](docs/ARCHITECTURE.md), [Security](docs/SECURITY.md), [UX](docs/UX.md), then [ADRs](docs/decisions/README.md).
2. **Harness operating rules:** [AGENTS.md](AGENTS.md), [Harness Engineering methodology](docs/HARNESS_ENGINEERING.md), [Harness policy](docs/AGENT_POLICY.md), [multi-agent workflow](docs/MULTI_AGENT_WORKFLOW.md), [worktree workflow](docs/WORKTREES.md), [testing](docs/TESTING.md), [verification](docs/VERIFY.md), [definition of done](docs/DEFINITION_OF_DONE.md), [platform policy](docs/PLATFORM_POLICY.md), and [distribution](docs/DISTRIBUTION.md).
3. **Requirement and task context:** [requirements lifecycle](docs/REQUIREMENTS.md), the selected requirement and bounded task contract under `tasks/`, affected Architecture Contracts, and [current checkpoint](docs/CURRENT_CHECKPOINT.md).
4. **Runtime configuration:** `RUNTIME.md`, then `config/agent-models.env` when the assigned role needs execution routing. These are operational inputs, never product or Harness-methodology authority.

The known functional baseline is `015d466` (`main` and `origin/main`).
Verification is deterministic and local:

```bash
CI=true ./scripts/verify
```

Do not weaken the gate or alter protected product architecture merely to
accommodate harness work. See `CODEX_HANDOFF.md` for the protected boundaries
and continuation rules. A short intent such as `Implementar REQ-XXXX.` is a
complete Orchestrator assignment; the procedure is `prompts/orchestrator.md`.
