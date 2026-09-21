# REQ-HARNESS-001 evidence

## Rework status

- Status: `IN_REVIEW` pending a fourth independent review. Baseline: `015d466`.
- No runtime or product behavior was changed. The only authorized changes under
  `crates/` are:
  - `crates/project-app/src/app.rs`: test fakes/helpers/assertions related to
    Architecture Contract verification (including AC-003/AC-007 traceability).
  - `crates/project-app/src/intent.rs`: test-only counter/assertion for the
    AC-003 low-confidence exclusive fallback.
  Runtime behavior under `crates/` remains outside scope; every listed change
  is test code only, with no functional architecture change.
- Pre-existing out-of-scope untracked files remain unread, unchanged, unstaged,
  and undeleted: `a.zip`, `opencode.json`,
  `crates/project-app/examples/knowledge_grounding_ab_live.rs`, and
  `crates/project-app/examples/scratch_ab_live.rs`.

## Formal closure

- Requirement status: `DONE`. Baseline: `015d466` (`checkpoint: stabilize conversation knowledge and creation architecture`).
- Final independent review: `PASS` — ready for human approval. The reviewer identity is not documented and is not inferred here.
- Human approval for closure: granted.
- AC-001 through AC-014 were reviewed; their individual documents, manifest mapping, and focused selectors passed the architecture gate.
- Final gate results are recorded below: `./scripts/architecture-verify`, `CI=true ./scripts/verify`, `cargo fmt --all -- --check`, and `git diff --check` all passed.
- No runtime or product behavior changed. Changes under `crates/` are test-only; M11 remains `FUTURE`.
- Future hardening remains: diff-aware enforcement, CODEOWNERS, remote CI, branch protection, and independent enforcement of simultaneous contract/test/gate changes.

### Final verification capture

- `./scripts/architecture-verify` — PASS (`14 contracts; mapped tests executed`).
- `CI=true ./scripts/verify` — PASS (exit 0).
- `cargo fmt --all -- --check` — PASS.
- `git diff --check` — PASS (no output, exit 0).
- Git visibility was captured with `git status --short` and `git ls-files --others --exclude-standard`; no files were staged, committed, pushed, or deleted.

## Third independent-review rework evidence

### AC-003 — low-confidence ExclusiveFallback

- Reused existing selector:
  `project-app/lib::low_confidence_uses_exclusive_fallback_once`
  (`crates/project-app/src/intent.rs`). It injects a low-confidence semantic
  `CorpusThematic` result and proves the resolved route is `OrdinaryChat` with
  `SemanticFallback { LowConfidence }`, not the semantic result. The fourth
  review finding is addressed by a test-only low-confidence delegate counter:
  `assert_eq!(classifier_calls.load(Ordering::Relaxed), 1,
  "low-confidence classifier calls")`; this explicitly proves one classifier
  invocation and no second classification on that path.
- Added that selector to `docs/architecture-contracts/manifest.tsv` for
  AC-003 and documented it in `AC-003-exclusive-fallback.md`.
- The existing error-path test
  `classifier_failure_falls_back_to_deterministic_routing` remains mapped. Its
  test-only changes at `app.rs` lines 16758–16859 count the failing classifier
  and assert exactly one call plus `SemanticFallback { Unavailable }`.

### Fourth independent-review rework — AC-003 low-confidence call count

- Finding: the low-confidence selector proved fallback/provenance/exclusive
  replacement but did not explicitly prove that its classifier delegate ran
  exactly once.
- Reinforced test: `low_confidence_uses_exclusive_fallback_once` now gives its
  low-confidence `CorpusThematic` delegate a test-only `AtomicUsize` counter.
  It retains the low confidence, `OrdinaryChat`, `SemanticFallback {
  LowConfidence }`, and no-`CorpusThematic` assertions.
- Exact one-call assertion:
  `assert_eq!(classifier_calls.load(Ordering::Relaxed), 1,
  "low-confidence classifier calls")`. This proves no second classification
  occurs on the low-confidence path.
- AC-003 mapping is unchanged: the selector name is unchanged and remains in
  `docs/architecture-contracts/manifest.tsv` under AC-003.
- Actual rework gate results:
  - `cargo test --locked -p project-app --lib low_confidence_uses_exclusive_fallback_once` — PASS (1 passed).
  - `./scripts/architecture-verify` — PASS (`14 contracts; mapped tests executed`).
  - `CI=true ./scripts/verify` — PASS (exit 0).
  - `cargo fmt --all -- --check` — PASS.
  - `git diff --check` — PASS (no output, exit 0).
- No runtime or product behavior changed; the `intent.rs` modification is only
  the test-local delegate counter and assertion. No reviewer `PASS` is claimed.

### AC-007 — classifier, PerItem, and K6 scratch isolation

- Reused and mapped two production seams:
  `compact_per_item_production_seam_one_ready_material` and
  `k6_selected_per_source_production_seam_one_ready_material`, alongside the
  pre-existing classifier selector.
- Strengthened only test assertions in `crates/project-app/src/app.rs`:
  - lines 20069–20103: PerItem asserts one disposable scratch session, zero
    conversational provider calls, and no staged document body in visible
    messages;
  - lines 20150–20175: K6 asserts one disposable scratch session, zero
    conversational provider calls, and no staged document body in visible
    messages;
  - lines 19277–19284: classifier scratch content is absent from visible
    messages (existing third-review rework assertion).
- The PerItem/K6 test prompts may send their bounded worker evidence to their
  own scratch request; the assertions prove it does not enter the persisted
  visible conversation and that no normal conversational provider call is made.

### Scope correction

- `requirement.md` and `TASK-H7-rework-integration.md` now distinguish
  prohibited runtime/product behavior under `crates/` from the narrowly allowed
  minimal deterministic tests, assertions, and test fakes/helpers needed to
  verify Architecture Contracts.
- This is not a broad retroactive authorization. The exact authorized
  `crates/` changes are only `crates/project-app/src/app.rs` (test
  fakes/helpers/assertions, including the error-classifier test fake at lines
  16758–16859) and `crates/project-app/src/intent.rs` (the test-only
  low-confidence exclusive-fallback counter/assertion). Runtime behavior under
  `crates/` remains outside scope.

## Architecture gate result

Command executed:

```bash
./scripts/architecture-verify
```

Actual terminal result (exact final result format):

```text
architecture-verify: PASS (14 contracts; mapped tests executed)
```

The gate validated all 14 contract documents and selectors, then executed 27
mapped focused tests. It does not report “5 focused suites / 142 tests”.

## Required final command results

- `CI=true ./scripts/verify` — PASS (exit 0): includes `scripts/test-agent-launch`,
  `scripts/test-session-budget`, `scripts/architecture-verify`, M0 harness
  contract, Rust format/clippy/workspace all-target tests, frontend checks, and
  distribution checks.
- `cargo fmt --all -- --check` — PASS.
- `git diff --check` — PASS (no output, exit 0).
- `git status --short` and `git ls-files --others --exclude-standard` — executed
  in the final visibility capture; no files were staged, committed, pushed, or
  deleted.

## Changed-file visibility

This rework changes `docs/architecture-contracts/manifest.tsv`,
`docs/architecture-contracts/AC-003-exclusive-fallback.md`,
`docs/architecture-contracts/AC-007-scratch-isolation.md`,
`tasks/active/REQ-HARNESS-001/requirement.md`,
`tasks/active/REQ-HARNESS-001/TASK-H7-rework-integration.md`, this evidence
file, and test-only code in `crates/project-app/src/app.rs` plus
`crates/project-app/src/intent.rs`. The former contains Architecture Contract
verification fakes/helpers/assertions; the latter contains the AC-003
low-confidence exclusive-fallback counter/assertion. Neither changes runtime
or product behavior, and runtime behavior under `crates/` remains out of
scope.

The worktree also contains the pre-existing Harness migration changes relative
to `015d466`; no commit, push, reviewer `PASS`, or requirement closure is
asserted here.
