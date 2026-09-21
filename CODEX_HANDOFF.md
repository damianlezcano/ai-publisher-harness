# EducAI — Current Engineering Handoff

## Current state

EducAI is an implemented desktop application for non-technical users. It keeps
materials and creations local, lets the user work with AI, preview creations,
and share a project temporarily through a managed publication session.

The known functional baseline is commit `015d466`
(`checkpoint: stabilize conversation knowledge and creation architecture`).
At the time of this handoff, `main` and `origin/main` point to that commit.
Treat it as functional and do not redesign product behavior while evolving the
harness.

M0 through M10 have implementation and verification evidence in this
repository. M11 (component update and rollback lifecycle) remains a future
product milestone; do not infer that it is approved work merely because it is
described in historical design documents.

## Product and security boundaries

The product remains desktop-first and deliberately non-technical in its user
experience. The application owns projects, materials, creations, preview, and
publication. OpenCode and cloudflared are internal components, not user-facing
concepts.

The following boundaries are protected:

- Files remain local; originals in `inputs/` are immutable from the agent's
  perspective.
- Project data is separated into `inputs/`, `workspace/`, `outputs/`, and
  `publish/`.
- Only explicit `publish/` roots may be publicly served; `inputs/` and
  `workspace/` must never be exposed.
- Publication is project-scoped. Stopping one share must not affect another.
- UI depends on application core, which depends on ports/interfaces, which
  depend on adapters—not the reverse.
- The default UX must not expose implementation concepts such as ports,
  tunnels, provider credentials, servers, filesystem paths, or model IDs.

`docs/SECURITY.md`, `docs/ARCHITECTURE.md`, and `docs/UX.md` define the full
invariants. This summary does not replace them.

## Protected conversation and Knowledge architecture

The baseline also protects the turn-routing and Knowledge architecture recorded
in `docs/ARCHITECTURE.md`, `docs/KNOWLEDGE_ARCHITECTURE.md`, and
`docs/CURRENT_CHECKPOINT.md`. In particular, do not casually alter:

- OrdinaryChat isolation, semantic classifier authority, exclusive fallback,
  downstream no-reclassification, or single route binding;
- NormalSemantic, CorpusThematic, CorpusExhaustive, KnowledgeInventory,
  PerItem, K6, and active-material scope semantics;
- bounded conversation context; conversational, ephemeral-Knowledge, and
  scratch session responsibilities;
- Creation-with-Knowledge session rotation, `CompletedWithoutOutput`, scratch
  permission safety, answer grounding, exhaustive negative truth, provider-call
  bounds, raw-corpus forwarding limits, or project-level Knowledge isolation.

A proposal that needs to violate one of these boundaries is an architecture
change, not ordinary feature work. Stop, identify the affected contract or
invariant, and follow the architecture-change process.

## Harness entry points

Use this single authority order. It distinguishes product constraints from
Harness operation, task-local context, and changeable runtime configuration;
later groups never override an earlier one.

1. **Product constraints and architecture:** this handoff, `docs/PRODUCT.md`,
   `docs/ARCHITECTURE.md`, `docs/SECURITY.md`, `docs/UX.md`, then ADRs in
   `docs/decisions/`.
2. **Harness operating rules:** `AGENTS.md`, `docs/HARNESS_ENGINEERING.md`,
   `docs/AGENT_POLICY.md`, `docs/MULTI_AGENT_WORKFLOW.md`,
   `docs/WORKTREES.md`, `docs/TESTING.md`, `docs/VERIFY.md`,
   `docs/DEFINITION_OF_DONE.md`, `docs/PLATFORM_POLICY.md`, and
   `docs/DISTRIBUTION.md`.
3. **Requirement and task context:** `docs/REQUIREMENTS.md`, the selected
   requirement and bounded task contract under `tasks/`, affected Architecture
   Contracts, and `docs/CURRENT_CHECKPOINT.md`.
4. **Runtime configuration:** `RUNTIME.md`, then `config/agent-models.env`
   when the assigned role needs execution routing. These are operational inputs,
   never product or Harness-methodology authority.

`docs/HARNESS_ENGINEERING.md` is the canonical methodology for prompt,
context, Harness, evaluation, loop, and graph engineering.
`examples/README.md` is historical/example traceability only; it is not a
methodology authority.

## Continuing work

Before editing, identify the milestone or requirement, exact owned paths,
acceptance criteria, verification commands, and author/reviewer assignment.
Use a separate worktree for an implementation task; keep the integration
checkout lead-owned. A Worker implements only its task contract, a Reviewer is
independent and returns `PASS` or `REWORK`, and the Orchestrator closes work
only after required gates and human review where applicable.

Run the focused checks for the affected surface and finish with:

```bash
CI=true ./scripts/verify
```

The gate must not be weakened, tests must not be removed to ease migration, and
no external provider, tunnel, or credential is required for the deterministic
local suite. Manual release and human-product gates are additive, never a
replacement for the local gate.

## Product direction

The initial distribution target is Linux x86_64, with AppImage built in the
controlled Ubuntu 24.04 root; Windows x64 portability and native packaging are
governed by `docs/PLATFORM_POLICY.md`. Sidecar versions are pinned and checked.
Future work must preserve portability without introducing platform-specific
behavior outside an approved requirement.
