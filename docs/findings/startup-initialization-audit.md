# Startup and initialization audit: pin vs oxpinyin

Scope: where initialization time and memory go: library load,
`pinyin_init` / `zhuyin_init`, system-data mapping and checksums, native
database opens, user-profile loading and replay, option setup and
`*_alloc_instance`. The first usable candidate is the outer boundary of startup
and is measured only to expose initialization cost deferred until first use.
Steady-state typing, repeated guessing, runtime algorithm tuning and training
are out of scope. This document analyses and instruments; it changes no
production code. Measurements are in
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

## Fixtures (what they are, and what they are not)

All three populated profiles were built through the public import API by the
pin, then checked to load (witness inputs answer differently from `empty`).
Their shapes differ, so they are **not** a controlled size-scaling series.

| profile | import calls | distinct phrases | frequency per row | libraries touched | user journal bytes |
| --- | ---: | ---: | ---: | --- | ---: |
| `empty` | 0 | 0 | – | – | 0 |
| `modest` | 80 | 2 (the same two in each library) | 5 | all four | 392 (122/90/90/90) |
| `phrases` | 10 000 | 10 000 | 20 000 | almost all in `gb_char` | 467 290 (`gb_char`) |
| `journals` | 10 000 | 10 000 | 20 000 | all four (quota 3000/3000/3000/1000) | 515 044 (four journals: 141 434, 150 386, 68 342, 154 882) |

20 000 is the **frequency** each imported row carries, not a row count. Earlier
drafts of these documents called `phrases` "20 000-row" and "20k imports"; that
was wrong. Row count, distinct phrases and frequency all differ between
`modest` and `phrases`, and so does which library receives the rows, so no
timing difference between them is attributable to size alone.

## Measured results

Source: `../perf/startup-baseline-2026-10.md` (Linux, Berkeley DB, one host).

- Library load: oxpinyin 0.50× the pin in time; 0.82 M against 4.12 M
  instructions.
- `empty`: time to first candidate 0.71× (pinyin), 0.73× (zhuyin).
- `modest`: oxpinyin `*_init` 0.27× the pin. The pin is slower on `modest`
  (6.7 ms, 11.8 M Ir, 8.25 MiB anonymous memory) than on `phrases` (3.1 ms,
  10.3 M Ir, 0.8 MiB anonymous).
- `phrases` and `journals`: oxpinyin `*_init` 6.8–8.3× the pin (about 21 ms
  against 3 ms; 141 M Ir and 268 k allocation calls against 10 M Ir and 1.3 k).
- Same-bytes control (`ox-control` against `ox`): 0.94–1.05× for init and time
  to first candidate in every cell, which is the noise floor of these runs.
- The first-result phase is about 0.1–0.34 ms and is not where startup goes.

## The pin's `modest` anomaly: measured, then hypotheses

Measured on the pin only (Callgrind, `pinyin` facade, stop after `init`, inclusive
cost; one run per profile; the oxpinyin object was not profiled this way):

Command (Valgrind 3.22.0; the profile directory is a private copy of the
fixture): `valgrind --tool=callgrind --callgrind-out-file=<out> initbench child
--facade pinyin --lib <pin libpinyin> --system-dir <data> --user-dir
<profile copy>/user --bind now --stop-after init`, then
`callgrind_annotate --inclusive=yes <out>`. Totals: 16.58 M (`modest`) and
15.09 M (`phrases`) for the whole child process.

| inclusive instructions | `modest` | `phrases` |
| --- | ---: | ---: |
| `pinyin_init` | 11.81 M | 10.32 M |
| `_load_phrase_library` | 11.08 M | 9.63 M |
| `SubPhraseIndex::merge` (journal replay) | 5.84 M | 3.81 M |
| `SubPhraseIndex::add_phrase_item` | 5.83 M | not among the largest entries |
| `MemoryChunk::set_content` | 5.84 M | 1.14 M |

In `modest`, nearly all of `merge` is `add_phrase_item` calling
`MemoryChunk::set_content`. In `phrases`, `merge` spends its time in
`PhraseIndexLogger::next_record`, and `add_phrase_item` does not appear among the
largest inclusive entries. That is the measured difference. Not measured:
which record types the journals hold.

Unverified hypotheses, each testable and none asserted:

1. The `modest` journals replay new-phrase (ADD) records, whose
   `add_phrase_item` appends through `MemoryChunk::set_content`; the `phrases`
   journal replays records for phrases already in the system library, which would
   not append. `MemoryChunk::set_content` / `ensure_has_more_space`
   (`memory_chunk.h`) is a copy-on-resize (see the pin section above), which
   would also explain the 8.25 MiB anonymous footprint.
   Test: decode the journal records' types.
2. Each of the four libraries is resized once in `modest`, against one in
   `phrases`. Test: count `set_content` calls and bytes.

## Candidates for reducing initialization time and space (hypotheses; not implemented)

None of these has been attributed to a function in oxpinyin by measurement:
the instruction and allocation figures above are whole-process, per phase, and
the per-function profile exists for the pin only. They are where to look first,
in order of how much of the measured gap each could cover.

1. **Work proportional to the user profile at init** (`seed_txn`, journal
   replay, buffered `put` replay into the in-memory libdb, per the source read
   above). The 141 M Ir and 268 k allocation calls at init on `phrases` and
   `journals` are the largest measured gap. Candidate Rust changes: avoid the
   deep `libraries.clone()` and per-override clones; write puts directly rather
   than buffering and replaying them; skip `DB->sync` on an in-memory handle.
   Any change must keep journal replay order and `user.conf` effects, which the
   persistence scenario observes.
2. **Deferred user-index loading.** If the first candidate does not need the
   user tables, loading them lazily moves cost out of init and into first use;
   the first-candidate phase is how that would show. Needs a behavioural
   argument first.
3. **The checksum pass** over the four system chunks. Both implementations pay
   it, by parity with `get_check_sum`; a cheaper check would change behaviour
   on corrupt files, so it is a policy decision, not a free win.
4. **Anonymous memory at first candidate on `phrases`/`journals`** (about 5 MiB
   more than the pin, 4× the minor faults): follows from item 1 if the copies
   are what is retained; not separately established.

The next measurement that would turn these into findings is a Callgrind and
DHAT per-function profile of the oxpinyin object on `phrases`, which this PR did
not capture.

Out of scope here (steady state): `CountCache::unigram` does not memoise
absent rows.

## Observed behavioural difference (recorded, not gating, unresolved)

The hostile-input probe found one: zhuyin facade, inputs `'` and `''`
(parse consumes 1 and 2 bytes, 0 candidates). The pin's `zhuyin_guess_sentence`
and `zhuyin_guess_candidates_after_cursor` return false; oxpinyin returns true
for both, with no sentence.

Pin side (source): `zhuyin_guess_sentence` (`src/zhuyin.cpp:895`) returns the
result of `get_nbest_match`, which returns false when `m_matrix->size() == 0`
(`src/lookup/phonetic_lookup.h:743-745`); a keyless parse leaves the matrix empty
(`src/storage/phonetic_key_matrix.cpp:34-38`).

Against the compatibility register: `compatibility-policy.md` row 53, item 2,
records exactly this contract (the pin answers false for one, two and three
apostrophes under the default and the parity word, and lists zhuyin among the
facades) as **CLOSED in code** (#542). The object this benchmark loaded, built
from this tree, still answers true. A focused check on both objects with a
fresh instance, no option set, `0x20` and `0x18a`, on `'` and `''` found the
same difference at every option word, so the option word is not the cause. The
pin answered false in every case.

What is not established: why a build from this tree contradicts a row recorded
as closed (a regression, a feature or build-configuration dependence of the
object built here, or a facade the row's cases did not cover). It was not
investigated further, not fixed, and not classified as acceptable. It affects
only inputs outside the timed workload; the report keeps it in its own table. It
should be checked against the register by a maintainer.
