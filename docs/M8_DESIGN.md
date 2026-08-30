# M8 Advanced Attachments and Resource Experience — Design

Status: **Approved** (architecture accepted by the human owner; no
implementation yet). ADR-0010 and ADR-0011 are Accepted. This document is the
durable design handoff for the M8 implementation session (a fresh
`opencode-go/deepseek-v4-flash` orchestrator).

## 1. Executive summary

M8 makes the "Materials → Creations → Preview" loop feel natural to a
non-technical user without touching the M1-M7 architecture. It adds:

1. **Clipboard image/screenshot paste** (Ctrl+V → material), fully validated
   backend-side, with no new clipboard privilege granted to the frontend.
2. **Richer attachments**: multi-file drop with deterministic per-file
   reporting, partial-failure semantics, content deduplication, and safe
   remove; the original files are never mutated.
3. **Prompt attachments**: the user can attach project materials to the *next*
   prompt by reference; the backend resolves authorized material IDs into agent
   workspace copies. `AgentEngine`'s port is unchanged.
4. **Safe in-app previews**: images and text/Markdown render in-app; PDF and
   office documents open in the system handler (no office suite is built).
5. **Isolated interactive web preview**: generated HTML/JS runs in a separate,
   zero-capability webview fed by a loopback-only, token-guarded preview server.
   This is the single highest-risk area and carries an explicit fallback.

The decisive architectural result: **the frontend never sends a filesystem path
or clipboard bytes that the backend does not re-validate; the backend resolves
every material/creation by ID; generated web content never gains Tauri IPC.**

`project-core` gains one additive operation (`remove_material`); the
`AgentEngine` trait and `AgentPrompt` model stay identical. All other M8 work is
facade, adapter, or frontend.

## 2. M7 / M8 / next-milestone boundary

| Milestone | Owns | Excludes |
| --- | --- | --- |
| M7 | AI provider onboarding (CLOSED) | attachments, previews, clipboard |
| M8 | Clipboard image paste, multi-file drop, material lifecycle (add/inspect/remove/dedup), prompt attachments, creation/resource details, safe previews, isolated web preview | office editor, cloud sync, accounts/payments, packaging, provider redesign, Windows release, autonomous publishing, plugins |
| M9 | Education-focused UX polish (re-numbered; content designed later) | — |

M8 **consumes** M7 provider/model selection as an existing capability (the
global model choice applies to attached prompts exactly as it does today). M8
does **not** redesign provider onboarding, the `ProviderConnector` port, or the
shared `OpenCodeBackend`.

## 3. M8 user journeys

1. **Paste a screenshot.** The user copies a screenshot, focuses the prompt box,
   presses Ctrl+V. The image becomes a material ("captura.png") in the current
   project, shown as a card; no file dialog, no path, no format choice.
2. **Drop several files at once.** The user drags `manual.pdf`,
   `diagrama.png`, and a corrupted file onto the project. The first two are
   added as materials; the third is reported "No pudimos agregar ese archivo."
   Nothing else is affected, and the originals are untouched.
3. **Create with attachments.** The user attaches `manual.pdf` and
   `diagrama.png` to the prompt "Creá una actividad usando estos materiales" and
   sends. The AI receives the materials as context and produces a creation.
4. **Inspect and remove.** The user sees each material as a chip/card with a
   type label and size; a remove action deletes only that material (never the
   source file). Duplicate content is detected and reported instead of
   re-imported.
5. **Preview a creation.** For an image or text/Markdown creation the user sees
   it in-app; for a generated web app, an isolated preview window opens; for a
   PDF/DOCX the system handler opens it. Nothing previewed is published.
6. **Understand a creation.** Each creation shows type, created time, and
   share/private state, with open/preview actions.

## 4. Clipboard / paste architecture

**Frontend is never granted unrestricted clipboard read.** The image is obtained
through the DOM `paste` event, which only delivers data the user explicitly
pasted into the app. No `tauri-plugin-clipboard-manager` is added (that plugin
would grant on-demand clipboard read beyond the paste gesture).

Flow:

```
paste event (Ctrl+V) on prompt/textarea
  -> inspect event.clipboardData.items
     - image file item (type image/*)  -> read Blob -> ArrayBuffer -> command
     - text item only                  -> default text paste (unchanged)
  -> invoke "material_add_image(projectId, fileName, contentType, data: Vec<u8>)"
  -> backend: validate -> store via ProjectService.add_material -> MaterialView
```

Backend `material_add_image` validation (all fail-closed):

- `content_type` must be an allowed image type: `image/png`, `image/jpeg`,
  `image/webp`, `image/gif`, `image/bmp`, `image/svg+xml`.
- **Magic-byte sniff**: the actual bytes must match the declared type (PNG
  `\x89PNG`, JPEG `FF D8 FF`, GIF `GIF8`, WEBP `RIFF....WEBP`, BMP `BM`).
  SVG is validated for the `svg` root element only (it is text; the renderer
  never executes it — see §10/§11). Mismatch → reject ("Esa imagen no es
  válida.").
- **Size cap** for clipboard images: 25 MB (configurable constant). Oversize →
  "Esa imagen es demasiado grande."
- A deterministic **file name** is synthesized from the detected format
  (`captura-<timestamp>.png`), sanitized through `safe_file_name`. The display
  name is human-friendly ("Captura").
- **Duplicate paste**: content SHA-256 is compared against existing project
  materials; a re-paste of the same bytes returns the existing material and
  reports "duplicate" (see §5), so Ctrl+V twice does not create two copies.
- **Atomic creation**: `ProjectService.add_material` already writes content
  first, then commits metadata under optimistic concurrency (§M1). A failed
  import leaves no metadata reference.
- **Malformed clipboard data**: empty bytes, non-image content, or an image that
  fails sniffing is rejected with a single friendly message; nothing is
  written.

Notes:
- If WebKitGTK proves unable to deliver image items via `paste`
  (`clipboardData.items` empty for images), the fallback is to add
  `tauri-plugin-clipboard-manager` *only* behind a dedicated "Pegar imagen"
  button — this is a documented risk (§27) and is **not** part of the default
  M8 design. This is validated in manual smoke (§19).

## 5. Multi-file attachment architecture

The current M6 path calls `material_add_from_path` once per dropped file with no
reporting and no dedup. M8 replaces this with a single batch command:

```
materials_add_from_paths(projectId, paths: Vec<String>) -> MaterialsImportReport
```

`MaterialsImportReport { items: Vec<MaterialImportResult> }` (ordered, input
order), where:

```
MaterialImportResult {
  sourceName: String,       // sanitized base name only, never a full path
  status: "added" | "duplicate" | "unsupported" | "failed",
  materialId: Option<String>,   // set on "added" and "duplicate"
  reason: Option<String>,        // human message on unsupported/failed
  material: Option<MaterialView>,
}
```

Semantics:

- **Deterministic result reporting**: one entry per input, in input order;
  partial success is explicit.
- **Partial failure**: each file is processed independently; one bad file never
  aborts the batch. Files that fail are reported with a reason; files that
  succeed are added.
- **No original modification**: every import copies bytes into `inputs/<id>/`
  via the M1 content store; the source is only ever read.
- **Symlink/path validation**: reuse the M6/`read_source_file` policy — reject
  symlinks, directories, and non-regular files before reading (project-fs
  re-validates containment on write). No traversal is possible because the
  backend only ever uses the *sanitized base name* plus a server-generated ID.
- **Duplicate handling**: content SHA-256 is computed before import and compared
  against (a) existing project materials and (b) earlier entries in the same
  batch. A match yields `status: "duplicate"` with the existing `materialId`
  and is not re-imported.
- **Size/type policy**: a per-file size cap (100 MB for files; 25 MB for
  clipboard images) and the existing regular-file rule. Oversize →
  `unsupported` ("demasiado grande").
- **Progress UX**: not required for M8 (imports are local and fast). The
  frontend shows a single busy state and then renders the report. A progress
  bar is deferred.

The batch command lives in `project-app` and composes the existing
`ProjectService.add_material`; it does **not** bypass project-core/project-fs
invariants (write containment, symlink rejection, atomic metadata).

## 6. Prompt attachment model

Decision: **prompt attachments are backend-resolved by ID; the frontend never
sends a path.** This reuses the M5 `AgentEngine` port unchanged and does not
redesign AgentEngine.

Contract (command layer):

```
agent_send(projectId, prompt, attachmentIds: Vec<String>)
```

The frontend holds only opaque material IDs (from `MaterialView`). The backend:

1. Validates each ID (`MaterialId::parse`) and **authorizes** it against the
   current project's `materials` (a material from another project is rejected —
   cross-project reference is a test case). Unknown/foreign IDs → `invalid_input`.
2. Reads the bytes via `ProjectService.read_material` (which re-checks SHA-256
   integrity).
3. Passes an `AgentRequest` with a new `attachments` field to `AgentService`.

`project-agent` change (additive, port unchanged):

```rust
pub struct AgentRequest {
    pub project_id: String,
    pub prompt: AgentPrompt,
    pub attachments: Vec<AgentAttachment>,   // NEW (default empty)
}
pub struct AgentAttachment {
    pub display_name: String,   // e.g. "manual.pdf" (already sanitized)
    pub kind: String,           // stable kind code, e.g. "pdf", "image"
    pub bytes: Vec<u8>,
}
```

`AgentService.run` provisions each attachment into the session workspace
**before** `open_session` (so the files are part of the session baseline and are
never reported as agent artifacts), under `workspace/materials/<n>-<safe_name>`
using `project_core::safe_file_name`, and prepends a deterministic context block
to the prompt text:

```
Materiales adjuntos (usá estos archivos como contexto; están en la carpeta "materials"):
- manual.pdf (PDF)
- diagrama.png (imagen)
```

Only sanitized names and kind labels are injected (never paths, never bytes).
The `AgentEngine` trait, `AgentPrompt`, `AgentService` public signature
semantics, and the OpenCode adapter remain unchanged. A defensive filter in
artifact normalization excludes any path under `materials/` from creation
registration (belt-and-suspenders; the pre-baseline write already prevents it).

The provider/model selection (M7) applies to attached prompts exactly as today
(no change to `AgentPrompt.model`).

## 7. Backend / API changes

`project-core`:

- `ProjectService::remove_material(&mut self, pid, mid) -> CoreResult<()>`:
  remove from metadata and delete the `inputs/<id>` content dir (original never
  affected). Guarded by the existing optimistic-concurrency `replace`.
- `ProjectContentStore::remove_material(&mut self, p, m)` (new trait method) +
  `FilesystemProjectContentStore` impl (symlink-checked, fixed-root `inputs/<id>`
  removal).

`project-fs`:

- `FilesystemProjectContentStore::remove_material` (containment + symlink
  checks, then `remove_dir_all` on `inputs/<id>` only).

`project-app` (facade additions):

- `add_material_image(project_id, file_name, content_type, bytes)` — §4.
- `import_materials(project_id, paths) -> MaterialsImportReport` — §5.
- `remove_material(project_id, material_id)`.
- `material_path(project_id, material_id)` + `open_material(...)` (system
  handler, mirroring `creation_path`/`open_creation`).
- `preview_data(project_id, resource_id, resource_kind) -> PreviewData` — §10.
- `preview_open_web(project_id, creation_id)` — §11.
- `preview_close(token)` — §11.
- `run_agent(project_id, prompt, attachment_ids)` — §6.
- `material_view`/`creation_view` DTOs gain `createdAt` (and keep existing
  fields); `CreationView` also exposes `revision`.

`project-preview` (new crate) — §11.

No change to `AgentEngine`, `ProviderConnector`, publisher, or tunnel.

## 8. Material lifecycle UX

- **add**: drag/drop (batch), file picker (single, reused for batch of one), and
  clipboard paste (image).
- **inspect**: card/chip shows name, type label, size, and created time. An
  "Abrir" action opens the material in the system handler (read-only). Images
  preview in-app via the same `preview_data` path.
- **remove**: per-card remove with an inline confirmation (name shown). Removing
  deletes only the app's stored copy under `inputs/<id>`; the user's source file
  is never touched.
- **duplicate handling**: re-importing identical content is reported as
  "duplicate" and points to the existing material (§5).
- **unsupported file**: non-regular/symlink/oversize → per-file "unsupported"
  result with a friendly reason; the batch continues.
- **large file**: size cap with "demasiado grande".
- **failed import**: per-file "failed" result + a single `aria-live` error
  announcement summarizing failures; partial successes remain.

## 9. Creation / resource UX

- `CreationView` adds `createdAt` and `revision` (revision already exists in
  core but was not surfaced). The UI shows kind + created time + share/private
  state; revision is not turned into a version-control UI (revision stays a
  single value today).
- Actions per creation, by kind:
  - image → "Vista previa" (in-app) + "Abrir" (system) + visibility toggle.
  - text/Markdown → "Vista previa" (in-app) + "Abrir" (system) + visibility.
  - web → "Vista previa" (isolated webview) + "Abrir en navegador" (system) +
    visibility.
  - document/pdf/other → "Abrir" (system) + visibility (no in-app renderer).
- "Update/regenerate": **not** added as a UI surface in M8. A follow-up prompt
  ("Hacé la actividad más fácil") produces a new creation (revision 1, new id)
  because M5 registers each artifact as a distinct private creation; M8 does not
  invent a revision/regenerate flow. This is documented as the current domain
  semantic and is not hidden or mislabeled.

## 10. Preview strategy by resource type

| Resource | Strategy | Mechanism |
| --- | --- | --- |
| Image (png/jpeg/webp/gif/bmp) | In-app `<img>` | `preview_data` returns bytes+type; rendered as blob/object URL. No path exposure. |
| SVG | In-app `<img>` only (never inlined into DOM) | treated as a static image; script execution disabled by rendering via `<img>` |
| Markdown / text | In-app, escaped | rendered as escaped plain text (`<pre>`); no HTML injection. A safe Markdown renderer is deferred. |
| PDF | System handler | `creation_open` (existing) |
| DOCX / PPTX / XLSX / ODT | System handler | `creation_open`; icon/card + "Abrir". No renderer. |
| Generated web app | Isolated preview webview (§11) or system browser | `preview_open_web` |
| Other / unknown | System handler | `creation_open` |

`preview_data(project_id, resource_id, resource_kind) -> PreviewData` returns
`{ contentType, dataBase64 }` with a **preview size cap** (2 MB for images/text;
larger resources fall back to "Abrir"). It resolves the ID against the project
(authorization), reads bytes through the content store, and never returns a
path. Markdown is not HTML-rendered server-side.

## 11. Web preview security boundary (ADR-0010)

Generated HTML/JS is untrusted. The trust boundary has three layers:

### Layer 1 — Isolated, zero-capability webview

The preview is shown in a **separate `WebviewWindow` with label `preview`** whose
dedicated capability file grants **zero permissions** (no app commands, no
`core:event`, no dialogs, no fs/shell). A Tauri command is only invocable from a
window whose capability set includes it; the empty preview capability means
generated JavaScript cannot call any `invoke`/Tauri IPC. The window is created
backend-side (Rust) so the URL and capabilities are never chosen by the
frontend.

### Layer 2 — Loopback-only, token-guarded preview server

A new `project-preview` crate hosts a minimal axum server that:

- binds `127.0.0.1` only (loopback; never `0.0.0.0`),
- serves a single **immutable copy** of the target creation's `outputs/<id>`
  directory under a **128-bit random, single-use token** (`/preview/<token>/…`),
- is **read-only** (GET/HEAD), has **no directory listing** (root and token
  paths return 404/403), enforces **path containment** (canonicalize +
  `starts_with`, reject symlinks — the M2 publisher's proven pattern), and
  derives `Content-Type` from extension with safe defaults,
- is **torn down** when the preview window closes (or on `preview_close`), and
  its token is invalidated then.

The copy is produced by `project-app` from the already-validated
`creation_path` (existing canonical containment), so the server never reads live
mutable outputs and **never** touches `inputs/`, `workspace/`, `publish/`, or any
other project.

### Layer 3 — Optional CSP hardening

The preview window applies a restrictive CSP (e.g. `default-src 'self'; 
script-src 'self'; connect-src 'none'; object-src 'none'; base-uri 'none'`) to
prevent generated JS from phoning out or loading remote code. This is
defense-in-depth on top of Layer 1.

### Invariants (named tests)

1. Preview webview capability is empty (no command invocable).
2. Preview server binds loopback only; a non-loopback bind is rejected.
3. Unknown/invalid/expired token returns 404.
4. Traversal (`..`), absolute paths, and symlink escapes return 404.
5. Directory listing is disabled.
6. The served tree is the copied creation only; `inputs/`/`workspace/`/`publish/`
   are unreachable through the server.
7. Requests are read-only; non-GET/HEAD methods are refused.
8. Closing the preview tears the server down and invalidates the token.

### Fallback

If, during implementation, the isolated webview + preview server cannot be made
to satisfy invariants 1-8 within M8's scope, the **acceptable fallback is to
keep `preview_open_web` as a system-browser open (option A) and defer embedded
web preview**. The command surface (`preview_open_web`) is kept regardless, so
the fallback is an implementation choice behind a stable interface, not an API
change. This fallback is decided at review of task 4 (§22), not silently during
implementation.

## 12. Tauri capability changes

- **Main window** (`default.json`): unchanged — no new clipboard, fs, shell, or
  process permissions. Clipboard image paste needs none (DOM `paste` event).
- **New `preview.json` capability**: `windows: ["preview"]`, `permissions: []`
  (empty). This is the empty trust boundary for the preview webview. It is
  required before any `preview_open_web` work lands.
- No `tauri-plugin-clipboard-manager`, no `dialog:allow-save` (save/download-as
  is deferred), no generic `opener` passthrough from the frontend. The existing
  `dialog:allow-open` continues to serve the file picker.

## 13. Frontend state / event implications

- **Materials panel**: renders cards/chips with type + size + created time +
  remove + open. Drag/drop routes to `materials_add_from_paths` and renders the
  per-file report. Paste is handled at the prompt composer.
- **Prompt composer**: maintains a transient set of attached material IDs
  (chips) that clear on send; Ctrl+V images become both a new material and an
  auto-attached chip.
- **Preview**: a modal/panel for image/text previews (close/back via Escape and
  a visible control); web preview is a separate window (no in-modal state).
- **Event model**: unchanged — commands are the source of truth; `agent://task`
  events carry the same payload (attachment IDs do not leak into events). No new
  Tauri events are required; preview teardown is command-driven.
- **Backend = source of truth**: the frontend re-reads `project_open` after
  import/remove/preview; no client-side filesystem state.

## 14. Publication interaction

**generation != publication** is preserved. Adding, previewing, removing, or
attaching a material never changes visibility or publishes anything. The
publication snapshot continues to copy only `Public` creations at publish time;
`remove_material` cannot affect a snapshot (materials are never in `publish/`),
and `preview_open_web` serves a private, loopback copy that is not a publish
root. No M8 command touches `PublicationManager` except the existing
publish/unpublish surface.

## 15. Failure / error UX

New `ErrorCode` variants and messages (Spanish, no raw ids/paths):

| Code | Message |
| --- | --- |
| `MaterialUnsupported` | "No pudimos agregar ese archivo." |
| `MaterialTooLarge` | "Ese archivo es demasiado grande." |
| `MaterialImageInvalid` | "Esa imagen no es válida." |
| `MaterialDuplicate` (reported via import status, not an error) | "Ese archivo ya está en el proyecto." |
| `PreviewUnavailable` | "No pudimos mostrar la vista previa." |
| `PreviewTooLarge` | "Este recurso es grande; abrilo con la aplicación." |
| `AttachmentInvalid` | "No pudimos adjuntar ese material." |

Partial import failures are reported through `MaterialsImportReport`, not thrown
as a single error, so one bad file does not present as a total failure.

## 16. Security threat model

Named regression tests for each:

1. **Clipboard image forgery** — declared type vs magic bytes mismatch rejected;
   oversized rejected; empty bytes rejected.
2. **Malformed clipboard content** — text-only paste never triggers image import;
   non-image clipboard data is ignored or errors cleanly.
3. **Multiple attachments / partial failure** — one bad file never aborts the
   batch; per-file status is correct and deterministic.
4. **Duplicate paste/import** — identical content is not stored twice.
5. **Attachment ID validation** — a material ID from another project is
   rejected; a malformed/unknown ID is rejected.
6. **Cross-project material reference** — project B cannot attach project A's
   material.
7. **Preview authorization** — `preview_data`/`preview_open_web` resolve IDs
   within the project only; foreign/unknown IDs → 404/error.
8. **Generated web isolation** — §11 invariants 1-8.
9. **XSS-like names** — file/creation names containing `<script>`, `..`, `/`,
   `\`, control bytes are sanitized by `safe_file_name` and rendered escaped
   (React default); asserted in tests.
10. **Arbitrary path rejection** — batch import and material/creation open reject
    traversal, absolute, and symlink sources; no frontend path is trusted.
11. **Resource open boundaries** — `open_material`/`open_creation` resolve only
    `inputs/<id>`/`outputs/<id>` via canonical containment; preview server never
    serves `inputs`/`workspace`/`publish`.

No SECURITY.md invariant is relaxed. Invariant #12 ("treat externally supplied
HTML/JS as untrusted") is now implemented by ADR-0010 rather than deferred.

## 17. Accessibility approach

- All attachment controls are real `<button>`/`<input>` elements with labels;
  remove and open have accessible names including the material name.
- Preview modal: focus moves into the modal on open, focus is trapped, and is
  returned to the trigger on close; Escape and a visible "Cerrar" control both
  work; `aria-modal` and a descriptive label are set.
- Import/paste results and errors are announced via `role="alert"` / `aria-live`
  regions.
- Images in preview carry an `alt` derived from the display name.
- The web preview is a separate window; the main window announces "Abriendo
  vista previa…" and the preview window is closable by keyboard (standard window
  close).
- No new color/contrast debt; existing accessible baseline is preserved.

## 18. Deterministic test strategy

All offline, no real AI/Cloudflare/browser:

- `project-core`: `remove_material` lifecycle (removes metadata + content,
  conflict semantics, missing-material error, original untouched).
- `project-fs`: `remove_material` path containment + symlink rejection; import
  containment unchanged.
- `project-app`: new named suites `materials` (image ingestion incl. magic-byte
  forgery, oversize, malformed; batch import incl. partial failure, duplicate,
  symlink/traversal rejection; remove), `attachments` (attachment ID
  authorization, cross-project rejection, prompt augmentation through
  `FakeAgentEngine`), `preview` (`preview_data` authorization + caps).
- `project-agent`: `agent_attachment` — AgentService provisions materials into
  workspace **before** session baseline; artifacts under `materials/` are never
  registered; prompt augmentation is deterministic; `AgentEngine` trait is
  unchanged (existing tests stay green).
- `project-preview`: `preview_security` (invariants 1-8 in §11) and
  `preview_lifecycle` (open/serve/teardown/token invalidation), using a fake
  creation tree in a temp dir.
- Frontend (Vitest): paste handler (image vs text), attachment chips, remove
  confirmation, import report rendering, preview modal a11y/focus/escape,
  error announcements.
- `scripts/verify` gains the M8 suites and the "M8 contract passed" gate.

## 19. Optional desktop smoke strategy

`scripts/smoke-preview` (manual, Fedora, never in verify): launch the dev app,
create a project, paste a clipboard image, drop a mixed batch, attach materials
and generate, preview an image and a generated web app, confirm the preview
window has no working IPC (a diagnostic that attempts `invoke` and expects
failure), then close. SKIPs cleanly when a desktop/webkit environment is
unavailable. Real clipboard/drag behavior is exercised here, not in verify.

## 20. Module / dependency changes

```
project-core   (extend) remove_material (+ ProjectContentStore::remove_material)
project-fs     (extend) FilesystemProjectContentStore::remove_material
project-agent  (extend) AgentRequest.attachments + AgentService provisioning
                         (AgentEngine port & AgentPrompt UNCHANGED)
project-app    (extend) import/preview/attachment facade + DTO fields + errors
project-preview(NEW)    loopback token-guarded preview server (axum, tokio, serde)
app/src-tauri  (extend) M8 commands + preview.json (empty capability) + preview window
app/src        (extend) paste, chips, cards, preview UI, a11y
```

Dependency direction is unchanged: UI → `project-app` → ports/adapters.
`project-core` gains no provider/agent/OpenCode dependency. `project-preview`
depends on `project-core` (ids/types) and mirrors `project-publisher`'s
containment discipline; it does not import publication or tunnel.

## 21. ADR(s) proposed

- **ADR-0010** — Untrusted generated-content preview isolation (zero-capability
  webview + loopback token server + CSP; fallback to system browser).
- **ADR-0011** — Prompt attachment contract: backend resolves authorized
  material IDs; workspace provisioning without changing `AgentEngine`.

Both are Accepted; their implementation tasks may begin.

## 22. Task breakdown

| # | Task | Level | Depends | Worktree | Ownership |
| --- | --- | --- | --- | --- | --- |
| 0 | Design + ADR approval | HIGH_ARCHITECTURE | — | — | V4 Pro + Human |
| 1 | `remove_material` in project-core + project-fs (+ tests) | MEDIUM | 0 | `m8/material-lifecycle` | project-core, project-fs |
| 2 | `project-app` import/preview/attachment facade + DTO fields + errors (+ fakes) | MEDIUM_HIGH | 1 | `m8/app-import-preview` | crates/project-app/** |
| 3 | `AgentRequest.attachments` + `AgentService` workspace provisioning + prompt augmentation + artifact exclusion | HIGH_CODING | 0 | `m8/agent-attachment` | project-agent/** |
| 4 | `project-preview` crate (loopback token server + containment + teardown) + security suite | HIGH_CODING | 0 | `m8/preview-server` | project-preview/** |
| 5 | Tauri commands + capabilities (`preview.json` empty) + preview window wiring | MEDIUM | 2,3,4 | `m8/tauri-preview` | app/src-tauri/** |
| 6 | Frontend paste/chips/cards/preview UI + a11y + component tests | MEDIUM | 5 | `m8/preview-ui` | app/src materials/creations/chat |
| 7 | Named suites wiring + verify gate + smoke script + docs/VERIFY | MEDIUM/HIGH | 5,6 | `m8/m8-tests` | tests, scripts, docs/VERIFY |
| 8 | Gate/docs/ADR + verify + checkpoint | HIGH_ARCHITECTURE | 7 | main | docs, verify |

Tasks 1, 3, and 4 are independent once ADRs are accepted (task 0). Task 3 does
not depend on 1/2 (it only needs the attachment contract, not the import UI).

## 23. Reasoning level per task

1 MEDIUM · 2 MEDIUM_HIGH · 3 HIGH_CODING · 4 HIGH_CODING · 5 MEDIUM ·
6 MEDIUM · 7 MEDIUM/HIGH · 8 HIGH_ARCHITECTURE.

## 24. Proposed worktrees

`../ai-publisher-m8-material-lifecycle`, `-app-import-preview`,
`-agent-attachment`, `-preview-server`, `-tauri-preview`, `-preview-ui`,
`-m8-tests` (+ review per task). Integration checkout (`main`) is lead-only.

## 25. Implementation model allocation

Orchestrator: `opencode-go/deepseek-v4-flash` (fresh session after approval).

| Task | Author | Reviewer |
| --- | --- | --- |
| 1 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 2 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 3 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash |
| 4 | Cursor Grok 4.6 medium | OpenCode Go DeepSeek V4 Flash (security review) |
| 5 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 6 | Cursor Composer 2.5 (or DeepSeek Flash) | OpenCode Go DeepSeek V4 Flash |
| 7 | OpenCode Go DeepSeek V4 Flash | Cursor Grok 4.6 medium |
| 8 | DeepSeek V4 Pro (lead) | OpenCode Go DeepSeek V4 Flash |

Fallbacks per `AGENT_POLICY.md`; `MODEL_REQUESTED == MODEL_ACTUAL` enforced via
`scripts/agent-launch`.

## 26. Author / reviewer

Author ≠ reviewer, cross-family when practical. Task 4 (preview server,
security-invariant change) and task 3 (agent pipeline) get an independent
security review as a second pass per `AGENTS.md` ("treat a security-invariant
change as a security-review task"). Task 4's reviewer explicitly evaluates the
§11 fallback decision and reports approve/fallback/request-changes.

## 27. Risks / debt

- **WebKitGTK paste delivery**: DOM `paste` may not populate image items on all
  WebKitGTK versions; validated in manual smoke; fallback is a narrow
  clipboard-manager plugin behind an explicit button (§4). Highest UI risk.
- **Web preview isolation**: the highest security risk. Mitigated by the empty
  capability file (tested) + loopback token server + CSP, and by a documented
  fallback to system browser if invariants cannot be met (§11).
- **OpenCode diff semantics**: material provisioning depends on the session
  baseline excluding pre-existing files; mitigated by provisioning *before*
  `open_session` plus a defensive `materials/` artifact filter (§6, §18).
- **Preview server duplication**: re-implements containment rather than reusing
  `project-publisher`; accepted to avoid coupling publication and preview trust
  domains. Revisit if a shared static-serving core emerges.
- **Markdown rendering deferred**: text/Markdown is shown escaped (no inline
  HTML); rich Markdown rendering is a future, non-blocking polish.
- **Batch import memory**: import reads files into memory one at a time; fine
  for M8 caps, streaming is future work.
- **No revision/regenerate UI**: follow-up prompts create new creations by
  current semantics; not hidden, not mislabeled.

## 28. Definition of Done M8

- [x] ADR-0010/0011 and this design accepted before code.
- [ ] Clipboard image paste works end-to-end (manual smoke), backend-validated, no new clipboard privilege.
- [ ] Multi-file import reports deterministic per-file results with partial-failure + content dedup; originals never mutated.
- [ ] Material remove works and never touches source files; core `remove_material` is tested.
- [ ] Prompt attachments resolve by authorized material ID only; `AgentEngine` port unchanged; cross-project references rejected.
- [ ] In-app previews for images and text/Markdown (escaped); PDF/office via system handler.
- [ ] Generated web preview satisfies ADR-0010 invariants (empty preview capability + loopback token server) OR the documented system-browser fallback is chosen at review.
- [ ] `./scripts/verify` (M8 gate), `git diff --check`, independent security review (tasks 3 & 4), handoff.
- [ ] No M8-excluded scope (office editor, cloud sync, accounts/payments, packaging, Windows release, provider redesign, autonomous publishing, plugins).

## 29. scripts/verify incremental

M8 keeps M7 and adds (offline, deterministic; documented in `docs/VERIFY.md`):

```bash
# Rust (existing + new)
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked -p project-app --test materials
cargo test --locked -p project-app --test attachments
cargo test --locked -p project-app --test preview
cargo test --locked -p project-agent --test agent_attachment
cargo test --locked -p project-preview --test preview_security
cargo test --locked -p project-preview --test preview_lifecycle
# ... M1-M7 suites unchanged ...

# Frontend (existing suite covers M8 components)
pnpm --dir app run test

# Tauri
cargo check --manifest-path app/src-tauri/Cargo.toml
git diff --check
```

Final gate prints `verify: M8 contract passed` (gated on the new
`project-preview` security suite, ahead of the M7 provider gate).

## 30. Explicit next-milestone scope

M9 = Education-focused UX polish (re-numbered from `CODEX_HANDOFF.md`). Content
is designed later; M8 does **not** begin it. M8 defers: rich Markdown rendering,
save/download-as, PDF in-app rendering, office renderers, revision/regenerate
UI, clipboard-manager plugin, and any persistent preview infrastructure.
