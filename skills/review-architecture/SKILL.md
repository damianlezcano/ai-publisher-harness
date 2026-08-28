---
name: review-architecture
description: Review cross-layer, durable, and security-sensitive changes.
---

# Review Architecture

Read the ordered source of truth before reviewing. Verify dependency direction
and adapter isolation, then identify whether an ADR is required. For
publication, verify the publisher is constrained to registered publish roots
and no UI/core boundary leaks technical implementation details. Return
evidence-backed findings and a decision; never merge or self-approve the
author's change.
