# REQ-METRICS-001: Métricas visibles de conversación, proveedor y Knowledge

- Status: `BACKLOG` — prioridad inmediata; ejecutar antes de `REQ-WIN-001`.
- Planning baseline SHA: `4ba4ad3139cd61ac993dfc6fe7980651b95db165`.
- Human gate: `TARGETED_HUMAN` — una persona debe aprobar que el lenguaje, la
  jerarquía y la interpretación de las métricas visibles sean comprensibles y
  veraces para el usuario no técnico.
- Execution roles: the Orchestrator selects bounded authors and an independent
  Reviewer only when this requirement moves to `tasks/active/`. Routing follows
  `RUNTIME.md`; no model/provider is prescribed here.

## Evidence and current problem

The product already persists `TurnMetrics` on the owning user turn, renders a
compact line under a completed assistant response through
`turnMetricsByAssistantId`, and exposes a detailed conversation surface. The
current UI combines three categories whose meaning and applicability are not
yet specified in one user-facing contract:

1. provider telemetry: provider/model, input/output/cache tokens, cost, remote
   calls, and duration;
2. local Knowledge structural facts and estimates: corpus/evidence sizes,
   candidate/evidence counts, estimated tokens, reduction, semantic state, and
   preparation duration; and
3. internal observability: routing/classifier/scratch/indexing diagnostics and
   session logs.

Existing code already protects some distinctions, but the requirement is
needed to make every visible field semantically correct and consistent:

- `AssistantMetrics` labels provider values as real only when
  `source == provider_actual`; its compact line may show duration and a
  retrieval-context reduction.
- `ConversationMetrics` sums provider fields only under its documented
  availability rules, while duration and remote calls have different addition
  semantics.
- `turnMetricsByAssistantId` deliberately declines ambiguous legacy bindings
  rather than lending a failed/cancelled/recovered turn's metrics to the wrong
  answer.
- Checkpoints record that OpenCode may omit token fields, that K6 can make
  multiple remote calls, and that unknown Knowledge facts must stay `null`, not
  become zero. They also record past human evidence where `provider calls = 0`
  revealed an execution failure, so an absent/zero count must not be presented
  as successful provider usage.

There is no current evidence authorizing invented billing, savings, latency,
or provider-call semantics beyond these existing contracts.

## Objective

Define, implement, and verify a single truthful user-facing metrics contract
for completed assistant turns and Conversation Details. It must make clear what
is actual provider telemetry, what is an EducAI local estimate or structural
fact, and what stays internal-only, without exposing prompts, message text,
attachments, material bodies, vectors, credentials, paths, raw payloads, or
implementation infrastructure.

## Scope

- Inventory every metric presently persisted, returned by desktop DTOs,
  rendered per assistant turn, rendered in Conversation Details, or surfaced
  through the current session-log model.
- Establish field-by-field semantics, unit, source, scope (turn vs
  conversation), applicability, aggregation rule, unavailable rendering, and
  whether it is user-facing or internal-only.
- Review and correct the user-visible metric set and its copy/organization as
  warranted by that inventory. This includes provider/model identity, reported
  provider tokens/cache/cost, remote-call count, elapsed turn duration, and
  Knowledge corpus/retrieval/evidence/preparation metrics.
- Preserve durable assistant-to-turn attribution across reload, failed,
  cancelled, recovered, retried, and legacy-message cases; correct the binding
  only if evidence demonstrates a false attribution.
- Make provider telemetry visibly distinct from local Knowledge estimates and
  retain `No disponible` for genuinely unavailable information.
- Define and test the meaning of provider calls for ordinary chat, local
  inventory/complete-negative work, retrieval answers, and multi-call K6 or
  aggregate work; it is a count of executed provider requests for that logical
  turn, not a success claim or a token estimate.
- Validate accessibility and responsive behavior of the compact line and
  details/popover, including keyboard operation and truthful degradation.

Probable implementation surfaces, to be confirmed by the inventory task, are
`app/src/types.ts`, `app/src/messages.ts`, `app/src/turnMetricsBinding.ts`,
`app/src/components/AssistantMetrics.tsx`,
`app/src/components/ConversationMetrics.tsx`, their tests, and the existing
DTO/application metric mapping under `crates/project-app/`. These are not an
authorization to broaden the product surface.

## Required semantics and accuracy rules

- A reported provider token/cache/cost value is shown as actual only when the
  backend explicitly marks it `provider_actual`. Missing provider telemetry is
  `No disponible`; no local estimate may fill it.
- Knowledge corpus/evidence token values and context reduction are local
  estimates, never billed tokens, exact savings, or a performance guarantee.
  Reduction is meaningful only for an applicable serialized Knowledge
  retrieval mode; local inventory, ordinary chat, and K6/aggregate work must
  not be made to imply a `100%` reduction.
- Numeric zero means a measured zero under that field's contract. It must not
  stand in for unavailable, failed, not-applicable, or unmeasured data.
- Duration is elapsed time for the logical completed turn as currently
  persisted, not a provider-only latency measurement unless a future contract
  explicitly adds that separate field.
- Conversation accumulation must only aggregate fields whose source and unit
  make addition truthful. It must not aggregate per-turn Knowledge snapshots
  (corpus, selected evidence, modes, source names) as if they were totals.
- A successful assistant response receives only the metrics of its durable
  owning user turn. Ambiguity must fail closed to no metrics, never borrow from
  an adjacent assistant message.
- User-facing source names remain display names only; no filesystem path or
  material content is introduced.

## UX impacts

The expected UX is a compact, optional per-answer summary plus an accessible
details affordance, and Conversation Details organized around “Uso real del
proveedor” versus “Optimización Knowledge (estimaciones locales)”. Copy must
remain Spanish, plain-language, and avoid exposing OpenCode, runtime, internal
route names, identifiers, or diagnostic-only values. The targeted human gate
decides whether a field is comprehensible enough to remain visible, be renamed,
grouped differently, or stay internal-only.

## Non-goals

- Do not add a billing system, estimate missing provider tokens/costs, or make
  claims about provider invoices.
- Do not expose prompts, responses, raw provider payloads, attachments,
  material/chunk/vector content, credentials, paths, session IDs, or logs as
  metrics.
- Do not redesign routing, Knowledge retrieval, session responsibilities,
  provider selection, or the log subsystem merely to make a metric easier to
  display.
- Do not turn internal classifier/scratch/indexing diagnostics into default UI
  telemetry.
- Do not modify publication, Windows packaging, sidecar behavior, or unrelated
  app/crate behavior.

## Architecture and security impact

- Affected contracts: `AC-012` (provider-call bound) and `AC-013` (corpus
  privacy). The work must preserve their invariants; it is not authorized to
  change either contract.
- Related protected UX/security boundaries: current `docs/product/UX.md` and
  `docs/product/SECURITY.md` prohibit exposing internal metadata, paths, or
  protected content. If truthful display requires changing an Architecture
  Contract or either protected authority, stop with
  `ARCHITECTURE_CHANGE_REQUIRED: AC-XXX`.

## Acceptance criteria

1. A committed metric inventory maps every exposed field to source, unit,
   scope, applicability, unavailable behavior, and user-facing/internal
   disposition; unproven semantics are explicitly marked unavailable rather
   than inferred.
2. The UI distinguishes real provider telemetry from local Knowledge estimates
   at both per-turn and conversation-detail surfaces, with no estimate rendered
   as actual provider usage.
3. Provider tokens/costs absent from OpenCode stay unavailable; zero is only
   shown when the backend records a real zero.
4. Remote-call count, duration, and conversation totals have tested semantics
   for ordinary chat, local-only/inventory, retrieval, K6/multi-call, failure,
   retry, and reload where applicable.
5. Assistant metrics cannot cross-bind between adjacent, failed, cancelled,
   recovered, retry, or ambiguous legacy turns.
6. Knowledge reduction and all Knowledge estimates appear only where their
   applicability is truthful, retain their estimate notice, and never reveal
   protected content or paths.
7. Visible copy remains Spanish, accessible, keyboard-operable, responsive,
   and compliant with the product’s non-technical vocabulary.
8. Focused backend/frontend tests, architecture verification, the general gate,
   independent `PASS`, and the targeted human UX review are recorded before
   closure.

## Verification plan

The active requirement must select focused tests after the inventory identifies
the changed surfaces. At minimum it must run the metric binding/component tests,
the relevant `project-app` metric/DTO tests, frontend type/lint/format/build
checks, `./scripts/architecture-verify`, `CI=true ./scripts/verify`, and
`git diff --check`. The human review uses representative real/unavailable,
local-only, retrieval, and multi-call cases without collecting sensitive
content.

## Planned task contracts and sequence

1. `TASK-METRICS-1` — inventory and semantic decision record; no behavior
   change. It must settle the field matrix and route uncertain fields to
   unavailable/internal-only.
2. `TASK-METRICS-2` — bounded implementation of the approved visible contract,
   binding, DTO/copy, and focused regression tests.
3. `TASK-METRICS-3` — cross-surface verification, accessibility/human evidence,
   and independent-review handoff.

Only after selection may the Orchestrator assign authors/reviewers, refresh the
base SHA, and move this directory to `tasks/active/REQ-METRICS-001/`.

## Open questions

- Which existing visible fields a targeted human review finds useful enough for
  a non-technical default surface versus detail-only/internal-only placement.
- Whether the current duration should remain named as a turn elapsed time or
  needs a clearer Spanish label; no provider-latency meaning is assumed.
- Whether current backend sources can distinguish all provider-call subtypes
  needed for a clearer label without exposing routing internals. If not, retain
  the existing bounded count and document its limitation.
