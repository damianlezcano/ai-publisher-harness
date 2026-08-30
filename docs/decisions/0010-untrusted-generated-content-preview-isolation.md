# ADR-0010: Untrusted generated-content preview isolation

- Status: Accepted

## Context

M6 opens generated web creations in the system browser, which is safe (the
browser holds no Tauri IPC) but not an in-app preview. M8 wants an embedded
preview for generated HTML/JS. SECURITY.md #12 requires treating externally
supplied HTML/JS as untrusted in the desktop preview context. Generated content
is produced by an AI and may be malicious or malformed; it must never gain
privileged Tauri access (arbitrary commands, filesystem, dialogs, event bus) and
must never read other projects' files.

## Decision

Preview generated web content in a dedicated, isolated webview fed by a
loopback-only, token-guarded preview server, with a documented fallback to the
system browser:

1. **Zero-capability preview window.** The preview is a separate `WebviewWindow`
   (label `preview`) whose capability file grants **no** permissions. A Tauri
   command is invocable only from a window whose capability set includes it;
   with an empty capability set, generated JavaScript cannot invoke any
   `invoke`/IPC, dialog, fs, shell, or event command. The window and its URL are
   created backend-side; the frontend never chooses the URL or capabilities.

2. **Loopback token server.** A `project-preview` crate serves an immutable copy
   of a single creation's `outputs/<id>` directory at `/preview/<token>/…`:
   loopback-only bind, 128-bit random single-use token, read-only, no directory
   listing, canonical path containment with symlink rejection, and teardown on
   preview close. It never serves `inputs/`, `workspace/`, `publish/`, or other
   projects.

3. **Defense-in-depth CSP** on the preview window (`script-src 'self';
   connect-src 'none'; object-src 'none'; base-uri 'none'`).

4. **Fallback.** If invariants 1-8 (see M8_DESIGN §11) cannot be satisfied
   within M8, the preview stays a system-browser open behind the same
   `preview_open_web` command surface.

## Consequences

- Generated content runs with no privileged Tauri surface; SECURITY.md #12 is
  implemented rather than deferred.
- A new small crate (`project-preview`) is introduced, independent of the
  publication/tunnel trust domain. It re-implements safe static-file containment
  rather than coupling to `project-publisher` (conscious duplication).
- Preview is ephemeral and single-use; there is no persistent preview server.
- Eight named isolation invariants become regression tests.

## Alternatives considered

### System browser only (no embedded preview)

Safest and already implemented, but fails the M8 goal of an in-app preview
experience. Retained as the explicit fallback.

### `convertFileSrc` / Tauri asset protocol into a secondary webview

Avoids a server but requires dynamic filesystem scope for arbitrary generated
trees and complicates guaranteeing that sibling assets load while other project
trees remain unreachable. Rejected for the token-server approach, which has an
explicit, testable boundary.

### iframe with sandbox inside the main window

A sandboxed iframe inherits the main window's origin and capability context;
blob/loopback origins and WebKitGTK sandbox semantics are harder to bound than a
separate zero-capability window. Rejected.

### Serve previews through the local publisher

The publisher serves only registered `publish/` roots of published projects and
cannot serve private creations. Rejected (violates publication invariants).
