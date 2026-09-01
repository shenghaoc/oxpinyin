# verify-nightly root-cause record

## Date

2026-09-01

## Context

The `verify-nightly` workflow had never been green: it has only
`schedule` and `workflow_dispatch` triggers, so the introducing PR's CI
could not run it, and the first `workflow_dispatch` (run 33415003204)
failed every Rust-building job. Earlier fix attempts (PRs #264-#267)
addressed the apt/bootstrap layer and the miri `--component` syntax, but
left four genuine root causes unfixed. This record is addended by the
workflow comment that replaces the old Miri lane and by PR #271's commit
message, which numeral the four fixes.

## Root causes fixed (PR #271)

1. **fuzz-soak**: `fuzz/fuzz_targets/fixture_model.rs` imported
   `oxpinyin_core::fixture::{FixtureDictionary, FixtureLanguageModel}`, a
   module that does not exist in `oxpinyin-core`. The types live in the
   `oxpinyin-testsupport` crate. The target failed to compile with E0432
   before it could run. Fixed by adding `oxpinyin-testsupport` to
   `fuzz/Cargo.toml` and correcting the import.

2. **nextest, overflow-lane, coverage**: the `refuses_a_tracked_cache_path`
   test (`crates/pinyin-oracle/tests/model_fetch.rs`) asserted the fetch
   script's stderr contains `git would track`. In CI the checkout carries
   no `.git` (the bare `debian:testing` container has no git at checkout
   time, so `actions/checkout` uses the REST-tarball fallback), the script's
   `git rev-parse --is-inside-work-tree` probe fails, and the refusal falls
   into the no-git branch whose message is `refusing to write model bytes
   under the repository without git ignore checks`. The refusal is
   correct; only the branch-specific wording differs. Fixed by asserting
   the common refusal sentinel `refusing to write model bytes`, which both
   branches print. (This supersedes PR #267's proposed pre-checkout-git
   approach: making the test robust to the no-git checkout is sufficient
   and removes the second apt site per job that the old plan needed.)

3. **geiger**: `cargo-geiger 0.13` cannot scan a virtual manifest (the
   root `Cargo.toml` is one), so `cargo geiger` errored and the
   `--output-format text` fallback panicked on argument parsing. Fixed by
   scanning each member individually with `--manifest-path`.

## Miri removed

The Miri lane is removed, not just bounded. It is meaningless for this
codebase:

- `oxpinyin-core` is `#![forbid(unsafe_code)]` — zero unsafe to check.
- `oxpinyin-store`'s unsafe is entirely FFI into native C libraries
  (`kyotocabinet`/`tkrzw`/`lmdb`), which Miri cannot step into.
- Miri therefore only interprets safe Rust at 10x-1000x slowdown, never
  producing a finding. Without a cap it hit the 6-hour job ceiling and
  was force-cancelled; with a cap it burned a 45-minute runner for no
  signal.

## Mutants kept

The `mutants` trial lane is kept. Unlike Miri it produces real test-
strength signal: run 33415003204 reported 860 mutants tested, 113 missed
— concrete evidence of coverage gaps. It is `continue-on-error`, so it
costs run time but never fails the workflow. It does not layer on Miri;
the two were already independent jobs.

## Verification

Run 33453513925 (PR #271 branch): nextest, overflow-lane, coverage,
fuzz-soak, geiger, and lizard all pass; the coverage and geiger artifacts
upload. Miri was still present in that run (removed after); mutants is the
only lane still pending at the time of writing.
