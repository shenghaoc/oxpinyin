# Startup and initialization audit: pin vs oxpinyin

Scope: library load, `pinyin_init` / `zhuyin_init`, option setup,
`*_alloc_instance`, and the first usable candidate. Steady-state typing,
repeated guessing and training are out of scope. Measurements are in
`../perf/startup-baseline-2026-10.md`; the tool is
`crates/oxpinyin-initbench` (`../runbooks/initbench.md`).

Method: source first, then binary, then differential. Facts are what the cited
code does; hypotheses are marked and are not asserted as causes. The pin is
libpinyin 2.11.92 at commit `074a2219c90feaf962d0d24f034514033ece5f99`,
Berkeley DB backend, read from a checkout at that commit (`git rev-parse HEAD`
checked). Pin cites are `file:line` in that tree. oxpinyin cites are file and
item name. Ghidra was not needed: source plus `readelf`/`objdump` answered
every question below, and nothing here depends on a decompiler.

## Pin: what `pinyin_init` does (`src/pinyin.cpp:326-444`)

Verified from source:

- Default options are `USE_TONE` only (`:329`). `zhuyin_init` defaults to
  `USE_TONE | FORCE_TONE`. `*_set_options` stores the word and returns true;
  `*_alloc_instance` does not return NULL.
- `check_format` (`:172-218`, called `:344`) loads `user.conf`
  (`table_info.cpp:325-376`; prints `open … failed.` when absent), raises the
  open counter, and **writes `user.conf` on every init** (`:186-187`,
  `table_info.cpp:377-397`). A non-conforming profile (versions differ, or
  `open_counter > 6`, `OPEN_COUNTER_LIMIT`, `table_info.cpp:32,409`) makes the
  init `unlink` the user files (`:194-215`).
  `pinyin_fini` decrements the counter (`:1196-1198`) and `zhuyin_fini` does not,
  so a benchmark must never reuse a profile between samples. initbench gives
  every sample a private copy.
- Four system phrase libraries (`gb_char`, `gbk_char`, `opengram`, `merged`) are
  `mmap`ped `PROT_READ|PROT_WRITE, MAP_PRIVATE` and **fully scanned** by an XOR
  checksum (`:255`, `memory_chunk.h:475-523`, `get_check_sum`). Addon topic
  libraries are not loaded at init (`:434`).
- Per library a user `.dbin` journal is `malloc`+`read`+checksummed and
  replayed onto the mapping (`:271-276`, `phrase_index.cpp:286-299`).
- BDB tables: every BDB-backed class constructor creates a throwaway in-memory
  BDB that `attach`/`load_db` then discards (`chewing_large_table2_bdb.cpp:52-64`,
  `phrase_large_table3_bdb.cpp:65-79`, `punct_table_bdb.cpp:28-42`): seven per
  init. Six system BDBs are opened `DB_RDONLY` and read on demand (`bdb_utils.h:27-40`).
  Three user BDBs are copied record by record into in-memory DBs (`copy_bdb`,
  `bdb_utils.h:43-72`), so user-data size appears in init cost.
- `pinyin_train` after a plain guess trains nothing: `train_result3` trains only
  positions that carry a chosen-candidate constraint. Profiles are therefore
  built through the import API, not by training.

Binary evidence (`readelf -d`, `.init_array`, `.rela.dyn` on the BDB build of
the pin, read-only): five `.init_array` entries (four `_GLOBAL__sub_I_*` from
`chewing_key.cpp`, `pinyin_parser2.cpp`, `zhuyin_parser2.cpp`,
`phonetic_key_matrix.cpp`) because `content_table` and the zhuyin/special tables
are header-defined with non-constexpr `ChewingKey` constructors
(`chewing_key.h:49-61`, `pinyin_parser_table.h:3976`); 6459 `.rela.dyn` entries
(6438 `R_X86_64_RELATIVE`); lazy PLT. That is the load-time cost the pin pays
and a Rust cdylib does not (4.12 M Ir for the pin against 0.82 M, measured).

## oxpinyin: what `pinyin_init` does

Path: `pinyin_init` (oxpinyin-capi `context.rs`) → `CapiContext::try_new`
(`state.rs`) → `ContextCore::try_open` (oxpinyin-facade) →
`Runtime::open_with_law` (oxpinyin-runtime).

- System chunks: `PhraseLibraries::open` → `PhraseLibrary::open` → `map_chunk`
  (oxpinyin-data `phrase_library.rs`): read-only `mmap`, header check, then
  `chunk_checksum`, the same full XOR pass as the pin (parity by design).
- User side: `load_libraries` reads user chunk files with `fs::read` (copy);
  `load_user_pinyin_index` / `load_user_phrase_index` walk the user BDBs into
  in-memory maps; `load_logs_with_mask` replays each `.dbin`.
- `seed_txn` (oxpinyin-user `store_libpinyin.rs`): clones the whole libraries
  map, clones each override, and `BdbTxn::put` (oxpinyin-store `bdb/mod.rs`)
  buffers every put before `BdbStore::write` replays the buffer into the
  in-memory libdb and calls `DB->sync`. Cost scales with profile size; an
  empty profile clones empty maps.
- `open_session` duplicates `system_overrides` into `StoreInner.system_items`.

## Measured, and what it tells us (Linux, Berkeley DB, see the baseline for CIs)

| claim | status |
| --- | --- |
| Library load: oxpinyin 0.50× the pin, 4.1 M vs 0.82 M Ir | measured |
| Empty profile: time to first candidate 0.71× (pinyin), 0.73× (zhuyin) | measured |
| 80-import profile: oxpinyin init 0.27× the pin | measured; the pin is *slower* here than on the larger profiles, 6.7 ms vs 3.1 ms, with 8.25 MiB anonymous memory against 0.8 MiB. Cause not established (hypothesis: a libdb or phrase-index growth path the 80-row profile triggers). The pin's behaviour, not a benchmark artefact: all three oxpinyin controls and the pin were run on identical bytes |
| 20 000-row profiles: oxpinyin init 6.8–8.3× the pin, 21 ms vs 3 ms; 141 M Ir and 268 k allocations vs 10 M Ir and 1.3 k | measured; dominated by profile-size-proportional work (`seed_txn`, journal replay, `put` replay into libdb). Instruction attribution by function was not captured, so the split between those three is a hypothesis |
| oxpinyin first-result phase 2.8–4× the pin's, but ~0.2 ms | measured; small against init |

Same-bytes control (`ox-control` vs `ox`): ratios sit at 1.00× within the CIs
for init and time to first candidate, so the benchmark's own noise is below the
effects reported.

## Ranked candidates (not implemented; none asserted as a bottleneck)

1. **Profile-size init cost** (`seed_txn`, journal replay, buffered `put`
   replay). 0.27× on a small profile and 7× on a 20 000-row one means the cost
   is per profile row, not fixed. Candidates: avoid the deep `libraries.clone()`
   and per-override clones; write puts directly instead of buffering and
   replaying; skip `DB->sync` on an in-memory handle; defer seeding to first
   use. Must stay bug-for-bug: journal replay order and `user.conf` effects are
   observable (checked by the persistence scenario).
2. **Deferred user-index load.** If the first candidate does not need the user
   tables, loading them lazily moves work out of init; the first-candidate
   phase would take it back. Needs a behavioural argument about observable
   ordering before any change.
3. **Checksum pass.** Parity with the pin (`get_check_sum`); both pay it. A
   cheaper check would be an observable change on corrupt files, so this is a
   policy question, not a free win.
4. **Resident memory at first candidate on large profiles.** 5 MiB more
   anonymous memory than the pin and 4× the minor faults; follows from item 1.

Out of scope here, noted only: `CountCache::unigram` does not memoise absent
rows. That is a steady-state matter.

## Observed behavioural difference (recorded, not gating)

The hostile-input probe found one: zhuyin facade, inputs `'` and `''`
(parse consumes 1 or 2 bytes, 0 candidates). The pin's `zhuyin_guess_sentence`
returns false and `zhuyin_guess_candidates_after_cursor` returns false;
oxpinyin returns true for both, with no sentence. Pin side: `zhuyin_guess_sentence`
(`src/zhuyin.cpp:895`) returns the result of `get_nbest_match`, which returns
false when `m_matrix->size() == 0` (`src/lookup/phonetic_lookup.h:743-745`).
This was seen at option word `0x18a`; whether `compatibility-policy.md` already
covers it at another option word has not been re-checked here, so it is
reported as a difference, not classified. It affects only inputs the timed
workload never uses, and the report keeps it in its own table.
