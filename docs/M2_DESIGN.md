# M2 Local Publisher Design

Status: Approved for implementation. ADR-0003 is accepted. This document
authorizes only the strict M2 scope recorded here.

## 1. Executive summary

M2 adds one in-process, loopback-only, read-only HTTP publisher. It may serve
multiple explicitly registered `publish/` directories at opaque route keys:
`http://127.0.0.1:<dynamic-port>/fotosintesis-a7k2/`. It has no UI, tunnel,
Internet reachability, QR, Cloudflare, OpenCode, AI/provider configuration,
sidecar, or publication-preparation behavior.

An HTTP request can resolve only to a regular, non-hidden file below the
registered project's `publish/` root. It never receives a project root and can
never obtain `inputs/`, `workspace/`, or `outputs/`.

## 2. M1 / M2 / M3 boundary

| Milestone | Owns | Explicitly does not own |
| --- | --- | --- |
| M1 | Portable project metadata and four fixed directories | HTTP, publish selection, route state |
| M2 | Generic local server, validated in-memory route registration, serving already-prepared `publish/` trees | Copying/selecting outputs, persistent publish state, UI lifecycle policy, tunnels |
| M3 | `PublicationManager`: durable publish/unpublish intent, route allocation/persistence policy, content preparation orchestration, lifecycle policy across projects | Cloudflare and Internet exposure |

M2 chooses option **B**: it serves content already present in `publish/`.
Option A (copying from `outputs/`) belongs with a later preparation policy;
doing it now would blur selection, stale-content replacement, and security
responsibility.

## 3. ADRs proposed

ADR-0003 proposes Tokio + Axum behind a framework-free `LocalPublisher` port,
opaque ASCII route keys, dynamic loopback binding, and the `PublishRoot`
capability boundary. No separate route ADR is needed: route identity and the
filesystem boundary are inseparable from this server decision.

## 4. Module architecture

```
project-core                 project-fs                 project-publisher
-------------                ----------                 -----------------
ProjectRepository  <---     ProjectPublishRootProvider  LocalPublisher adapter
PublicationManager (M3)      validates project/publish/  RouteRegistry
                                                        Axum/Tokio HTTP server
                                                        read-only PublishRoot handle
```

M2 introduces `crates/project-publisher/`. A narrow M2 integration seam may
live in `project-fs` to create a canonical `PublishRoot` only for an existing
project's fixed `publish/` directory. The publisher receives no `Project`, no
`ProjectContentStore`, no arbitrary `PathBuf`, no Tauri type, and no Cloudflare
or OpenCode dependency. `PublicationManager` remains a specified M3 contract,
not M2 code.

## 5. Contracts and interfaces

### `LocalPublisher` — M2 application port

**Responsibility:** run one loopback server and atomically add/remove runtime
registrations.

**Operations:** `start() -> PublisherEndpoint`, `register(PublishedProject)`,
`unregister(PublicationRoute)`, `local_url() -> Option<LoopbackUrl>`,
`stop()`, and `is_running()`.

**Knows:** route keys, a `PublishRoot` capability, registration lifetime, and
user-safe serving errors. **Does not know:** project names/metadata, how
content was prepared, Tauri, Cloudflare, OpenCode, inputs/workspace/outputs,
or arbitrary filesystem paths.

**Errors:** `AlreadyRunning`, `NotRunning`, `RouteConflict`, `InvalidRoute`,
`InvalidPublishRoot`, `BindFailed`, `RegistrationFailed`, `ShutdownFailed`.
**Invariants:** binds only 127.0.0.1, accepts only registered roots, and does
not mutate served content.

### `RouteRegistry` — M2 internal component

**Responsibility:** validate and map one route segment to one `PublishedProject`
for a running publisher.

**Operations:** `reserve`, `lookup`, `release`, `contains`; all keyed by
`PublicationRoute`.

**Knows:** normalized opaque route grammar and uniqueness. **Does not know:**
HTTP encoding/parser behavior, projects, filesystem, content type, or future
public URLs. **Errors:** `InvalidRoute`, `RouteConflict`, `NotRegistered`.
**Invariants:** no duplicate route; no empty/dot/slash/percent/control route;
case-insensitive ambiguity is rejected using canonical lowercase ASCII.

### `PublishedProject` and `PublishRoot`

`PublishedProject` is `{ route, publish_root }`. `PublishRoot` is a validated,
canonical directory capability made by infrastructure; it is not constructible
from an untrusted caller string. It proves only the directory is the fixed
`publish/` root of one existing project at registration time. It does not
authorize traversal outside it; every request rechecks the resolved candidate
to defend against later symlink changes.

### `PublicationManager` — specified now, implemented in M3

**Responsibility:** decide which project is published, prepare or validate its
public tree, allocate/persist its route, and coordinate the single publisher.
**Operations (M3 proposal):** `publish(project_id)`, `unpublish(project_id)`,
`list_published()`, `endpoint()`. **Does not know:** Cloudflare, OpenCode, raw
HTTP handling, or generic project filesystem access. It delegates serving to
`LocalPublisher` and preparation to a later explicit component. It never
exposes an input/workspace/output root.

## 6. Publication model

M2 state is process-local only:

```
PublishedProject { route: PublicationRoute, publish_root: PublishRoot }
```

The route registration is explicit API input for M2 tests/infrastructure. M2
does not write `project.json`, infer a creation from `outputs/`, or expose
project metadata. M3 will decide stable route generation and persistence; it
can reuse M2's route grammar without changing the HTTP boundary.

## 7. Filesystem boundary

At registration, infrastructure resolves the expected fixed path
`projects/<project-id>/publish`, checks that it is a directory and canonical
within the project, and creates `PublishRoot`. The publisher serves only
metadata-derived relative URL segments. For each request it rejects empty,
absolute, platform-prefixed, dot, hidden, and invalid segments before joining;
it canonicalizes the candidate and verifies it remains below canonical publish;
it rejects symlink components and non-regular files.

No listing or fallback may traverse to `inputs/`, `workspace/`, `outputs/`, a
different project's root, or the configured projects base directory. The
publisher uses no write/delete/execute filesystem operation.

## 8. Routing

`PublicationRoute` grammar is `[a-z0-9]+(?:-[a-z0-9]+)*`, length 1–80. It is
an opaque lowercase ASCII single segment. Future UX may derive
`fotosintesis-a7k2` from a display slug plus random suffix, but M2 does not
derive or persist it.

Routes are unique in the running registry. Decode a request path once using a
strict URL parser; reject malformed escapes and any decoded `/`, `\\`, NUL,
dot segment, or non-ASCII route segment; never decode twice. Route comparison
is byte-stable lowercase ASCII, avoiding Unicode normalization and
case-insensitive platform ambiguity. A request without trailing slash redirects
to `/<route>/` only for the exact route root; `/<route>/` maps to
`publish/index.html`. Other paths preserve normal relative asset resolution.

## 9. Lifecycle

M2 provides low-level `start`, `register`, `unregister`, and `stop`, making it
possible to test that removing A leaves B served. It does not decide when an
application starts/stops the server, whether a project is published, or persist
registrations. M3 will implement first-start/reuse, per-project unpublish, and
optional stop after last project as an application policy.

## 10. Port and binding strategy

`start` binds exactly `127.0.0.1:0`; the OS selects an available ephemeral
port. `PublisherEndpoint` exposes a parsed loopback `local_url`, never a
wildcard or LAN address. Dynamic binding prevents collisions and user
configuration. IPv4 loopback is explicit in M2; IPv6 `::1` is a later
compatibility decision, not a silent broadening.

## 11. HTTP behavior

| Request/outcome | M2 behavior |
| --- | --- |
| `GET` regular file | `200`, exact bytes, extension-mapped `Content-Type`, `X-Content-Type-Options: nosniff`, `Cache-Control: no-store` |
| `HEAD` regular file | Same status and representation headers as GET, no body |
| Other methods | `405` with `Allow: GET, HEAD`, no filesystem action |
| `/<route>` | `308` to `/<route>/` (only exact registered root) |
| `/<route>/` | Serve `index.html` only if a regular safe file exists; otherwise `404` |
| subdirectory | `404`; no directory index and no implicit index below a directory |
| missing/invalid/hidden/symlink file | `404`, without boundary details |
| root `/` or unknown route | `404`, never enumerate routes |
| conditional/range | Not implemented; valid GET/HEAD receives ordinary full representation |
| ETag | Not implemented; avoids cache semantics before a need exists |

MIME types come from a controlled extension mapping; M2 performs no content
sniffing. Unknown files use `application/octet-stream` and force attachment.
DOCX, XLSX, PPTX, and unknown binaries use
`Content-Disposition: attachment; filename*=UTF-8''...`. PDF and known web
images (PNG, JPEG, WEBP, GIF, and comparable safe image formats) are inline;
HTML/CSS/JS are inline so a prepared web site renders. Never reflect
unsanitized request text in headers or HTML.

## 12. Web content

A prepared tree containing `index.html`, `style.css`, `app.js`, and `assets/`
works through `/<route>/`: root index is served and normal relative files
resolve beneath the registered root. No HTML rewriting, directory listing,
service-worker injection, CSP transformation, or preview UI is added.

## 13. Documents and downloads

M2 remains a generic static server. If `publish/` contains only `guia.docx`,
`actividad.xlsx`, `presentacion.pptx`, or `material.pdf`, they are available at
direct routes and download/open using the host's normal app handling. PDF is
`application/pdf` and may be inline; DOCX, XLSX, and PPTX get registered MIME
plus attachment disposition. Images use image MIME and attachment disposition.
Unknown extensions become attachment octet streams.

M2 does not synthesize a document landing page. A future preparer may put a
static `index.html` and links in `publish/`; M2 then serves it normally. This
keeps landing generation out of LocalPublisher and out of the M3 selection
decision.

## 14. Security model

Security invariants 1–7 and 9–11 are directly exercised:

```
HTTP client -> strict request/route parsing -> RouteRegistry -> PublishRoot
            -> segment validation -> canonical containment + symlink check
            -> read-only regular file response
```

The server has no project-root capability, does not enumerate routes, writes
nothing, binds only loopback, exposes no management endpoint, and maps failed
authorization/path resolution to non-descriptive `404`. It rejects hidden
segments (including `.git` and `.DS_Store`), symlink files/directories and
links created after registration, absolute/Windows/UNC forms, NUL bytes,
malformed percent escapes, double-encoded traversal, and invalid Unicode
normalization. It uses `nosniff` for every response and a safe disposition
policy.

## 15. Tests

All tests use temporary M1-shaped projects and real loopback HTTP requests; no
Internet, tunnel, Tauri, UI, or external process is permitted.

| Level | Mandatory cases |
| --- | --- |
| Unit | Route grammar, collision/release, single-decode rules, MIME/disposition policy, HTTP method matrix, endpoint loopback validation |
| Integration | Port 0 start; A index/HEAD/assets/MIME/DOCX/PDF/404/405; B register; A/B isolation; remove A leaving B; concurrent requests; Unicode and conflicting filenames; start-stop/restart behavior |
| Security | Literal/encoded/double traversal; absolute/Windows/UNC/NUL; malformed URL/request; inputs/workspace/outputs and cross-project denial; symlink before/after registration; hidden files; directory index; Unicode normalization; canonicalization mismatch; MIME sniffing; unsafe headers; loopback-only bind |

Fixtures create `publish/` directly, never by copying `outputs/`. Tests assert
no response or mutation outside expected files. Platform-specific symlink/UNC
cases may have explicit capability-aware skips only where the OS forbids setup;
other path-defense tests remain mandatory.

## 16. Task breakdown

| Task | Dependency | Ownership | Author / reviewer | DoD and commands |
| --- | --- | --- | --- | --- |
| 0. Approve M2 ADR/design | Human | Docs only | Codex Tierra / human | ADR-0003 accepted; no product code; `./scripts/verify`, `git diff --check` |
| 1. Publisher contracts and crate scaffold | 0 | manifests; `crates/project-publisher/src/{lib,port,model,error}.rs` | Cursor/Grok / Antigravity | Framework-free API and route tests; no FS/Tauri/Cloudflare/OpenCode; fmt, clippy, `cargo test -p project-publisher` |
| 2. Publish-root provider boundary | 1 | `crates/project-fs/src/publish_root.rs`, focused tests | Antigravity Flash / Cursor/Grok | Only fixed `publish/` produces capability; rejects malformed roots; M1 unchanged; relevant cargo checks |
| 3. HTTP serving and registry | 1, 2 | `crates/project-publisher/src/{axum_adapter,registry,serve}.rs`, HTTP tests | Cursor/Grok / Antigravity | Dynamic loopback, GET/HEAD/405/404/index/assets, A/B registry, MIME/disposition; no project path input; publisher checks |
| 4. Adversarial HTTP/filesystem tests | 2, 3 | `crates/project-publisher/tests/publisher_security.rs`, fixtures only | Antigravity Flash / Cursor/Grok | Threat cases or explicit OS rationale; no production redesign; security + publisher tests |
| 5. Verify/integration | 1–4 reviewed | `scripts/verify`, `docs/VERIFY.md`, integration wiring | Codex Tierra / Antigravity | Exact M2 gate wired; no frontend tooling; `./scripts/verify`, `git diff --check` |

Tasks 1 and 2 are sequential because task 1 defines the capability type. Tasks
3 and 4 are sequential to avoid shared publisher files. Task-1 review may
overlap task-2 planning, but no two authors share a checkout.

## 17. Reasoning classification

| Task | Level | Why |
| --- | --- | --- |
| 0 | HIGH | Architectural/security approval |
| 1 | MEDIUM | Bounded public Rust API and route validation |
| 2 | MEDIUM | Filesystem capability boundary |
| 3 | MEDIUM | Well-specified HTTP adapter and tests |
| 4 | MEDIUM | Adversarial tests against approved contract |
| 5 | HIGH | Cross-module integration and final security gate |

## 18. Worktrees

The integration checkout is Codex Tierra-only. After approval use author
worktrees `../ai-publisher-m2-contracts`, `../ai-publisher-m2-publish-root`,
`../ai-publisher-m2-http`, and `../ai-publisher-m2-security`. Reviewers get
separate read-only diff/review worktrees. Task 5 integrates only reviewed
commits on the reserved checkout.

## 19. Herdr delegation

For each task, the lead classifies reasoning, creates the worktree before its
pane, and sends the owner only files, contract, invariants, DoD, and commands.
The prompt includes the no-redesign rule in `docs/AGENT_POLICY.md`; the worker
returns changed files, result, risks, and SHA. The lead runs task checks, gives
the commit/diff to the independent reviewer, returns material findings only to
the author worktree, then integrates and runs the gate. Retry once if
recoverable; on a second failure switch agent type before Codex implementation.

## 20. Author/reviewer matrix

| Task | Author | Reviewer | Integrator |
| --- | --- | --- | --- |
| 1 | Cursor/Grok | Antigravity Flash | Codex Tierra |
| 2 | Antigravity Flash | Cursor/Grok | Codex Tierra |
| 3 | Cursor/Grok | Antigravity Flash | Codex Tierra |
| 4 | Antigravity Flash | Cursor/Grok | Codex Tierra |
| 5 | Codex Tierra | Antigravity Flash | Codex Tierra after review |

## 21. Risks and decisions still open

- Axum/Tokio version pinning and supported MSRV are implementation-time
  dependency choices; select versions compatible with pinned Rust.
- Canonicalization cannot fully eliminate races with a hostile local process.
  M2 rejects symlinks and revalidates per request; native-handle hardening to
  eliminate all TOCTOU risk is a future cross-platform hardening decision.
- IPv6 loopback support is deferred. M2's invariant is IPv4 `127.0.0.1`.
- Landing pages and output-to-publish preparation remain M3 decisions.
- Supplied HTML/JS is local loopback content; desktop preview isolation remains
  M8 work under security invariant 12.

## 22. M2 Definition of Done

- [ ] ADR-0003 is accepted before implementation.
- [ ] Exactly one LocalPublisher binds only `127.0.0.1` on a dynamic port.
- [ ] Registered routes serve only prepared `publish/` roots; A and B isolate.
- [ ] GET/HEAD, index, assets, documents, MIME/disposition, 404, and 405 pass.
- [ ] Traversal, symlink, encoding, hidden-file, cross-project, and header
      tests pass or have documented OS-capability rationale.
- [ ] No HTTP management endpoint or write operation exists.
- [ ] M1 tests remain passing; no Tauri/UI/tunnel/OpenCode/Cloudflare dependency.
- [ ] Each implementation task has distinct author/reviewer evidence and full
      incremental verification passes.

## 23. `scripts/verify` changes

M2 extends—not replaces—M1's gate. Once M2 code exists it runs:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test -p project-publisher --test publisher_http
cargo test -p project-publisher --test publisher_security
git diff --check
```

If workspace tests already execute named suites, avoid redundant execution only
if their explicit coverage is still printed. Keep harness/M1 filesystem checks.
Do not add Node/npm/frontend tooling before a real frontend workspace exists.

## 24. Explicitly deferred to M3

M3 decides and persists publication state; allocates stable friendly routes;
prepares/copies selected creations from `outputs/` into `publish/`; chooses
document landing UX; and applies first-start/reuse/per-project-unpublish/
last-stop policy. Cloudflare/tunnels remain M4, OpenCode M5, and desktop
preview M8.
