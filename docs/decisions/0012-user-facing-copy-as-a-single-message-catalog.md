# ADR-0012: User-facing copy as a single message catalog (i18n-ready, no framework)

- Status: Accepted

## Context

M9 is education-focused UX polish. Today every user-facing string is a Spanish
literal inlined across `app/src` React components, with only a few helpers in
`app/src/labels.ts`. Error recovery currently has no next-action guidance, empty
states are ad hoc, and there is no single place that defines canonical
terminology. The product is Spanish-first but will need localization later
(CODEX_HANDOFF.md targets eventual Windows/macOS/Linux builds). A future i18n
system must not require re-touching every component or re-auditing terminology.

## Decision

1. **Single message catalog.** All user-facing copy lives in one module,
   `app/src/messages.ts`, keyed by stable, semantic keys (e.g.
   `project.empty.title`, `sharing.temporaryNote`, `error.aiUnavailable`).
   Components render `messages.<key>` and never embed user-facing copy as
   literals. Dynamic values are injected via typed parameters (e.g.
   `messages.material.addedCount(n)`). The existing `labels.ts` helpers
   (`kindLabel`, `visibilityLabel`, `humanSize`, `humanDate`) are folded into or
   re-exported by the catalog module. `es-AR` is the only locale now.

2. **No i18n framework yet.** We do **not** introduce `react-i18next`, ICU, or a
   locale-file loader in M9. The catalog is a plain TypeScript object. This
   decision is about *structure*: making localization a later mechanical swap,
   not about shipping multiple locales now.

3. **Machine-readable errors already exist.** `AppError.code` is already a
   stable snake_case code surfaced to the frontend. M9 adds a frontend-only
   `guidance.ts` that maps codes (and provider/model states) to
   `{ title, message, actions }` using catalog keys. No backend change is needed
   to make error recovery actionable.

4. **Terminology lives above the code.** The canonical user-facing vocabulary is
   defined in `docs/UX.md` (higher in the source-of-truth order than ADRs); the
   catalog is its executable reflection. Changing a term is a UX.md change first,
   then a catalog edit.

## Consequences

- One place to audit copy, terminology, and Spanish phrasing; a future locale is
  a parallel catalog plus a selection mechanism, not a component rewrite.
- Tests can assert exact copy by key (`messages.project.empty.title`), making the
  terminology contract deterministic and preventing silent copy drift.
- No new runtime dependency; `scripts/verify` remains offline and unchanged in
  cost (existing `pnpm` suite covers the catalog tests).
- A stable key contract must be maintained: keys are semantic and additive;
  renaming a key is a breaking change to the frontend and its tests.

## Alternatives considered

### Keep inline literals and add a localization pass later

Rejected: a later i18n pass would require re-touching every component and
re-auditing terminology, the exact cost M9 exists to avoid.

### Adopt a full i18n framework (`react-i18next`) now

Rejected for M9: it adds a runtime dependency and message-interpolation
complexity that the single-locale MVP does not yet justify, and would widen M9's
scope beyond polish. The catalog structure preserves the option without the
cost.

### Put canonical terminology in an ADR instead of UX.md

Rejected: terminology is a product/UX decision, and `AGENTS.md` ranks UX.md above
ADRs as source of truth. The ADR governs the *structure* (catalog), not the words.
