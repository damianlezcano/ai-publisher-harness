# ADR-0011: Prompt attachment contract

- Status: Accepted

## Context

M8 lets a user attach project materials to a prompt ("Creá una actividad usando
[manual.pdf] [diagrama.png]") without exposing filesystem paths. M5's
`AgentEngine` sends only `AgentPrompt { text, model }` to OpenCode, which
operates inside a per-project `workspace/` session directory. Materials live in
`inputs/`, are immutable from the agent's perspective, and are never served or
published. The frontend must never be able to hand the agent an arbitrary path.

## Decision

1. **Backend-resolved IDs only.** The frontend sends opaque `materialId` values;
   the application facade validates each against the current project's
   `materials` (foreign or unknown IDs are rejected) and reads bytes through the
   content store. No filesystem path crosses the frontend→agent boundary.

2. **Workspace provisioning without changing `AgentEngine`.** `AgentRequest`
   gains `attachments: Vec<AgentAttachment>` (name, kind, bytes).
   `AgentService.run` copies each attachment into
   `workspace/materials/<n>-<safe_name>` **before** `open_session`, so the files
   are part of the session baseline and are never reported as agent artifacts;
   it then prepends a deterministic context block to the prompt text. The
   `AgentEngine` trait, `AgentPrompt`, and the OpenCode adapter are unchanged.

3. **Defensive artifact exclusion.** Artifact normalization additionally ignores
   any path under `materials/` so provisioned inputs can never be registered as
   creations.

## Consequences

- `project-agent` changes are additive; M1-M7 `AgentEngine`/`AgentService` tests
  stay green, and the OpenCode adapter/`project-opencode` are untouched.
- Only sanitized names and stable kind labels enter the prompt; raw bytes and
  paths never do.
- Cross-project material references are impossible by construction (authorized
  against the current project only).
- Provider/model selection (M7) applies to attached prompts unchanged.

## Alternatives considered

### Extend `AgentEngine.send` / `AgentPrompt` with file parts

Would push attachment semantics into the port and the OpenCode adapter (file
parts), widening the blast radius and coupling the contract to OpenCode's prompt
schema. Rejected; workspace provisioning keeps the port stable.

### Frontend reads material bytes/paths and passes them to the agent

Rejected: grants the frontend path/byte privileges and violates "backend is the
authority."

### Attachments as pure UI metadata (never given to the model)

Rejected: fails the product goal that attached materials actually inform the
generation.
