# Architecture Contracts

Each active Architecture Contract (AC) is an individual, change-sensitive
document. They protect observable behavior rather than incidental internal
implementation. The canonical test selectors are in `manifest.tsv`; the gate
validates their existence and runs the focused suites offline.

| ID | Contract | Document |
| --- | --- | --- |
| AC-001 | Ordinary Chat isolation | `AC-001-ordinary-chat-isolation.md` |
| AC-002 | Semantic authority | `AC-002-semantic-authority.md` |
| AC-003 | Exclusive fallback | `AC-003-exclusive-fallback.md` |
| AC-004 | Single route binding | `AC-004-single-route-binding.md` |
| AC-005 | Ephemeral Knowledge evidence | `AC-005-ephemeral-knowledge-evidence.md` |
| AC-006 | Bounded continuity | `AC-006-bounded-continuity.md` |
| AC-007 | Scratch isolation | `AC-007-scratch-isolation.md` |
| AC-008 | Evidence grounding | `AC-008-evidence-grounding.md` |
| AC-009 | Exhaustive negative truth | `AC-009-exhaustive-negative-truth.md` |
| AC-010 | Material scope precedence | `AC-010-material-scope-precedence.md` |
| AC-011 | Creation session boundary | `AC-011-creation-session-boundary.md` |
| AC-012 | Provider-call bound | `AC-012-provider-call-bound.md` |
| AC-013 | Corpus privacy | `AC-013-corpus-privacy.md` |
| AC-014 | Project Knowledge isolation | `AC-014-project-knowledge-isolation.md` |

Normal work that needs to change an invariant stops with
`ARCHITECTURE_CHANGE_REQUIRED: AC-XXX`. It needs an ADR, updated observables,
tests and gate, independent review, and explicit human approval.

Current protection is local executable gating plus documentary/process control;
it is not diff-aware technical prevention. See `docs/HARNESS_ENGINEERING.md`.
