# UX_REDESIGN_01 — Simple chat-first experience (bounded architecture delta)

Status: **APPROVED by the owner (2026-08-31). No code implemented in this session.**
Date: 2026-08-31
Scope: Architecture-only. Enables the approved chat-first UX (`UX_RELEASE_GATE_01`,
`docs/UX.md` §"Main screens") with the **smallest additive delta** over M1–M10.
No M11 work is started; this design targets the UX milestone that follows M11.

Author/reviewer of this design: HIGH_ARCHITECTURE session (DeepSeek V4 Pro).
Owner approval is required before the UX milestone is planned or any task begins.

---

## 1. Executive summary

The current UI is a 2×2 dashboard (B1) with a technical model/provider strip (B2);
the conversation is not persistent (I1) and is not the center of the screen. The
approved target is a chat-first shell: **LEFT** conversation list (newest first,
renameable), **CENTER** chronological messages/resources, **BOTTOM** prompt +
model + Compartir, **SETTINGS** separate with a close X.

This delta makes the minimal architectural changes needed to support that, while
preserving M1 project/filesystem, M2 local HTTP, M3 publication, M4 tunnel, M5
agent engine, M7 provider integration, M8 attachments/previews, M10 packaging.

Two durable decisions, both recorded as ADRs:

- **ADR-0014** — Durable conversation history lives in the existing `Project`
  aggregate (`project.json`, schema v3), not in `localStorage`. The user-facing
  "Conversación" maps to the existing `Project`/`ProjectId` identity (D2).
- **ADR-0015** — The free-model default is selected by a deterministic, grounded
  preference algorithm with a stable tie-break; no model name is embedded in
  product logic (the current `big-pickle`/`mimo` names come only from OpenCode's
  live catalog).

Everything else (sharing, provider onboarding, previews, packaging) is **reused,
not redesigned**.

## 2. Current problem

From `docs/UX_RELEASE_GATE_01.md` §5–§6 (all reproduced against the running UI):

- **B1** dashboard-first: `WorkspaceView` is four equal panels; the prompt is
  mid-screen; there is no persistent left list.
- **B2** technical surface: raw model id + `Modelo` selector + caveat occupy the
  top bar on every screen, before any product content.
- **I1** chat history is not persistent (client-only `useState`).
- **I3** navigation is a separate `Mis proyectos` screen.
- **I4** materials/creations are panels, not conversation context.
- **I5** share is over-exposed and duplicated.
- **I6** model selector placement/content; **I7** disconnected-state repeats 3×.

Root cause at the architecture level: there is **no durable message concept** in
the domain, and the frontend owns the only message state. The 2×2 layout is a
frontend-only defect but depends on the durable-history decision (D1) for a
correct fix.

## 3. Target UX / domain mapping

| Approved UX (`UX.md` §"Main screens") | Backend/domain |
| --- | --- |
| Conversaciones (left list, newest first, renameable) | `Project` list via `ProjectRepository::list` (already newest-first by `updated_at`) + rename via `rename_project` |
| Conversation (center, chronological messages + resources) | `Project` + new `Project.messages`; `materials`/`creations` remain the resource collections |
| Prompt + Modelo + Compartir (bottom) | `agent_send` (now persists messages), `model_*`, `publish`/`unpublish` |
| Settings (separate, close X returns to same conversation) | provider surface (`provider_*`), reused unchanged |
| Resources in conversation context | message→material/creation references, derived timeline |

No new domain entity is introduced for the conversation. The concept "Conversación"
is a **presentation label** over `Project` (D2, already decided in
`UX_RELEASE_GATE_01` §11). See §4.

## 4. Conversation ↔ Project mapping

- **Identity:** a conversation *is* a `Project`; its stable identity is
  `ProjectId` (UUIDv7). Nothing is renamed in the domain (`Project`,
  `ProjectId`, `ProjectName`, `project_*` commands, `ProjectView`).
- **Title:** `Project.name` (`ProjectName`), already validated and renameable via
  `rename_project` / `project_rename`.
- **Timestamps:** `created_at` / `updated_at` already exist and are maintained by
  every mutation. "Newest first" reuses `updated_at` (§8).
- **Sharing state:** `PublicationManager::list_published()` (M3/M4), surfaced on
  the list DTO (§17).
- **What changes:** `Project` gains one field, `messages: Vec<Message>`
  (§5/§6). That is the entire domain delta.

The only "rename" is user-facing copy (`Proyecto` → `Conversación`) in
`app/src/messages.ts`, covered by ADR-0012 catalog tests.

## 5. Durable message-history model

Add to `project-core` (new value objects, all validated like the existing
newtypes):

```
Message {
  id: MessageId,                    // UUIDv7, via IdGenerator::message_id
  role: "user" | "assistant",       // MessageRole enum
  text: String,                     // <= MAX_MESSAGE_TEXT_CHARS
  status: "ok" | "failed" | "cancelled",  // MessageStatus; "ok" for user msgs
  created_at: Timestamp,
  material_ids: Vec<MaterialId>,    // user msgs: attachments (validated subset)
  creation_ids: Vec<CreationId>,    // assistant msgs: generated resources
}
```

Rationale for each element:

- **`id`** — consistent with the existing discipline (every aggregate member has
  a UUIDv7 id); gives stable render keys and unambiguous ordering tie-break.
- **`role`** — user vs assistant; the only two senders in the timeline.
- **`text`** — the user's **raw** prompt (never the augmented agent prompt, see
  §17) or the assistant reply/error note.
- **`status`** — truthful rendering of a failed/cancelled run as an error state;
  the single most debatable field (see Alternatives).
- **`material_ids` / `creation_ids`** — *references* to the existing
  `Project.materials` / `Project.creations`; the resource collections remain the
  single source of truth for content (no byte or metadata duplication). This is
  what makes resources appear "in conversation" without a second resource store.

Ordering: `messages` is a single append-only `Vec<Message>`; render order is
array order. Array order equals chronological order because appends are
serialized per project (`AgentService` per-project lock) and each append bumps
`updated_at`. `MessageId` (UUIDv7, ms-precision) is the deterministic tie-break
for two messages in the same wall-clock second.

"User vs assistant message" and "relationship to generated resources/materials"
are exactly the `role` + `material_ids`/`creation_ids` fields above; no separate
threading, reactions, or edit/delete is introduced (out of scope — see §27).

## 6. Persistence / schema changes

- **Storage:** messages are persisted inside `project.json` by the existing
  `FilesystemProjectRepository` (atomic write + optimistic concurrency CAS on
  `updated_at` + per-project file lock). This is the single source of truth;
  **no** `localStorage` and no new store/table. Consistent with ADR-0002 and M1.
- **Schema:** bump `PROJECT_SCHEMA_VERSION` 2 → **3**. `messages` is
  `#[serde(default)]` so a v2 file deserializes with an empty list.
- **Domain invariants added to `validate()` / `validate_for_persist()`:**
  - `role` is `user` or `assistant`; `status` is one of the three variants.
  - `created_at` valid; `text` within `MAX_MESSAGE_TEXT_CHARS` (40 000 chars).
  - `material_ids` are unique and each references a present `Project.materials` id.
  - `creation_ids` are unique and each references a present `Project.creations` id.
  - User messages may carry `material_ids`, not `creation_ids`; assistant
    messages the converse (reject cross-assignment to keep the timeline
    unambiguous). This is a validation rule, not new runtime logic.
- **No new files or directories** under `projects/<id>/`; the layout of ADR-0002
  is unchanged.

## 7. Migration strategy

Mirror the existing v1→v2 machinery exactly (`Project::from_json`,
`SchemaV1Project`, `migrate_to_v2`):

1. Keep `LEGACY_PROJECT_SCHEMA_VERSION = 1`.
2. Add a v2 intermediate (current shape) and set `PROJECT_SCHEMA_VERSION = 3`.
3. `from_json`:
   - v1 → `SchemaV1Project` → materialize → migrate (visibility=private,
     route=None) → v3 (messages empty).
   - v2 → deserialize into the current `Project` struct (`messages` defaults to
     `[]`) → set `schema_version = 3`.
   - v3 → deserialize directly.
   - otherwise → `UnsupportedSchema`.
4. Replace/rename `migrate_to_v2` with `migrate_to_v3` (v2→v3 is a pure
   `schema_version` bump; v1→v3 applies the existing v1→v2 rules first).
5. **Backward/forward rule (consistent with the repo):** the reader always loads
   any supported older schema and writes only v3. There is no downgrade path.
6. Existing `project_migration.rs` and `project_lifecycle.rs` suites are extended
   (not rewritten) with v2→v3 and v1→v3 cases.

No user action is required; old projects open with an empty conversation.

## 8. Conversation ordering

- **Backend already sorts newest-first** in `FilesystemProjectRepository::list`
  and `ProjectService` test repo: `updated_at` descending, then `id` ascending.
  No change to the sort.
- The list **DTO** is extended (`ProjectSummary` += `created_at`, `updated_at`,
  `shared`) so the sidebar can render relative/absolute timestamps and a shared
  indicator without an N+1 fetch (see §17).
- **Side effect, accepted:** `rename_project` bumps `updated_at`, so renaming
  re-sorts the conversation to the top. This is acceptable ("most recent
  activity first") and avoids a separate `last_activity_at` column; revisit only
  if the owner objects.

## 9. Rename semantics

Unchanged: `project_rename` / `rename_project`. Inline rename in the sidebar;
`Esc` cancels, `Guardar` commits (existing `ConfirmDialog`/rename form reused).
Title rules (trim, `MAX_PROJECT_NAME_CHARS`, no `/`, `\`, NUL) are already
enforced by `ProjectName::parse`.

## 10. Materials / resources-in-conversation mapping

- **Creations in the timeline:** the assistant message's `creation_ids` render as
  inline creation cards (reuse the existing `CreationsPanel` card + `preview_*` /
  `creation_open` / `creation_set_visibility` actions, unchanged).
- **Materials in the timeline:** the user message's `material_ids` render as
  chips on that message (reuse `MaterialsPanel` item + `material_open` /
  `preview_data`).
- **Materials not attached to any message** (added via the composer's
  attach affordance without sending) render in a single collapsible
  "Materiales" section within the conversation (the gate's "chips/accordion").
  To avoid the duplication the gate flagged (I5/I7), a material appears in the
  timeline when referenced by a message and in the "unattached" section
  otherwise — never in both. This is a **frontend derivation** from
  `ProjectView.materials` + `ProjectView.messages`; no new backend.
- No permanent `Materiales` / `Creaciones` dashboard panels exist in the target
  layout; the existing components are re-laid-out, not rebuilt.

## 11. Automatic free-model discovery algorithm

Refines ADR-0009 (which already made free models the zero-friction default).
Grounded inputs are unchanged and come from OpenCode's live catalog
(`GET /api/model` → `enabled`, `status`, `cost`; `GET /config/providers` →
provider default map), mapped in `project-provider/src/adapter.rs` to
`ModelSummary { free, recommended, deprecated }`.

Deterministic preference (input: `list_models()`):

1. **usable** — drop `deprecated` (and already-dropped `disabled`/`!enabled`).
2. **free** — keep `free == true` (grounded on `cost == 0`).
3. **rank** (descending):
   1. `provider_id == "opencode"` AND `recommended` (zero-credential tier default)
   2. `provider_id == "opencode"` (any free model in that tier)
   3. `recommended` (provider default) on any provider
   4. any free model
4. **stable tie-break** — within a rank, order by `(provider_id, model_id)`
   ascending (lexicographic) and take the first. This removes the current
   `Iterator::find` dependence on the API's array order (non-deterministic).
5. **fallback** — if empty: `requires_choice = true` (existing `NOTICE_NONE`).
   Never auto-switch to a paid model or a different provider (ADR-0009 holds).

This is a small, deterministic rewrite of the existing `default_free_model` /
`pick_free` in `project-provider/src/service.rs`. No model **name** is embedded:
the `opencode` preference is a *tier* (zero-credential), and `recommended` is the
"appropriate capability" proxy (the provider's chosen default). "Capability" is
deliberately not ranked independently (no grounded latency/quality signal,
ADR-0009). "Usable" is `!disabled && !deprecated`.

> Explicit note for reviewers: `big-pickle`, `mimo`, etc. appear only in tests,
> the fake OpenCode server, harness agent config, and the gate OCR evidence — not
> in product source. The `name` the UI renders comes from OpenCode's catalog.

## 12. Explicit model-selection persistence

Current behavior is already correct and is **kept**:

- **Automatic default is ephemeral** — `get_selected_model()` computes
  `default_free_model()` at read time and does **not** persist it. The default
  therefore follows the live free-model set (if today's free model disappears,
  the default moves to the next deterministic free model automatically).
- **Explicit choice is durable** — `select_model()` persists `selected_model`
  (`{ provider_id, model_id }`) to `<app-data>/settings.json` (M7, no secret
  stored). `requires_choice`/`notice` already handle a disappeared stored model.
- **UI hygiene (B2 fix, frontend):** the compact model selector renders
  `ModelSummary.name` + a `Gratis` badge only; it never renders raw
  `provider_id`/`model_id`, and the "los modelos gratis pueden cambiar" caveat
  moves out of default view (kept only inside Settings). `SelectedModelView`
  already carries `name`/`free`/`notice`.

No backend change beyond §11's deterministic ranking.

## 13. Settings navigation semantics

- Settings is a **frontend overlay/screen** opened by a gear entry (replacing the
  `Conectá tu IA` top-bar button). It contains the provider list/connection
  flows (reused `ProviderPanel` internals, relabelled "Configuración").
- A close **X** (plus `Esc`) closes Settings and restores the **exact
  previously-selected conversation** (the current `ProviderPanel` already
  preserves workspace state; this is confirmed correct by the gate §3.8). The
  fix is the affordance: X instead of the text `Cerrar` button, and a gear
  instead of an inline `Conectá tu IA` button.
- No new command; `provider_*` and `model_*` are unchanged.

## 14. Provider integration reuse

M7's OpenCode provider architecture is reused **as-is** (ADR-0008/0009):
`OpenCodeProviderConnector`, `ProviderService`, `OpenCodeBackend`,
`settings.json`, OAuth/API-key flows, credential one-way ownership, restart-on-
mutation. **No second provider system is created.** ChatGPT/OpenAI, Gemini/
Google, DeepSeek, and any other OpenCode-exposed integration continue to flow
through the same `provider_list` / `provider_connect_*` surface.

## 15. Sharing reuse

M3/M4 semantics are reused **as-is**: `publish`, `unpublish`,
`publication_status`, `open_public_url`, QR dialog, stop-sharing confirmation,
and the temporary-link honesty copy. The only change is presentation: a single
`Compartir` action/status in the bottom bar (with `Copiar enlace` / `Abrir
enlace` / `Mostrar QR` / `Dejar de compartir`), removing the permanent
`Compartir` panel and the per-creation `Se compartirá` switch's duplication
(D3). No tunnel/publication architecture change.

## 16. Frontend state model

- `App.tsx` holds: `conversations: ProjectSummary[]` (now with `updated_at`,
  `shared`), `selectedId`, `conversation: ProjectView | null` (now with
  `messages`), `agentPhase`, and `settingsOpen`.
- **Single source of truth = backend.** `ProjectView.messages` drives the
  timeline. On send, the frontend echoes the user prompt optimistically (it has
  the text) and reconciles from `project_open` on each `agent://task` event
  (`working` → pull the persisted user message; `completed`/`failed` → pull the
  assistant message + creations). `registeredCreationIds` are no longer ignored
  (I2 fix) — but they are only a hint to trigger a refresh; the authoritative
  creations come from `project_open`.
- First launch: `project_list()` → if empty, `project_create(defaultTitle)` then
  open it; else open the first (newest). The default title
  ("Nueva conversación") lives in `messages.ts`, not in Rust.
- No global store is added; the existing `useState`/`useEffect` + `api` module
  pattern is preserved.

## 17. APIs / commands required

DTO changes (`project-app/src/dtos.rs`, mirrored in `app/src/types.ts`):

- `ProjectSummary` += `created_at: String`, `updated_at: String`, `shared: bool`.
  (`shared` is derived in `AppState::list_projects` by joining
  `publication.list_published()`; no new tunnel work.)
- `ProjectView` += `messages: Vec<MessageView>`.
- New `MessageView { id, role, text, status, created_at, material_ids,
  creation_ids }` (strings at the boundary; no paths/hashes leak).

Core additions (`project-core`):

- `Message`, `MessageId`, `MessageRole`, `MessageStatus`,
  `Project.messages`, `IdGenerator::message_id`,
  `ProjectService::append_user_message` / `append_assistant_message`
  (each a read-modify-`replace` under the existing CAS).

Facade (`project-app`):

- `AppState::send_message(project_id, prompt, attachment_ids)` orchestrates:
  resolve model (§12) → resolve+validate attachments (reuse
  `resolve_attachments`) → append **user** message (raw prompt + material ids) →
  run agent (existing `AgentService::run`, serialized per project) → append
  **assistant** message (`ok` with `registered_creation_ids`, or
  `failed`/`cancelled` with the human message) → return `AgentRunView`.
  The two appends hold the `projects` mutex only briefly; the long agent run is
  outside the lock.
- `AppState::list_projects` / `open_project` extended for the new DTO fields.

Commands (`app/src-tauri/src/commands.rs`, `lib.rs`, `app/src/api.ts`):

- `agent_send` (modified): persists the user message **synchronously** before
  dispatching the agent to the detached thread (so the prompt is durable even if
  the app crashes mid-run); then emits the existing `agent://task` events. The
  persisted user text is the **raw prompt**, never the augmented attachment
  prompt built in `AgentService::provision_attachments`.
- `project_open` (modified): returns `messages`.
- `project_list` (modified): returns enriched summaries.
- `project_create` / `project_rename` / `project_delete`: unchanged signatures.
- **No new command names** are required; the frontend vocabulary changes only.

## 18. Security implications

- **No new exposure surface.** Messages are written to `project.json` inside the
  owner-only app data dir (0700). The publisher serves only the `publish/`
  snapshot of explicitly public creations (SECURITY #3, #6, #9); `project.json`
  and `messages` are structurally outside any publish root. No route can serve a
  message (verified by existing publisher tests).
- **Credentials unchanged** — messages never touch provider secrets; SECURITY #8
  holds.
- **Reference integrity** — `material_ids`/`creation_ids` on messages are
  validated to be subsets of the owning project (no foreign/absent IDs); this
  mirrors ADR-0011's attachment authorization.
- **Dangling reference on material delete** — `remove_material` additionally
  strips the id from any message's `material_ids` (same ordering discipline as
  today: metadata-first, then content). Creations are never deleted in-product
  (visibility toggle only), so `creation_ids` cannot dangle.
- **Bounded growth / abuse** — `MAX_MESSAGE_TEXT_CHARS` caps each message;
  optional future cap on messages-per-project is noted, not implemented.
- **Logging** — message text must not be logged; the agent path already avoids
  logging prompts; add a scrubber test analogous to `redact_credentials` if the
  agent/backend logs anything new.
- **Preview isolation** (SECURITY #12) is untouched (ADR-0010).

## 19. Deterministic tests

| Area | Tests (names describe behavior) |
| --- | --- |
| Core message model | `messages_are_append_only_and_ordered`, `message_references_must_be_subset_of_project`, `user_message_cannot_reference_creations`, `assistant_message_cannot_reference_materials`, `message_text_capped`, `message_ids_are_uuid_v7` |
| Schema/migration | `v2_project_migrates_to_v3_with_empty_messages`, `v1_project_migrates_to_v3`, `unknown_schema_rejected` (extend `project_migration.rs`) |
| Filesystem durability | `messages_survive_reopen` (write → re-read via `FilesystemProjectRepository`) |
| Agent send | `send_message_persists_user_and_assistant_messages`, `failed_run_persists_failed_assistant_message`, `cancel_persists_cancelled_message` (fake agent engine) |
| Material delete | `deleting_material_clears_message_reference` |
| Free-model ranking | `free_default_is_deterministic_and_prefers_opencode_recommended`, `free_default_stable_under_catalog_reorder`, `no_free_model_requires_choice`, `paid_or_other_provider_never_auto_selected` |
| Frontend | component tests for sidebar ordering, inline-creation-on-complete (I2), single-share-action, X-close settings restore |

All use fixed clocks/IDs and local fakes (TESTING.md); none contact a real
provider/Cloudflare/internet.

## 20. Playwright headed visual tests

Reuse the `UX_RELEASE_GATE_01` harness (`docs/ux-release-gate-01/`, the
injected `window.__TAURI_INTERNALS__` shim + `capture.py`/`measure.py`) to
capture, per viewport (1366×768, 1440×900, 1920×1080):

1. First launch → one conversation open, free model `Gratis` badge only, no raw
   model id, no 2×2 grid, no technical strip (B1/B2 closed).
2. Left sidebar newest-first with timestamp + shared indicator; inline rename.
3. Center: user message → "Creando…" → inline creation card appears without
   reopen (I2 closed).
4. Bottom bar: prompt pinned to bottom, compact `Modelo`, single `Compartir`
   (+ copy/open/QR/stop when shared).
5. Settings via gear → provider list → **X** returns to the exact conversation.
6. Restart persistence: reload the shim with the same backend → conversation
   messages restore (D1 closed).

Assertions combine screenshots, per-screen OCR, and Chromium accessibility
trees, exactly as the gate did (the reviewer cannot view pixels directly; PNGs
are for human confirmation).

## 21. Accessibility

- Sidebar conversation list: `nav` landmark, each item a button with
  `aria-current` for the selected conversation.
- Message timeline: a live region (`aria-live="polite"`) so new
  assistant messages and completion toasts are announced; user/assistant
  authorship is conveyed by role labels, not color alone.
- Composer: `Ctrl/Cmd+Enter` sends (existing shortcut), focus trap/restore when
  Settings or dialogs open (existing `useFocusTrap`), `Esc` closes focused
  dialog / cancels rename.
- Settings close **X** is a labelled button (`aria-label="Cerrar"`), not an
  icon-only control without a name.
- Model selector and Compartir controls are keyboard-operable and labelled.

## 22. Exact task graph

Ownership boundaries follow file/directory ownership; each task is one branch /
one worktree. T1–T4 are backend; T5 is frontend (split for parallel ownership);
T6–T7 are verification/docs.

| # | Task | Ownership (files) | Accepts |
| --- | --- | --- | --- |
| T1 | Message domain + schema v3 + migration + validation | `crates/project-core/src/lib.rs` (+ `#[cfg(test)]`) | §5/§6/§7 invariants |
| T2 | FS adapter rehydration/validation for messages + migration tests | `crates/project-fs/src/lib.rs`, `crates/project-fs/tests/*` | §6/§7 |
| T3 | Service `append_*`, facade `send_message`, DTOs, commands | `crates/project-core`, `crates/project-app/{app.rs,dtos.rs}`, `app/src-tauri/src/{commands.rs,lib.rs}` | §17, §18 reference integrity |
| T4 | Deterministic free-model ranking | `crates/project-provider/src/service.rs` (+ tests) | §11/§12 |
| T5a | App shell + `ConversationsSidebar` (new) + first-launch bootstrap | `app/src/App.tsx`, `app/src/components/ConversationsSidebar.tsx` | §3/§8/§16 |
| T5b | `ComposerBar` (new) pinned bottom; remove model selector from app bar | `app/src/components/ComposerBar.tsx`, `app/src/components/ChatPanel.tsx` | §3 bottom bar |
| T5c | Conversation timeline + resources-in-context (inline creations/material chips) | `app/src/components/{WorkspaceView,CreationsPanel,MaterialsPanel}.tsx` | §10, I2 |
| T5d | Settings surface (gear + X) + single Compartir action | `app/src/components/provider/ProviderPanel.tsx`, `app/src/components/PublishPanel.tsx` | §13/§15 |
| T5e | Copy catalog: Conversación vocabulary, de-duplicated notices | `app/src/messages.ts`, `app/src/labels.ts` | §3, ADR-0012 catalog tests green |
| T6 | Playwright headed visual + a11y suite | `app/` (or `/tmp/opencode/ux-review` harness), `docs/ux-redesign-01/` evidence | §20/§21 |
| T7 | `scripts/verify` UX gate + docs/checkpoint update | `scripts/verify`, `docs/` | DoD |

T3 is the integration seam and is the last backend task to merge before T5.

## 23. Reasoning level by task

| Task | Reasoning level | Rationale |
| --- | --- | --- |
| T1 | MEDIUM_HIGH | schema migration + reference-integrity validation is delicate, security-adjacent |
| T2 | MEDIUM | mechanical adapter + tests |
| T3 | MEDIUM_HIGH | cross-module (core→facade→tauri), message atomicity, durable-history correctness |
| T4 | MEDIUM | small deterministic algorithm over existing grounded metadata |
| T5a–T5e | MEDIUM (a,c: MEDIUM_HIGH for event/timeline integration) | frontend re-layout; a/c touch shared App/event wiring |
| T6 | MEDIUM | harness + assertions, no product logic |
| T7 | LOW | docs + verify gate |

## 24. Worker model allocation

Optimized OpenCode Go allocation (approved by the owner 2026-08-31). Model IDs
resolve via `scripts/agent-launch` / `config/agent-models.env`; OpenCode Go never
uses GPT/Grok; Big Pickle is prohibited.

| Role | Model |
| --- | --- |
| Implementation orchestrator / integrate | `opencode-go/deepseek-v4-flash` |
| Coding — normal / complex | `opencode-go/kimi-k2.7-code` |
| Reasoning / independent review | `opencode-go/qwen3.8-max` |
| LOW / visual / CSS / copy | Cursor Composer 2.5 (fallback `opencode-go/mimo-v2.5`) |
| Difficult cross-cutting coding | `opencode-go/kimi-k2.7-code` (fallback Cursor Grok 4.6 medium) |
| HIGH_ARCHITECTURE | fresh `opencode-go/deepseek-v4-pro` **only on escalation** |

DeepSeek Flash coordinates and integrates; it does not implement every task
itself.

## 25. Author / reviewer allocation

AUTHOR ≠ REVIEWER always.

| Task | Author | Reviewer |
| --- | --- | --- |
| T1 (schema v3 + message model) | Kimi K2.7 Code | Qwen3.8 Max |
| T2 (fs adapter + migration) | Kimi K2.7 Code | Qwen3.8 Max |
| T3 (service/facade/commands) | Kimi K2.7 Code | Qwen3.8 Max |
| T4 (free-model ranking) | Kimi K2.7 Code | Qwen3.8 Max |
| T5a–T5e (frontend) | Composer 2.5 (leaf) / Kimi (wiring) | Qwen3.8 Max + catalog check |
| T6 (Playwright/a11y) | Composer 2.5 / MiMo V2.5 | Qwen3.8 Max |
| T7 (verify gate + docs) | MiMo V2.5 | DeepSeek V4 Flash |

The lead integrates only reviewed commits and runs `./scripts/verify` after each
integration batch (MULTI_AGENT_WORKFLOW).

## 26. Definition of Done

- Every applicable box in `docs/DEFINITION_OF_DONE.md`.
- `./scripts/verify` passes **once the pre-existing toolchain drift is resolved**
  (see §28) — otherwise the UX gate is added as a contract heading and verified
  with the same lint set as the rest of the repo.
- `git diff --check` clean; Rust + frontend suites green; ADR-0014/0015 accepted.
- B1 and B2 are closed (no 2×2 grid; no technical strip; no raw model id in
  default view); D1 (restart persistence) and D3 (single Compartir action)
  verified by the Playwright suite; ADR-0012 catalog tests stay green.
- No security invariant in `docs/SECURITY.md` regressed (§18 named tests).

## 27. Explicit untouched M1–M10 areas

- **M1 project/filesystem** — project dir layout, atomic write, CAS, symlink/
  traversal defenses: unchanged (only `project.json` schema + `messages` added).
- **M2 local HTTP / publisher** — route table, read-only publisher, publish-root
  enforcement: unchanged.
- **M3 publication / M4 tunnel** — snapshot/route/state, Cloudflare Quick Tunnel:
  unchanged.
- **M5 agent engine** — `AgentEngine` port, `AgentService`, artifact→creation
  registrar, per-project serialization: unchanged (send path wraps it, does not
  modify it).
- **M7 provider integration** — connector, credential ownership, OAuth, settings:
  unchanged (§14).
- **M8 attachments/previews** — `resolve_attachments`, ADR-0010 preview
  isolation, clipboard image, import batch: unchanged (reused).
- **M9 copy catalog** — `messages.ts` stays the executable catalog (ADR-0012);
  changes are additive copy only, tests green.
- **M10 packaging** — sidecar resolution, `scripts/fetch-sidecars`, AppImage/RPM,
  version alignment: unchanged.
- **M11** — not started, no work in this delta.

## 28. Environment drift note (not part of this delta)

`./scripts/verify` currently fails at `cargo clippy` with
`clippy::chunks-exact-to-as-chunks` in `crates/project-preview/src/token.rs:27`
because the environment's `cargo`/`rustc`/`clippy` are **1.98.0** (Fedora)
while `rust-toolchain.toml` pins **1.97.1** (no rustup installed to honor it).
`git status` and `git diff --check` are clean; M1–M10 are CLOSED; M11 is not
started. This is a pre-existing toolchain mismatch, not caused by and not in
scope for this design. It must be resolved (rustup toolchain or a lint-allow) by
an implementation task before `./scripts/verify` can pass again; flagging for
the lead.

## Alternatives considered (summary)

- **localStorage for history** — rejected: creates a second source of truth and
  breaks the "durable across restart, single owner" requirement (ADR-0014).
- **A new `Conversation` entity/table** — rejected: adds a parallel identity and
  store for zero benefit while `Project` already has identity, timestamps,
  rename, sharing (D2).
- **A separate messages file/`messages.json`** — rejected: a second metadata file
  complicates atomicity and CAS; messages belong in the single atomic
  `project.json` aggregate.
- **Drop `Message.status`** — considered; kept because a failed/cancelled run
  must render as an error, not a normal reply. Could be revisited if the owner
  accepts a uniform rendering.
- **Rank free models by inferred "capability"** — rejected (ADR-0009: no grounded
  signal); `recommended` is the provider-default capability proxy.
