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

```
<app-data>/projects/<project-id>/outputs/<creation-id>/
  index.html          # web entry (any `.html` is stored under this name)
  …sibling CSS/JS/images copied as sidecars
```

Metadata lives on `Creation` in `project.json` (`displayName`, `kind`,
`visibility`, `relativePath`, `byteSize`, `revision` currently always `1`).
A later turn that modifies the **same** activity (same kind + display name)
overwrites this tree in place and keeps the same Creation id. A distinct
activity (different folder/display name) creates a new id.

## 6. Previewable outputs

In-app **Abrir** for a web Creation:

1. Resolves `outputs/<creation-id>/` for that project (same Creation as the card).
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
```

This is a **copied snapshot** of currently public Creations (ADR-0004), not a
live view of `outputs/`. The local HTTP publisher serves only registered
`publish/` roots. Cloudflare Quick Tunnel points at that publisher. The public
URL path uses the durable `publicationRoute` allocated on first publish; rename
and republish keep the same route.

Active sharing (tunnel, port, public hostname) is **runtime-only**. After quit,
links stop working even though `publish/` files remain on disk.

When a shared Creation is updated in place, the app rebuilds this snapshot
(`replace` on the existing route) so the **same public URL** serves the new
bytes. If that rebuild fails, the assistant message must not claim the public
link is already updated.

## 8. Publication model (from code, not intent)

| Question | Actual behavior |
| --- | --- |
| Live from Creation files? | No. Publisher never reads `outputs/` or `workspace/`. |
| Copied snapshot? | Yes. `PublicationSnapshotStore::prepare` copies public creations into `publish/`. |
| Separate publish tree? | Yes: `publish/` is a sibling of `outputs/`. |
| Same URL after update? | Yes, if republish/`replace` succeeds (same `publicationRoute`). |
| Generic landing page? | Only when no public web Creation exists. Share promotes the target web Creation before snapshotting. |

## 9. Conversation history / messages

Messages persist inside `project.json` (`messages: Vec<Message>`, schema v3).
No `localStorage`, no separate `messages.json`. User text is appended in
`send_message_persist` before the agent runs; the assistant outcome is appended
in `send_message_run`. Switching conversations is `project_open` of another id.

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

- `revision` is stored and validated as `1`; in-place Creation updates overwrite
  files without bumping revision (no schema migration).
- `workspace/` persists after turns; it is scratch, not a second Creation store.
- Publication is a snapshot, not live files. Updating a shared Creation requires
  an explicit republish/replace of `publish/` (implemented on the agent-complete
  path when the project is already published).
- Share/tunnel state is not persisted; a restart does not restore public URLs.
- Preview uses a temp copy of `outputs/<id>`, not `publish/` and not the AppImage.

If a future change migrates this tree, it must be a dedicated storage task with
an upgrade plan. This pass does not restructure storage.
