# Berkeley DB adapter access patterns

Date: 2026-10-22 UTC. Status: results for `perf/bdb-direct-access`.

The Berkeley DB adapter (`crates/oxpinyin-store/src/bdb/`) is compared
with libpinyin at the pin, 074a2219, and with libdb 5.3.28 operation by
operation. Two changes follow from it: point reads copy out of the
handle's own return buffer at exact size (B1), and the user bigram's key
walk copies no values (B2). Output, files on disk and the C ABI are
unchanged.

## Sources read

- libpinyin at 074a2219: `src/storage/ngram_bdb.cpp`,
  `chewing_large_table2_bdb.cpp`, `phrase_large_table3_bdb.cpp`,
  `punct_table_bdb.cpp`, `bdb_utils.h`, `src/storage/ngram.cpp`,
  `src/pinyin.cpp`.
- libdb: github.com/berkeleydb/libdb at
  `e66a5e66d5a8db89d13532012f28d17d375e8564`, whose `dist/RELEASE` is
  5.3.28. Paths below are relative to that tree.
- Debian `db5.3` `5.3.28+dfsg2-11`, the version in the oracle image's
  `debian:testing` snapshot (`apt_snapshot=20260831T000000Z`). Its
  `debian/copyright` excludes only `docs/csharp/fti` and
  `test/xa/src2/run.sh`. Of its 19 patches, one touches a file cited
  here: `001-checkpoint-opd-deadlock.patch` adds five lines to
  `__dbc_iget`'s off-page-duplicate branch at `src/db/db_cam.c:868`.
  Citations in `db_cam.c` past that line shift by five in the patched
  source (`:1055` becomes `:1060`, `:1133` becomes `:1138`); the code
  they name is unchanged. libpinyin's tables store no duplicates, so the
  branch is not reached. No patch touches `db_ret.c`, `db_iface.c`,
  `db_am.c`, `db_overflow.c`, `db.c`, `db_method.c` or `hash_stat.c`.

## How libdb returns data

`__db_retcopy` (`src/db/db_ret.c:91-169`) fills an output DBT by its
flags:

- `DB_DBT_MALLOC`, `DB_DBT_REALLOC`: libdb allocates for the caller
  (`:136-140`).
- `DB_DBT_USERMEM`: the caller's buffer; `DB_BUFFER_SMALL` when
  `ulen < len` (`:141-143`).
- No flag (a zeroed DBT): the owner's return buffer, reallocated only
  when the record is larger than the buffer (`:144-155`).
- `DB_DBT_PARTIAL` trims the copy to `dlen` bytes from `doff`
  (`:106-114`); a zero-length copy needs no buffer (`:125-130`).

Overflow records go through `__db_goff` under the same rules
(`src/db/db_overflow.c:120-153`), which returns before reading any page
when zero bytes are wanted (`:120-123`).

`DB->get` lends its transient cursor the handle's buffers
(`SET_RET_MEM`, `src/db/db_iface.c:800`; macro at
`src/dbinc/db_int.in:969-974`). They stay valid until the next call on
that handle and are freed at close (`src/db/db.c:1172-1182`). A cursor
uses its own buffers (`RESET_RET_MEM`, `src/db/db_am.c:207`); they stay
allocated on the free queue after `c_close` and are freed in
`__dbc_destroy` (`src/db/db_cam.c:163,196-201`). A zeroed output DBT is
refused only on a `DB_THREAD` handle (`src/db/db_iface.c:2921-2923`).

## Operations

| operation | oxpinyin before | the pin |
| --- | --- | --- |
| point read | `Db::get`: a 4 KiB `Vec` per call through `DB_DBT_USERMEM`, returned with that capacity; a value over 4 KiB took `DB_BUFFER_SMALL` and a second lookup | zeroed DBT, decoded in place from the handle's buffer: `Bigram::load` (`ngram_bdb.cpp:127-146`, `SingleGram(..., copy=false)` at `ngram.cpp:41-46`), `ChewingLargeTable2::search_internal` (`chewing_large_table2_bdb.cpp:175-191`), `PhraseLargeTable3::search` (`phrase_large_table3_bdb.cpp:189-205`); `PunctTable::load_entry` copies (`punct_table_bdb.cpp:147-158`) |
| put, del | borrowed slices, no copy | the same (`ngram_bdb.cpp:148-176`) |
| cursor | `Cursor::get`: `DB_DBT_USERMEM` into a cursor-owned pair of 4 KiB buffers, recycled through the handle, grown on `DB_BUFFER_SMALL` | zeroed DBTs into the cursor's buffers (`phrase_large_table3_bdb.cpp:225-262`, `bdb_utils.h:55-66`) |
| ordered scan | one cursor, `DB_SET_RANGE` then `DB_NEXT`, rows borrowed | `search_suggestion`, the same shape |
| hash sort-scan | `sorted_rows`: two copies per row, then a sort | none |
| user-bigram load, save | `visit_db`: each borrowed row put into the other handle | `copy_bdb` (`bdb_utils.h:43-74`) |
| user-bigram keys | `visit_db`, which also copied every value | `get_all_items` (`ngram_bdb.cpp:178-215`), which also copies every value |

## Where the 4 KiB stayed

Traced through every production caller of the point read:

- `MergedGram::merge` (`oxpinyin-core`) moves a single-sided gram into
  `MergedGram.chunk` without copying it. `StepCosts.rows`
  (`oxpinyin-engine`, `nbest.rs`) keeps one `NbestStepRow` per beam
  predecessor, up to `NBEAM = 32`, for one n-best position. Before B1
  each such row held at least 4 KiB, from `bigram.db` and from the
  in-memory user bigram alike.
- `GenericUserStore::check_training_step` keeps the original row in
  `TrainingCheck.grams` when the overflow guard declines it, for one
  training call.
- `CountCache.bigram_rows` (up to 8192 entries, about 32 MiB at 4 KiB
  each) would keep rows for a session. Its only production feeder,
  `RuntimeLm::nbest_step_costs`, has no live caller.
- Every system-table lookup (`pinyin_index.bin`, `phrase_index.bin`,
  `punct.bin`, the addon tables) decodes the value and drops it.
- No production caller reaches `ReadStore::get`, `ReadStore::scan` or
  `WriteTxn::get` on `BdbStore`: production sessions keep their values
  in memory.

## Changes

**B1, point read.** The value DBT is zeroed, as in every libpinyin point
read. libdb copies the record into the handle's buffer, and `Db::get`
copies it out at exact size while `SharedDb`'s lock is held, so no
caller code runs under the lock and no native buffer escapes it. One
lookup serves every size. An empty record returns an empty `Vec` without
reading the buffer pointer, which libdb leaves null until a non-empty
read.

#766 replaced a zeroed 4 KiB buffer (`vec![0; 4096]`) with an
uninitialized one. B1 keeps that: no Rust buffer is zeroed, and only the
`size` bytes libdb wrote are read. The 4 KiB size itself dates from the
first backend (f09f4630), whose `DB_THREAD` opens required a flagged
output DBT; the handles have been opened without `DB_THREAD` since
f3945bc9.

The handle's buffer grows to the largest record read through it and is
freed at close, as in the pin.

**B2, user-bigram keys.** `Cursor::key` moves the cursor exactly as
`Cursor::get` does, with the value DBT asking for zero bytes at offset
zero (`DB_DBT_PARTIAL`). libdb copies none of the value and reads no
overflow page for it. The keys and their order are unchanged.

**Not changed.**

- The hash sort-scan (`sorted_rows`) has no production caller: its only
  callers are `persistence::load` and datagen's `count_hash`, both
  reached from tests alone. A record count to reserve from is not
  available safely in any case: `DB_FAST_STAT` returns the meta page's
  `key_count`, which only a full stat maintains (`src/hash/hash_stat.c:56-74`),
  and a full stat on a writable handle rewrites the meta page
  (`:96-107`).
- Cursors keep `DB_DBT_USERMEM`. Rows already land in reused buffers,
  and a zeroed DBT would trade the Rust pair for libdb's per-cursor
  buffers at about the same cost.
- The framed tier's two copies per row (`TableRows::next`): it has no
  production caller.

## Results

`tools/bisection/store-ops.py --profile-only` drives the
`backend_matrix_bdb` worker's `--op` mode against the pin-built system
tables of the bdb cell. Base, a same-bytes control (the base binary under
another path of equal length) and the candidate run in one window on one
host. Every sample's checksum equals the base's. Callgrind counts `Ir`
inside `ops::measured`; DHAT counts every allocation in the process, libdb
included, as a run's totals minus a zero-iteration run's. The peak is
the whole process's, so where the setup peaks higher than the timed
region it is the setup's. The control equals the base exactly on every
row but one, so those rows are valid.

| operation | Ir/call A → B | Δ Ir | blocks/call A → B | bytes/call A → B | peak heap A → B |
| --- | ---: | ---: | ---: | ---: | ---: |
| `get_raw.tree.empty` | 4,842 → 4,413 | -8.9% | 1 → 0 | 4,096 → 0 | 19,361,799 → 19,361,799 |
| `get_raw.tree.small` | 4,817 → 4,505 | -6.5% | 1 → 1 | 4,096 → 4 | 23,170,983 → 23,170,983 |
| `get_raw.tree.medium` | 6,653 → 6,484 | -2.5% | 1 → 1 | 4,096 → 32.6 | 18,027,422 → 18,027,422 |
| `get_raw.tree.large` | 11,680 → 11,898 | +1.9% | 1 → 1 | 4,528 → 1,173 | 17,139,589 → 17,139,589 |
| `get_raw.tree.miss` | 4,217 → 3,933 | -6.7% | 1 → 0 | 4,096 → 0 | 35,271 → 35,271 |
| `get_raw.tree.retain` | 6,715 → 6,535 | -2.7% | 1 → 1 | 4,120 → 55.8 | 418,421 → 323,445 |
| `get_raw.hash` | 6,046 → 5,884 | -2.7% | 1 → 1 | 4,165 → 264 | 22,041,784 → 22,041,784 |
| `get_raw.hash.small` | 4,850 → 4,680 | -3.5% | 1 → 1 | 4,096 → 26.4 | 22,041,870 → 22,041,870 |
| `get_raw.hash.medium` | 6,560 → 6,474 | -1.3% | 1 → 1 | 4,096 → 411 | 22,041,871 → 22,041,871 |
| `get_raw.hash.large` | 46,525 → 47,812 | +2.8% | 2 → 1 | 12,637 → 8,584 | 22,041,869 → 22,041,869 |
| `get_raw.hash.miss` | 5,245 → 4,977 | -5.1% | 1 → 0 | 4,096 → 0 | 22,041,869 → 22,041,869 |
| `scan_raw.suggestion` | 7,228 → 7,230 | +0.0% | 0.5 → 0.5 | -11 → -11 | 15,411,896 → 15,411,896 |
| `scan_raw.full.tree` | 366,457,122 → 366,665,533 | +0.1% | 3 → 3 | 9,434 → 9,434 | 35,268 → 35,268 |
| `scan_raw.full.hash` | 325,323,411 → 325,379,798 | +0.0% | 112,804 → 112,804 | 25,192,854 → 25,192,854 | 21,822,514 → 21,822,514 |
| `framed.get` | 5,970 → 5,693 | -4.6% | 2 → 2 | 4,116 → 28 | 935,242 → 935,242 |
| `framed.scan` | 41,264 → 41,280 | +0.0% | 34 → 34 | 290 → 290 | 935,242 → 935,242 |
| `framed.txn_scan` | 25,783 → 25,622 | -0.6% | 26 → 26 | 2,321 → 277 | 935,325 → 935,325 |
| `framed.is_empty` | 5,636 → 5,637 | +0.0% | 1 → 1 | 13 → 13 | 935,327 → 935,327 |
| `user_bigram.get` | 11,457 → 12,894 | +12.5% | 1 → 1 | 5,692 → 1,782 | 22,041,867 → 22,041,867 |
| `user_bigram.get.small` | 4,328 → 4,180 | -3.4% | 1 → 1 | 4,096 → 26.2 | 22,041,873 → 22,041,873 |
| `user_bigram.get.large` | 99,557 → 114,890 | +15.4% | 2 → 1 | 26,340 → 22,328 | 22,041,872 → 22,041,872 |
| `user_bigram.miss` | 4,719 → 4,457 | -5.6% | 1 → 0 | 4,096 → 0 | 22,041,868 → 22,041,868 |
| `user_bigram.store` | 5,891 → 5,891 | +0.0% | 0 → 0 | 1.3 → 1.3 | 22,041,869 → 22,041,869 |
| `user_bigram.remove_store` | 6,203 → 6,203 | +0.0% | 0 → 0 | 0 → 0 | 22,041,876 → 22,041,876 |
| `user_bigram.keys` | 13,228,490 → 12,289,725 | -7.1% | 4,113 → 4,107 | 680,013 → 213,712 | 22,041,865 → 22,041,865 |
| `lifecycle.open_system` | INVALID | — | 45.8 → 45.8 | 15,331 → 15,331 | 84,989 → 84,989 |
| `lifecycle.user_load` | 16,988,647 → 16,991,098 | +0.0% | 161 → 162 | 555,576 → 551,496 | 22,041,867 → 22,041,867 |
| `lifecycle.user_save` | 29,722,362 → 29,724,422 | +0.0% | 3,849 → 3,849 | 815,008 → 815,008 | 22,041,867 → 22,041,867 |
| `session.typing_training` | 50,667 → 50,594 | -0.1% | 5.2 → 5.1 | 14,633 → 11,595 | 22,042,065 → 22,042,065 |

`lifecycle.open_system`'s Callgrind row is INVALID: the control differed
from the base by 1 `Ir` in this run and by 3 in an earlier one. Its DHAT
row is valid and unchanged.

Every point read allocates less, and none keeps spare capacity. Small
and medium values (mean 4 to 411 bytes), misses and empty values execute
1.3–8.9% fewer instructions. Larger values execute more, because B1
copies each value twice (into the handle's buffer, then out at exact
size) where A copied it once: +1.9% for `get_raw.tree.large` (over 256
bytes, mean 1,173), and +2.8% and +15.4% for the rows over 4 KiB, where
A also paid a second lookup. On the user bigram's mixed sample, which
holds the system bigram's rows, that is +12.5%. No row is worse than A
on both `Ir` and peak heap.
`get_raw.tree.retain`, which holds 32 results at once, peaks 94,976
bytes lower. The unchanged paths (scans, saves, loads) move by 0.1% or
less.

### Typing workload

The keystroke cycle of `crates/pinyin-oracle/benches/alloc_profile.rs`
(`--keystroke-only --repeats 2`, the 20 `CYCLE_INPUTS`), built without
the `dhat-heap` feature, against the same tables. Valgrind's DHAT and
Callgrind count the whole process, minus a `--repeats 0` run. In-process
`dhat` would miss libdb's allocations, so it is not used.

| | A | control | B |
| --- | ---: | ---: | ---: |
| blocks | 325,558 | 325,558 | 325,256 (−302) |
| bytes | 67,417,964 | 67,417,964 | 64,220,020 (−4.7%) |
| peak heap | 912,056 | 912,056 | 931,152 (+19,096, +2.1%) |
| `Ir` (diagnostic only) | 300,794,131 | 300,794,724 | 300,653,078 |

The DHAT control is exact, so the blocks, bytes and peak rows are
results. The `Ir` row is diagnostic only, not a result: the same-bytes
control differs from the base by 593 `Ir`, so §7.5's exact rule for
instruction counts is not met. The peak heap
is 19,096 bytes higher because the system bigram's handle keeps a return
buffer as large as the largest row it has read: DHAT attributes the
whole difference to `__db_goff` under `DB->get` from `BdbStore`. The pin
keeps the same buffer. Neither driver has a prediction mode, so
prediction is not covered here.

### Timing and residency

Pending. `tools/dev/slot.py` has no exclusive mode, so timing and
`VmHWM` would share the host with other work.

### Library sizes

Stripped `libpinyin.so.15.0.0` and `libzhuyin.so.15.0.0`, Berkeley DB
backend. Each is `cargo build --locked --release -p oxpinyin-capi -p
oxpinyin-zhuyin-capi --features oxpinyin-capi/shipped`, then
`tools/packaging/relink-versioned.sh` from the staticlib against the
crate's `.ver` file, then `strip --strip-unneeded`.

| file | A | B |
| --- | ---: | ---: |
| `libpinyin.so.15.0.0` | 2,700,104 | 2,700,104 |
| `libzhuyin.so.15.0.0` | 2,454,344 | 2,454,344 |

## Upstream reports

- Every BDB `save_db` passes `m_db` to `tmp_db`'s `sync`. It is item 8
  of `upstream-report-drafts.md`. It has no observable effect.
- The save is not unlink-then-create at the user's files.
  `pinyin_save` passes `save_db` a `.tmp` name and replaces each final
  file with `rename(2)` (`pinyin.cpp:997-1018`, `:1025-1130`), so the
  unlink only ever removes the `.tmp` sibling. The mixed profile a
  partly failed save leaves is already recorded (rows 68 and 69 of
  `compatibility-policy.md`). No new report is drafted.

## Follow-up

The pin decodes each point read in place and allocates nothing; B1 still
makes one exact allocation per lookup and, for values over 4 KiB, one
extra copy. A borrowing read on the store trait removes both. That
belongs to #1044.

## Commands

```
store-ops.py --base <A worker> --candidate <B worker> --data <bdb cell data> --work <dir> --profile-only --out <file>
valgrind --tool=dhat <alloc_profile> --keystroke-only --repeats {2,0}
valgrind --tool=callgrind <alloc_profile> --keystroke-only --repeats {2,0}
```

The raw records are not retained; the commands regenerate them.

## Measured builds

| build | tree | `backend_matrix_bdb` sha256 | `alloc_profile` sha256 |
| --- | --- | --- | --- |
| A | de66a0ad | `74b7a1166d8e50dc35ab135f8002c506ad870d304fd3718b9f289fe849511868` | `598601faaf6c4cf19340417d11b2b50208c9719e954a8d0cd1253e626c002458` |
| B | bedb50f8 | `c8de86781582a9b92347835dd35f89a9a8dd86bf8d97049ae589135c9eb429db` | `7f85babd1725d477f60c06ecd221c03a12f5f7f43603d4f9f01f4ad6bc9a2dae` |

Each worker is `cargo bench --no-run` of that tree with
`-p oxpinyin-store --no-default-features --features bdb --bench
backend_matrix_bdb`, and `-p pinyin-oracle --bench alloc_profile`. The library sizes were built
from 25683b3f and 66579cb2, which differ from de66a0ad and bedb50f8 only
in `crates/oxpinyin-store/benches/support/ops.rs`, a file the libraries
do not compile, and relinked with `relink-versioned.sh` at 6a03de5a.

The branch was then rebased onto main 3a01e11d, which changes no file
this branch touches. The harness and B2 commits on it are 5b2cbd1e and
37639f9a; their `crates/oxpinyin-store/` trees equal de66a0ad's and
bedb50f8's.
