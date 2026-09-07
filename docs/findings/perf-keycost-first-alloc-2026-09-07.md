# Key-Cost Walk Elimination — 2026-09-07

## Status

Investigation closed. `6886dc1f` removes the unconditional key-cost
table construction from the real-unigram path; this document records
the causal evidence, the controlled parent→HEAD measurement, and the
standing boundary around the retained fallback-only cache. The next
performance investigation starts from `6886dc1f` and the residual
~1.24× first-result / ~1.16× steady gap — not from the pre-change
~2.8× first-result number, which this work halves.

## Executive summary

- **Root cause.** The first `new_session` on a runtime walked the
  frozen syllable-key inventory — ~430–440 dictionary lookups, each
  scored — to build a per-key cost table (`key_cost_table`,
  `crates/oxpinyin-core/src/scoring.rs:325`). The table's only
  readers are the two `Scorer::with_key_costs` sites
  (`crates/oxpinyin-engine/src/session.rs:1348`, `:1916`), both on
  the `else` of a `has_real_unigrams()` gate, and
  `BigramLanguageModel::has_real_unigrams()` returns the literal
  `true` (`crates/oxpinyin-data/src/lm/mod.rs:566`, delegated through
  `RuntimeLm` at `crates/oxpinyin-runtime/src/lib.rs:755`). No
  runtime-backed session could ever read the table: the walk was pure
  dead work on the shipped path.
- **No upstream analogue.** The pin's `pinyin_alloc_instance`
  allocates instance state and performs no dictionary or model read
  (`src/pinyin.cpp:1310-1333` at `0c5e80e1`); the sweep prices each
  candidate inline from the phrase item it has already fetched
  (`unigram_gen_next_step`/`bigram_gen_next_step`,
  `src/lookup/phonetic_lookup.h:643-698`) over a live
  `get_phrase_index_total_freq` (`src/storage/phrase_index.h:615-617`).
  The pre-computed table is an oxpinyin-only artifact retained solely
  for the pre-frequency fallback scorer.
- **Probe evidence (2026-09-06 session).** Instrumenting both
  `else` arms fired 0 times across 77 runtime + capi tests (including
  the w3-fixture opens) and 4,740 times across the engine's own 115
  tests, whose mock models take the trait default `false`. The table
  is built and discarded on every runtime-backed session.
- **Measured effect of `6886dc1f`** (controlled parent→HEAD, arm64,
  four cells, both backends): first allocation ~17 ms → measurement
  floor (~1 µs); time-to-first-result ~2.78–2.82× → ~1.23–1.24× of
  libpinyin; session-ready RSS −8.98 MiB on both backends; steady
  state unchanged within the control band; `.so` size unchanged.
- **Boundary.** The visibility-stamped key-cost cache is retained,
  now reachable only from the pre-frequency fallback branch
  (`cached_key_costs`, `crates/oxpinyin-runtime/src/lib.rs:985`). It
  is stale under user training. Do not "fix" that by invalidating it
  while nothing on the real-unigram path reads it — that reintroduces
  the walk. See "The stale-but-unread cache".

## Background

The walk entered through the front door: the frozen
[scoring SPEC](scoring-spec.md) requires per-key costs to complete at
scorer construction so `EdgeCost` is a table lookup that cannot fail.
`87f9a49e` deferred the walk from `Runtime::open` to first session
allocation, which fixed `pinyin_init` (tens of ms → ~2–5 ms) but
relocated the cost: the [2026-09-05 x86_64
matrix](perf-backend-matrix-2026-09.md) measured the relocated walk at
56.9 ms (Tkrzw) / 42.1 ms (KC) and the resulting time-to-first-result
at 1.71× / 2.25× of libpinyin, with steady cycles already at parity
(0.94×/0.95×).

`6886dc1f` gates both construction sites on the same predicate that
already decides which decoder runs (`has_real_unigrams()`:
`session.rs:259` for `Session::new`, `lib.rs:961` for
`Runtime::new_session`). The fallback path is unchanged — same eager
walk at construction, same construction-time error surfacing,
`EdgeCost` still infallible — so the frozen SPEC needed no amendment.
The runtime's visibility-stamped cache moved into `cached_key_costs`,
called only from the fallback branch and the epoch-race test.

## Experimental design

Parent `87f25055` versus HEAD (`6886dc1f`), one pass each, on this
host. The passes ran against the pre-rebase pair -- the parent was
the landing tip when the change was cut -- and the chain was rebased
onto the then-main tip `2a99761a` before opening the PR (the engine
gained the #356 character-offset port underneath). Per the rebase
discipline the oracle pins are re-validated on the rebased chain in
CI; the numbers below describe the pre-rebase measurement.

| Property | Value |
|---|---|
| Host | Apple silicon (darwin 27, arm64); Docker 29.7.2, linux/arm64 containers |
| Image | `oxpinyin-matrix:latest` — debian:testing snapshot 20260831, gcc 15.3.0, Rust 1.97.1 per `rust-toolchain.toml`; **same image and invocation for both passes** |
| libpinyin | 2.11.91 (`0c5e80e1`), built in-image `--with-dbm=Tkrzw` / `--with-dbm=KyotoCabinet` |
| Trees | detached git worktrees, one per commit; **separate `CARGO_TARGET_DIR` per worktree** |
| Data | image-baked libpinyin installs — one shared directory per backend pair (the S1 control), identical across passes by construction |
| Harness | `tools/bisection/run-perf-same-data.sh` + `bisect --perf`; `bisect.c` byte-identical in both trees |
| Script delta | the KC cell's one-line feature-selection fix (the core of `b7ee0cea`) applied **identically in both worktrees**; the NEEDED guard and summarizer fix in `b7ee0cea` postdate the passes and did not affect them |
| Schedule | 20 interleaved speed rounds × 4 cells, `PERF_CYCLES=8`; 10 RAM rounds × {ram-init, ram-cycle}; `taskset -c 0` on every process |
| Statistics | medians; 95% percentile-bootstrap CIs (10,000 resamples; whole-run resampling for pooled cycle metrics) |
| Drift control | both libpinyin cells measured in both passes |

ARM64 qualification: absolute values are not comparable to the x86_64
matrix (different ISA, host, container runtime); the controlled
parent→HEAD delta is the result. The parent's walk here (17.0–17.4 ms)
is consistent with the ARM64 KC baseline's 17.6 ms record
([perf-baseline-kc-2026-09.md](../perf/perf-baseline-kc-2026-09.md)) and well
under x86_64's 42.1/56.9 ms — the walk was always host-scaled dead
work.

## Matrix

Medians [95% CI]; n = 20 speed processes per cell per pass (10 per
RAM mode); steady pools cycles 1..7 of every run; ttf = init + first
alloc + cold cycle, computed per process.

**Parent `87f25055`:**

| Cell | init | first alloc | cold cycle | steady cycle | ttf | session-ready RSS |
|---|---:|---:|---:|---:|---:|---:|
| libpinyin-tkrzw | 0.802 [0.782, 0.824] | ~0.3 µs | 9.584 [9.265, 10.066] | 8.522 [8.283, 8.656] | 10.472 [10.047, 10.790] | 12,870 KiB |
| oxpinyin-tkrzw | 1.080 [1.035, 1.102] | **17.364 [17.217, 17.975]** | 10.359 [10.012, 10.896] | 9.869 [9.448, 10.053] | 29.147 [28.432, 30.220] | 22,034 KiB |
| libpinyin-kc | 0.857 [0.769, 0.951] | ~0.3 µs | 9.248 [8.971, 9.453] | 8.471 [8.294, 8.566] | 10.161 [9.820, 10.339] | 17,990 KiB |
| oxpinyin-kc | 0.962 [0.926, 1.054] | **16.995 [16.646, 17.574]** | 10.496 [10.096, 10.657] | 9.898 [9.566, 10.156] | 28.615 [28.037, 29.348] | 28,388 KiB |

**HEAD `6886dc1f`:**

| Cell | init | first alloc | cold cycle | steady cycle | ttf | session-ready RSS |
|---|---:|---:|---:|---:|---:|---:|
| libpinyin-tkrzw | 0.796 [0.756, 0.872] | ~0.3 µs | 9.861 [9.421, 10.570] | 8.686 [8.476, 9.063] | 10.663 [10.215, 11.481] | 12,738 KiB |
| oxpinyin-tkrzw | 1.148 [1.093, 1.332] | **0.001 [0.001, 0.001]** | 12.046 [11.441, 12.818] | 10.037 [9.828, 10.326] | 13.177 [12.569, 14.219] | 12,840 KiB |
| libpinyin-kc | 1.004 [0.863, 1.075] | ~0.3 µs | 9.691 [9.363, 10.090] | 8.565 [8.397, 8.775] | 10.744 [10.312, 11.110] | 17,032 KiB |
| oxpinyin-kc | 1.210 [1.039, 1.333] | **0.001 [0.001, 0.001]** | 11.936 [11.418, 12.447] | 10.107 [9.837, 10.341] | 13.268 [12.576, 13.933] | 19,198 KiB |

"Session-ready RSS" is the harness's `after_init` snapshot, which
bisect takes **after** `alloc_instance` (`tools/bisection/bisect.c`,
snapshot at :614 follows the alloc at :604) — i.e. the footprint with
a live first session, not bare init. Stripped `.so` size: 1,576,960
bytes for all four builds, both passes (page-granular; the commit's
delta sits beneath alignment).

## Per-axis attribution

- **first alloc (eliminated).** ~17 ms of first-allocation latency
  was removed, taking the operation from milliseconds to
  approximately the measurement floor (~1 µs), CI-separated by four
  orders of magnitude on both backends. The multiplicative ratio is
  not meaningful at that denominator.
- **cold cycle (+1.69 / +1.44 ms, real).** The parent's walk
  pre-touched the decode pages, so its first cycle was cheap; HEAD's
  first cycle now pays its own page faults. Cross-check: alloc Δ +
  cold Δ = −15.68 ms (Tkrzw) / −15.55 ms (KC) against measured ttf Δ
  −15.97 / −15.35 — the ≤0.32 ms residuals are the sub-ms init drift
  both cells share. ~10% of the removed cost reappears here and
  nothing beyond that: latency was not merely moved.
- **steady (unchanged).** +1.7% / +2.1% against the libpinyin
  control's own +1.9% / +1.1% drift. Implementation ratio
  1.158×/1.168× → 1.156×/1.180× — the small steady gap is
  host-dependent, as the x86_64 matrix already observed.
- **session-ready RSS (−8.98 MiB both backends).** The harness
  snapshot lands after the alloc, so the parent's rows carried the
  walk's touched pages; HEAD drops them (libpinyin moved <1 MiB).
  oxpinyin-Tkrzw's session-ready footprint is now 12,840 KiB against
  libpinyin's 12,738 KiB: +0.8%, a point estimate over n = 10 RAM
  processes with no interval. The acceptance gate's CI item (§8) covers
  the speed axes only, so no interval was computed for this axis and
  the two figures are not established as equal.
- **ttf ratio.** 2.783× → 1.236× (Tkrzw), 2.816× → 1.235× (KC),
  same-pass quotients. The residual gap decomposes into the
  reallocated cold paging (~2.2 ms), init (~0.3 ms), and steady
  (~1.3 ms).
- **controls.** Worst libpinyin movement across passes: +5.7% ttf
  (KC, driven by sub-ms init drift on both implementations) — an
  order of magnitude below the oxpinyin deltas. The run is clean
  evidence for the alloc and RSS changes.

## The stale-but-unread cache — standing boundary

`Runtime`'s key-cost cache (now `cached_key_costs`,
`crates/oxpinyin-runtime/src/lib.rs:985`, called only from
`new_session`'s fallback branch at `:961` and the epoch-race test) is
stamped with the **library-visibility mask only**. Training mutates
both inputs the walk reads — `RuntimeLm::score`'s user-count delta,
and `RuntimeDict::lookup_into`'s appended user phrases — without
touching the mask, so a cached table goes stale across training.
`UserStore` already exposes `generation()`
(`crates/oxpinyin-user/src/store.rs:392-440`), which drives
`UserLookup`'s rebuild; it is deliberately not part of the cache
stamp.

This is a known non-fix as of `6886dc1f`, recorded so nobody
re-arms the cost accidentally:

1. On every runtime-backed session the table is now never built at
   all (real unigrams), so the staleness has no observable effect.
2. Invalidating on generation would rebuild the ~430-read table
   after every training event — reintroducing exactly the cost this
   change removed — to refresh a value the shipped path discards.

**Do not add the generation stamp while the table is unread.** The
staleness becomes live the moment a runtime session can take the
fallback branch. Note the asymmetry that makes that a one-edit
mistake: `has_unigrams()` (dynamic, `unigram_total() > 0`,
`lm/mod.rs:289`) sits beside the literal `has_real_unigrams()`
(`lm/mod.rs:566`), and the dynamic one already feeds `model_cost`.
If the fallback ever becomes runtime-reachable, the cache must gain
the generation stamp in the same change, with the rebuild cost
measured against it.

## Harness findings (fixed in `b7ee0cea`)

The first benchmark attempt aborted in round 1: the script's KC cell
built with default features, and `05688575` ("build: change default
backend from kyotocabinet to tkrzw") had flipped the workspace
default — so the "oxpinyin-kc" cell silently built a second Tkrzw
artifact, whose processes failed `pinyin_init` on KC-format data.
Confirmed functionally in both directions: the mislabeled artifact
opens Tkrzw data; a true KC build (`--no-default-features --features
kyotocabinet`) opens libpinyin-installed KC data and rejects Tkrzw
data. **There is no data-compatibility problem** — an intermediate
option-mismatch theory from the debugging trail was an artifact of
the same mislabel and is disproven. Before the flip the script was
correct, and the 2026-09-05 x86_64 matrix passed `--features
kyotocabinet` explicitly, so its KC cell was never affected.

`b7ee0cea` makes `build_capi` take a mandatory backend, always build
`--no-default-features --features <backend>`, and verify the built
artifact's `NEEDED` entries link that backend's system library before
any cell is measured; it also fixes the inline RAM summarizer (bisect
nests the counters under `after_init`/`after_last`, so every RSS
column printed `nan`). The guard was verified both ways: correct
builds pass; a Tkrzw artifact presented for the KC cell fails loudly.

Open follow-ups, deliberately not patched piecemeal: `run-key-surface-diff.sh:32-34`
documents the same pre-flip assumption ("default features → KC") for
its oracle-data guidance, and `run-train-diff.sh`,
`run-scheme-diff.sh`, `run-key-surface-diff.sh` all build
`-p oxpinyin-capi` with implicit defaults — the correct
backend/oracle pairing per harness needs a systematic survey.

## Acceptance gate

| # | Check | Status |
|---|---|---|
| 1 | all four cells measured in both passes | PASS |
| 2 | same image and container invocation for both commits | PASS |
| 3 | separate `CARGO_TARGET_DIR` per commit | PASS |
| 4 | one shared data directory per backend pair (S1) | PASS |
| 5 | data dirs unchanged after 320 processes (46/46 files byte-size-identical, no sidecars/lock files) | PASS |
| 6 | libpinyin drift controls measured (worst +5.7% ≪ oxpinyin deltas) | PASS |
| 7 | harness identical between trees (modulo the KC one-liner, applied identically) | PASS |
| 8 | 95% CIs on speed axes | PASS |
| 9 | no production code changes during measurement (worktree-only, discarded) | PASS |

## Next target

From `6886dc1f`: the residual **~1.24× time-to-first-result** and
**~1.16× steady** against same-backend libpinyin on this host — now
legitimate decoder/paging work rather than startup bookkeeping. The
~2.2 ms cold-cycle component is first-cycle paging that libpinyin
absorbs differently at init; the ~1.3 ms steady component is the
known host-sensitive gap. Neither belongs to this investigation.
