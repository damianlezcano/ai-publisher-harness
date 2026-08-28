# M1 Project Core Design

Status: Approved for implementation. ADR-0001 and ADR-0002 are accepted. This
document authorizes only the M1 scope described below.

## Executive summary

M1 delivers local project lifecycle, copied materials, and generated creations
with a portable on-disk project directory. It has no AI, chat, desktop UI,
publisher, tunnel, public URL, or sidecar implementation. The core is designed
as a testable Rust application library reached later through a narrow Tauri
adapter. M1's user-visible capabilities are create, open/list, rename, delete,
add/read material, and create/list/read creation.

## Module architecture

The proposed eventual workspace is deliberately small:

```
crates/project-core/       domain models, errors, use cases, ports
crates/project-fs/         local filesystem adapter for project-core ports
apps/desktop/              Tauri adapter and future TypeScript UI (not M1 UI)
tests/project-core/        black-box filesystem acceptance tests
```

`project-core` has no dependency on Tauri, OS-specific APIs, OpenCode,
Cloudflare, HTTP, or a UI framework. `project-fs` depends on the core port and
uses the local filesystem. `apps/desktop` depends inward on core and adapters,
never the reverse. If M1 is implemented without a desktop screen, it may expose
a test-only/application API; it must not fabricate a user-facing UI.

## Domain model

| Value/model | Fields and rule |
| --- | --- |
| `ProjectId`, `MaterialId`, `CreationId` | UUIDv7 strings; immutable and opaque |
| `ProjectName` | Required trimmed user label; bounded length; never a path |
| `RelativeProjectPath` | Normalized relative POSIX path; no empty segment, `.`, `..`, root, separator escape, or absolute form |
| `Project` | ID, name, timestamps, `ProjectState`, materials, creations |
| `ProjectState` | M1 only: `local`; publication state is excluded |
| `Material` | ID, display/original filename, content type if known, byte size, SHA-256, `inputs/<id>/<safe-name>` |
| `Creation` | ID, display name, kind, content type if known, byte size, `outputs/<id>/<safe-name>`, `revision: 1`, optional parent ID |
| `CreationKind` | `web`, `document`, `image`, or `file`; an extensible closed vocabulary in metadata |
| `Timestamp` | RFC 3339 UTC instant |

The display name is user-facing metadata. The stored filename is separately
sanitized and never used as an authorization mechanism. Creation metadata is
private in M1; future publication selection will be explicit rather than
inferred from its presence.

## Initial `project.json` schema

`schemaVersion` is an integer starting at `1`. Unknown future fields are
rejected by M1 readers rather than silently discarded. All timestamps are RFC
3339 UTC strings, all identifiers UUIDv7 strings, and all stored paths are
relative POSIX strings.

```json
{
  "schemaVersion": 1,
  "projectId": "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22",
  "name": "Fotosíntesis",
  "createdAt": "2026-08-28T15:00:00Z",
  "updatedAt": "2026-08-28T15:02:11Z",
  "state": "local",
  "materials": [
    {
      "materialId": "0198e4a6-79b2-7b51-9e68-c2eb7af3db14",
      "displayName": "Guía de clase.pdf",
      "originalFileName": "Guía de clase.pdf",
      "relativePath": "inputs/0198e4a6-79b2-7b51-9e68-c2eb7af3db14/guia-de-clase.pdf",
      "contentType": "application/pdf",
      "byteSize": 48291,
      "sha256": "lowercase-64-character-hex-digest",
      "createdAt": "2026-08-28T15:01:00Z"
    }
  ],
  "creations": [
    {
      "creationId": "0198e4a6-86d6-7c16-b4c4-3197b355cf10",
      "displayName": "Actividad interactiva",
      "kind": "web",
      "relativePath": "outputs/0198e4a6-86d6-7c16-b4c4-3197b355cf10/index.html",
      "contentType": "text/html",
      "byteSize": 9214,
      "revision": 1,
      "parentCreationId": null,
      "createdAt": "2026-08-28T15:02:11Z"
    }
  ]
}
```

`workspace/` and `publish/` are intentionally absent from metadata: their
locations are fixed by the layout, which removes an accidental-publication
selection path. M1 metadata stores no credentials, source absolute path, AI
provider/session, Cloudflare, tunnel, URL, QR, or publication state.

## Contracts

### `ProjectRepository` (core port)

**Responsibility:** persist and query the `Project` metadata aggregate by ID.

**Operations:** `create(project)`, `get(id)`, `list()`, `replace(project,
expectedUpdatedAt)`, `delete(id)`. `replace` performs optimistic concurrency
and atomic metadata replacement.

**Knows:** IDs, valid metadata schema, project root ownership, atomic metadata
protocol. **Does not know:** source files, content bytes, UI, Tauri, AI, HTTP,
or publication semantics.

**Errors:** `NotFound`, `AlreadyExists`, `Conflict`, `CorruptMetadata`,
`UnsupportedSchema`, `StorageUnavailable`, `AtomicWriteFailed`.

**Invariants:** never emits a project with invalid paths or duplicate IDs; list
order is deterministic (updatedAt descending, then ID); it only recognizes
direct child directories whose name equals the project ID.

### `ProjectContentStore` (core port)

**Responsibility:** write/read bytes only in the fixed material and creation
areas of an existing project.

**Operations:** `storeMaterial(projectId, materialId, source)`,
`readMaterial(projectId, materialId)`, `storeCreation(projectId, creationId,
content)`, `readCreation(projectId, creationId)`, and `removeProjectTree(id)`.
The returned stored-content descriptor contains relative path, size, and, for
materials, hash.

**Knows:** fixed roots (`inputs`, `outputs`, `workspace`, `publish`), safe
relative paths, atomic file placement. **Does not know:** project names, user
flows, Tauri, AI, HTTP, or which output will become public.

**Errors:** `NotFound`, `SourceUnreadable`, `InvalidPath`, `PathEscape`,
`SymlinkRejected`, `WriteFailed`, `IntegrityMismatch`.

**Invariants:** writes never target `workspace` or `publish` in M1; a material
write is copy-only and will not mutate the external source; read operations
resolve only metadata-derived paths beneath their fixed root.

### `ProjectService` (application use-case service)

**Responsibility:** orchestrate lifecycle and preserve consistency between
metadata and content.

**Operations:** `createProject`, `openProject`, `listProjects`, `renameProject`,
`deleteProject`, `addMaterial`, `readMaterial`, `createCreation`,
`listCreations`, and `readCreation`.

**Knows:** domain rules, repositories, clock, ID generator, and transaction
ordering. **Does not know:** Tauri commands, UI rendering, raw OS paths beyond
the boundary's source handle, OpenCode, Cloudflare, or HTTP.

**Errors:** domain errors above plus `InvalidName`, `DuplicateMaterial`,
`InvalidCreation`, and a user-safe `OperationFailed`. It never returns a
credential or filesystem absolute path to the UI.

**Invariants:** it commits content before metadata references it; updates
`updatedAt` only on committed changes; does not modify existing material bytes;
and compensates only its own newly written temporary/unreferenced content.

### Rejected M1 abstractions

Separate `MaterialRepository` and `CreationRepository` are not proposed. They
would independently rewrite the same `project.json` aggregate and create
avoidable write races. Materials and creations are owned collections of
`Project`; their bytes belong in `ProjectContentStore`.

A generic `FileSystem` port is also rejected. It tends to mirror operating
system APIs and gives false testability. The two semantic ports above allow
pure core tests with fakes, while adapter integration tests use real temporary
directories. An internal atomic-writer seam may be used only to inject
replace/flush failures in adapter tests.

## Persistence and filesystem strategy

1. Project creation validates the name and builds `projects/.staging-<id>` with
   all four directories and an atomically written `project.json`; it then
   renames the staging directory to `projects/<id>`.
2. Adding a material copies the selected source into a new file under
   `inputs/<material-id>/`, hashes while copying, flushes and renames it, then
   atomically replaces metadata. A later name conflict cannot overwrite an
   earlier input because the ID directory differs.
3. Creating a creation follows the same protocol under `outputs/<creation-id>/`.
4. Every adapter path is constructed from an ID and a validated,
   metadata-derived `RelativeProjectPath`; it rejects absolute paths, `..`,
   symlinks, and roots outside the project before I/O. Directory enumeration
   ignores `.staging-*` and temp files.
5. If metadata parsing/schema validation fails, opening returns a corruption
   error and makes no repair or overwrite. The user can recover the portable
   directory through a future recovery UX.

The adapter is the only layer allowed to know the configured projects base
directory. The future publisher receives only a canonical `publish/` directory
handle from a different port; it never receives `ProjectContentStore`.

## M1 acceptance tests

All tests use a temporary local projects root, deterministic clock/IDs, and
synthetic fixtures. They assert public service behavior.

| Scenario | Expected result |
| --- | --- |
| Create and reopen | Creates the four roots and schema-valid metadata; reopening preserves ID/name/timestamps |
| List and rename | Lists deterministically; rename survives a new service/repository instance |
| Add material | Copies source to `inputs`, persists metadata, and reads identical bytes after restart |
| Original immutability | Source bytes and stored material SHA-256 remain unchanged after add/read and attempted duplicate handling |
| Conflicting filenames | Two materials named `guide.pdf` have different IDs/paths and intact content |
| Create/list creation | Persists an output in `outputs`, lists it, and survives restart |
| Relative paths | Metadata contains no absolute path; every stored path remains below its fixed root |
| Atomic metadata | A simulated replace failure leaves the previous parseable metadata unchanged; success leaves no torn JSON |
| Corrupt/invalid metadata | Malformed JSON, unknown schema, duplicate IDs, invalid timestamp/path return typed error and do not overwrite files |
| Project-boundary defense | IDs/paths containing traversal or absolute forms cannot read/write outside the project; symlink target escape is rejected |
| Directory separation | Inputs, workspace, outputs, and publish all exist; material writes touch only inputs, creation writes only outputs, and fixed roots cannot be substituted |
| Delete | Deletes only the resolved project directory; missing ID returns `NotFound`; never accepts a caller filesystem path |

Additional error cases: blank/overlong name; missing source; unavailable base
directory; duplicate project ID; file write failure; metadata conflict; missing
material/creation; and project directory whose name and `projectId` disagree.

## Detailed implementation tasks

| Task | Dependency | Files owned | Suggested author / reviewer | Worktree | Definition of Done and verification |
| --- | --- | --- | --- | --- | --- |
| 1. Approve ADRs and initialize workspace/toolchains | User approval | root manifests, CI, verify docs/scripts | Codex / OpenCode | `m1/bootstrap` | Pinned Rust/Node toolchains; no product behavior; format/lint/type/test commands wired; reviewer approves architecture boundary; `./scripts/verify` |
| 2. Pure core models, errors, ports, and service tests | 1 | `crates/project-core/**` | Codex / Antigravity | `m1/project-core` | All unit acceptance rules that do not need local I/O pass; no Tauri/FS imports; `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test -p project-core`, `./scripts/verify` |
| 3. Filesystem adapter and integration tests | 1; contracts from 2 committed | `crates/project-fs/**`, `tests/project-core/**` | OpenCode / Codex | `m1/project-fs` | Implements atomic layout/content storage and all filesystem acceptance tests; security reviewer checks boundary cases; commands from task 2 plus `cargo test -p project-fs --test project_lifecycle`, `./scripts/verify` |
| 4. Integration and M1 verification gate | 2 and 3 | integration manifests, `scripts/verify`, docs | Codex / OpenCode | `m1/integration` | Integrates reviewed commits; verify invokes exact M1 checks; no UI or Tauri command implementation; full gate passes |

Tasks 2 and 3 are sequential because the adapter implements the core ports.
Task 1 is first. After task 2's contract commit, an optional Antigravity threat
model review can run in parallel with task 3, but no other author edits task
3's checkout. Task 4 is strictly last.

## Worktrees and Herdr plan

The integration checkout remains lead-owned. Create only one author checkout at
a time for this dependency chain: `../ai-publisher-m1-bootstrap`, then
`../ai-publisher-m1-project-core`, then
`../ai-publisher-m1-project-fs`, and finally
`../ai-publisher-m1-integration`. Reviewers get separate read-only diff or
worktree checkouts and never patch an author's checkout.

Before each delegation, the lead verifies `HERDR_ENV=1`, inspects live state,
creates the worktree, and splits a sibling pane with `--current` and
`--no-focus`. The lead starts only the needed agent kind in the returned pane,
prompts it with task ownership/acceptance/security criteria, and uses the
agent's unique name to wait/read. A `blocked` state is inspected before any
response. The lead integrates only reviewed commits and runs the full gate.

Suggested Herdr sequence: Codex author for task 1; OpenCode reviewer; Codex
author for task 2; Antigravity reviewer; OpenCode author for task 3; Codex
security reviewer; Codex integrator for task 4; OpenCode final reviewer.
Cursor Agent is intentionally unused in M1 because there is no UI task.

## M1 Definition of Done

- [ ] ADR-0001 and ADR-0002 are accepted before implementation.
- [ ] Every M1 acceptance test above passes, including restart, atomic-write,
      corruption, traversal, symlink, and directory-separation cases.
- [ ] `project-core` has no Tauri, OpenCode, Cloudflare, HTTP, or UI imports.
- [ ] No project metadata contains credentials, absolute paths, AI/session,
      tunnel, URL, QR, or publication data.
- [ ] The four fixed roots exist for every created project; only inputs and
      outputs receive M1 content writes as specified.
- [ ] Each implementation block has different author/reviewer and a recorded
      committed handoff.
- [ ] Formatting, lint, type checks, tests, integration tests, and
      `./scripts/verify` pass without network access.

## Required `scripts/verify` commands after M1

The exact commands become mandatory when Task 1 installs the toolchain:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked -p project-fs --test project_lifecycle
git diff --check
```

`scripts/verify` additionally runs a named M1 filesystem/security integration
suite (for example `cargo test -p project-fs --test project_lifecycle`) and
`git diff --check`. It fails if a Rust manifest, lockfile, or required Rust
command is absent. M1 has no frontend workspace, so it must not run `npm` or
introduce frontend tooling. A later milestone adds those gates together with a
real TypeScript package.
