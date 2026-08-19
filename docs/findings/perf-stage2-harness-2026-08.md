# Stage-2 measurement harness (continues the W8 baseline)

Date: 2026-08-19 · Status: **measurement only — no decode, capi, or core
behavior changed.** Continues `docs/findings/perf-baseline-2026-08.md` and
`docs/findings/perf-exploration.md`.

Pin: `libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c` + model20
`59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`. The
Criterion header and the profile script both print this SHA.

## What this adds

1. **Criterion 0.8** groups on the W8 C-ABI surface
   (`crates/oxpinyin-capi/benches/stage2.rs`), over the pinned model dir:
   - `parse_more_full_pinyins` — short / medium / junk-leading
   - `guess_candidates` — offset 0 and mid-phrase
   - `guess_sentence_get_sentence_0`
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
configured with `noise_threshold(0.50)`, so a `--save-baseline` /
`--baseline` comparison only flags a change larger than 50%. CI does not
run the benches — that optional large-regression gate is local, not a
merge policy change.

## Criterion snapshot (this host, pinned tables)

`cargo bench --locked -p oxpinyin-capi --bench stage2`. Median of 20
samples after 500 ms warmup. Not a pin.

| Group | Median |
|---|---:|
| `parse_more_full_pinyins/short` (`ni`) | 150 µs |
| `parse_more_full_pinyins/medium` (`nihaozhongguo`) | 242 µs |
| `parse_more_full_pinyins/junk_leading` (`1nihao`) | 20 µs |
| `guess_candidates/offset_0` | 13 µs |
| `guess_candidates/mid_phrase` (offset 5) | 11 µs |
| `guess_sentence_get_sentence_0` | 455 µs |
| `user_store_count_delta_hot_token` | 374 ns |

`pinyin_parse_more_full_pinyins` includes `Session::type_pinyin` /
`refresh` (the decode). `pinyin_guess_candidates` then snapshots the
already-built list; offset is still ignored by the engine, so the two
guess arms measure the same copy. Junk-leading is cheap because the
segment graph stops at the non-`a-z`/`'` prefix. `count_delta` is the
cached-snapshot overlay, not a redb transaction per call.

## Hottest 3 stacks

Recorded with `tools/profile/run-w8-cycle.sh` against the
`--profile profiling` cargo-c install. This host has
`perf_event_paranoid=2`, so `samply record` (preferred) refused; the
script fell back to Valgrind Callgrind. 8 W8 cycles, 4.39 billion `Ir`.
Init is still in the profile (`pinyin_init` inclusive ~43%); the ranking
below is **self-`Ir`**, which is decode-dominated and matches the W8
engine-level Callgrind in `perf-exploration.md`.

1. **`__memcmp_avx2_movbe` (16.8% self).** Callers:
   `Session::refresh` → `search_scan` → `SharedDict::lookup` →
   `LookupTable::get` → `memcmp` (in-memory BTree key compare), plus
   init-time `BTreeMap::insert` while slurping the redb tables.
2. **`malloc` / `_int_malloc` (8.7% + 3.4% self).** Callers:
   `LookupTable::get` (value clone), `append_scan_entries` (candidate
   `String`s / `Vec<Candidate>` growth), `Candidate::retain`
   (`dedup_by_text_keep_first`), `pinyin_guess_candidates` (`CString`
   snapshots).
3. **`LookupTable::get` (7.2% self).** The phrase-table get on the
   window-scan lookup and `SystemDictionary::phrase_text` paths.

No algorithm change follows from this ranking; it is the Stage-2 starting
point on the installed C ABI.

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
#   w8-cycle.profile.json.gz | callgrind.out | flamegraph.svg}
```

On a machine with `perf_event_paranoid ≤ 1`, samply writes
`target/profile/w8-cycle.profile.json.gz`;
`tools/profile/extract-hot-stacks.py` summarises it.
