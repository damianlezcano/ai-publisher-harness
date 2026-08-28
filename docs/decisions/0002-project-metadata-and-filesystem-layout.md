# ADR-0002: Project metadata and filesystem layout

- Status: Accepted

## Context

M1 establishes a portable, local project unit before AI, publishing, or tunnel
behavior exists. Materials must remain original and immutable from the agent's
perspective. Future publication must be structurally unable to select materials
or workspace content by accident.

## Decision

Persist each project as one self-contained directory:

```
projects/<project-id>/
  project.json
  inputs/
  workspace/
  outputs/
  publish/
```

`project.json` is the metadata aggregate and contains only relative,
forward-slash paths rooted in its project directory. `inputs/` contains copied
original material bytes. The application never overwrites, renames, or deletes
an existing material file; future agent access is read-only by contract.
`workspace/` is internal scratch space. `outputs/` contains creation artifacts.
`publish/` is empty or generated in M1 and is the sole candidate publish root in
M2. No metadata field can nominate `inputs/`, `workspace/`, or `outputs/` as a
publish root.

Use stable UUIDv7 identifiers for projects, materials, and creations. Store
material and creation files below an ID-named directory, preserving a sanitized
display filename, so equal source filenames cannot collide. Store SHA-256 and
byte size for materials to verify the copied original remains unchanged.

Metadata changes use a write-temp, flush-file, atomic-replace, flush-parent
directory protocol provided by the storage adapter. Content files are written
to a same-tree temporary file, flushed, and atomically renamed before metadata
references them. Project creation uses a staging directory under `projects/`,
then a single rename to its ID. The adapter handles platform-specific atomic
replacement semantics. Failed operations return an error; cleanup may remove
only known temporary/staging entries and never unrelated project data.

## Consequences

- A project can be copied or backed up as one directory without rewriting
  absolute paths.
- M1 does not implement file history. Creation records include `revision: 1`
  and optional `parentCreationId` so later versioning can add revisions without
  changing identity semantics.
- OS-level immutable flags are not used because they are inconsistent across
  platforms. Immutability is enforced by core APIs and verified by hashes;
  sidecar sandboxing is deferred to the OpenCode adapter milestone.
- Metadata writes are safe against torn files, but a sudden crash may leave a
  harmless unreferenced temporary file, which startup cleanup may handle.

## Alternatives considered

### One shared database plus an external asset directory

This eases global queries but weakens portability and creates a coordination
point outside the project. It is unnecessary before cross-project indexing.

### Flat files named only by user filename

This is simpler initially but makes conflicting names and stable metadata hard.

### Content-addressed material storage

It could deduplicate content but complicates portability, garbage collection,
and user ownership before there is evidence that it is needed.

### Operating-system read-only permissions for inputs

They are platform-dependent and may frustrate legitimate recovery/migration.
The core API plus integrity metadata provides a portable M1 guarantee.
