# Codex Handoff

You are the lead engineering agent for this project. The repository is intentionally starting as a harness/specification repository rather than an implementation repository.

## Product objective

Build a desktop application for non-technical users, initially focused on education, that lets a user describe an idea in natural language, provide local source materials, let AI create resources locally, preview those creations, and publish the project temporarily to the Internet with one button.

The application must hide technical concepts such as ports, DNS, tunnels, servers, npm, Docker, Git, Cloudflare and OpenCode from the end user.

The product is NOT a Google Docs clone, LMS, collaborative editor, hosting platform, IDE, or permanent cloud service.

## Core user flow

1. Create/open a project.
2. Chat with AI.
3. Optionally drag/drop files, attach files, paste images/screenshots, or paste text.
4. AI creates resources locally.
5. User previews creations.
6. User presses `Publish`.
7. The application exposes the project through the currently active publication session and shows URL + QR.
8. User presses `Stop sharing` for that project.

## Key product decisions already made

- Desktop-first product.
- Tauri is the preferred desktop shell unless a later ADR replaces it.
- OpenCode is the agent engine and provider/model abstraction.
- Our app should not directly implement OpenAI/Gemini/DeepSeek/OpenRouter integrations for the MVP.
- OpenCode stays invisible to the non-technical user.
- Files remain local.
- Original user inputs are immutable/read-only from the agent's perspective.
- Project data is separated into `inputs`, `workspace`, `outputs`, and `publish`.
- `inputs` must NEVER be exposed by the HTTP publisher.
- Interactive outputs should prefer portable static web technology (HTML/CSS/JS/assets) rather than arbitrary backends.
- Documents such as DOCX/PPTX/XLSX/PDF are downloaded/opened using the host/client applications; we do not build online editors.
- The project is the publication unit.
- The project screen has one `Publish` button and, once published, one `Stop sharing` button.
- The user does not manually select files to publish each time.
- Private creations, e.g. teacher answer sheets, must be classifiable as private and excluded from publication.
- One local HTTP publisher serves all published projects.
- The first published project starts one Cloudflare Quick Tunnel.
- All other published projects reuse that tunnel using distinct URL paths.
- Example:
  - `https://random.trycloudflare.com/fotosintesis-a7k2`
  - `https://random.trycloudflare.com/sistema-solar-k91p`
- Stopping one project must not affect other published projects.
- When the last published project stops sharing, the tunnel may be shut down.
- Quick Tunnel URLs are temporary and may change between publication sessions.
- Each published project gets its own QR code.
- If the desktop app closes while projects are published, warn that the links will stop working.
- The root URL should NOT enumerate all published projects by default.
- No account with our product is required for the initial MVP.
- Credentials/configuration for AI providers should be delegated to OpenCode and stored locally where possible.
- The final product should be installable on Windows, macOS and Linux with platform-specific builds.
- OpenCode and cloudflared should be bundled/managed as internal components/sidecars rather than requiring users to install them separately.
- Do not track upstream `latest` blindly. Use pinned, tested compatible versions and a component update strategy with rollback.

## Harness engineering principles

This project is intentionally being developed with multiple local coding agents and a harness-engineering approach.

The human owns:
- product intent
- architecture boundaries
- priorities
- UX decisions
- irreversible tradeoffs

Agents own:
- implementation
- tests
- refactors
- reviews
- supporting documentation

Do NOT send multiple agents into the same checkout. Prefer one git worktree per independent implementation task.

Author and reviewer should be different agents whenever practical.

Every task must have executable acceptance criteria and should finish with `./scripts/verify` once that script exists.

## Expected agent ecosystem

The human has multiple local agents available, including Codex CLI, Cursor Agent, OpenCode with free models, and Antigravity CLI. `herdr` may be used by Codex as the orchestration layer.

Do not require every agent for every task. Use parallelism only where boundaries are sufficiently independent.

Suggested responsibilities:
- Codex Lead: decomposition, architecture, core implementation, integration, orchestration.
- Cursor Agent: UI implementation and UX-focused work.
- OpenCode agent: independent implementation/review/testing tasks.
- Antigravity CLI: focused research or independent analysis when useful.
- Reviewer agent: different from author for architecture/security/quality review.

## Required architecture boundaries

Target dependency direction:

UI -> Application Core -> Ports/Interfaces -> Adapters

Application Core concepts:
- ProjectManager
- MaterialManager
- CreationManager
- PublicationManager

Adapters:
- OpenCodeAgentAdapter
- LocalPublisherAdapter
- CloudflareTunnelAdapter
- filesystem/storage adapter
- secure credential/config adapter if needed

Rules:
- UI must not invoke cloudflared directly.
- UI must not call OpenCode implementation details directly.
- OpenCode adapter must know nothing about Cloudflare.
- Tunnel adapter must know nothing about project semantics.
- Publisher must know routes and publish roots, not AI semantics.
- Project core must not depend on Tauri-specific APIs.

## Project filesystem model

Conceptual per-project layout:

```
projects/<project-id>/
  project.json
  inputs/
  workspace/
  outputs/
  publish/
```

Semantics:
- `inputs/`: immutable originals supplied by the user.
- `workspace/`: agent scratch/work area; not normally visible to user.
- `outputs/`: user-visible creations generated by AI.
- `publish/`: generated temporary publication view; this is the ONLY tree the HTTP server may expose.

## Product vocabulary

User-facing terms:
- Project
- Materials
- Creations
- Preview
- Publish
- Stop sharing

Avoid exposing:
- port
- tunnel
- reverse proxy
- API key unless unavoidable in advanced setup
- runtime
- server
- DNS
- container

## MVP creation types

At minimum, plan for:
- static/interactive web output
- DOCX
- PDF
- PPTX
- XLSX
- images
- arbitrary downloadable files

## Milestones

### M0 - Harness
No product implementation required yet.

Deliver:
- repository conventions
- AGENTS.md
- PRODUCT.md
- ARCHITECTURE.md
- UX.md
- SECURITY.md
- ADR convention
- test conventions
- scripts placeholders
- worktree workflow
- Definition of Done template

### M1 - Project Core
Deliver project lifecycle and filesystem model without AI or Cloudflare.

Acceptance examples:
- create/open/delete project
- attach file to inputs
- paste image to inputs
- generate/store output representation
- inputs are never modified

### M2 - Local Publisher
Deliver local HTTP publication only.

Acceptance examples:
- published static web opens in browser
- documents download with correct filenames/content types
- only publish tree is reachable
- path traversal blocked
- unpublished project route unavailable

### M3 - Multi-project Publication Manager
Deliver project-level Publish/Stop-sharing semantics locally.

Acceptance examples:
- publish A
- publish B
- both routes work
- stop A -> B remains available
- stop B -> zero published projects

### M4 - Cloudflare Tunnel Adapter
Connect one publisher server to one Quick Tunnel.

Acceptance examples:
- first publish starts tunnel
- second publish reuses tunnel
- each project gets unique path
- stop one project does not restart/stop tunnel
- last stop can close tunnel
- public URL and QR available per project

### M5 - OpenCode Adapter
Introduce OpenCode behind a stable internal AgentEngine interface.

Capabilities to investigate/implement:
- send prompt
- continue session
- attach project context/materials
- cancellation/status
- provider/model discovery where OpenCode exposes it
- error propagation

### M6 - AI Chat and Creations
Connect chat -> OpenCode -> workspace/outputs.

### M7 - Attachments UX
Drag/drop, file picker, clipboard image/screenshot, pasted text.

### M8 - Preview
Interactive web preview and document/file handling.

### M9 - Education-focused UX polish
Projects list, simple chat, Materials, Creations, Preview, Publish, QR.

### M10 - Packaging
Windows, macOS, Linux builds; sidecar packaging.

### M11 - Component Updates
Pinned compatibility manifest for app/OpenCode/cloudflared, safe updates and rollback.

## Definition of Done philosophy

A task is not complete because code exists.
A task is complete when:
- architecture rules are respected
- tests demonstrate behavior
- lint/format/type checks pass
- security-sensitive boundaries are validated
- public behavior is documented
- review findings are resolved or explicitly accepted

## Security invariants

These are non-negotiable:
- inputs are never publicly served
- workspace is never publicly served
- publisher serves only generated publish roots
- prevent path traversal/symlink escape
- do not expose hidden/system files accidentally
- bind local publisher safely
- no arbitrary remote command execution through publication routes
- credentials are not stored in project files or logs
- no public directory index that reveals unrelated projects
- publishing one project must not leak another

## First instruction to Codex Lead

Do NOT start implementing the application yet.

First:
1. Read all repository docs.
2. Critique this harness for missing product/architecture/security decisions.
3. Propose only necessary ADRs for unresolved choices.
4. Complete Milestone M0.
5. Define `scripts/verify` contract and worktree/agent workflow.
6. Produce a short execution plan for M1-M4 with explicit task boundaries suitable for orchestration through herdr.
7. Ask the human only for decisions that are truly blocking and cannot be safely deferred behind an interface/ADR.

When M0 is complete, stop and present the result before starting M1.
