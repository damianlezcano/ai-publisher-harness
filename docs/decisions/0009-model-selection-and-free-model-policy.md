# ADR-0009: Simplified model selection and free-model default policy

- Status: Accepted

## Context

M7 must let a non-technical user choose a model without seeing a
developer-oriented catalog. The installed OpenCode exposes model discovery
(`GET /api/model`) with per-model `name`, `cost`, and `status`, plus a provider
default map (`GET /config/providers`). Some models are free (the `opencode`
provider ships zero-credential, `cost: 0` models with a public API key). Model
availability is not guaranteed over time (catalog updates can remove models),
and the product must never surprise the user with an unexpected cost or provider
switch.

## Decision

M7 presents **one global model choice** (stored in `<app-data>/settings.json`)
with a **free recommended default** so first launch works with zero
configuration.

### Grounded labels only

The UI exposes only classifications that can be grounded reliably from the
catalog: `Gratis` (grounded on `cost == 0`), `Recomendado` (grounded on the
provider default map or the provider's first enabled model), and `deprecated`
(grounded on `status == "deprecated"`). "Fast"/"More capable" labels are not
offered because the catalog provides no latency signal to ground them.

### Selection and fallback

- Selection is explicit and applies to the next prompt via
  `AgentPrompt.model` (no backend restart).
- When the stored model disappears:
  1. If the same provider still has a free or recommended model, select it and
     tell the user ("Este modelo ya no está disponible; usamos el recomendado.").
  2. Otherwise, surface "Este modelo ya no está disponible. Elegí otro." and
     require an explicit choice.
- The product **never silently switches provider** and **never silently switches
  to a paid model**. If only paid models remain, the product stops and asks.

### Free-model policy

Free models (the `opencode` tier) are the zero-friction starting point and the
default. The UI labels them "Gratis" and states that free availability is not a
promise ("Puede cambiar con el tiempo"). We never guarantee permanence.

## Consequences

- The default UX is a short, human-readable list ("Recomendado", "Gratis", then
  the user's connected providers), not the 212-provider catalog.
- Cost transparency is coarse (free badge / "De pago") rather than per-token
  pricing; fine-grained cost display is deferred.
- Model-disappearance and provider-removal degrade to explicit, non-technical
  prompts instead of silent switches.

## Alternatives considered

### Expose the full model catalog

Rejected: overwhelming and technical; fails the non-technical persona and leaks
provider/model IDs into the default UX.

### "Fast / More capable / Free" tiers

Rejected: "fast" and "more capable" cannot be grounded reliably from the catalog
(no latency/quality signal), so the labels would be misleading.

### Automatically fall back to any model/provider

Rejected: risks silently switching to a paid model or a different provider,
violating the cost/consent invariant.
