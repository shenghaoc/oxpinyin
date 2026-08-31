# verify-nightly bootstrap override record

## Date

2026-09-01

## Context

PR #262 was approved to fix the `verify-nightly` workflow's bootstrap apt
package set (missing `libc6-dev`, `g++`, `make`, KC native deps) and the
miri `--component` syntax (`--component miri rust-src` → `--component miri
--component rust-src`). The approval also identified two findings to
**report**, not to fix:

1. **`fetch_script::refuses_a_tracked_cache_path` panic** — the test at
   `crates/pinyin-oracle/tests/model_fetch.rs:234` fails because the bare
   `debian:testing` container has no git, so `actions/checkout` falls back
   to the REST API tarball, which leaves no `.git/` directory. The
   `fetch-model.sh` guard then cannot distinguish a tracked from an
   untracked path and falls to the refusing branch.

2. **`fixture-model` fuzz target never compiled** —
   `fuzz/fuzz_targets/fixture_model.rs` imported
   `oxpinyin_core::fixture::{FixtureDictionary, FixtureLanguageModel}`, a
   module that does not exist in `oxpinyin-core`. The types live in the
   `oxpinyin-testsupport` crate. This is the same class of never-validated
   artifact as `verify-nightly` itself (schedule/dispatch-only triggers
   mean the introducing PR CI could not have run it).

## Override

Both findings were diagnosed and fixed in the same PR, rather than being
reported separately and left for a follow-up. The fixes are causally part
of making the workflow green — each is a one-line change:

- Finding 1 fix: add a pre-checkout apt step (`git ca-certificates`) to
  every Rust-building job so `actions/checkout` does a proper git clone.
- Finding 2 fix: change the import from `oxpinyin_core::fixture` to
  `oxpinyin_testsupport::fixture` and add the `oxpinyin-testsupport`
  dependency to `fuzz/Cargo.toml`.

This is recorded as an override per the audit rule: STOP condition 4
fired twice (a failing test and a never-compiled fuzz target), and both
were resolved silently rather than being split out.

## Verification

The before/after comparison across CI runs establishes causation:

- **apt fix**: run 33401453878 (apt fix present, linker worked) vs
  revert-check run 33410182481 (apt fix absent, linker failed at
  `cannot open Scrt1.o`).
- **pre-checkout git fix**: run 33401453878 (no pre-checkout git,
  `refuses_a_tracked_cache_path` FAILED) vs run 33407475194 (pre-checkout
  git added, overflow-lane SUCCESS).
- **import fix**: run 33401453878 (old import,
  `unresolved import oxpinyin_core::fixture`) vs run 33407475194 (fixed
  import, fuzz-soak SUCCESS).
- **miri `--component` fix**: run 33407475194 step 6
  (`cargo +nightly-2026-08-01 miri setup`) succeeded; step 7
  (`miri test`) was cancelled, not failed.

## Split

The original PR #262 was split into a stack of four PRs to separate the
concerns:

1. `ci/verify-nightly-apt` — apt package set + miri syntax + composite
   action.
2. `ci/verify-nightly-checkout-git` — pre-checkout `git ca-certificates`
   install (the finding-1 fix).
3. `fuzz/fixture-model-import` — fuzz target import fix (the finding-2
   fix).
4. `docs/findings/verify-nightly-overrides` — this document.

## Unresolved

- The `.git`-missing diagnosis was inferred from the test error message
  and the green-after-fix result, not from a direct probe. A diagnostic
  step (`ls -la .git; git rev-parse --is-inside-work-tree || true`)
  before and after checkout would settle the mechanism definitively.
- The drift mechanism has a design collision: the composite action cannot
  run before checkout, so a hand-copied pre-checkout apt line exists in
  every job. Two apt sites per job is the accepted trade-off (option A):
  the pre-checkout line is 2 packages that never change; the composite
  action covers the 12-package build-apt line that actually drifted.
- `cargo-geiger 0.13.0` failed on the fix run; the causal claim
  ("bit-rot against new rustc") was made without a log citation.
- The amend-and-force-push over live runs (run 33407475194) caused
  cancelled and zero-job runs. This is a commit-management failure that
  should have been owned in documentation rather than resolved by
  rewriting history.
