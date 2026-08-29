# M3 Publication Manager Design

Status: Approved for implementation. ADR-0004 is Accepted.

## 1. Resumen ejecutivo

M3 implements the local semantics behind **Publicar** / **Dejar de compartir**.
`PublicationManager` selects public creations, makes a validated immutable
snapshot under `publish/`, assigns a durable friendly route, and manages exactly
one M2 `LocalPublisher`. It knows nothing of Cloudflare, tunnels, cloudflared,
OpenCode, AI/models/providers, Tauri/UI, or QR.

Publication is an explicit snapshot: changes in `outputs/` never alter what is
served until the next successful Publish. Preflight passed on 2026-08-29:
`./scripts/verify` and `git diff --check`.

## 2. Boundary M2/M3/M4

| Milestone | Owns | Excludes |
| --- | --- | --- |
| M2 | Safe loopback HTTP and generic route-to-`PublishRoot` serving | Selection, snapshots, durable routes, publication policy |
| M3 | Visibility, snapshot preparation, durable route, one-publisher runtime lifecycle | Tunnel, public URL, QR, UI, OpenCode |
| M4 | One Quick Tunnel, session base URL and QR built from M3 route | Snapshot contents and LocalPublisher policy |

M4 joins an ephemeral base URL to M3's route; it must not assume the result is
permanent.

## 3. ADRs propuestos

ADR-0004 proposes schema-v2 visibility/route metadata, runtime-only active
state, journaled snapshot replacement, and atomic M2 route-root replacement.
No additional ADR is needed for scoped layouts or task sequencing.

## 4. Arquitectura

```
Future UI -> PublicationManager (project-core application service)
             -> ProjectRepository (metadata/migration)
             -> PublicationSnapshotStore (project-fs)
             -> ProjectPublishRootProvider (project-fs)
             -> LocalPublisher (project-publisher)
```

`PublicationSnapshotStore` is a semantic port: it gets a validated plan and
only produces the fixed `publish/` tree. It does not accept arbitrary paths.
The filesystem adapter derives `outputs/<creation-id>` and controlled staging.
The publisher still sees only `PublishedProject { route, publish_root }`.

M3 adds `replace(PublishedProject) -> Result<()>` to `LocalPublisher`. It
atomically changes a registered route root under the registry lock; it is not
an observable unregister/register pair.

## 5. PublicationManager contract

```text
publish(project_id) -> Publication
unpublish(project_id) -> Removed | AlreadyLocal
list_published() -> Vec<Publication>
endpoint() -> Option<LoopbackUrl>
recover() -> Result<()>
```

`Publication` has project ID, route, and current loopback endpoint only. No
absolute paths, tunnel URL, PID, or handle escapes. `publish` is idempotent in
intent but refreshes an existing publication. `unpublish` is a no-op when
already local. Typed internal errors cover metadata/migration, preparation,
publisher start/register/replace/unregister/stop, and recovery without leaking
filesystem details.

## 6. Creation visibility model

Every creation has a closed lowercase enum, `visibility: public | private`.
New creation APIs require it explicitly; the only safe default is `private`.
M3 only applies and enforces persisted metadata. It must never infer visibility
from filename, Creation name, file type, content, keywords, or heuristics. A
Creation is public only if a higher layer or an explicit operation set it so.
Future OpenCode/agent logic may decide that metadata from user intent, but that
semantic decision remains outside M3; M3 has no OpenCode dependency.

Schema-v1 projects are accepted only to atomically migrate on first M3
mutation: each legacy creation becomes `private`, `schemaVersion` becomes 2,
and `publicationRoute` remains absent until first route allocation. A failed
migration leaves metadata and publication untouched.

## 7. Snapshot/preparation strategy

For a per-project lock M3 reads validated metadata and builds a deterministic
plan of public creations only. It creates a sibling
`.publish-staging-<operation-id>`, never writes `publish/` during preparation,
and copies only regular files under each selected `outputs/<creation-id>`.

It rejects every symlink component/leaf, hidden/reserved/traversal-like name,
non-regular file, and source outside that creation root. It revalidates before
and after copy, flushes files/directories, generates required HTML, then
validates the complete staging tree. This detects symlink races as far as the
portable pathname model permits.

M3 writes a flushed `.publish-swap-<operation-id>.json` journal, renames
`publish/` to `.publish-previous-<operation-id>`, renames staging to `publish/`,
flushes the project directory, validates a new `PublishRoot`, and then atomically
replaces/registers the M2 route. One prior tree is retained through the next
successful update or unpublish so in-flight requests using the old root do not
observe deletion.

Recovery is deterministic before registrations: remove uninstalled staging;
restore previous if the old tree moved but the new tree did not install; or
validate installed `publish/` and clean only exact journal-owned siblings. An
unprovable state fails closed and is never registered. If swap/replace fails,
the old registered root and retained previous tree stay valid; rollback restores
only when it can safely prove the target.

## 8. Web project strategy

A public `web` creation's declared entry lives within
`outputs/<creation-id>/` and must be a safe regular `index.html`. M3 copies its
whole creation directory, preserving `app.js`, `style.css`, and `assets/` with
normal relative URLs. Generated HTML is not transformed.

MVP permits at most one public web creation. More than one is a preparation
error: selecting/composing independent apps silently is worse than deferring
that product rule.

## 9. Document landing strategy

With no public web creation, M3 creates `publish/index.html` headed “Material
del proyecto”. Each public document/image/file is at an ASCII,
collision-proof path such as `files/<creation-id>/download.pdf`. The landing
uses escaped `displayName` and offers Descargar; PDF offers Abrir / Descargar.

HTML text and attributes escape `& < > " '`. URLs contain only controlled ASCII
ID/validated extension segments and are percent-encoded by segment when needed.
Unicode filenames are display metadata, never an authorization path. Listing
order is stable: normalized display name then creation ID. `index.html`,
`materials.html`, and `files/` are reserved generated paths.

## 10. Mixed project strategy

With one web creation plus documents, the web app remains `publish/index.html`.
Documents remain under `files/<creation-id>/...`; M3 adds generated
`publish/materials.html` listing them. This exposes supplemental materials
without rewriting untrusted/generated web HTML. A future UI/creation convention
can link to that page.

## 11. Route strategy

Choose persistent metadata, not a session route. On first publish derive an
ASCII slug by deterministic Unicode transliteration/decomposition, lowercase,
non-alphanumeric-to-hyphen collapse, trimming and length bound; use `project`
if empty. Append cryptographically random short lowercase base32, e.g.
`fotosintesis-a7k2m9`.

Compare with all persisted routes and retry collision, enforcing M2's lowercase
ASCII 1–80 route grammar. The route never changes after allocation: duplicate
names differ by suffix, Unicode gets the best readable transliteration, rename
does not break an active/known route, and republish retains it.

## 12. Persistent versus runtime state

| Persistent | Runtime only |
| --- | --- |
| Creation visibility; optional publication route; last successful `publish/` bytes | Local/Preparing/Published/Failed; registrations; endpoint/port; handles/PID; future tunnel URL/QR |
| Journal, staging, previous snapshot only for recovery | No active state survives restart |

After restart all projects are Local and unregistered. M3 never auto-publishes.
`Failed` describes an operation; an update failure does not displace an existing
Published state.

## 13. Lifecycle, republish, and failures

| Event | Behavior |
| --- | --- |
| Publish A from zero | Prepare; start one publisher; register A |
| Publish B | Prepare; reuse publisher; register B |
| Unpublish A while B | Unregister A; B remains live |
| Unpublish last B | Unregister then stop publisher |
| Repeated Publish A | Explicit snapshot update; same route; atomic root replace |
| Repeated Unpublish A | `AlreadyLocal`, no adapter call |
| Preparation/start/register failure | No new registration; prior A snapshot remains live |
| Unregister failure | Retain Published runtime state; do not stop |
| Last-stop failure | Route stays removed; report recoverable error and retry on recovery/next lifecycle call |

No file watcher exists. Publish over a published project is the future UI's
implicit “Update Publication”.

## 14. Concurrency model

Use a global lifecycle mutex plus a per-project mutex, with ascending project-ID
order for any future multi-project operation. A/B preparation may run in
parallel; start/register/stop transitions serialize. Publish A twice serializes.
Publish A plus Unpublish A serializes in arrival order. M2's atomic replacement
maps each HTTP request to one immutable old/new root, while retention protects
in-flight old-root reads; no request sees a mixed snapshot.

## 15. Persistence/schema changes

```json
{
  "schemaVersion": 2,
  "publicationRoute": "fotosintesis-a7k2m9",
  "creations": [{ "visibility": "public" }]
}
```

No persistent project publication state is added. Migration and route allocation
use M1's atomic metadata-replace protocol. Unknown/malformed schemas remain
fail-closed.

## 16. Security model

All M1/M2 invariants remain. M3 tests and enforces: private creation never
copied; inputs/workspace never a source; only selected creation roots under
outputs are read; symlink/race/hidden/reserved/traversal-like source rejection;
escaped landing HTML; collision-free destinations; previous snapshot preservation;
route uniqueness; A/B isolation; and M2's post-registration serving defenses.
The documented hostile-local-process TOCTOU limitation is not broadened.

## 17. Tests (tests first)

| Level | Required coverage |
| --- | --- |
| Unit | Visibility/default; v1→v2 migration plan; slug/collision/Unicode/rename; landing escape/order/URLs; web-plan validation; transitions |
| Integration | 1–20: first publish/start/snapshot/public-only/private/inputs/workspace; web, document landing, mixed; A/B lifecycle; repeated publish/unpublish; duplicate names, Unicode, rename, update |
| Lifecycle/recovery | 21, 25: injected prepare/swap/register failures preserve old snapshot; start/register/unregister/stop failures; journal recovery; restart local/no auto-register |
| Security | 5–7, 22: symlink and race, traversal-like/hidden/reserved names, collisions, A/B isolation, landing escaping |
| Concurrency | 23–24: A+B, A+A, publish/unpublish race, HTTP during replace; deterministic state/no partial tree |
| Migration/regression | 26–28: migration atomicity; every M1 and M2 suite stays green |

Tests use temporary trees, deterministic fakes and a local M2 publisher only;
never a tunnel, UI, provider, AI, or public network.

## 18. Task breakdown, levels, agents, review

| Task | Ownership | Level | Preferred author | Reviewer |
| --- | --- | --- | --- | --- |
| 0 Design/ADR approval | docs | HIGH_ARCHITECTURE | Codex Terra | Human owner |
| 1 Schema/visibility/migration | `project-core`, `project-fs` metadata | MEDIUM_HIGH | Cursor Grok 4.6 | OpenCode Go DeepSeek V4 Flash |
| 2 Snapshot/recovery adapter | `project-fs` publication modules | HIGH_CODING | Cursor Grok 4.6 | OpenCode Go Kimi K2.7 Code |
| 3 Manager/routes/lifecycle | `project-core` publication modules | HIGH_CODING | Cursor Grok 4.6 | OpenCode Go DeepSeek V4 Flash |
| 4 Atomic M2 replace seam | `project-publisher` port/registry | MEDIUM_HIGH | Cursor Grok 4.6 | OpenCode Go DeepSeek V4 Flash |
| 5 Lifecycle/security integration | dedicated M3 tests | MEDIUM_HIGH | Cursor Grok 4.6 | OpenCode Go DeepSeek V4 Flash |
| 6 Gate/docs/integration | verify/docs/handoff | HIGH_ARCHITECTURE | Codex Terra | independent DeepSeek review |

Fallbacks follow `docs/AGENT_POLICY.md`: LOW Composer→MiMo; MEDIUM
DeepSeek→Composer→Qwen; MEDIUM_HIGH Grok→DeepSeek→Kimi; HIGH_CODING Grok→Kimi;
HIGH_ARCHITECTURE Codex Terra. OpenCode Go never uses GPT/Grok.

## 19. Proposed worktrees and ownership

Do not create before approval. The integration checkout stays Codex Terra-only.

| Worktree | Branch | Scope |
| --- | --- | --- |
| `../ai-publisher-m3-schema` | `m3/schema` | Task 1 |
| `../ai-publisher-m3-snapshot` | `m3/snapshot` | Task 2 |
| `../ai-publisher-m3-manager` | `m3/manager` | Task 3 |
| `../ai-publisher-m3-publisher-replace` | `m3/publisher-replace` | Task 4 |
| `../ai-publisher-m3-security` | `m3/security` | Task 5 tests |

Reviewers use separate review worktrees and inspect committed diffs only. Tasks
1/4 establish reviewed contracts; 2/3 follow; 5 follows integrated behavior;
6 alone integrates.

## 20. Risks/debt

- Portable directory replacement requires journaled two-rename recovery; native
  handle transactions are deferred hardening.
- One retained snapshot costs bounded disk space but protects in-flight reads.
- Multiple independent public web apps are explicitly deferred.
- Mixed-project documents are discoverable at `materials.html`, not injected
  into untrusted web HTML.
- M3 has no user UI to mark legacy output public; future trusted creation/UI
  workflows must do that explicitly.

## 21. Definition of Done M3

- [ ] ADR-0004/design accepted before code.
- [ ] Public-only, validated all-or-nothing snapshots; document/web/mixed work.
- [ ] Durable unique ASCII routes remain stable across rename/republish.
- [ ] Exactly one publisher follows first-start/reuse/per-project-stop/last-stop.
- [ ] Idempotency, recovery, failure preservation and concurrency tests pass.
- [ ] v1 migration is atomic and private-by-default; M1/M2 stay green.
- [ ] Formatting, lint, named tests/security/integration, `./scripts/verify`,
      independent security review, and handoff evidence pass.
- [ ] No forbidden M3 dependency (Tauri, tunnel, Cloudflare, OpenCode, AI, QR).

## 22. Incremental scripts/verify

M3 retains M2 checks and adds named local suites:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked -p project-fs --test project_lifecycle
cargo test --locked -p project-publisher --test publisher_http
cargo test --locked -p project-publisher --test publisher_security
cargo test --locked -p project-publication --test publication_lifecycle
cargo test --locked -p project-publication --test publication_security
cargo test --locked -p project-fs --test project_migration
git diff --check
```

The implementation may place `project-publication` inside `project-core` only
if these named suites remain explicit. It updates this documentation and the
script together; no Node/external process is introduced.

## 23. Explicitly M4

Only M4 adds Cloudflare/cloudflared, a one-tunnel session, public URLs, QR, and
tunnel retry/failure policy. It reuses M3's publisher and route but cannot alter
visibility or snapshots. UI, Tauri, OpenCode/AI/providers, packaging/updater,
and Windows release remain out of scope.
