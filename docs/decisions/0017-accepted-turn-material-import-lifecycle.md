# ADR-0017: Accepted-turn material import lifecycle

Status: accepted

## Decision

Pre-send attachment staging is non-durable UI state. Dropping, selecting, or
removing an attachment creates no Material, project copy, Knowledge row,
embedding, history row, provider call, or import operation.

For a pasted image, the renderer keeps the bytes in a composer-owned in-memory
selection under an opaque staging ID. Removing the chip releases that entry;
closing an unsent composer drops it with the process/UI state. The staging ID
is neither a Material ID nor a path and is never persisted or logged. Clipboard
bytes cross the native boundary only as part of `Enviar`; there is no eager
`material_add_image` command path.

`Enviar` is the commitment boundary. The application creates one durable,
project-local accepted-import operation before copying the first accepted
Material. The operation is owned by the accepted user turn, binds its persisted
Materials, and records explicit phase/counter transitions:

`accepted -> copying -> indexing_lexical -> indexing_embeddings -> completed`

or `pending_retry` after a partial failure. Per-Material Knowledge states remain
the independent `Pending`, `Ready`, `Failed`, and `Unsupported` contract.

SQLite and filesystem copies are not claimed to be atomic. The ledger is
committed before copies, Material persistence precedes derived indexing, and a
turn is only linked once its Material list is durable. The first durable
`prepared > 0` (`copied`) write carries the turn link in the same ledger update,
so the invariant `prepared > 0 => turn_id != NULL` holds: a crash between the
material copy and user-turn persistence leaves the operation at `copied == 0`,
never a recoverable-looking `prepared > 0` row without a turn. A crash in the
narrower window between the turn append and that single ledger write leaves the
turn durable but the operation at `copied == 0` with no link — also a safe
pre-turn denial (never fabricated, never resent). On restart, an
incomplete ledger is discoverable and remains `pending_retry`; it is never
silently marked ready or automatically advanced. Recovery is an explicit retry
of that accepted operation, not a global worker.

Reopen recovery has one explicit seam: after project reconstruction, the
application reads the durable operation read model (`acceptedImport`) and, when
it describes an incomplete operation whose `agent_state` is `not_started`,
invokes the existing `resume_accepted_import_operation` command exactly once per
operation. This is a bounded, deterministic LOCAL recovery trigger — never a
generic background worker/daemon — and it never depends on ephemeral composer
or drag/drop state. Local work is idempotent: durable Material copies are reused
if present, lexical indexing is `INSERT OR IGNORE`-based, ready embeddings are
reused by generation, and progress counters are re-derived rather than advanced
blindly. The remote boundary is gated on `agent_state`: only `not_started` may
continue to the provider request; `started_outcome_unknown` (and any other
state) is never auto-resent, because a prior outbound request may already have
succeeded before process termination.

A pre-turn interruption — the process was killed after material copy but before
the user turn was persisted, leaving `turn_id` null and no message/prompt to
reuse — is a terminal, non-retryable denial (`recovery_no_turn`). The frontend
surfaces a truthful "re-send" message without a retry action, because retrying
can never recover a prompt that was never durably recorded.

Embedding selection has an accepted-batch boundary. It considers chunks reached
by the newly accepted Material IDs once, reuses ready vectors for the active
generation, and does not repeat a whole-corpus selection per file. One verified
local provider/session is reused for that batch when available.

## Superseded restriction and remaining prohibition

This narrowly supersedes the previous Knowledge statement that prohibited any
automatic worker/queue/scheduler/UI only to support an accepted turn's durable
Material import/index operation and its explicit recovery contract.

It does **not** authorize a generic job framework, global scheduler,
distributed queue, unrelated worker subsystem, arbitrary autonomous Knowledge
processing, pre-send indexing, post-send rollback/cancellation, or hidden work
unrelated to a user-accepted turn.

## Consequences

Progress is structural and durable: copied/total, lexical completed/total,
embedding completed/total, created/reused embeddings, and failures. The UI may
render these counts after a refresh without fabricating time percentages. A
Knowledge failure never removes an accepted Material or its committed user
message; it leaves a sanitized typed per-Material failure and a retryable
operation state.
