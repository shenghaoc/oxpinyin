# Perf and size in CI — the PR gate rejected, nightly snapshots adopted in principle (2026-09-12 UTC)

Status: **the per-PR gate is rejected and will not be built. The nightly
snapshot series is implemented** — `perf-snapshot` in
`.github/workflows/verify-nightly.yml`, with `tools/perf-gate/snapshot.sh`
and `tools/perf-gate/series.py`. This file replaces the proposal it used
to carry; the proposal text is preserved at this path as merged in PR #407
(commit `d8de0ab2092bc090fce431277968b44fb533da2a`), and the path is kept
so inbound references resolve to the decision.

## What was rejected, and what was not

The rejected thing is **a gate on the PR path**: a check that measures the
tree, compares it to a committed baseline, and fails the build. The
maintainer's reasons, in their terms:

1. **Not feasible to implement in CI.** Not a threshold-tuning problem.
2. **Not feasible to run before-and-after in CI.** A gate is a comparison,
   so it needs two measurements taken under conditions alike enough for
   their difference to mean something, and a PR run cannot produce that
   pair. Everything downstream of it — baselines, fingerprints, agreement
   floors — was machinery for making an untrustworthy comparison look
   trustworthy.
3. **Development happens on both macOS and Linux**, so no single committed
   baseline represents the work. The repository's own records already say
   numbers are comparable only within one container on one host, with a
   whole-session offset of 9–11% measured between two runs of the same
   harness on the same quiet machine
   (`../findings/perf-steady-cycle-cross-host-2026-09-07.md`).
4. **It rests on performance measurement regardless.** Gating only
   deterministic quantities narrows the noise; it does not change what the
   lane is.
5. **The always-latest container is a deliberate policy choice.** The
   proposal asked to pin the perf lane's image by digest, against the
   `debian:testing # deliberately unpinned` convention the other lanes
   carry. That convention stands.

What was **not** rejected is the underlying want: knowing when the Stage 2
numbers move, without a person having to remember to look.

## The direction: nightly snapshots, compared to the previous night

Measure once a night on `verify-nightly.yml`'s runner, store the result,
and compare it to the stored series. This answers the objections above
rather than working around them:

- **The before/after problem dissolves.** There is no attempt to produce a
  pair inside one run. Night *N* is compared to night *N−1*, each measured
  on its own runner, and what accumulates is a **series** rather than a
  single reading against a single baseline. What a history adds over a
  one-shot reading is **temporal** attribution: it localizes the night on
  which a level changed. It does **not** by itself say whether the cause
  was the toolchain, the runner image, or our own commits — a step and a
  large source change look identical in the numbers alone. Separating them
  requires the environment to be recorded **with each sample**, so that a
  move can be read against whether the environment moved too; see "What it
  would record".
- **Nothing blocks.** Tier 3's existing failure policy applies — "nightly
  findings open issues, they do not auto-block unless a ratchet exists"
  (`../safety/ci-strategy.md`). A regression lands and is seen the next
  morning. That is the accepted cost, and it is the same deal the fuzz
  soak already runs on.
- **Cross-platform stops mattering**, because the series does not claim to
  represent both platforms. It is one Linux environment's trend line, and
  it covers **only the code paths that environment executes** — the shared
  ones. Anything behind `cfg(target_os = "macos")`, any macOS-specific
  allocator, linker or filesystem behaviour, and any divergence that only
  appears on that platform stay **unmeasured**. The series is silent about
  them rather than covering them, and it must not be read as coverage of
  work done on a Mac.
- **The unpinned container becomes a feature.** `verify-nightly.yml`'s own
  header says the nightly lanes "exist to surface drift early, and the
  rolling distro toolchain is part of what they exercise". A size series
  that steps when gcc or glibc moves is reporting something true. The
  original proposal wanted to suppress exactly that signal; the nightly
  wants to see it.
- **No new mechanism is needed to keep the series.** The fuzz-soak job
  already persists state across nightly runs with `actions/cache` — run-id
  key, prefix restore-key, latest-wins — and a snapshot series can use the
  same pattern. Nothing is committed to the tree. Three conditions on it:

  - **Its own namespace.** A `perf-snapshot-` key prefix, disjoint from
    `fuzz-corpus-`. Two lanes sharing a prefix would restore each other's
    payloads.
  - **Only the schedule writes.** `verify-nightly.yml` carries both
    `schedule` and `workflow_dispatch`. A manual run may measure and report
    freely, but must **never** append to the series or overwrite the cache:
    an off-cadence sample taken to test something would otherwise become
    the predecessor the next real night compares against. Gate the persist
    step on `github.event_name == 'schedule'`.
  - **A missing predecessor is normal, not an error.** The cache is not
    durable — GitHub evicts unused entries, and the first run has nothing
    to restore. With no predecessor the lane records its sample, reports
    that there is nothing to compare, and succeeds. Each sample is *also*
    uploaded as a workflow artifact, which outlives the cache, so the
    history survives an eviction even though the night-over-night
    comparison skips one night.

### What it would record, and what it must not threshold

Record freely; threshold almost nothing. The failure mode of the rejected
proposal was thresholds, not measurement: a number that merely *appears in
a series a person reads* costs nothing when it is noisy, while a number
that fails a build has to be defensible every single night.

| quantity | record | flag a step change | why |
|---|---|---|---|
| section byte sums, stripped size | yes | yes | deterministic; moves only on code or toolchain |
| allocations per steady cycle | yes | yes | a pure function of the code path |
| callgrind Ir (oxpinyin object) | yes | yes | simulated, exact |
| RSS after the first and last cycle | yes | no | environment-sensitive; a trend to read, not a trigger |
| **wall clock** | **no** | **no** | the evidence against it is unchanged — see reason 3 above and `perf-stage2-harness-2026-08.md`'s instruction not to make its benches a required check |

"Flag" means draw a human's attention — an issue, a summary line — never
fail a merge.

### What it does not give

- **Not a gate.** A regression is detected after it lands, not prevented.
- **Day-granularity attribution.** A moved number points at everything
  merged since the last nightly, not at one PR. Narrowing it is a manual
  bisect, as with the fuzz soak.
- **One platform.** It measures shared code paths on Linux only; macOS-
  specific paths and behaviour are unmeasured, not covered.

## What this settles

**Constitution §2, "pinned reference stack".** Maintainer ruling, same
date: it means **the pinned version of libpinyin**. The rejected proposal
had listed the term as undefined and made a budget ceiling conditional on
someone defining it. It is defined, and the budget is measured against the
pin.

Which pin is a recorded freeze, not a floating reference:
**libpinyin 2.11.92**, commit `074a2219c90feaf962d0d24f034514033ece5f99`
(upstream `main` at the time; untagged there), recorded in
[`../testing/oracle-environment.md`](../testing/oracle-environment.md)
under its 2026-09-06 UTC amendment, where the earlier
`2.11.91`/`0c5e80e1` rows stand unedited as the previous freeze. Moving
the pin is a reviewed change to that document, not something a measurement
may do on its own — a budget whose reference can drift silently is not a
budget.

## Amended 2026-10-18 UTC: binary-size recording

The pin +10% install-size limit is retired; the pin remains the
behaviour/ABI oracle. Build as usual and record in the PR the byte sizes
of the stripped regular files `libpinyin.so.15.0.0` and
`libzhuyin.so.15.0.0`, each counted once, plus their build's revision and
backend. The files must be stripped (for example, `file` reports
“stripped”, not “not stripped”). The release recipe is
`tools/packaging/release-stage.sh` / `tools/packaging/relink-versioned.sh`
with `strip --strip-unneeded`; the script currently skips stripping if
`strip` is missing, so its completion alone does not establish this.

Exclude model data, symlinks, headers, `.pc`, `.a`, debug packages,
packaging files and `DT_NEEDED` libraries. Note any increase over base in
the PR. Decreases are welcome; insignificant ones need no mention.
Binary size per PR is not complexity: work can be split across PRs until
each piece is arbitrarily small, so it is outside the runtime time/space
trade rule. No CI gate is reinstated; docs-only PRs record nothing.

Rust's static `std`/`alloc` and monomorphised code versus C++'s shared
`libstdc++` make a fixed cross-language file-size cap a poor rule.

## As built

The lane is `perf-snapshot` in `.github/workflows/verify-nightly.yml`: a
non-required Tier 3 job, no change to `ci-aggregate`, no effect on branch
protection, nothing on the PR path.

| piece | what it does |
|---|---|
| `tools/perf-gate/snapshot.sh` | builds the shipped artifact through `cinstall` and an `alloc-count` fixture build, measures, emits one JSON sample with the environment recorded beside the numbers |
| `tools/perf-gate/series.py` | appends to the series, compares against the previous sample, writes the step summary |
| `tools/perf-gate/series.test.sh` | 30 cases over the rules above |

Two implementation notes worth knowing when reading a report:

- **A flagged move exits non-zero**, which turns the nightly job red. That
  is the attention mechanism, and it blocks nothing: this job is not a
  required check. A move across an environment change never flags.
- **Instruments degrade rather than lie.** No valgrind means
  `ir_oxpinyin_object` is `null`, not `0`; an artifact without the
  `alloc-count` readers records `null` allocations. `series.py` never
  compares a `null`, so a missing instrument cannot read as a 100%
  improvement.
