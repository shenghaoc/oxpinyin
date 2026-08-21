# Stage-2 init cut: interpolation2.text parse + redb slurp (2026-08-21)

Status: **measured init-path change.** The task card named the two
leftovers #132 left behind — `interpolation2.text` parsing and redb table
materialization — and this PR is only those. `key_cost_table`,
`scheme.rs`, `loses_to`, apostrophe handling, n-best, the redb on-disk
format, and the #118 reverse-map policy are untouched. The keystroke path
is unchanged: `fill_lookup`'s memcmp call count and its cost are
bit-identical (tables below).

Parity pins unchanged (STOP line at the end). No merge on this branch.

Host: same W8 protocol as #132/#129 — `tools/profile/run-w8-cycle.sh`
(`PERF_CYCLES=8`, `--profile profiling` cargo-c install, Callgrind at
`perf_event_paranoid=2`), `tools/bisection/run-perf-baseline.sh` dlopen
scoreboard (`PERF_RUNS=20 PERF_CYCLES=8 PERF_RAM_RUNS=10`, `PERF_CPU=3`,
installed-`.so` via `pkg-config oxpinyin`, alternating vs the pinned
oracle `libpinyin.so`), and `load_profile` over `/tmp/oxpinyin-export` +
the verified model20 cache. Before = `6b476b1` (origin/main, post-W12),
after = this branch, same day, same shared host. rustc 1.97.1, valgrind
3.26.0.

## Why: the profile named exactly those two

Full capi-shaped init, Callgrind on `load_profile full` (release), before:
PROGRAM TOTALS **1,164.3e6 Ir**. Inclusive attribution by init component:

| component | Ir | share |
|---|---:|---:|
| pinyin_index slurp walk (`for_each_row` → `load_pinyin_index`) | 350.9e6 | 30.1% |
| resolve pass (record → `PhraseEntry`, #132's in-place collect) | 194.9e6 | 16.7% |
| phrase_index slurp walk (`load_phrase_index`) | 174.6e6 | 15.0% |
| **interpolation2.text parse** (`set_unigrams_from_interpolation2`) | **156.7e6** | **13.5%** |
| bigram slurp (`BigramLanguageModel::open`) | 117.7e6 | 10.1% |
| everything else (process-end drop glue, punct, main) | ~70e6 | ~6% |

The two named leftovers are 995e6 of 1,164e6 Ir; the wall told the same
story — cumulative init 151.8 (dict) + 42.7 (bigram) + 25.5 (interp) ms,
punct 0.0. Inside interp: the final `(u32, u64)` sort 60.4e6,
`SplitWhitespace::next` 45.3e6 self + one `Vec<&str>` allocation per line
(63,907 lines) 7.7e6, `read_line` ~14e6. Inside the slurps: `BTreeMap`
insert walks (phrase 12.1e6 self, bigram), 23-key `syllable_initial`
prefix scans 58.6e6, the `initial_keys` `String` sort 31.8e6 + its
memcmp, one boxed `{token,freq}` slice and one boxed hits slice per
pinyin row (93,349 of each, both freed or retained per row). No third
leftover appeared; no STOP.

## What changed

**interp.rs — one walk, lighter sort.**
`span_item_line` replaces `split_whitespace().collect::<Vec<&str>>()`:
one byte walk per line keeping only the spans the validation reads
(head, token, second-to-last, last, field count); the phrase text
between token and `count` is spanned over, never split. ASCII bytes take
`u8::is_ascii_whitespace` (identical to `char::is_whitespace` on ASCII);
non-ASCII bytes decode once. `read_line` stays, so an invalid-UTF-8 line
is still a `Read` error at the same line. The final sort now orders
8-byte `(token, index)` pairs and gathers once — the same comparison
count moving a quarter of the bytes. Error messages, precedence,
`'+'`-signed parses, whitespace-only-line rejection, and the
section-end/skip rules are unchanged (differential test against
`split_whitespace`).

**table.rs — the walk order made a type.**
`LeByteKey(u32)` orders a token as its 4 little-endian storage bytes —
which is *not* integer order, and is exactly the order redb's B-tree walk
yields for the exporter's `token.to_le_bytes()` keys. The u32-keyed maps
below append rows in walk order under this key and binary-search with
the same wrapping; `ensure_sorted_unique` (from #132, now
`(K: Ord, V)`-generic and shared) verifies the order in one O(n) pass
and repairs off-order input with `BTreeMap::insert`-equivalent
keep-last semantics.

**dict.rs — denser materialization.**
- `phrase_index`: `BTreeMap<u32, CompactString>` → `Vec<(LeByteKey,
  CompactString)>`; the per-row insert walk and node allocations become
  an append. `phrase_text` / `build_text_tokens` / `pronunciations`
  surfaces keep their behavior (iteration stays ascending in walk order;
  the reverse map's per-text token lists still sort).
- pinyin rows stage as `(key → range into one flat {token, freq} vec)`;
  resolve fans records out into **one `PhraseEntry` arena**, and a key's
  hits are its arena slice — `fill_lookup` still copies from one
  contiguous slice per key. Two boxed slices per row (records + hits,
  186k allocations and their frees) become two vecs.
- resolve reads the unigram totals from a sorted snapshot vec (the
  `BTreeMap`'s own in-order walk, O(n) to build) instead of a tree get
  per record; the retained `unigrams` map is untouched (public
  `unigram_map`/`unigram_count` surfaces).
- initial keys stage as **packed `u128`s** (`initials.rs`): a first-two-
  bytes LUT answers `syllable_initial` (the 23-key inventory is all
  one/two lowercase letters, so two bytes decide the longest match), and
  each projected key folds into five-bit slots whose packed order equals
  the joined-string order (proof in the doc comment; differential tests
  including 1,000 deterministic random keys). The load sorts and dedups
  integers, then decodes only the 45,404 survivors. Keys longer than 25
  syllables (export max is 14) fall back to the string path; so would a
  future three-letter inventory.

**lm/mod.rs — bigram vec.**
`bigram: BTreeMap<u32, BigramRow>` → `Vec<(LeByteKey, BigramRow)>` append
+ order check; `load_successors` binary-searches. `BigramRow`'s public
shape (per-row `Vec<(u32, u32)>`) is untouched.

Complexity: three map builds go from O(n log n) insert walks to O(n)
appends + an O(n) order check; two sorts move from 16/24-byte records to
4/16-byte keys; lookups stay O(log n), now contiguous. Retained space
drops (no tree nodes, no per-row malloc headers); the transient resolve
peak grows ~1 MiB (snapshot + arena coexist). No regression where the
constitution forbids both-axes worsening.

## Numbers

Dict-only load (`load_profile dict`, release, same export):

| | before (`6b476b1`) | after | after/before |
|---|---:|---:|---:|
| Callgrind PROGRAM TOTALS | 870.8e6 | 466.0e6 | **0.54×** |
| `SystemDictionary::open` (median of 5) | 136.3 ms | 93.6 ms | **0.69×** |
| after-open RSS (cumulative step) | 37,072 KiB | 31,472 KiB | −5.5 MiB |
| after-open HWM (transient peak) | 38,152 KiB | 39,060 KiB | +0.9 MiB |

Full capi-shaped init (`load_profile full`, release): PROGRAM TOTALS
1,164.3e6 → **694.2e6 (0.60×)**. Inclusive: pinyin slurp 350.9 → 199.4e6,
phrase slurp 174.6 → 99.4e6, interp 156.7 → 125.5e6, bigram 117.7 →
95.0e6. Wall (cumulative, fresh process): 220.0 → 162.2 ms; init RSS
78,376 → 71,008 KiB.

Isolated medians (median of 5): `parse_interpolation2` 16.0 → **13.9 ms**
(records 63,907, total 50,913,735 — unchanged); interp sort blob 60.4e6 →
15.8e6 Ir.

W8 8-cycle Callgrind (profiling `.so`, init + 8 cycles):

| | before (post-#132) | after | after/before |
|---|---:|---:|---:|
| PROGRAM TOTALS | 2,089.9e6 | 1,662.3e6 | **0.80×** |
| `__memcmp_avx2_movbe` self | 158.0e6 (7.56%) | 85.2e6 (5.12%) | **0.54×** |
| `fill_lookup` → memcmp (keystroke key-get) | 6.8e6 (314,784×) | 6.8e6 (314,784×) | unchanged |

W8 dlopen scoreboard (20 speed runs, 10 RAM runs per mode, alternating
oracle/oxpinyin, CPU 3). The before run was clean; two after runs hit a
contended host (the oracle's own init read 2.19–2.23 ms and its steady
cycle 32–35 ms — the oracle binary did not change), so the table quotes
the quietest after run (oracle init 1.607 ms, closest to before's
1.494 ms):

| | before | after |
|---|---:|---:|
| `pinyin_init` oxpinyin | 236.470 ms [225.031, 329.425] | **188.660 ms [167.785, 255.963]** (median −47.8 ms; min 167.8 vs 225.0) |
| `pinyin_init` oracle | 1.494 ms | 1.607 ms |
| init ratio | 158.3× | 117.4× |
| `pinyin_alloc_instance` | 4.741 ms | 4.763 ms (key_cost_table untouched) |
| steady cycle | 16.030 ms (0.784× oracle) | 17.178 ms (0.647× oracle) |
| cold cycle | 16.262 ms (0.723× oracle) | 15.379 ms (0.557× oracle) |
| post-init RSS | 79,654 KiB (6.63×) | **71,976 KiB (5.99×)**, RssAnon 68,260 |
| lifetime peak HWM | 80,122 KiB (5.22×) | **72,094 KiB (4.69×)** |

Absolute headline: **`pinyin_init` 236.5 → 188.7 ms on the like-for-like
run** (best-case min 225.0 → 167.8 ms), `SystemDictionary::open` 136.3 →
93.6 ms, interp parse 16.0 → 13.9 ms, post-init RSS −7.7 MiB. The ×
columns are soft — the oracle drifts on this host — and the two
contended after runs are kept in
`target/profile/init-slurp/final-scoreboard{,2}.log` for the record.

## Parity (STOP line)

`real_tables_session_reports_parity` / `sentence_surface_reports_parity`
(`cargo test --locked --release -p pinyin-oracle --test
real_tables_integration`):

```text
top-1 10178 / top-5-set 10190 / prefix-10 94872 of 98930 / absent 0 / tie-swaps 1036
sentence 488 / 385 / 370
```

Unchanged. Full `cargo test --locked --workspace` green (580 passed);
fmt and `clippy --workspace --all-targets -D warnings` clean. Pins did
not move.

## Residual (not this pass)

- **redb walk floor**: `entry_ranges` + `BtreeRangeIter::next` +
  `RangeIterState` + access-guard drops ≈ 130e6 Ir across the three
  tables' ~288k rows — not removable without an on-disk format change
  (forbidden) or a measured mmap/zerocopy win that avoids materializing
  typed rows (not attempted; the decode needs typed rows regardless).
- `resolve_hits` closure 75.1e6: two binary searches + `PhraseEntry`
  build per record (146,238 records); merging the two searches into one
  keyed structure costs what it saves (measured estimate, not taken).
- `memcpy` 99.5e6 on the 8-cycle: the bigram's 15 MB of values and the
  entry arena fill — inherent to materializing typed rows from encoded
  values.
- `_int_malloc` 89.4e6 (8-cycle): 93k `Box<str>` keys, the bigram's
  per-row `Vec` (public `BigramRow` shape), and arena growth.
- Interp `read_line` + span walk ≈ 45e6; the 83 MB file's `\2-gram`
  section is skipped (only ~1.9 MB read).
- The init ratio is still >100×: the floor is now the redb walk plus
  typed-row materialization itself, which is the representation question
  (#132 scoreboard observation 1), not this PR's parse-and-slurp tax.
