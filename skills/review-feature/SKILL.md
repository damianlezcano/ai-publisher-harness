---
name: review-feature
description: Independently review a bounded implementation commit.
---

# Review Feature

Review a committed diff authored by another agent from a separate checkout.
Check acceptance criteria, regression coverage, UX vocabulary, source-of-truth
compliance, and every affected `docs/SECURITY.md` invariant. Run focused tests
when possible. Return actionable findings ordered by severity, followed by
`approve` or `request changes`; do not edit the author's checkout.
