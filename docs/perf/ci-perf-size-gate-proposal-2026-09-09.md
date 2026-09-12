# Perf and size in CI — the PR gate rejected, nightly snapshots adopted in principle (2026-09-12 UTC)

Status: **the per-PR gate is rejected and will not be built. A nightly
snapshot series is the agreed direction, not yet implemented** — the lane
is a CI-policy change and that ask is open. This file replaces the
proposal it used to carry; the full design is in history (`git log
--follow` this path, PR #407) and the path is kept so inbound references
resolve to the decision.

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
  single reading against a single baseline. A series is also what makes an
  environment change legible: a toolchain bump shows up as a one-time step
  in the level, our own work shows up as a trend or a jump on a known day.
  A one-shot gate cannot tell those apart; a history can.
- **Nothing blocks.** Tier 3's existing failure policy applies — "nightly
  findings open issues, they do not auto-block unless a ratchet exists"
  (`../safety/ci-strategy.md`). A regression lands and is seen the next
  morning. That is the accepted cost, and it is the same deal the fuzz
  soak already runs on.
- **Cross-platform stops mattering**, because the series does not claim to
  represent both platforms. It is one environment's trend line over the
  shared source; macOS work still appears in it, since the code is the
  same code.
- **The unpinned container becomes a feature.** `verify-nightly.yml`'s own
  header says the nightly lanes "exist to surface drift early, and the
  rolling distro toolchain is part of what they exercise". A size series
  that steps when gcc or glibc moves is reporting something true. The
  original proposal wanted to suppress exactly that signal; the nightly
  wants to see it.
- **No new mechanism is needed to keep the series.** The fuzz-soak job
  already persists state across nightly runs with `actions/cache` — run-id
  key, prefix restore-key, latest-wins — and a snapshot series can use the
  same pattern. Nothing is committed to the tree.

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
| RSS at init and after cycle | yes | no | environment-sensitive; a trend to read, not a trigger |
| **wall clock** | **no** | **no** | the evidence against it is unchanged — see reason 3 above and `perf-stage2-harness-2026-08.md`'s instruction not to make its benches a required check |

"Flag" means draw a human's attention — an issue, a summary line — never
fail a merge.

### What it does not give

- **Not a gate.** A regression is detected after it lands, not prevented.
- **Day-granularity attribution.** A moved number points at everything
  merged since the last nightly, not at one PR. Narrowing it is a manual
  bisect, as with the fuzz soak.
- **One platform.** It says nothing about macOS beyond what the shared
  source implies.

## What this settles

**Constitution §2, "pinned reference stack".** Maintainer ruling, same
date: it means **the pinned version of libpinyin**. The rejected proposal
had listed the term as undefined and made a budget ceiling conditional on
someone defining it. It is defined, and the budget is measured against the
pin.

## Next step

Implementing the nightly lane is a CI-policy change to
`.github/workflows/verify-nightly.yml`, which AGENTS.md puts behind an
explicit ask. It is a much smaller ask than the rejected one: a
non-required Tier 3 job, no change to `ci-aggregate`, no effect on branch
protection, nothing added to the PR path. That ask is open.
