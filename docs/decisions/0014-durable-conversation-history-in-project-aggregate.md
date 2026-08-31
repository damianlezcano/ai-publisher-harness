# ADR-0014: Durable conversation history lives in the project aggregate

- Status: Accepted

## Context

The approved chat-first product direction (UX_RELEASE_GATE_01, D1) requires that
a conversation's message history survive application close, restart, and
switching conversations. Today the frontend keeps messages only in client
`useState`; reopening a conversation shows an empty chat. There is no durable
message concept in the domain. `Project` (M1/ADR-0002) already owns stable
identity (UUIDv7), a renameable title, timestamps, materials, creations, and
sharing state — everything a "conversation" needs except the message list.

## Decision

The user-facing concept "Conversación" maps to the existing `Project` /
`ProjectId` identity (D2). Message history is a new `messages: Vec<Message>`
field on the `Project` aggregate, persisted inside `project.json` by the
existing `FilesystemProjectRepository` (atomic write, optimistic concurrency CAS
on `updated_at`). `PROJECT_SCHEMA_VERSION` becomes 3; v1/v2 files migrate
losslessly (messages default empty). Messages reference existing materials and
creations by id (no content duplication); ordering is append-only array order
with UUIDv7 message ids as the tie-break. No `localStorage` and no second store
is introduced.

## Consequences

- One durable source of truth for history; restart/switch restoration is a
  `project_open` read, no special restore path.
- Conversation identity, rename, timestamps, and sharing are reused unchanged.
- `project.json` grows with history; `MAX_MESSAGE_TEXT_CHARS` caps per-message
  size (a per-project cap is a future concern).
- Messages are never published: they are structurally outside every publish
  root (publisher serves only the public-creation snapshot).
- Message reference integrity (material/creation ids must be a subset of the
  project) is a core validation rule; material deletion clears its message
  references.

## Alternatives considered

### Frontend `localStorage`

Rejected: a second, non-durable-across-devices source of truth that the backend
cannot validate, migrate, or share; violates the "single owner" discipline.

### A new `Conversation` entity and store

Rejected: parallel identity and persistence for no benefit while `Project`
already carries all required attributes (D2).

### A separate `messages.json` per project

Rejected: splits the metadata aggregate, complicates atomic commit and
optimistic concurrency, and re-opens the torn-write problem the single
`project.json` atomic replace already solves.
