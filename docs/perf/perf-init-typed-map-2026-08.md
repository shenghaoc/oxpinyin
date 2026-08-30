# Stage-2 leftover: init typed-map insert (2026-08-21)

Status: **measured init-path change.** Continues
`docs/perf/perf-fill-lookup-2026-08.md` (which left the typed-map
insert as a note) and #129's typed maps
(`docs/findings/data-load-audit-2026-08.md`). This PR is
`oxpinyin-data/src/dict.rs` only. The keystroke path
(`fill_lookup`) keeps its #121 shape — one get plus
`extend_from_slice` — and only the get's backing store changed.
`key_cost_table`, `scheme.rs`, the redb format, `LookupTable`, the
u32-keyed `phrase_index` / `unigrams` maps, and the LM/interp loaders are
untouched.

Parity pins unchanged (STOP line at the end). No merge on this branch
yet.

Host: same W8 protocol as #129/#121 — `tools/profile/run-w8-cycle.sh`
(`PERF_CYCLES=8`, `--profile profiling` cargo-c install, Callgrind
fallback at `perf_event_paranoid=2`), `tools/bisection/run-perf-baseline.sh`
dlopen scoreboard (`PERF_RUNS=20 PERF_CYCLES=8 PERF_RAM_RUNS=10`,
`PERF_CPU=3`, installed-`.so` via `pkg-config oxpinyin`, alternating vs
the pinned oracle `libpinyin.so`), and `load_profile` over
`/tmp/oxpinyin-export` + the verified model20 cache. Before = `5f9bc7f`
(origin/main), after = this branch, same day, same host (shared; the
alternating design and the deterministic Callgrind columns carry the
comparison). rustc 1.97.1, valgrind 3.26.0.

## Why: the leftover the profiles named

Post-#129/#121 the W8 8-cycle Callgrind still showed
`__memcmp_avx2_movbe` at 249.0e6 self-`Ir` (10.75%), and the caller tree
attributed **83.5e6 of it (3,768,186 calls) to
`BTreeMap<Box<str>, Box<[(u32, u32)]>>::insert`** inside
`load_pinyin_index`'s row closure — ~40 key compares per row over 93,349
rows — plus the `FromIterator` sort's 2.0e6. That is the "init typed-map
insert" the task names. The rest of the memcmp (`syllable_initial`
66.5e6, `SyllableKey::from_option_text` 36.8e6, `initial_keys` sort
~30e6, keystroke key-get 10.1e6) is not this PR.

Dict-only Callgrind before: PROGRAM TOTALS 1,084.0e6, memcmp 156.4e6
(14.43%, #1), `BTreeMap<Box<str>, …>::insert` self 95.2e6 (8.78%).
Median-of-5 `SystemDictionary::open` 201.1 ms.

## What changed

- `PinyinIndex` is a **sorted vector map**
  (`Vec<(Box<str>, Box<[PhraseEntry]>)>`) instead of
  `BTreeMap<Box<str>, Box<[PhraseEntry]>>`. redb 4.1 `table.iter()` is
  `range(..)` over its B-tree — ascending primary-key walk — so
  `load_pinyin_index` **appends** each row instead of inserting: the
  O(n log n) per-row compare pass is gone, and the
  `into_iter().map().collect()` pass becomes an in-place map over the
  vec (no second tree build).
- `ensure_sorted_unique` verifies the walk order in one O(n) compare
  pass and, if a table ever arrived otherwise, sorts and keeps the
  **last** row per key — the value `BTreeMap::insert` would have left.
  On well-formed tables the repair never runs; it exists so binary
  search never silently sees misorder or duplicates. Unit-tested on the
  repair path (redb cannot produce that input, by primary-key
  uniqueness).
- Lookups: `index_hits` (binary search get) for `fill_lookup`;
  `pinyin_prefix_exists` via `partition_point` — first key ≥ joined,
  then the same exact/boundary-extension checks the tree's
  `contains_key` + `range` pair made. Both stay O(log n) key compares;
  iteration order (`pronunciations`) is unchanged.

Complexity: init insert compares O(n log n) → O(n); lookups unchanged
class; retained space a contiguous 4-word slot per key vs tree nodes.
Time and space both improve; nothing trades off.

## Numbers

Dict-only load (`load_profile dict`, release, same export):

| | before (`5f9bc7f`) | after | after/before |
|---|---:|---:|---:|
| Callgrind PROGRAM TOTALS | 1,084.0e6 | 869.5e6 | **0.80×** |
| `__memcmp_avx2_movbe` self | 156.4e6 (14.43%) | 73.6e6 (8.47%) | **0.47×** |
| `BTreeMap<Box<str>, …>::insert` self | 95.2e6 (8.78%) | *(symbol gone)* | — |
| `SystemDictionary::open` (median of 5) | 201.1 ms | 177.7 ms | **0.88×** |
| after-open RSS / HWM | 38,848 / 42,844 KiB | 37,076 / 38,092 KiB | −1.75 / −4.65 MiB |

(The transient peak drops more than the resident set: the raw insert map
and the collected tree no longer coexist during `collect`.)

W8 8-cycle Callgrind (profiling `.so`, init + 8 cycles):

| | before | after | after/before |
|---|---:|---:|---:|
| PROGRAM TOTALS | 2,317.0e6 | 2,089.9e6 | **0.90×** |
| `__memcmp_avx2_movbe` self | 249.0e6 (10.75%) | 158.0e6 (7.56%) | **0.63×** |
| insert → memcmp | 83.5e6 (3,768,186×) | *(symbol gone)* | — |
| `fill_lookup` → memcmp (keystroke key-get) | 10.1e6 (459,365×) | 6.8e6 (314,784×) | **0.67×** |

The keystroke get also got cheaper: a binary-search probe does one
memcmp per level against a contiguous vec, vs the tree's per-node scans
— 459k memcmp invocations down to 315k over the same cycle count.

W8 dlopen scoreboard (20 speed runs, 10 RAM runs per mode, alternating
oracle/oxpinyin, CPU 3):

| | before | after |
|---|---:|---:|
| `pinyin_init` oxpinyin | 326.665 ms [310.499, 454.432] | **268.944 ms [251.884, 308.487]** (0.82×) |
| `pinyin_init` oracle | 1.860 ms | 1.591 ms |
| init ratio | 175.6× | 169.1× |
| `pinyin_alloc_instance` | 5.431 ms | 4.675 ms (0.86×) |
| steady cycle | 19.768 ms (0.767× oracle) | 18.214 ms (0.735× oracle) |
| cold cycle | 19.398 ms (0.695× oracle) | 16.551 ms (0.663× oracle) |
| post-init RSS | 81,318 KiB (6.76×) | **79,524 KiB (6.63×)**, RssAnon 75,744 |
| lifetime peak HWM | 81,446 KiB (5.15×) | **79,776 KiB (5.04×)** |

The oracle's own init moved 1.86 → 1.59 ms between the two runs (shared
host), so the ratio column is soft; the absolute −57.7 ms oxpinyin init
and the deterministic Callgrind columns are the honest deltas.

## Parity (STOP line)

`real_tables_session_reports_parity` / `sentence_surface_reports_parity`
(`cargo test --locked --release -p pinyin-oracle --test
real_tables_integration`):

```text
top-1 10177 / top-5-set 10189 / prefix-10 94871 of 98930 / absent 1 / tie-swaps 1036
sentence 488 / 385 / 370
```

Unchanged. Full `cargo test --locked --workspace` green; fmt and clippy
clean. Pins did not move.

## Residual (not this pass)

- `syllable_initial` prefix compare (66.5e6 memcmp on the 8-cycle) and
  `SyllableKey::from_option_text` (36.8e6) — core-crate surfaces, out of
  this PR's scope.
- `initial_keys` `sort_unstable` + `dedup` (~30e6 memcmp): a separate
  structure (the incomplete-index probe list), not the typed map.
- The 158×-class init story is still `interpolation2.text` parsing and
  the redb slurp (the scoreboard observation-1 representation change);
  this PR only removes the compare tax #129 left on the slurp.
