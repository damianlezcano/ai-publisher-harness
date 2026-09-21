# Architecture

## Principle
Keep product/domain logic independent of Tauri, OpenCode and Cloudflare.

Persistent on-disk layout (app data directory, conversations, materials,
workspace, Creations, preview temp, publish snapshots) is documented from
the current implementation in `docs/architecture/STORAGE_LAYOUT.md`. The AppImage is
package media, not the user-data container.

## Knowledge available vs Knowledge needed (Phase 1–3)

Routing is decided **per turn**. A project may have persisted Knowledge
(`knowledge.sqlite`, READY files, a corpus, embeddings) without the current
prompt using it. Knowledge available does **not** imply using Knowledge. When
the intent cannot be resolved structurally, the multilingual semantic
classifier is the authority. `Intent::OrdinaryChat` may be the classifier's
result even when an index exists.

`Intent::OrdinaryChat` is the strong per-turn boundary: **no Knowledge
processing**. After routing resolves OrdinaryChat the turn must not run
retrieval (hybrid/lexical/semantic), query embeddings, assemble or serialize
Knowledge context, set `retrieval_mode=normal`, or open a fresh RAG session.
It uses the project's normal conversational session (`open_session`). A later
Knowledge turn in the same conversation still works; a Knowledge follow-up
only binds when `resolve_followup` resolved it as a Knowledge follow-up.

The classifier gate is structural, not linguistic: no index, no current-turn
attachments, and no prior referent → skip remote classification. Local
question-word helpers must not skip the classifier or force Knowledge when
wording is still ambiguous.

Intent resolution precedence (Phase 3): typed follow-up → creation pre-gate →
semantic classifier → deterministic fallback (error/low-confidence only) →
summary-depth clamp (K6 compact invariant) → `BoundRoute`. Downstream engines
consume that route and do not re-classify the prompt. Fallback is exclusive
replacement, not a competing keyword classifier.

## OpenCode session frontiers (Phase 4)

Session identity follows **responsibility**, not a single global `session_id`.

| Responsibility | Session | Reuse | What is sent | Continuity |
| --- | --- | --- | --- | --- |
| Visible OrdinaryChat / tool-using creation | Conversational `open_session` | Cached per `project_id` until backend restart, conversational send failure, or Creation that serialized Knowledge evidence | User prompt + instructions; Creation may include this-turn document-wide evidence | OpenCode transcript of that session |
| Knowledge answer with serialized evidence (`normal`, `exhaustive`, `thematic`) | Ephemeral `open_fresh_session` | Never; does not replace the conversational cache | Current prompt + bounded EducAI visible history + this-turn evidence | EducAI persisted messages (bounded restatement) + typed `resolve_followup` referents + this-turn retrieval |
| Classifier / K6 / PerItem / internal summarizers | Scratch | Never | Worker prompt only | None (must not enter the visible chat transcript) |

Knowledge evidence is ephemeral by design for retrieval answers: `<knowledge_evidence>` is assembled
for the current turn and must not accumulate on a later OrdinaryChat OpenCode
transcript. For `NormalSemantic`, `CorpusThematic`, and `CorpusExhaustive`, that
block is the documentary source of the answer. The agent cwd/workspace does not
represent the Knowledge corpus, so an empty filesystem is not an empty corpus.
The shared Knowledge-answer grounding instruction states that fact without
changing the OpenCode build agent, tools, or session policy. Document bodies
inside `<knowledge_evidence trust="untrusted">` remain untrusted instructions.
Creation is the exception that may serialize document-wide
material on the conversational session (tools/workspace/`revise_existing`).
After that send (success, failure, or cancel once the prompt was sent), EducAI
invalidates the conversational cache so the next OrdinaryChat opens a clean
`open_session`. Continuity for a follow-up such as “eso” does **not** require
sharing `session_id`. EducAI restates at most the last few visible user and
assistant turns (truncated, evidence markup stripped). Typed follow-ups still
bind through persisted referents (`MaterialSet` / `ThemeSet`), not OpenCode
history.

OrdinaryChat after a Knowledge retrieval turn therefore cannot inherit raw chunks from
OpenCode. A later OrdinaryChat still uses the cached conversational session
unless a prior Creation turn serialized Knowledge evidence onto it.

## Turn lifecycle, transitions, and continuity (Phase 5)

Routing stays per turn. A conversation is not a permanent Knowledge mode.
The same EducAI conversation may alternate OrdinaryChat, Knowledge QA,
follow-ups, attachments, and a new material scope without carrying previous
`<knowledge_evidence>` into later turns.

Continuity is explicit and bounded:

- visible user/assistant EducAI messages (at most 4, ~480 characters each,
  ~1400 characters total, UTF-8 safe, current prompt omitted, evidence markup
  stripped);
- persisted referents (`MaterialSet` / `ThemeSet`) and `resolve_followup`;
- the conversation-active material set when the current contract uses it.

Continuity is not: prior chunks, prior `retrieval_mode`, classifier/scratch
transcripts, K6 nodes, PerItem batch bodies, or a reused Knowledge OpenCode
transcript.

Ephemeral Knowledge `send` failure restores the conversational cancel target
and does not drop the conversational session cache. Cancel during an
ephemeral turn aborts that ephemeral session, then the map returns to the
conversational session. Restart/reopen drops in-memory OpenCode session ids;
durable EducAI messages, Knowledge index, referents, and materials remain.

## Phase 6 — consolidated routing, session, and provider-call model

Phases 1–5 remain the architecture. Phase 6 did not redesign them. It locked
the resulting contracts as testable invariants.

### Intent → engine (per turn)

| Intent | Engine | Knowledge processing | Answer LLM |
| --- | --- | --- | --- |
| `OrdinaryChat` | conversational chat | none | 1 |
| `KnowledgeInventory` | local metadata | sqlite metadata only | 0 |
| `NormalSemantic` | hybrid K3/K4 | ephemeral Knowledge session | 1 |
| `CorpusExhaustive` | full READY scan | local if complete-negative; else ephemeral Knowledge | 0 or 1 |
| `CorpusThematic` | thematic prep | ephemeral Knowledge if LLM synthesis | 0 or 1 |
| `PerItemBatchAggregate` / `BatchSummary` | compact per-item | scratch worker(s) | 0 chat |
| `WholeCorpusSummary` / `PerSourceSummary` | K6 | scratch worker(s) | 0 chat |
| `Creation` | agent/tools | conversational session; may serialize document-wide Knowledge evidence when the creation contract requires it; that session is then non-reusable for later OrdinaryChat | 1 or local clarify |

`Intent::uses_knowledge()` is false only for `OrdinaryChat` and `Creation`.
Availability of `knowledge.sqlite` never selects an engine.

### Session policy (responsibility, not a unified `session_id`)

| Role | When | Reuse | Cancel |
| --- | --- | --- | --- |
| conversational | OrdinaryChat; Creation/tools | cached `open_session` per project until restart, conversational send failure, or Creation that serialized Knowledge evidence | abort that session |
| ephemeral_knowledge | serialized RAG evidence (`retrieval_mode` `normal` / `exhaustive` / `thematic`) | never; does not replace conversational cache | abort ephemeral, restore conversational target |
| scratch classifier | `should_classify` | never | abort scratch |
| scratch PerItem | compact batches | never | abort scratch |
| scratch K6 | document/batch/global nodes | never | abort scratch |

`knowledge_uses_ephemeral_session` inspects the **prepared evidence package**,
not the prompt. It is not a second classifier.

### Provider-call matrix (OpenCode `prompt_async` / answer inference)

Remote embeddings are not provider calls. Create-session / abort / GET message
are control plane.

| Case | Classifier | Retrieval remote | Answer / workers |
| --- | --- | --- | --- |
| A OrdinaryChat, no Knowledge | 0 | 0 | 1 chat |
| B Knowledge available, classifier → OrdinaryChat | 1 | 0 | 1 chat |
| C Inventory | 1 if gate | 0 | 0 answer |
| D Exhaustive complete-negative | 1 if gate | 0 | 0 answer |
| E NormalSemantic | max 1 | 0 remote (local hybrid) | 1 ephemeral |
| F PerItem | max 1 | 0 | bounded scratch batches |
| G K6 | max 1 | 0 | K6 remotes per plan/cache |

A trusted semantic decision is never mixed with fallback. Fallback runs at
most once. The compact/K6 clamp does not call the classifier again.

### Invariants

- OrdinaryChat ⇒ `knowledge == None`, `retrieval_mode == None`, 0 query
  embeddings, conversational session (a new conversational id if the previous
  Creation turn serialized Knowledge evidence).
- `uses_knowledge() == false` ⇒ no Knowledge preparation (Creation may still
  attach document-wide material under its own contract).
- Serialized Knowledge evidence with `retrieval_mode` `normal` / `exhaustive` /
  `thematic` ⇒ ephemeral Knowledge session.
- Creation may serialize Knowledge evidence on the conversational session for
  tools/workspace; that session_id is not reused by a later OrdinaryChat.
- Visible `conversation_context` ⇒ at most 4 messages, ~480 chars each,
  ~1400 chars total, UTF-8 safe, same conversation, no current prompt, no
  `<knowledge_evidence>`, no scratch/K6/PerItem bodies.

## Layers

```
UI
  -> Application Core
      -> Ports / Interfaces
          -> Adapters
```

## Core services
- ProjectManager
- MaterialManager
- CreationManager
- PublicationManager

## External adapters
- OpenCodeAgentAdapter
- FilesystemProjectStore
- LocalPublisherAdapter
- CloudflareTunnelAdapter

## Publication design
One local HTTP publisher serves zero or more published projects.

Example route table:

```
/fotosintesis-a7k2 -> <project A publish root>
/sistema-solar-k91p -> <project B publish root>
```

Cloudflare sees only one local origin, e.g. `localhost:<publisher-port>`.

M2's publisher is a read-only adapter: it receives only registered canonical
`publish/` roots and opaque route keys, never a general project root. It serves
content already prepared under `publish/`; the application decision to publish,
route persistence, and content preparation belong to M3. See proposed
ADR-0003 and `docs/history/milestones/M2_DESIGN.md`.

## Provider / model architecture (M7)

M7 delegates provider authentication, credential ownership, and model discovery
to OpenCode's v2 integrations API. The shared `OpenCodeBackend`
(`project-opencode`) owns the single `opencode serve` process used by both the
agent engine and the provider connector; the `AgentEngine` port and
`AgentService` semantics are unchanged by this mechanical refactor.
`ProviderConnector` (`project-provider`) is the credential-domain port whose
single adapter, `OpenCodeProviderConnector`, drives the OpenCode server.

Credentials are owned by OpenCode, one-way: the frontend submits a secret
exactly once (or never, for OAuth), OpenCode stores it in its isolated
`auth.json` (0600), and there is no read-back command and no app-owned secret
store. Model selection is optional per conversation in `project.json`; when no
conversation selection exists, the global free default (the `opencode` tier,
`cost: 0`) is used. A selected model is captured for future turns and cannot
change an active turn; credential mutations
restart the shared backend so no stale session uses a removed credential
(ADR-0008, ADR-0009).

## Knowledge summarization routing (K6)

Ordinary questions use bounded hybrid Knowledge retrieval; its top-k evidence
set is not a source inventory. Summary requests over an explicit selected set
are split into three deterministic routes by intent depth:

- **generic aggregate** (`selected_batch_aggregate`): "resumen general de estos
  archivos" — one bounded aggregate call over the exact current-turn set;
- **compact per-source** (`per_item_batch_aggregate`): "resumime cada archivo
  por separado", "resumí brevemente cada documento", "Résume chaque fichier
  séparément" — one brief identifiable result per selected source through a
  bounded number of aggregate remote calls (never one call per document, never
  the K6 document-node pipeline);
- **deep per-source** (`selected_per_source` / K6): "analizá detalladamente cada
  documento", "resumen exhaustivo y profundo de cada archivo" — the durable
  hierarchical document/batch/global K6 summarization.

The compact path resolves its exact material set with the same deterministic
precedence as the deep path (current-turn attachments, then the newest
compatible prior MaterialSet referent, then the conversation-active material
set), reuses local compact representations (persisted `Ready` document
summaries when present, otherwise deterministic bounded chunk representatives),
validates exact output cardinality, and never re-embeds or re-indexes.

A compact/depth clamp guards the summary-depth boundary: when the deterministic
local detector resolves wording as an unequivocal compact per-file request, a
successful semantic classifier cannot promote it to `WholeCorpusSummary` or
`PerSourceSummary`/K6; only explicit deep/detail wording stays on the K6 path.
Compact batching enforces a hard application-level request budget in exact
serialized UTF-8 bytes of the prompt that is sent (instruction, source names,
`[Mxxx]` framing, and representatives all count), so provider-call count is a
bounded function of the budget and item cap, never one call per document.

An explicit per-file request with composer selected materials (for example,
"analizá detalladamente cada archivo") uses K6 planning over exactly that
selected set. K6 creates/reuses a document summary for every ready selected
source and may retain its bounded batch/global nodes for cache and accounting,
but the response surfaces one labelled entry per selected source. Historical
project materials are not implicitly added.

For “ordenado por fecha”, only an unambiguous ISO-like date in a sanitized
filename is a trusted ordering hint. Dated files sort chronologically; ties
and undated files retain composer order, and no date is inferred from content.
An unsupported or unindexed selected source remains an explicit labelled
failure entry rather than silently disappearing. Supported indexed text stays
on the Knowledge/K6 path and is never raw-forwarded solely for coverage.

K6 remote synthesis uses a dedicated OpenCode scratch session. Completing a
node polls `GET /session/{id}/message?limit=1000&directory=`, matching OpenCode
1.18.25: a **bare array** of `{info, parts}` (the `{data:[...]}` envelope is
used by other list endpoints, not this route). Role, id, and `finish` live
under `info`. `finish` is omitted while streaming, `"tool-calls"` on
intermediate tool turns, and a terminal reason (`"stop"`, `"length"`,
`"content-filter"`, `"error"`) when the turn is done. K6 denies scratch
session tools so a permission wait cannot replace completion, aborts the scratch
session on timeout, and accepts either a bare array or a `data` envelope for
compatibility.

The message-list completion parser for the summarizer and classifier paths is
centralized in `project-opencode::messages` (`session_messages`,
`message_role`/`message_id`/`parent_message_id`/`assistant_finish`/`message_text`/
`message_error`, and `detect_terminal_assistant`). The compact per-item
summarizer and the K6 summarizer/classifier read this one parser instead of
maintaining divergent copies. The live agent chat engine (`project-agent`) still
uses its own stricter `finish == "stop"` completion predicate and is NOT yet
migrated to this shared detector, so centralization is NOT total.

`detect_terminal_assistant` returns a structural `TerminalDetection` with
absolute precedence: a provider error (`info.error` or `finish == "error"`) on
the newest relevant assistant message is a terminal failure and is never
converted into success; `finish == "stop"` (with non-empty text) is the only
normal success; `finish == "length"` is terminal but truncated (`truncated =
true`, no text, never presented as complete); `finish == "content-filter"` is a
terminal failure; a `time.completed` step with no recognized `finish`, no error,
no tool call, and no text/parts is a terminal EMPTY outcome
(`terminal_source=completed_without_output`), never a success and never a
timeout; `"tool-calls"`, `"unknown"`, absent finish (without a completed empty
state), and a bare `time.completed` with text are non-terminal (the poll keeps
waiting and eventually times out). The summarizer emits safe structural
telemetry (`terminal_source`, `observed_finish`, `assistant_message_seen`,
`terminal_detected`, `provider_error`, `truncated`, `timeout_reason`) without
logging prompt/assistant bodies.

OpenCode 1.18.25 can also finish a scratch turn WITHOUT ever writing `finish`
(or `time.completed`): the correlated assistant text is complete and stable
while every explicit terminal marker is absent. For that one case the
summarizer and classifier use a shared `ScratchCompletionTracker` in
`project-opencode::messages` (plus `scratch_text_candidate`), which extracts
the newest relevant assistant that is non-stale, parent-correlated, error-free,
finish-absent, tool-free, and text-bearing, then accepts its text
(`terminal_source=stable_text`) only after the structural fingerprint (message
id, parent id, text length, an in-memory-only content hash, part types) has
been QUIESCENT for `SCRATCH_MIN_STABLE_POLLS` polls spanning at least
`SCRATCH_MIN_STABLE_DURATION` (1s). Streaming text resets the window every poll,
so partial text can never finish early, and any later provider error or
`finish` still wins on the next poll. Explicit terminal signals remain
authoritative; the fallback is scoped to the scratch behavior that lacks
terminal metadata and does not weaken normal-chat completion (`project-agent`
keeps its own stricter `finish == "stop"` predicate).

## Dependency rules
- UI does not invoke OpenCode/cloudflared directly.
- Core does not import Tauri APIs.
- Tunnel adapter does not understand Project objects.
- OpenCode adapter does not understand publication.
- Local publisher serves registered publish roots only.
- `project-core` has no provider/OpenCode dependency (unchanged).
- Only `project-opencode`, `project-agent`, and `project-provider` know OpenCode.
- Credentials never appear in project files, logs, URLs, or bundles (SECURITY.md #8).
- Diagnostics are a bounded in-memory process buffer mirrored to stderr; they
  are not persisted and contain metadata only, never prompt, credential,
  attachment/generated content, or raw auth payloads.
