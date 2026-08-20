# Stage-2 measurement harness (continues the W8 baseline)

Date: 2026-08-19 · Status: **measurement only — no decode, capi, or core
behavior changed.** Continues `docs/findings/perf-baseline-2026-08.md` and
`docs/findings/perf-exploration.md`. This PR is docs/tools only; it must
not land in the same diff as #120's engine change.

Pin: `libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c` + model20
`59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`. The
Criterion header and the profile script both print this SHA.

## What this adds

1. **Criterion 0.8** groups on the W8 C-ABI surface
   (`crates/oxpinyin-capi/benches/stage2.rs`), over the pinned model dir:
   - `parse_more_full_pinyins` — short / medium / junk-leading
   - `guess_candidates` — offset 0 and mid-phrase
   - `guess_sentence_get_sentence_0/full_nbest_post_116` — full n-best,
     post-#116
   - `user_store_count_delta_hot_token`
2. **`[profile.profiling]`** in the workspace `Cargo.toml`:
   `inherits = "release"`, `debug = "line-tables-only"`, `lto = "thin"`.
3. **`tools/profile/run-w8-cycle.sh`** — cargo-c install of oxpinyin-capi
   with that profile, then the same 20-input parse+guess+count cycle as
   `bisect --perf`. Prefers `samply record`; falls back to
   `cargo flamegraph` / `perf record`, then Valgrind Callgrind. Artifacts
   go under `target/profile/` (gitignored).

Guardrails: these benches do **not** assert the 10177-class candidate pins
(those stay in `pinyin-oracle` `real_tables_integration`). Criterion is
configured with `noise_threshold(0.50)` — deliberately wide, so a
`--save-baseline` / `--baseline` comparison only flags a change larger
than 50%. CI does not run the benches and must not grow a required check
for them.

`pinyin_parse_more_full_pinyins` includes `Session::type_pinyin` /
`refresh` (the decode). `pinyin_guess_candidates` then snapshots the
already-built list; offset is still ignored by the engine, so the two
guess arms measure the same copy. Junk-leading is cheap because the
segment graph stops at the non-`a-z`/`'` prefix. `count_delta` is the
cached-snapshot overlay, not a redb transaction per call.

## Re-run on `main` after #120

#120 (`perf(engine): cut keystroke-heap allocs`, plus #118's borrowed
`LookupTable::get`) targeted the same three stacks this harness first
named: `LookupTable::get` `memcmp`, table value clone, `Vec<Candidate>`
growth. Re-ran `tools/profile/run-w8-cycle.sh` (`PERF_CYCLES=8`,
`--profile profiling` cargo-c install) on `origin/main` `fcbc227`
(post-#118/#119/#120). Same host, `perf_event_paranoid=2`, Callgrind
fallback. Not a pin.

| self-`Ir` | before (#120) | after (`fcbc227`) | absolute |
|---|---:|---:|---:|
| PROGRAM TOTALS | 4.393e9 | 2.462e9 | **0.56×** |
| `__memcmp_avx2_movbe` | 737e6 (16.8%) | 241e6 (9.8%) | **0.33×** |
| `_int_malloc` | 380e6 (8.7%) | 141e6 (5.7%) | **0.37×** |
| `malloc` | 147e6 (3.4%) | 45e6 (1.8%) | **0.31×** |
| `_int_free` | 239e6 (5.4%) | 76e6 (3.1%) | **0.32×** |
| `LookupTable::get` | 315e6 (7.2%) | *(inlined into `fill_lookup`)* | — |
| `SystemDictionary::fill_lookup` | — | 299e6 (12.2%) | new #1 |

`memcmp` callers shifted off `LookupTable::get`. Remaining `memcmp`
inclusive-from-caller: init `BTreeMap<Box<str>, …>::insert` 83e6,
`syllable_initial` 64e6, `SyllableKey::from_option_text` 35e6,
`fill_lookup` itself only 8.8e6. Remaining `malloc` on the C-ABI cycle
is `CString::new` in `pinyin_guess_candidates` (72e6) plus init
`for_each_row` table load — not `Vec<Candidate>` growth.

### Current hottest 3 stacks (post-#120)

1. **`SystemDictionary::fill_lookup` (12.2% self).** The borrowed phrase
   get (`lookup_into` → `fill_lookup`). This is what is left of
   `LookupTable::get` after the clone site went away. Addressed in
   `docs/findings/perf-fill-lookup-2026-08.md`.
2. **`__memcmp_avx2_movbe` (9.8% self).** No longer the window-scan table
   get. Mostly init typed-map insert and `syllable_initial` prefix
   compare.
3. **`_int_malloc` (5.7% self).** `CString` snapshots at guess time, and
   init table materialization.

### `key_cost_table` / init

- **`key_cost_table`: do not touch on this evidence.** Self-`Ir` 0.06%
  (1.4e6 of 2.46e9). It was already out of the #120 cycle; it is still
  noise here.
- **Init stays the separate W8 track.** `SystemDictionary::open` /
  `for_each_row` / `parse_interpolation2` / typed-map insert are still
  the 158× `pinyin_init` and 8.22× RSS story
  (`perf-baseline-2026-08.md`, `perf-alloc-2026-08.md`). #118 dropped
  the reverse-map; the text model and table slurp remain. That is not
  this harness PR and not a follow-up to the leftover `memcmp` on the
  keystroke path.

## Criterion snapshot (this host, pinned tables)

`cargo bench --locked -p oxpinyin-capi --bench stage2`. Median of 20
samples after 500 ms warmup. Not a pin. First column is the harness
branch before rebase onto #120; second is `fcbc227`.

| Group | pre-#120 | post-#120 | note |
|---|---:|---:|---|
| `parse_more_full_pinyins/short` | 150 µs | 37 µs | >50% improvement, flagged |
| `parse_more_full_pinyins/medium` | 242 µs | 122 µs | 49.6%, within 50% noise |
| `parse_more_full_pinyins/junk_leading` | 20 µs | 16 µs | within 50% noise |
| `guess_candidates/offset_0` | 13 µs | 9.0 µs | within 50% noise |
| `guess_candidates/mid_phrase` | 11 µs | 9.7 µs | within 50% noise |
| `guess_sentence_get_sentence_0/full_nbest_post_116` | 455 µs *(empty trellis)* | 11.0 ms | see below; **not a #120/#119 regression** |
| `user_store_count_delta_hot_token` | 374 ns | 350 ns | no change |

The 50% threshold did what it is for: parse short counted as real,
medium 49.6% stayed in-noise, junk/guess 14–31% did not flag,
`count_delta` did not. Do not tighten it into a required check.

### The 455 µs → 11 ms `guess_sentence` jump

The 455 µs Criterion estimate was saved against **`64170b3`**
(`docs(w13): review-nits on the sentence-surface note`, 2026-08-19).
That SHA is an ancestor of **#116** `5e9a975` (`fix(capi): forward
SharedLm::nbest_step_costs so guess_sentence emits rows`). Before that
forward, C-ABI `pinyin_guess_sentence` ran an empty trellis (no
step-costs). After it, the same call is a real beam-32 n-best.

This is empty trellis → full n-best, not a #120 or #119 regression.
The bench is labeled `full_nbest_post_116`. Re-save the local Criterion
baseline on current `main`:

```sh
cargo bench --locked -p oxpinyin-capi --bench stage2 -- --save-baseline post-116
```

`noise_threshold(0.50)` stays. Benches stay out of required checks.

## Repro

```sh
# benches (needs PINYIN_EXPORT_DIR or /tmp/oxpinyin-export, and
# tools/model/fetch-model.sh)
cargo bench --locked -p oxpinyin-capi --bench stage2

# optional large-regression compare (local; not CI)
cargo bench --locked -p oxpinyin-capi --bench stage2 -- --save-baseline pin
cargo bench --locked -p oxpinyin-capi --bench stage2 -- --baseline pin

# profile the W8 candidate cycle against installed oxpinyin-capi
tools/profile/run-w8-cycle.sh
# artifacts: target/profile/{header.txt,w8-cycle-timing.json,
#   w8-cycle.profile.json.gz | callgrind.out | w8-cycle.svg}
```

On a machine with `perf_event_paranoid ≤ 1`, samply writes
`target/profile/w8-cycle.profile.json.gz`;
`tools/profile/extract-hot-stacks.py` summarises it.
