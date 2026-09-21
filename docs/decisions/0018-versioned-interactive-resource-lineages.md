# ADR-0018: Interactive resources are immutable, versioned lineages

- Status: Accepted

## Context

A generated interactive activity is a bundle of files (`index.html`,
`estilos.css`, `app.js`, assets) registered as a `Creation`. When the user asks
for a change ("cambiar el color del fondo a blanco") and the agent modifies only
`estilos.css`, the previous registrar treated "changed files in this turn" as
"the complete new resource": it minted a standalone `estilos.css` File creation
(no `index.html`/`app.js`), which the UI offered to open but could not preview
as an interactive activity. In-place updates also mutated the original creation
in place, so there was no trustworthy historical snapshot and no stable lineage
identity distinct from `display_name`/slug.

## Decision

A logical interactive resource is a **versioned lineage** of complete,
immutable snapshots, identified by a real stable `lineage_id` (the V1
`CreationId` when safe). The persisted `Creation` gains additive, optional,
serde-compatible fields: `lineageId`, `versionNumber`, `isCurrent` (the existing
`parentCreationId` is reused as the parent version). Legacy `project.json`
records with none of these fields load as a one-version lineage (version 1,
current, lineage = own id) and stay byte-stable.

New-version algorithm (implemented in `project-fs`):

1. Resolve the base version (the lineage's current version).
2. Allocate a new `CreationId` (`version_id`).
3. Create `outputs/.staging-<version-id>/`.
4. Recursively copy the complete base version into staging.
5. Overlay only the changed/new files produced by this turn.
6. Validate the resulting tree (reject symlinks, traversal, unsafe entries).
7. fsync, then atomically rename `.staging-<id>` → `outputs/<id>`.
8. Persist metadata / current-version pointer only after the snapshot is
   complete.

The registrar groups a turn's changed artifacts: changed CSS/JS/images of an
existing web bundle belong to that lineage (a new VERSION), never to a
standalone File creation. A new bundle starts a new lineage (V1) captured from
the full workspace bundle directory. A crash before promotion never makes a
partial tree current; a crash after promotion leaves an orphan immutable
directory that is never current; stale `.staging-*` directories are recovered on
the next version build.

Preview ("Abrir") resolves the exact `outputs/<version-id>/` referenced by the
card — an old card opens V1, a new card opens V2, never "latest".

Publication materializes every version of the shared web lineage under
`versions/<id>/`. `/slug/` serves a generated version-history landing page
(publication metadata, never a copied creation index); `/slug/latest/` aliases
the current version's own snapshot; `/slug/<version-id>/` and
`/slug/<version-id>/<asset>` serve that exact immutable version. The
authoritative Share URL is the immutable current-version URL
(`/<slug>/<current-version-id>/`), so a copied link always identifies the exact
version that was shared. Unpublish removes the route but never deletes
`outputs/` or `publish/` history; re-publish restores it. The words `latest` and
`versions` are reserved; a version segment must parse as a version id belonging
to that project.

File deletion is deliberately unsupported: copy(base) + overlay(changed) is the
model, so a file removed from the workspace by the agent MAY still be present in
the next version until an explicit deletion/tombstone protocol exists.

## Consequences

- Every version is self-contained and independently previewable.
- Previous versions are immutable and byte-identical.
- Lineage identity is independent of display name/slug.
- Restart preserves lineage/version/current from `project.json`.
- Publishing exposes a generated version-history page at `/slug/` plus the
  current version at `/slug/latest/` and every historical version at
  `/slug/<version-id>/`; the Share/Copy/Open/QR URL is always the immutable
  current-version URL.
- Standalone file creations still work (each is a one-version lineage; a
  regenerated image becomes V2 of its own lineage).
- `outputs/<creation-id>/` for existing projects is treated as V1; nothing is
  moved, and legacy files keep loading.

## Alternatives considered

### Changed files as the complete new resource

Rejected: this is the root cause being fixed — a CSS-only change produced an
unopenable CSS "creation".

### In-place mutation with a `revision` bump

Rejected: mutating the previous version destroys historical snapshots and
makes "Abrir on the old card opens V1" impossible.

### Hard links / reflink for snapshot copy

Rejected for now: ordinary recursive copy is the supported, portable model; hard
links or reflink would couple immutability to filesystem-specific semantics.

### Per-version `current` flag vs a project-level current pointer

A per-version `current` flag is used because it fits the existing aggregate
cleanly. Validation guarantees exactly one current version per lineage, unique
and monotonic version numbers, and that a parent belongs to the same lineage.