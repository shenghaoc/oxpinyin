# Performance-record provenance audit — 2026-09-07

## Purpose and non-scope

A **survey**. For every document in this repository carrying performance
figures anyone might cite, it records four things: when the measurement was
taken and at which commit, which side of the `panic = "abort"` window it
sits on, what configuration produced it, and whether the raw captures behind
it still exist anywhere.

**It corrects nothing.** Where the survey observes a discrepancy it is stated
as an observation with no proposed fix. The corrections this map implies are
separate work with separate review — including the correction to
`perf-backend-matrix-2026-09.md`, which this audit was partly commissioned to
inform and which is deliberately not attempted here.

## The headline: the raw captures are gone

**Of the 22 figure-bearing records surveyed, exactly one still has its raw
captures.** For the other 21 the published table is the only surviving
artifact.

This governs how much weight the corpus can carry, independently of what the
provenance map below says about any individual record. A figure whose
captures are gone cannot be re-derived — it can only be **re-measured**, which
is a different operation producing a different number on a different day.

The search was exhaustive on the measuring host: every `speed.jsonl` /
`ram-cycle.jsonl` under the home tree, plus the harness scripts' default
`/tmp/matrix-out` and `/tmp/perf-out` (reaped; `/private/tmp` is cleared by
the system). Two capture trees survive:

| tree | backs | contents |
|---|---|---|
| `~/oxp-perf-b5fdfad/out` | `docs/perf/perf-baseline-kc-2026-09.md` | 49 JSONL files; per-commit subtrees under `pts/`, `pts3/`, `pts4/` |
| `~/oxp-steady-cycle-arm64/out` | the 2026-09-07 cross-host record (open PR, not in main — outside this corpus) | 20 JSONL files across five workload sizes |

Re-measurement is further constrained by a known instrument behaviour: a
whole-session offset of 9–11% co-moving across all four cells has been
observed on the arm64 host between two sessions running the same harness on
the same machine. **Absolute agreement is therefore not a valid verification
criterion** for any re-measurement, and some of what this map records as
configuration difference may be that offset instead.

## The `a41605ea` window

Release `panic = "abort"` was adopted and reverted inside one day. Measured
cost on ARM64/KC, from the record: +5.5–5.7% steady cycle, +5.3–6.4% cold.

| | commit | committed | UTC |
|---|---|---|---|
| adopted | `a41605ea` | 2026-09-05T02:53:16+08:00 | **2026-09-04T18:53:16Z** |
| reverted | `1b0c84a0` | 2026-09-05T21:45:47+08:00 | **2026-09-05T13:45:47Z** |

An **18 h 52 min** window. `5e630ed0` is a pre-rebase twin of the revert and
is not a second event. Every record dated before 2026-09-04 is outside the
window by date alone; only the 2026-09 records needed commit-level
resolution, and one of them turns on minutes (see `perf-so-size-2026-09.md`).

## The one re-derivable figure — and it is perishable

`~/oxp-perf-b5fdfad/out` retains per-commit capture subtrees for a whole
series, **including both an `a41605ea` arm and a `noabort` arm**:

```
out/pts3/{77c3fb78, 8147b7d7, 828e2033, 87f9a49e, a41605ea, b40e3542}/perf/
out/pts4/{b5fdfad8, noabort}/perf/
out/pts/{77c3fb78, 8147b7d7, 828e2033, 87f9a49e, 94b38948, a41605ea, b40e3542}/perf/
```

each holding `speed.jsonl`, `ram-init.jsonl`, `ram-cycle.jsonl`.

This makes the `panic = "abort"` delta **the only figure in the corpus that
can be independently re-derived from raw data rather than re-measured** — and
it is precisely the correction factor every in-window record would need. This
audit does not re-derive it; that is separate work. What matters here is that
the material exists and where it lives.

**It is unbacked.** It sits in one home directory on one laptop, outside the
repository, with no copy anywhere. One `rm -rf` and it joins the other
twenty-one. Whoever takes up the in-window corrections should copy it before
doing anything else.

## Provenance map

`side` is relative to the `a41605ea` window. `captures` is whether raw JSONL
behind the figures survives anywhere on the measuring host.

| record | measurement date | at commit | side | configuration | harness | captures |
|---|---|---|---|---|---|---|
| `findings/perf-backend-matrix-2026-09.md` | 2026-09-05 | `b5fdfad8` (2026-09-05T12:23:49Z) | **INSIDE** | x86_64, i7-9750H | `bisect --perf` | no |
| `perf/perf-baseline-kc-2026-09.md` | 2026-09-04, amended 2026-09-05 | `94b38948` (2026-09-04T00:45:03Z), amendment spans the window | **SPANS** | ARM64, Apple silicon | `bisect --perf` | **yes** |
| `perf/perf-so-size-2026-09.md` | 2026-09-04 | facade tip `b40e3542` (2026-09-04T18:19:07Z) plus a deliberate `a41605ea` arm | **STRADDLES** | x86_64 | criterion | no |
| `findings/perf-store-opt-2026-09.md` | 2026-09-06 | `ab56dc79` (2026-09-05T15:41:34Z) | after | x86_64 | criterion | no |
| `findings/perf-backend-matrix-bdb-store-2026-09.md` | 2026-09-06 | not stated | after | storage backends | `bisect --perf` | no |
| `findings/perf-keycost-first-alloc-2026-09-07.md` | 2026-09-07 | parent `87f25055` → `6886dc1f` | after | arm64, Apple silicon | `run-perf-same-data.sh` + `bisect --perf` | no |
| `findings/runtime-direct-libpinyin-data-2026-09-02.md` | 2026-09-02 | not stated | before | not stated | not stated | no |
| `findings/perf-p2-chewing-table-2026-09-01.md` | 2026-09-01 | not stated | before | mini fixtures; KC/Tkrzw measured off-CI | not stated | no |
| `findings/perf-backend-matrix-2026-08-31.md` | 2026-08-31 | not stated | before | Apple silicon | `bisect --perf` | no |
| `findings/perf-baseline-kc-2026-08-31.md` | 2026-08-31 | not stated | before | Apple silicon | `bisect --perf` | no |
| `findings/perf-baseline-kc-validation-2026-08-31.md` | 2026-08-31 | not stated | before | Apple silicon, arm64 | `run-perf-baseline.sh` | no |
| `findings/perf-mmap-system-indexes-2026-08-31.md` | 2026-08-31 | not stated | before | not stated (status: REJECTED) | not stated | no |
| `perf/perf-python-shared-engine-2026-08.md` | 2026-08-27 | not stated | before | not stated | not stated | no |
| `perf/perf-init-text-slurp-2026-08.md` | 2026-08-21 | not stated | before | not stated | `run-perf-baseline.sh` | no |
| `perf/perf-init-typed-map-2026-08.md` | 2026-08-21 | not stated | before | not stated | `run-perf-baseline.sh` | no |
| `perf/perf-fill-lookup-2026-08.md` | 2026-08-20 | not stated | before | not stated | not stated | no |
| `findings/data-load-audit-2026-08.md` | 2026-08-19 | not stated | before | i7-9750H | `run-perf-baseline.sh` | no |
| `perf/perf-alloc-2026-08.md` | 2026-08-19 | not stated | before | not stated | not stated | no |
| `perf/perf-stage2-harness-2026-08.md` | 2026-08-19 | not stated | before | not stated | not stated | no |
| `perf/perf-baseline-2026-08.md` | 2026-08-16 | not stated | before | not stated | not stated | no |
| `perf/perf-candidate-cap-2026-08.md` | 2026-08-16 (see below) | `f8e2c11d` | before | not stated | criterion (`scan_perf`) | no |
| `perf/perf-exploration.md` | 2026-08-14 | not stated | before | not stated | not stated | no |

## Observations

Stated as observations. No fix is proposed for any of them here.

1. **The most-cited 2026-09 record sits inside the window.**
   `perf-backend-matrix-2026-09.md` was measured at `b5fdfad8`, which is
   1 h 22 min inside the window. The document says so itself in three places
   (`:25`, `:55`, `:234`) and names the ARM64-measured cost of the policy. So
   its oxpinyin figures carry a penalty its libpinyin comparison side does
   not. What that implies for the readings drawn from it is not this survey's
   to say.

2. **One record turns on minutes, not days.** `perf-so-size-2026-09.md`'s
   facade tip `b40e3542` is committed at 2026-09-04T18:19:07Z — **34 minutes
   before** the window opens. No date-granularity check resolves that; it
   needed commit timestamps. The document also carries a deliberate
   `a41605ea` arm, since measuring the policy was its subject, so it
   straddles the boundary by design rather than by accident.

3. **Harness revision is essentially never recorded.** Not one record in the
   corpus pins the harness to a commit. Ten name a script or `criterion`; the
   rest name nothing. `bisect.c` and the runner scripts changed repeatedly
   over the surveyed period, so "measured with `bisect --perf`" does not
   identify what was run.

4. **Configuration is recorded unevenly.** Nine records name a host or ISA;
   thirteen name nothing beyond the figures. Two hosts appear across the
   corpus (Apple silicon arm64, i7-9750H x86_64), and per the session-offset
   observation above, cross-record comparison of absolutes is unsafe even
   within one host.

5. **Measurement commit is stated in six records of twenty-two.** For the
   rest, the only temporal anchor is a document date, which is when the note
   was written, not necessarily when the measurement ran.

6. **`perf-candidate-cap-2026-08.md` carries no internal date.** Its title
   says only "(2026-08)". The 2026-08-16 above is **established from git**,
   not inferred: `git log --follow` traces the file through the 2026-08-31
   docs reorganisation (`1ead580d`) back to its introducing commit
   `f8e2c11d`, "fix(engine): remove the candidate-list cap", 2026-08-16. Had
   git not settled it, this row would read "undated"; an inferred date in a
   provenance audit is the defect the audit exists to find.

7. **`perf-mmap-system-indexes-2026-08-31.md` is marked REJECTED** —
   architecture withdrawn, kept as a record. Its figures should not be cited
   as current regardless of provenance.

## Corpus boundary

Selection was by **content, not filename**: every tracked `docs/**/*.md`
carrying three or more timing or footprint figures. That is what pulled in
`findings/data-load-audit-2026-08.md` (36 figures) and
`findings/runtime-direct-libpinyin-data-2026-09-02.md` (11), neither of which
a `perf-*` filename convention would have caught.

Three documents were checked and **excluded, having no performance figures at
all**:

- `findings/build-flags-audit.md` — a configure-flag/ABI mapping table.
- `findings/backend-selection-audit.md` — zero matches for
  faster/slower/perf/benchmark/measur.
- `findings/matrix-split-tables.md` — no timing or footprint figures.

They are listed rather than silently dropped so the boundary is visible as
tested rather than assumed.

## What this audit can and cannot conclude

It establishes **provenance metadata** — date, commit, window side,
configuration as recorded, harness as named — because that lives in the
documents and in git, and it is verifiable today.

It **cannot verify figures** for 21 of 22 records, because the raw captures
do not exist. Any record it flags can only be re-measured, and
re-measurement cannot be checked against the old absolutes: the 9–11%
session offset means a faithful re-run will disagree with a correct original.
A correction programme built on this map should treat re-measurement as
producing a new record with its own provenance, not as validating an old one.
