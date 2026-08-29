# Worktree Workflow

Each independent change gets one branch and one checkout. The lead assigns the
task, files or directory ownership, acceptance criteria, author, and reviewer
before work starts. No two agents may edit the same checkout simultaneously.

## Checkout ownership

- The integration checkout is lead-owned and is not an author workspace.
- An author works only in its assigned worktree and commits a cohesive change.
- A reviewer uses a separate read-only review worktree or diff view; reviewers
  do not patch the author's checkout.
- Follow-up fixes remain with the author in its own worktree. The reviewer
  re-reviews the amended commit when the finding is material.
- The lead integrates only reviewed commits after verification. Delete a
  worktree only after the branch is merged or its work is explicitly abandoned.

## Worktree lifecycle

- During a milestone, author and reviewer worktrees may remain open for audit.
- After the milestone is integrated, verified, and approved, audit and clean
  historical worktrees.
- Never remove a worktree with unintegrated commits or uncommitted changes.
- The integration checkout remains reserved for the lead.
- Worktree cleanup is part of milestone closure.

## Branch naming and handoff

Use `m<milestone>/<short-task>` branches, e.g. `m2/publisher-route-guard`.
The author handoff includes commit SHA, changed files, exact test commands and
results, security invariants checked, risks, and reviewer request. The reviewer
returns findings by severity and an approve/request-changes decision.

## Creating a worktree

Use a sibling path outside the integration checkout, such as
`../ai-publisher-m2-publisher-route-guard`. With Git directly:

```bash
git worktree add -b m2/publisher-route-guard ../ai-publisher-m2-publisher-route-guard
```

When working inside Herdr, use `herdr worktree create` after checking its
installed help and read the returned identifiers; never guess paths or IDs.
