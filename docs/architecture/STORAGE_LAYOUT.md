# Storage layout

This document describes **where EducAI actually stores data today**, as
implemented in this repository. It is not a proposal. The AppImage (or other
installable package) is **executable/package media only**. It is not the
persistent user-data container. Moving or deleting the AppImage does not move
or delete conversations, materials, or creations.

## Platform data root

The application data directory is resolved once at startup by Tauri
`app.path().app_data_dir()` (`app/src-tauri/src/lib.rs`). That path is the
`AppConfig.data_dir` / `AppState` base (`crates/project-app/src/app.rs`).

Typical values (environment-specific; placeholders):

| Platform | Actual root used by this build |
| --- | --- |
| Linux | `$XDG_DATA_HOME/com.educai.publisher` or `~/.local/share/com.educai.publisher` |
| macOS | `~/Library/Application Support/com.educai.publisher` |
| Windows | `%APPDATA%\com.educai.publisher` |

The bundle identifier is `com.educai.publisher` (`app/src-tauri/tauri.conf.json`).
The current working directory and the AppImage mount (`/tmp/.mount_EducAI…`) are
**not** used as the project store. Sidecar binaries may be resolved next to the
executable for packaging; user data is not.

The data root is created with owner-only permissions on Unix (`0o700`) before
first use (`ensure_app_data_dir`). Startup fails closed if that directory cannot
be created or protected.

```
<app-data>/                          # Tauri app_data_dir(); persistent
  settings.json                      # model selection / featured order (no secrets)
  projects/
    <project-id>/
      project.json                   # conversation metadata + messages
      inputs/                        # original user materials (immutable copies)
      workspace/                     # agent scratch (session directory)
      outputs/                       # registered Creations
      publish/                       # publication snapshot (HTTP origin)
  opencode/                          # isolated OpenCode XDG home
    data/
    cache/
    state/
  opencode-scratch/                  # provider-connector scratch (tests/OAuth)
```

Preview copies live in the process temp directory (`tempfile` prefix
`m8-preview-`), not under `<app-data>/`.

## 1. General config (provider / model / app settings)

| What | Path | Lifecycle |
| --- | --- | --- |
| Selected model, featured provider order | `<app-data>/settings.json` | Persistent, global, not per-conversation. Atomic temp+rename. Missing/corrupt → empty defaults. **No secrets.** |
| OpenCode credentials (`auth.json`) | `<app-data>/opencode/data/opencode/auth.json` (OpenCode's isolated `XDG_DATA_HOME`) | Persistent, global. Owned by OpenCode. Mode `0600`. Never copied into a project. |
| OpenCode config / cache / state | `<app-data>/opencode/` with `XDG_CONFIG_HOME`, `XDG_DATA_HOME=…/data`, `XDG_CACHE_HOME=…/cache`, `XDG_STATE_HOME=…/state` | Persistent across launches. Isolated from the user's global OpenCode via `--pure` + XDG. |
| Runtime process metadata | In-memory (`PublicationManager.published`, preview token map, agent sessions) | Lost on quit. Active share URLs are not durable (ADR-0004). |

The AppImage does not contain these files.

## 2. Conversation / project metadata

A user-facing **Conversación** is a `Project` (`ProjectId`, ADR-0014).

```
<app-data>/projects/<project-id>/project.json
```

`project.json` holds schema v3: id, display name, timestamps, optional durable
`publicationRoute`, optional per-conversation `model` (provider/model IDs only),
materials, creations, and `messages`. Paths inside it are
relative and forward-slash. Identity is a UUIDv7; rename changes only `name` and
`updatedAt`.

## 3. User attachments / materials

Originals are copied into the project, never referenced in place:

```
<app-data>/projects/<project-id>/inputs/<material-id>/<sanitized-filename>
```

SHA-256 + byte size are stored in `project.json`. Materials are immutable from
the agent's perspective (ADR-0002). They are never published.

During an agent turn, authorized attachments are **also copied** into the
session workspace as `workspace/materials/<n>-<name>` so the bound OpenCode
session can read them. That copy is scratch, not the durable original.

## 4. Agent workspace

```
<app-data>/projects/<project-id>/workspace/
```

This directory **is** the OpenCode session `directory` (`?directory=`). The
agent writes generated HTML/CSS/JS here. It is not shown as a user folder.
There is no explicit cleanup of `workspace/materials/` after a turn (accepted).
Workspace files are **not** what Abrir/Compartir serve; those use registered
`outputs/` (and `publish/` for the public URL).

## 5. Generated artifacts / Creations

A logical interactive resource is a **versioned lineage** of complete,
immutable snapshots (ADR-0018):

```
<app-data>/projects/<project-id>/outputs/<version-id>/
  index.html          # web entry (any `.html` is stored under this name)
  …sibling CSS/JS/images copied as sidecars
```

Each version directory is **self-contained and immutable**. A later turn that
modifies an existing activity (e.g. only `estilos.css`) builds a NEW version
directory by copying the base version's complete tree and overlaying only the
changed files, then atomically renames it into place; the previous version is
never mutated. V1..V3 of the same activity are three separate
`outputs/<version-id>/` trees.

Metadata lives on each `Creation` in `project.json`: `displayName`, `kind`,
`visibility`, `relativePath`, `byteSize`, `revision` (always `1`), plus the
versioned-lineage fields `lineageId`, `versionNumber`, `parentCreationId`, and
`isCurrent`. Legacy records without these fields load as a one-version lineage
(version 1, current, lineage = own id); they are never physically moved and
their `project.json` stays byte-stable. Exactly one version is current per
lineage; `parentCreationId` points to the previous version in the same lineage.

Version building happens in a `.staging-<id>` directory under `outputs/`
before an atomic rename, so a crash before promotion can never make a partial
tree current. Stale `.staging-*` directories are recovered on the next version
build.

## 6. Previewable outputs

In-app **Abrir** for a web Creation:

1. Resolves `outputs/<version-id>/` for that project (the exact version the card
   references, never "latest").
2. Copies that tree to a **temporary** directory (`m8-preview-*` under the OS temp dir).
3. Serves it from a loopback token server at
   `http://127.0.0.1:<ephemeral>/preview/<token>/` (token root maps to `index.html`).
4. Opens a zero-capability WebviewWindow at `…/index.html`.
5. On window close, the server stops and the temp copy is deleted.

Preview is ephemeral. It is not a second artifact store. Non-web Abrir uses the
host opener / in-app preview bytes from the same `outputs/` (or `inputs/` for
materials).

## 7. Publishable / shared content

```
<app-data>/projects/<project-id>/publish/
  index.html                # generated version-history landing page (metadata)
  versions/
    <version-id>/           # every published version, immutable
      index.html
      …
```

This is a **copied snapshot** of currently public Creations (ADR-0004), not a
live view of `outputs/`. The local HTTP publisher serves only registered
`publish/` roots. Cloudflare Quick Tunnel points at that publisher. The public
URL path uses the durable `publicationRoute` allocated on first publish; rename
and republish keep the same route.

Public routes:
- `/slug/` serves a generated version-history landing page (V1..VN, current
  marked "Actual", each with an "Abrir" link to its immutable version URL).
- `/slug/latest/` aliases the current version's actual resource.
- `/slug/<version-id>/` serves that exact immutable historical version.
- `/slug/<version-id>/<asset>` serves an asset of that exact version.

The words `latest` and `versions` are reserved. Unpublish only removes the
route registration; it never deletes `outputs/` or `publish/` history.
Re-publish restores the same route and historical URLs.

The authoritative Share URL exposed to Copy / Open / QR is the immutable
current-version URL (`/<slug>/<current-version-id>/`), so a shared link always
identifies the exact version the user shared at that moment. A later version
moves the share URL (and `/slug/latest/`) to the new version while every
previously copied historical URL keeps resolving to its exact version. For
non-web (document/file) publications the share URL is the route root.

Active sharing (tunnel, port, public hostname) is **runtime-only**. After quit,
links stop working even though `publish/` files remain on disk.

When a shared Creation lineage gains a new version, the app rebuilds this
snapshot (`replace` on the existing route) so the **same public URL** serves the
new current bytes while historical versions remain reachable. If that rebuild
fails, the assistant message must not claim the public link is already updated.

## 8. Publication model (from code, not intent)

| Question | Actual behavior |
| --- | --- |
| Live from Creation files? | No. Publisher never reads `outputs/` or `workspace/`. |
| Copied snapshot? | Yes. `PublicationSnapshotStore::prepare` copies public creations into `publish/` (all versions under `versions/<id>/`, plus a generated landing `index.html` and optional `materials.html`). |
| Separate publish tree? | Yes: `publish/` is a sibling of `outputs/`. |
| Same URL after update? | Yes, if republish/`replace` succeeds (same `publicationRoute`; the share URL and `/slug/latest/` move to the new current version, historical versions stay under `/slug/<version-id>/`). |
| Landing page? | `/slug/` is a generated version-history page for the shared web lineage; a materials page exists only when public non-web Creations exist. |
| Historical versions? | Every version of a public lineage is exposed under `/slug/<version-id>/` while the project is published. |

## 9. Conversation history / messages

Messages persist inside `project.json` (`messages: Vec<Message>`, schema v3).
No `localStorage`, no separate `messages.json`. User text is appended in
`send_message_persist` before the agent runs; the assistant outcome is appended
in `send_message_run`. Switching conversations is `project_open` of another id.

Per-turn telemetry (`TurnMetrics`) is stored on the user message that owns the
logical turn, and now includes `sourceNames` — the grounded source display
names that were formerly concatenated into the assistant text as a `Fuentes:`
block. New assistant messages carry no embedded `Fuentes:` suffix; the UI maps
each assistant message to its preceding user message's `TurnMetrics` to render
the per-turn compact line and detail popover. Older persisted messages whose
`text` already contains a generated `Fuentes:` suffix are left byte-stable
(no stripping or rewriting), and their `sourceNames` defaults to empty.

## 10. Persistent vs temporary vs reconstructed

| Category | Persistent | Temporary | Reconstructed / cached |
| --- | --- | --- | --- |
| settings.json | Yes (global) | | Corrupt file → defaults |
| project.json + inputs/ + outputs/ | Yes (per conversation) | | |
| workspace/ | Yes, leftover scratch after turns | | Not a user-facing restore path |
| publish/ | Last snapshot files remain | Active tunnel/URL die on quit | Republish rebuilds from public Creations |
| Preview temp + loopback server | | Process lifetime | Recreated on each Abrir |
| Agent OpenCode sessions | | Process lifetime | Reopened per project after backend restart |
| OpenCode models.json catalog | Cache under `opencode/cache` | | Refetched |
| Sidebar "Compartido" | Derived from in-memory published set + list DTO | Lost on quit (honest: share is session-scoped) | |

Deleted with the conversation (after fail-closed unpublish): the entire
`projects/<id>/` tree (messages, materials, creations, workspace, publish).
Retained globally: `settings.json`, OpenCode `auth.json` / XDG tree,
other conversations.

## 11. What happens when…

**The AppImage moves or is relaunched from another path.** User data stays in
`app_data_dir()`. Sidecars may come from the new bundle. Conversations reopen
from disk.

**cwd changes.** Project paths are absolute under `app_data_dir()`, not cwd.

**A conversation is renamed.** Only `project.json` `name` / `updatedAt` change.
Directory name remains the project id. Public route does not change.

**A conversation is deleted.** Unpublish first (fail-closed). Cancel in-flight
agent. Delete `projects/<id>/`. Other conversations and global config remain.

**The machine is offline / Cloudflare is down.** Local `outputs/` and preview
still work. Public URLs require the live tunnel + publisher.

## Discrepancy vs architectural intent

Intent (ADR-0002 / CODEX_HANDOFF) matches the on-disk project layout
(`inputs` / `workspace` / `outputs` / `publish`). Known implementation notes:

- `revision` is stored and validated as `1`; in-place Creation updates are NOT
  supported anymore. Modifying an activity builds a new immutable version
  (ADR-0018).
- `workspace/` persists after turns; it is scratch, not a second Creation store.
- Publication is a snapshot, not live files. Updating a shared Creation requires
  an explicit republish/replace of `publish/` (implemented on the agent-complete
  path when the project is already published and the new version belongs to a
  shared lineage).
- Share/tunnel state is not persisted; a restart does not restore public URLs.
- Preview uses a temp copy of `outputs/<version-id>`, not `publish/` and not the
  AppImage.

If a future change migrates this tree, it must be a dedicated storage task with
an upgrade plan. This pass does not restructure storage.
