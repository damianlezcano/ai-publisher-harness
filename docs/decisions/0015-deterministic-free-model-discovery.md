# ADR-0015: Deterministic free-model discovery

- Status: Accepted

## Context

ADR-0009 established free models as the zero-friction default and forbade silent
switches to a paid model or a different provider. Its ranking, however, relies
on `Iterator::find` over the order returned by OpenCode's `GET /api/model`, which
is not contractually ordered. If the catalog reorders, the auto-selected default
could change between launches. The approved direction also requires that no
model name be embedded as product logic ("do not hardcode Big Pickle").

## Decision

The default free model is chosen by a deterministic preference over grounded
catalog metadata (`enabled`, `status`, `cost`, and the provider default map):

1. usable = `!disabled && !deprecated`
2. free = `cost == 0`
3. rank descending:
   1. `provider_id == "opencode"` AND `recommended`
   2. `provider_id == "opencode"`
   3. `recommended` on any provider
   4. any free model
4. stable tie-break within a rank: `(provider_id, model_id)` ascending; take the
   first.
5. empty ⇒ `requires_choice` (existing "no model" notice).

The `opencode` preference is a zero-credential *tier* preference, not a model
name; `recommended` is the provider default and serves as the "appropriate
capability" proxy. No model name is stored in product code; the rendered name is
whatever OpenCode's catalog supplies.

## Consequences

- Auto-selection is reproducible and stable under catalog reordering.
- The ephemeral default continues to follow the live free-model set (it is not
  persisted); explicit user selection remains durable in `settings.json`.
- ADR-0009's cost/consent invariants are unchanged (no auto-switch to paid or to
  a different provider).

## Alternatives considered

### Keep order-dependent `.find`

Rejected: non-deterministic across catalog updates; a silently changing default
surprises the non-technical user.

### Rank by inferred capability ("fast"/"smart")

Rejected: no grounded latency/quality signal in the catalog (ADR-0009), so the
labels would be misleading.

### Embed a known-free model name as the default

Rejected: violates "do not hardcode"; the free set changes over time.
