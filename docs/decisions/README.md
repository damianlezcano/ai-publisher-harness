# Architecture Decision Records

Use ADRs only for choices with meaningful long-term consequences or alternatives.

Naming:
`NNNN-short-title.md`

Template:
- Status
- Context
- Decision
- Consequences
- Alternatives considered

Use one of these statuses: `Proposed`, `Accepted`, `Superseded by ADR-NNNN`,
or `Rejected`. ADRs are immutable after acceptance except for status and a
link to their successor. Write an ADR before implementing a choice that has a
meaningful long-term cost, crosses an architecture boundary, or changes a
security invariant. Do not create ADRs for scoped implementation details.

Filename numbers are four digits, monotonically increasing, and never reused.
Start from `0001` when the first decision is accepted.
