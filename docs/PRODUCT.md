# Product Specification

## Mission
Reduce the gap between what AI can create and what a non-technical person can immediately show or share.

Initial focus: educators who want to create resources in real time and expose them temporarily to students or other people.

## Core promise
Describe -> Create -> Preview -> Publish -> Share.

## Primary persona
A teacher or other non-technical user who already uses AI services and wants to create interactive resources or downloadable documents without learning software development or deployment.

## Product concepts
### Project
Container for conversation, materials, creations and publication state.

### Materials
User-provided context: files, pasted text, screenshots, images. Materials remain local and are never automatically published.

### Creations
AI-generated deliverables visible to the user.

### Publish
Expose the project temporarily through a public URL and QR.

### Stop sharing
Remove only that project's public route while preserving the local project.

## Publication semantics
- Publish is project-level.
- Multiple projects may be published simultaneously.
- All published projects share one publication session/tunnel.
- Each project receives a unique route beneath the session base URL.
- Stopping one project does not affect others.
- Stopping the final project may close the tunnel.

## Non-goals
- collaborative editing
- permanent hosting
- LMS
- online office suite
- IDE
- cloud-first storage
- account system for MVP
