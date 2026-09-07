# Performance-record provenance audit — 2026-09-07

## Purpose and non-scope

A **survey**. For every document in this repository carrying performance
figures anyone might cite, it records four things: when the measurement was
taken and at which commit, which side of the `panic = "abort"` window it
sits on, what configuration produced it, and whether raw captures behind it
were found by the searches described below.

**It corrects nothing.** Where the survey observes a discrepancy it is stated
as an observation with no proposed fix. The corrections this map implies are
separate work with separate review — including the correction to
`perf-backend-matrix-2026-09.md`, which this audit was partly commissioned to
inform and which is deliberately not attempted here.

## The search surface, and what it can support

**Superseding `caabdc27`.** That commit searched one host's home tree plus two
named default paths — `/tmp/matrix-out` and `/tmp/perf-out` — called the search
exhaustive, and reported that exactly one of 22 records retained raw captures.
The search was not exhaustive and the count did not follow from it. The
superseded claim stays in history; this section replaces it.

The defect was the surface, not the number. So the surface comes first.

**Filename set.** `speed.jsonl`, `ram-init.jsonl`, `ram-cycle.jsonl` — the
three files `bisect --perf` writes through `run-perf-matrix.sh` and
`run-perf-same-data.sh`.

**A filename search cannot establish survival for this corpus, because the
corpus was not produced by one harness.** These three names do not reach
criterion output, Callgrind output, or the `scoreboard` and
`perf-init-slurp-*` directory forms that the W8 profile scripts write. Four
of the trees recorded below were found *by other means* — by reading the
documents and looking where they said to look — not by this search. Any
record produced by a harness outside these three filenames is invisible to
the search regardless of whether its captures exist. That is a property of
the method, and it bounds every statement here.

**arm64 search.** `find / -xdev \( -name speed.jsonl -o -name ram-init.jsonl
-o -name ram-cycle.jsonl \)`, whole local filesystem, ~45 s. **Not
exhaustive:** 1,648 paths were unreadable — 1,279 "Operation not permitted"
(macOS TCC/SIP) and 369 "Permission denied" — concentrated under
`/System/Volumes` (796), `/Users/shenghaochen` (510), `/private/var` (262)
and `/System/Library` (69). Those paths were not searched.

**arm64 container surface.** No podman is installed. Docker Desktop keeps
container filesystems inside a VM disk image that is not searchable from the
host, so a capture written inside a container and never bind-mounted out is
invisible to this search. Every measurement run in this workstream did
bind-mount its output — but that is a property of those runs, not of the
search, and it establishes nothing about runs made by anyone else.

**x86_64 search.** The same filename set over that host, plus its podman
surfaces. Root-owned paths returned `EACCES` and were not searched.

### Survival

Corpus records first, then trees outside the corpus. The attribution column
distinguishes what was checked against the published document on the machine
writing this from what is accepted on the other host's report; the two are not
the same and are not merged.

| record | host | captures | coverage | attribution |
|---|---|---|---|---|
| `perf-baseline-kc-2026-09.md` | arm64 | `~/oxp-perf-b5fdfad/out` | full; per-commit arms including `a41605ea` and `noabort` | verified locally |
| `perf-keycost-first-alloc-2026-09-07.md` | arm64 | `/private/tmp/perf-out-{parent,head}` | both passes; all 8 published session-ready RSS values reproduce exactly | verified locally |
| `perf-backend-matrix-2026-09.md` | x86_64 | `~/matrix-x86/out` | 80 speed rows at `b5fdfad8` | medians confirmed present in the document here; reproduction accepted on report |
| `perf-init-typed-map-2026-08.md` | x86_64 | `target/profile/{before,after}/scoreboard` | both arms | accepted on report |
| `perf-init-text-slurp-2026-08.md` | x86_64 | four `target/perf-init-slurp-*` | **partial** — 4 of 5 arms; the `before 2` arm (init 232.528 ms) has no survivor | arm structure confirmed here; survival accepted on report |
| `perf-baseline-kc-2026-08-31.md` | x86_64 | `/tmp/alloc-perf/out` | **partial** — the x86_64 amendment passage only, not the Apple-silicon body | passage confirmed present here; values accepted on report |
| *outside the corpus* — 2026-09-07 cross-host record | arm64 | `~/oxp-steady-cycle-arm64/out` | 20 JSONL over five workload sizes | verified locally |
| *outside the corpus* — same record, other host | x86_64 | amd64 cross-host tree | — | accepted on report |
| *unattributed* | arm64 | `…/worktrees/separate-libpinyin-libzhuyin-7acdbb/target/perf-baseline` | 40 speed rows, `oracle`/`oxpinyin`, 2026-09-04T12:55Z | no record attributed |
| *unattributable* | x86_64 | two `ibus-libpinyin/build/…` trees | same era and scale as `perf-baseline-2026-08.md`, but no pooling reproduces its medians | accepted on report |

### The count

**At least six of the 22 figure-bearing records retain raw captures**, two of
those six only partially.

"At least" is the strongest form the surface supports. It is a lower bound,
not a total: both hosts left unreadable paths unsearched, neither reached
container-internal filesystems, and the filename set does not cover every
harness that produced this corpus. **No record may be called unbacked on the
strength of this search** — only "no captures found by these two searches".

A figure whose captures are genuinely gone cannot be re-derived; it can only
be **re-measured**, which is a different operation producing a different
number on a different day. Re-measurement is further constrained by a known
instrument behaviour: a whole-session offset of 9–11% co-moving across all
four cells has been observed on the arm64 host between two sessions running
the same harness on the same machine. **Absolute agreement is therefore not a
valid verification criterion** for any re-measurement, and some of what the
map below records as configuration difference may be that offset instead.

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

Other records now also have surviving captures (see Survival above), so this
tree is no longer the corpus's only re-derivable data. What remains unique to
it is the **paired arms**: it is the only tree holding both an `a41605ea` and
a `noabort` measurement of the same series, which makes the `panic = "abort"`
delta itself re-derivable rather than merely re-measurable — and that delta is
precisely the correction factor every in-window record would need. This audit
does not re-derive it; that is separate work. What matters here is that the
material exists and where it lives.

**It is unbacked.** It sits in one home directory on one laptop, outside the
repository, with no copy anywhere. One `rm -rf` and it joins the other
twenty-one. Whoever takes up the in-window corrections should copy it before
doing anything else.

## Provenance map

### Method for the commit, configuration and harness columns

These three columns were first filled by first-match pattern extraction, which
under-reported all three; they have since been rebuilt by **reading each
document**. What that method establishes and what it does not:

- It establishes **what a document states**, which is not the same as what
  ran. Where configuration is given as a value it is reproduced here.
- It does **not** resolve delegation by reading the citing document alone.
  Roughly half this corpus does not state configuration — it *points* at it,
  in three distinguishable kinds tabulated under Delegation below. A cell
  reading `→ D<n>` is a pointer, not a value, and is deliberately not
  flattened into one: the pointing is a structural property of the corpus,
  and collapsing it would hide it inside the artifact meant to surface it.
- A delegation to a **script** is only resolvable by reading that script *at
  the record's date*. No document in this corpus pins the revision of the
  script it delegates to.
- A delegation to an **unstated prior run** is not resolvable from the
  document at all, and is marked as such rather than filled in.

Multi-host records are shown as multi-host. Two exist, and both were
previously mapped to one host.

`side` is relative to the `a41605ea` window. `captures` reports what the two
searches described above found: **yes** / **partial** where a tree was located
and attributed, **none found** otherwise. "None found" is not "none exists" —
see the count, which the surface supports only as a lower bound.

| record | measurement date | at commit | side | configuration | harness | captures |
|---|---|---|---|---|---|---|
| `findings/perf-backend-matrix-2026-09.md` | 2026-09-05 | `b5fdfad8` (2026-09-05T12:23:49Z) | **INSIDE** | x86_64, i7-9750H | `bisect --perf` | **yes** (x86_64) |
| `perf/perf-baseline-kc-2026-09.md` | 2026-09-04, amended 2026-09-05 | `94b38948` (2026-09-04T00:45:03Z), amendment spans the window | **SPANS** | ARM64, Apple silicon | `bisect --perf` | **yes** (arm64) |
| `perf/perf-so-size-2026-09.md` | 2026-09-04 | `bf83ffb9` (x86_64 arms); facade tip `b40e3542`; deliberate `a41605ea` arm | **STRADDLES** | **two hosts** — x86_64/redb (Linux EL10) and ARM64/KC re-measurement | criterion | none found |
| `findings/perf-store-opt-2026-09.md` | 2026-09-06 | `ab56dc79` (2026-09-05T15:41:34Z) | after | x86_64 | criterion | none found |
| `findings/perf-backend-matrix-bdb-store-2026-09.md` | 2026-09-06 | not stated | after | → D6 | `bisect --perf` | none found |
| `findings/perf-keycost-first-alloc-2026-09-07.md` | 2026-09-07 | parent `87f25055` → `6886dc1f` | after | arm64, Apple silicon | `run-perf-same-data.sh` + `bisect --perf` | **yes** (arm64) |
| `findings/runtime-direct-libpinyin-data-2026-09-02.md` | 2026-09-02 | not stated | before | not stated | not stated | none found |
| `findings/perf-p2-chewing-table-2026-09-01.md` | 2026-09-01 | not stated | before | mini fixtures; KC/Tkrzw measured off-CI | not stated | none found |
| `findings/perf-backend-matrix-2026-08-31.md` | 2026-08-31 | not stated | before | Apple silicon | `bisect --perf` | none found |
| `findings/perf-baseline-kc-2026-08-31.md` | 2026-08-31 | not stated | before | **two hosts** — Docker Desktop on Apple silicon (body); podman 5.8.2 on RHEL 10.2, x86_64, 12 vCPU (2026-09-03 amendment) | `bisect --perf` | **partial** — x86_64 passage only |
| `findings/perf-baseline-kc-validation-2026-08-31.md` | 2026-08-31 | `15e1b47` | before | Apple silicon, arm64 | `run-perf-baseline.sh` | none found |
| `findings/perf-mmap-system-indexes-2026-08-31.md` | 2026-08-31 | not stated | before | → D5 | not stated (status: REJECTED) | none found |
| `perf/perf-python-shared-engine-2026-08.md` | 2026-08-27 | `f2eedd7` (branch `feat/python-api`) | before | not stated | not stated | none found |
| `perf/perf-init-text-slurp-2026-08.md` | 2026-08-21 | before `6b476b1` | before | → D3 | `run-w8-cycle.sh` **and** `run-perf-baseline.sh` (`PERF_RUNS=20 PERF_CYCLES=8 PERF_RAM_RUNS=10 PERF_CPU=3`) | **partial** — 4 of 5 arms (x86_64) |
| `perf/perf-init-typed-map-2026-08.md` | 2026-08-21 | before `5f9bc7f` | before | → D2 | `run-w8-cycle.sh` | **yes** (x86_64) |
| `perf/perf-fill-lookup-2026-08.md` | 2026-08-20 | before `337d8d5` (origin/main) | before | → D1 | `run-w8-cycle.sh` (`PERF_CYCLES=8`, `--profile profiling`, Callgrind) | none found |
| `findings/data-load-audit-2026-08.md` | 2026-08-19 | not stated | before | i7-9750H, 12 logical CPUs, Linux 6.12, rustc 1.97.1 | `run-perf-baseline.sh` | none found |
| `perf/perf-alloc-2026-08.md` | 2026-08-19 | not stated | before | → D4 | criterion + dhat | none found |
| `perf/perf-stage2-harness-2026-08.md` | 2026-08-19 | not stated | before | not stated | criterion 0.8 + `run-w8-cycle.sh` | none found |
| `perf/perf-baseline-2026-08.md` | 2026-08-16 | not stated | before | i7-9750H @ 2.60 GHz, 12 logical CPUs, pinned to CPU 3, Linux 6.12, rustc 1.97.1, cargo-c 0.10.24 | `bisect --perf` | none found |
| `perf/perf-candidate-cap-2026-08.md` | 2026-08-16 (see below) | `f8e2c11d` | before | → D7 | criterion (`scan_perf`) | none found |
| `perf/perf-exploration.md` | 2026-08-14 | `017a610` (PR #46) | before | 12 logical cores, one measurement thread unless noted, rustc 1.97.1; Callgrind `Ir` + dhat 0.3 | criterion + Callgrind + dhat | none found |

## Delegation

Seven records do not state their configuration; they point at it. The kind of
pointer determines whether reading can resolve it.

| id | record | what it says | kind | resolved? |
|---|---|---|---|---|
| D1 | `perf-fill-lookup-2026-08.md` | "same W8 8-cycle Callgrind path as #121 (`tools/profile/run-w8-cycle.sh`, `PERF_CYCLES=8`, `--profile profiling`)" | script | **yes** — see below |
| D2 | `perf-init-typed-map-2026-08.md` | "same W8 protocol as #129/#121 — `tools/profile/run-w8-cycle.sh`" | script + document | **yes** — see below |
| D3 | `perf-init-text-slurp-2026-08.md` | "same W8 protocol as #132/#129 — `run-w8-cycle.sh`… `run-perf-baseline.sh` dlopen scoreboard" | script + document | **yes** — see below |
| D4 | `perf-alloc-2026-08.md` | "same crate-local Criterion/dhat harness as `docs/perf/perf-exploration.md`" | document | yes, by chaining to that document's stated host |
| D5 | `perf-mmap-system-indexes-2026-08-31.md` | "Continues `perf-backend-matrix-2026-08-31.md`" | document | yes, by chaining — the target states Apple silicon |
| D6 | `perf-backend-matrix-bdb-store-2026-09.md` | "extends the post-P6 4-cell matrix… same pin, same host family" | document | yes, by chaining to `perf-backend-matrix-2026-09.md` (x86_64, i7-9750H) |
| D7 | `perf-candidate-cap-2026-08.md` | "Same machine, same `PINYIN_EXPORT_DIR` / `PINYIN_MODEL_DIR`, back-to-back" | unstated prior run | **no** — no antecedent exists in the document |

### Resolving D1–D3: was the delegated-to harness stable?

The three records claim mutual comparability through one script. Whether that
holds is a question to answer, not assume, because the script has its own
history: created `e5f9fbf1` (2026-08-20), then `af642b91` (2026-08-20),
`b7a35f1f` (2026-08-23), `871139a1` (2026-09-07). One of those lands *between*
the records — D1 is dated 2026-08-20, D2 and D3 2026-08-21.

`af642b91` was read rather than assumed. In the script it adds model-cache
verification and a `PINYIN_MODEL_CACHE` fallback for `MODEL_DIR`. The same
commit also touches the measurement code — `benches/support/mod.rs` (+127) is
that verification mirrored in, and `benches/stage2.rs` is a bench-identifier
rename with a byte-identical body. **No measurement behaviour changed**, so
the three records' "same W8 protocol" claim holds. `b7a35f1f` removed a
dangling `oxpinyin-migrate` line and is cosmetic.

Two consequences follow anyway, and they are recorded as observations 7 and 8:
`871139a1` changed the script's `PIN_REF` to a different libpinyin pin, so the
delegation is stable *among* these records but not forward to today; and the
bench rename means the same measurement appears under two identifiers across
the 2026-08-20/21 boundary.


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

4. **Configuration is stated, delegated, or absent — in roughly equal
   parts.** After per-document reading: eleven records state a host or ISA
   directly, seven delegate (see Delegation), and four state nothing. Two
   hosts recur (Apple silicon arm64, i7-9750H x86_64) and **two records are
   multi-host** — `perf-baseline-kc-2026-08-31.md` (Apple-silicon body, an
   x86_64/RHEL amendment) and `perf-so-size-2026-09.md` (x86_64/redb arms, an
   ARM64/KC re-measurement). Both were previously mapped to a single host.
   Per the session-offset observation above, cross-record comparison of
   absolutes is unsafe even within one host.

5. **Measurement commit is stated in twelve records of twenty-two**, not the
   six the pattern found. The additions are `017a610`, `337d8d5`, `5f9bc7f`,
   `6b476b1`, `15e1b47` and `f2eedd7` — several of them the "before" commit of
   a before/after pair, which is still a temporal anchor. For the remaining
   ten the only anchor is a document date, which is when the note was written,
   not necessarily when the measurement ran.

6. **`perf-candidate-cap-2026-08.md` carries no internal date.** Its title
   says only "(2026-08)". The 2026-08-16 above is **established from git**,
   not inferred: `git log --follow` traces the file through the 2026-08-31
   docs reorganisation (`1ead580d`) back to its introducing commit
   `f8e2c11d`, "fix(engine): remove the candidate-list cap", 2026-08-16. Had
   git not settled it, this row would read "undated"; an inferred date in a
   provenance audit is the defect the audit exists to find.

7. **A live cross-reference silently changed meaning.** Three records say
   they used "the same W8 protocol" and delegate to
   `tools/profile/run-w8-cycle.sh`. On 2026-09-07, `871139a1` changed that
   script's `PIN_REF` from `libpinyin-2.11.91-0c5e80e1…` to
   `libpinyin-2.11.92-074a2219…`. The sentences in those documents did not
   change, still read as assurances, and now point at a **different oracle**
   than the one they were written about. This is a distinct defect class from
   a wrong figure or a missing column: not an error at the time of writing,
   but a reference whose truth value changed underneath it. No document in
   this corpus pins the revision of the file it delegates to, so the class is
   live wherever delegation occurs — which, per the Delegation table, is seven
   records. **Nobody will look for this unless it has a name**, and the audit
   had not named it before this pass.

8. **And its inverse: a name changed while the measurement did not.**
   `af642b91` renamed a criterion bench from `guess_sentence_get_sentence_0`
   to `guess_sentence_get_sentence_0/full_nbest_post_116` with a
   byte-identical body. A reader diffing bench identifiers across the
   2026-08-20/21 boundary would conclude the measurement changed; it did not.
   Taken with observation 7, identity and reference drift in opposite
   directions, and both are invisible to anyone reading a single document.

9. **The three rebuilt columns had the same weakness, and it is now fixed
   rather than deferred.** `configuration`, `harness` and `at commit` were
   first filled by first-match pattern, which cannot see what it was not told
   to look for — the same failure as the search surface this document already
   replaced, in three more columns. What the pattern got wrong, now corrected
   by reading: `perf-baseline-kc-2026-08-31.md` mapped to "Apple silicon"
   alone when its own amendment environment records podman 5.8.2 on RHEL 10.2,
   x86_64; `perf-so-size-2026-09.md` mapped to x86_64 alone when it carries an
   ARM64/KC re-measurement; `perf-baseline-2026-08.md` and
   `perf-exploration.md` mapped "not stated" while both state a host;
   `perf-init-text-slurp-2026-08.md` credited with one harness when it names
   two; and six measurement commits missed entirely. The columns are no longer
   a lower bound on what the documents state — they are what the documents
   state, with delegation shown as delegation.

10. **`perf-mmap-system-indexes-2026-08-31.md` is marked REJECTED** —
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

It **cannot verify figures** for those records whose captures were not
found — a set bounded below by the six in Survival above and not established
as any particular size, because the search surface does not support a total.
Such a record can only be re-measured, and
re-measurement cannot be checked against the old absolutes: the 9–11%
session offset means a faithful re-run will disagree with a correct original.
A correction programme built on this map should treat re-measurement as
producing a new record with its own provenance, not as validating an old one.
