# Stage-2 allocation pass — W8 candidate-cycle (2026-08-19)

Status: **measured decode-path change.** This is **keystroke heap** on the
123-step W8 candidate-cycle. It is not the W8 158× `pinyin_init` / table
slurp, and it is not `key_cost_table`. Those two still sit at the
`docs/findings/perf-baseline-2026-08.md` numbers.

Parity pins unchanged. `loses_to` / n-best `sort_by` untouched. No
bumpalo/typed-arena. Rebased onto main after #117 (`nbest_row` by rank)
and #118 (typed pinyin/phrase maps, borrowed `LookupTable::get`). The
`LookupTable::get` value-clone site named in the before-dump is already
gone on that base; this PR keeps the typed maps and attacks the session
scratch / CompactString / SmallVec / unused-`k_best` remainder.

Host: same crate-local Criterion/dhat harness as
`docs/findings/perf-exploration.md`. Release, rustc 1.97.1. Exported tables
`/tmp/oxpinyin-export`, model20 cache `target/model20/extracted/`.
`samply` / `perf` / `cargo-flamegraph` were not installed; the cycle is the
existing 20-input W8 candidate sequence (`CYCLE_INPUTS`, 123 parse+guess
steps), not a toy bench.

## Before (dhat + Criterion, no production edits)

dhat (`alloc_profile --dhat --keystroke-only`), decode loop only:

| | |
|---|---:|
| total | 35,438,142 B in 181,293 blocks |
| peak | 539,148 B in 3,901 blocks |
| **bytes/op** | **288,107** |
| **blocks/op** | **1,473.9** |

Criterion `keystroke_cycle_20_inputs`: **84.816 ms** [78.903, 91.859].

Named sites from the dump (first oxpinyin frame, total bytes):

| Site | Bytes | Blocks |
|---|---:|---:|
| window scan `Vec<Candidate>` grow (`append_scan_entries`) | 9.18 MB | 779 |
| refresh `Vec<(RankKey, Candidate)>` | 8.29 MB | 212 |
| `Vec<PhraseEntry>` grow in `Dictionary::lookup` | 8.14 MB | 3,441 |
| stable-sort scratch of the rank pairs | 4.71 MB | 99 |
| `dedup_by_text_keep_first` (`HashSet<String>` + retain clones) | 2.32 MB | 51,520 |
| `LookupTable::get` value clone | 0.59 MB | 54,482 |
| `append_scan_entries` phrase `String` | 0.16 MB | 53,733 |
| `k_best_to` (still invoked, result unused on the real-unigram path) | 0.35 MB | 3,558 |

**Not on this cycle:** n-best beam insert (`guess_sentence` is not called) and
`SharedLm::count_delta` (engine bench, empty user, and `score` was only
reached through the unused k-best). Both were still in the pass: n-best
`NSTORE=2` / beam-32 / path-16 stacks, C-ABI `CString` without a second
`String`.

## After

| | before | after | after/before |
|---|---:|---:|---:|
| bytes/op | 288,107 | 95,419 | **0.33×** |
| blocks/op | 1,473.9 | 19.8 | **0.013×** |
| total bytes | 35,438,142 | 11,736,481 | 0.33× |
| total blocks | 181,293 | 2,430 | 0.013× |
| dhat peak bytes | 539,148 | 810,632 | 1.50× (retained scratch) |
| dhat peak blocks | 3,901 | 9 | 0.002× |
| Criterion cycle | 84.816 ms | 47.220 ms | **0.56× (−44%)** |

Time improved. Total allocation volume improved. Decode-loop *peak* bytes
rose because session scratch keeps capacity across keystrokes (9 live
blocks vs 3,901 tiny ones). Process RSS is still the ~96 MiB init working
set from `perf-baseline-2026-08.md`; this peak is the keystroke heap, not
the tables.

## What changed (allowed tools only)

- **scratch buffers** on `Session`: candidate list, Schwartzian rank pairs,
  lookup hits, scan path. `CandidateList::swap_items` recycles capacity.
- **`compact_str::CompactString`** for `PhraseEntry` / `Candidate` /
  n-best row text (typical 1–4 CJK scalars stay inline).
- **`smallvec`** for expand-keys sequences (≤16), completions (≤32), k-best
  `states_at` (8), n-best node store (2) and beam (32).
- **`lookup_into`** on the typed pinyin map so the scan fills a
  caller-owned `Vec<PhraseEntry>` instead of allocating a fresh one per
  window. (`LookupTable::get` already borrows on this base, #118.)
- skip **`k_best` + `Scorer`** on the real-unigram refresh (result was
  unused; `parsed_prefix` already comes from `fewest_keys`).
- two-pass **`HashSet<&str>`** dedup (no per-kept-text `String`).
- C ABI: `CString::new(cand.text().as_bytes())` (one alloc, not String+CString).
- segmenter: reused span `String`; index the current column instead of
  cloning it (indexed trellis stays the algorithm).

## Parity (STOP line)

`real_tables_session_reports_parity` / `sentence_surface_reports_parity`:

```
top-1 10177 / top-5-set 10189 / prefix-10 94871 of 98930 / absent 1 / tie-swaps 1036
sentence 488 / 385 / 370
```

Unchanged.

## Residual (not this pass)

- `Vec<(RankKey, Candidate)>` stable-sort scratch (~4.7 MB total, 99 blocks)
  is std's driftsort buffer; not reused.
- First-touch growth of the collected candidate vec still lands in dhat
  totals (~4.1 MB) when an input (`n`) jumps past prior capacity.
- Init-time table materialization (586 ms / 96 MiB RSS) is unchanged.
