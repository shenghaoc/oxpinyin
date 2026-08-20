# Stage-2 leftover: `fill_lookup` (2026-08-20)

Status: **measured decode-path change.** Continues
`docs/findings/perf-stage2-harness-2026-08.md`. Post-#120 Callgrind named
`SystemDictionary::fill_lookup` as leftover #1 (12.2% self-`Ir`). This
PR is that scan/insert path in `oxpinyin-data`. Engine/capi call sites
already used `lookup_into`; they are unchanged. Scratch on `Session`
stays.

Parity pins unchanged. `loses_to` / n-best / `key_cost_table` untouched.
No arenas, no redb / `interpolation2.text` replacement. Typed-map
**insert** is init-only on this cycle and is not in the diff.

Host: same W8 8-cycle Callgrind path as #121 (`tools/profile/run-w8-cycle.sh`,
`PERF_CYCLES=8`, `--profile profiling` cargo-c install, Callgrind
fallback at `perf_event_paranoid=2`). dhat + Criterion
`keystroke_cycle_20_inputs` as in `perf-alloc-2026-08.md`. Release /
profiling, rustc 1.97.1. Exported tables `/tmp/oxpinyin-export`.

## Before (origin/main `337d8d5`, includes #120 / #121)

Callgrind 8-cycle W8 script, PROGRAM TOTALS **2.470e9**.

| self-`Ir` | count | share |
|---|---:|---:|
| PROGRAM TOTALS | 2.470e9 | 100% |
| `SystemDictionary::fill_lookup` | 299.5e6 | **12.12%** (#1) |
| `__memcmp_avx2_movbe` | 240.9e6 | 9.75% |
| `fill_lookup` → `memcmp` | 8.91e6 (397,641×) | 0.36% |

`fill_lookup` inclusive 314.8e6 (12.74%), 17,478 calls.
Hottest frames **inside** `fill_lookup` (inlined; from the profiling `.so`
line tables + callee tree — not guessed):

1. `BTreeMap<u32, Box<str>>::search` / `find_key_index` — `phrase_index.get`
2. `BTreeMap<u32, u64>::search` / `find_key_index` — `unigrams.get`
3. `compact_str::repr::inline` `copy_nonoverlapping` — `CompactString::from`
4. `Vec<PhraseEntry>::push`
5. `BTreeMap<Box<str>>::search` + `memcmp` (`slice/cmp.rs:339`) — the pinyin
   key get, only 8.9e6 of the 299e6 self

Typed-map **insert** (not the keystroke path): `BTreeMap<Box<str>, …>::insert`
192e6 inclusive, 83.4e6 `memcmp` at 3,768,186×, called from
`load_pinyin_index` `for_each_row` 93,349×. Init only. Left as a note.

dhat (`alloc_profile --dhat --keystroke-only`), decode loop only:

| | |
|---|---:|
| total | 11,736,481 B in 2,430 blocks |
| peak | 810,632 B in 9 blocks |
| **bytes/op** | **95,419** |
| **blocks/op** | **19.8** |

Criterion `keystroke_cycle_20_inputs`: **29.925 ms** [29.313, 30.875].

## After

| | before | after | after/before |
|---|---:|---:|---:|
| Callgrind PROGRAM TOTALS | 2.470e9 | 2.312e9 | **0.94×** |
| `fill_lookup` inclusive | 314.8e6 (12.74%) | 37.5e6 (1.62%) | **0.12×** |
| `fill_lookup` self | 299.5e6 (12.12%) | ~12.6e6 (0.55%) | **0.04×** |
| `memcmp` self | 240.9e6 (9.75%) | 244.0e6 (10.56%) | ~1.01× (still init) |
| `fill_lookup` → `memcmp` | 8.91e6 | 10.18e6 | key get, not the inner loop |
| dhat bytes/op | 95,419 | 95,465 | 1.00× |
| dhat blocks/op | 19.8 | 19.7 | 1.00× |
| Criterion cycle | 29.925 ms | 16.423 ms | **0.55× (−45%)** |
| after-init RSS | 117,876 KiB | 123,724 KiB | +5.8 MiB |

Hottest frames **after**, inside `fill_lookup` (inclusive 37.5e6):

- `memcmp` on the pinyin-key BTree get — 10.2e6 (0.44%), 459,064×
- `PhraseEntry` clone / `extend_from_slice` — 9.5e6 (0.41%)
- `index_key` — 5.0e6 (0.22%)

Init now also shows `resolve_hits` `FromIterator` 175e6 inclusive (93,349
keys, 146,238 records). That is the inner loop moved to `open`, once.
The W8 8-cycle totals still include one `pinyin_init`; the Criterion /
dhat numbers do not.

`memcmp` leftover is still init typed-map insert (83e6 from 3.77e6
inserts) and `syllable_initial`. Not this PR.

## What changed (allowed tools only)

- **Pre-resolved `Box<[PhraseEntry]>`** as pinyin-index values. Load
  phrase text first, aggregate unigrams, then attach matched/total at
  resolve time. `fill_lookup` is `get` + `extend_from_slice`.
- **`CompactString` in `phrase_index`** so resolve clones inline CJK
  instead of `CompactString::from(&str)` off `Box<str>`.
- Pinyin **keys stay `Box<str>`**. A CompactString-key trial raised init
  insert `memcmp` (192e6 → 233e6 inclusive) and did not cut the
  keystroke get (8.9e6 → 10.3e6). Length-first `Ord` would break the
  `SEARCH_CONTINUED` prefix range; not used.
- LookupTable byte maps, scheme/zhuyin, `loses_to`, arenas: untouched.

The extra RSS is the stored `PhraseEntry` (~56 B) in place of
`{token, freq}` (8 B) over ~146k records. Time dropped; space grew;
justified by the measured inner-loop `Ir`.

## Parity (STOP line)

`real_tables_session_reports_parity` / `sentence_surface_reports_parity`:

```text
top-1 10177 / top-5-set 10189 / prefix-10 94871 of 98930 / absent 1 / tie-swaps 1036
sentence 488 / 385 / 370
```

Unchanged.

## Residual (not this pass)

- Init typed-map insert `memcmp` (still ~10% self-`Ir` of the 8-cycle
  script). Separate from the keystroke path.
- Remaining `fill_lookup` is the pinyin-key BTree get + cloning the
  prebuilt slice into session scratch.
- `CString` at guess time; `pinyin_init` RSS / 158× wall.
