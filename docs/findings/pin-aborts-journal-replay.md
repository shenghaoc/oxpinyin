# Pin aborts reproduced in the journal replay

Status: implemented, 2026-10-31 UTC (member 5a of the stack that retires
class (c)). Rule: [compatibility policy, class (c)
retired](compatibility-policy.md#failure-behaviour).
Pin: libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`, read from a
`git archive` of the pin; `check_result` is `assert` in the reference build
(`src/include/pinyin_utils.h:27-31`). The facade records are
[pin-aborts-pinyin-facade.md](pin-aborts-pinyin-facade.md) and
[pin-aborts-zhuyin-facade.md](pin-aborts-zhuyin-facade.md).

The user `.dbin` of a system library is a `MemoryChunk` of `PhraseIndexLogger`
records. oxpinyin reads the log the way the pin reads it, record by record.
Each assertion that the pin reaches is a typed error at the pin's point. The
C adapters abort on it with `std::process::abort()`. oxpinyin does not
validate every file before it replays any. It has no refusal warning for an
init or a mask at rows 91, 92 and 97, and no replay-stop warning at row 108.

## The pin's walk

`PhraseIndexLogger::next_record` (`src/storage/phrase_index_logger.h:158-215`)
reads each fixed field with `MemoryChunk::get_content`, which reads nothing
past the end (`memory_chunk.h:372-377`):

- A missing type stays `LOG_INVALID_RECORD` and ends the walk through `m_error`.
- A missing token stays `null_token`.
- A missing length stays 0.
- The offset moves on in each case.

An unknown type ends the walk. A `MODIFY_HEADER` asserts
`token == null_token` (`:202`) before the walk reads its length. If the
length of a payload runs past the log, the walk copies it from past the
chunk. This is an over-read with no defined result (class (b)).

`SubPhraseIndex::merge` (`src/storage/phrase_index.cpp:439-517`) applies
the records in order:

- ADD stores the payload and reads its unigram in `add_phrase_item`
  (`:457` → `:215`). For a payload of 0 to 5 bytes, this asserts in the
  `check_result` of `get_unigram_frequency` (`memory_chunk.h:390`).
- REMOVE takes the item out, then returns at a mismatch (`:468-471`).
- MODIFY returns at a mismatch before it changes anything (`:483-484`).
- A header returns when its old total is not the current one (`:509-510`).

The records before a return stay applied.
`_load_phrase_library` ignores the return value (`src/pinyin.cpp:276`,
`src/zhuyin.cpp:219`).

`merge_with_mask` (`:301-330`) returns before it reads the log when the
library bits do not match. Otherwise it builds a new logger
(`mask_out_phrase_index_logger`, `:844-869`) in three walks over the old one:

1. `_peek_header` (`:721-746`) walks every record, so `:202` fires for a
   bad header anywhere. It then asserts `1 >= header_count` (`:745`). No
   header gives an empty logger.
2. `_compute_new_header` (`:750-804`) reads the unigram of every record
   that the mask keeps (`:778`, `:785`, `:791`, `:795`). This asserts at
   `memory_chunk.h:390` on a payload under six bytes.
3. `_mask_out_records` (`:817-842`) appends each kept record with both chunk
   pointers (`:838`). `append_record` asserts `NULL == oldone` for an ADD
   (`phrase_index_logger.h:245`) and `NULL == newone` for a REMOVE (`:257`).

The new logger is then merged.

A record type that `next_record` does not know ends `_peek_header` with
`m_error` set (`phrase_index_logger.h:214-216`). `rewind()` resets only the
offset (`:141-144`), and `has_next_record` stays false (`:128-129`). Thus,
after the count and assert of `_peek_header`, the two later walks read
nothing. The new logger is the header alone, with its total unchanged
(`phrase_index.cpp:851-869`). `LogReader::end` reports the end
(`LogEnd::Complete`, `LogEnd::UnknownType`, `LogEnd::OverRead`).
`masked_log` stops after the header checks on `LogEnd::UnknownType`. Thus a
kept ADD or REMOVE after the unknown type never reaches `:245` or `:257`.
Held by
`an_unknown_record_type_ends_every_masked_walk`,
`the_walk_reports_why_it_ended` and `mask-out-log-kept-add-bad-tail-*`.

## Preceding work

The init loop maps library `i`. It writes `mmap %s failed!` if the system
chunk of that library is absent (`pinyin.cpp:253-256`, `zhuyin.cpp:197-200`).
It then merges the log, where rows 91 and 108 assert (`:276`). Thus, when
init dies at library `i`, stderr has the lines of the absent libraries up to
and including `i`, and not those after it. `Runtime::open_with_law_reporting`
reports them the same way. The library loop stops at the fault, so its list of
absent libraries holds only those that it reached
([init loop](pin-aborts-remaining.md#the-init-library-loop-2026-11-06-utc)). Held by
`a_journal_fault_reports_only_the_libraries_through_its_own`.

Before these aborts, the pin changes only in-memory state. The exception is
the writes of `check_format` at init: libpinyin writes `user.conf`, and both
facades unlink the files of a non-conforming profile (`pinyin.cpp:172-218`,
`zhuyin.cpp:126-162`). The user tables of the pin are in-memory DBMs written
at save. The libraries before the faulty one are mapped and merged in memory.
The only stderr that the pin can write first is the `mmap %s failed!` line of
an earlier library. Such a library dies of SIGSEGV at `phrase_index.cpp:354`
(class (b), row 76) before the pin reaches a later library. Thus that line
never comes before an abort.

oxpinyin's init runs `check_format` first. The replay is the last profile
step of the user-store open, and it stops at the first faulty library in
sub-index order. `mask_out` masks the user tables first (rows 84, 86 and 87
abort there). It then reloads the libraries in the order of the mask loop
(`pinyin.cpp:1234-1291`, `zhuyin.cpp:769-826`). Everything that it changes
is session memory.

## Sites

| Row | Pin site(s) | Paths | oxpinyin item that returns the fault | Cases | Disposition |
|---|---|---|---|---|---|
| 91 | `storage/phrase_index_logger.h:202` `assert`, through `merge` and `_peek_header` | init's library loop; reload (`*_load_phrase_library`, `pinyin.cpp:276`, `zhuyin.cpp:219`); `mask_out` | `LogReader` (`LogFault::HeaderToken`) → `JournalFault::HeaderToken` from `persistence::merge_log` and `masked_log`; `OpenError::Journal`, `LibraryLoadFault::Journal`, `MaskOutOutcome::Journal` | `abort-init-log-header-token-*` (`userdir`), `abort-mask-out-log-header-token-*`, `abort-reload-log-header-token-*` (`userdir`); `replay-stop-before-header-token-{remove,modify}-*` (init completes) | POLICY-ACCEPTED; the stop cases MATCH |
| 92 | `storage/phrase_index.cpp:745` `assert` | `mask_out` | `masked_log` → `JournalFault::MultipleHeaders` | `abort-mask-out-multiple-log-headers{,-bad-tail}-*`, `abort-mask-out-log-header-untouched-*` (`userdir`), `abort-mask-out-second-library-multiple-log-headers-*` (`userdir`); controls `init-multiple-log-headers-*`, `mask-out-log-header-bad-tail-*`, `mask-out-unloaded-library-log-*`, `mask-out-second-library-other-bits-*` | POLICY-ACCEPTED; controls MATCH |
| 97 | `storage/phrase_index_logger.h:245` (ADD) and `:257` (REMOVE) `assert`, through `phrase_index.cpp:838` | `mask_out` | `masked_log` → `JournalFault::SurvivingRecord` | `abort-mask-out-surviving-add-*` (`userdir`); control `mask-out-system-selected-add-*` | POLICY-ACCEPTED; control MATCH |
| 108 | `include/memory_chunk.h:390` `assert` (`check_result`), through `phrase_index.cpp:457` → `:215`, and through `_compute_new_header` (`:778`-`:795`) | init's library loop; reload; `mask_out` | `Merge::apply` (`Stop::ShortAdd`) → `JournalFault::ShortAdd`; `masked_log` → `JournalFault::ShortMaskedItem` | `replay-short-add-{one-byte,five-bytes}-*` (`userdir`), `abort-reload-short-add-*` (`userdir`), `replay-short-add-before-over-read-*` and `replay-reload-short-add-before-over-read-*` (`userdir`) | POLICY-ACCEPTED |

`*` is `pinyin` and `zhuyin`. These tests hold the behaviour:

- The adapters: the death tests `init_aborts_on_a_log_header_token`,
  `init_aborts_on_a_short_add_payload`, `reload_aborts_on_a_log_header_token`
  and `mask_out_aborts_on_a_surviving_add` (both `e2e_tests.rs`), and the
  control `init_completes_when_the_merge_stops_before_a_bad_header`.
- The typed faults: the user crate's `a_header_token_the_merge_reaches_is_the_load_fault`,
  `a_header_token_after_a_mismatch_is_never_read`,
  `a_short_add_payload_is_the_load_fault` and
  `mask_out_replays_the_log_it_reads_and_returns_its_faults`.
- The walk: `user_files.rs`.

Notes:

- The kept-REMOVE assert (`:257`) and the unigram read of `_compute_new_header`
  are the same `merge_with_mask` walks as rows 97 and 92. They return the
  fault where the pin asserts. No contract case builds them; the user crate's
  test does.
- For an unknown record type or a header without its totals, the pin applies
  the records before it and reads a missing field as its initial value. The
  walk does the same. `an_unknown_record_type_keeps_the_records_before_it` and
  the `user_files.rs` field tests hold the prefix.
- A payload past the end of the log is the pin's over-read. It is class (b),
  with the parent's outcome, but only where the walk reaches it in the pin's
  order. The pin's `merge` reads each record as it applies it
  (`phrase_index.cpp:445-450`). Thus every fault or stop before the
  over-read comes first:
  - a header token (row 91, `:202`);
  - a short ADD (row 108, `memory_chunk.h:390`);
  - the mismatch stops (`phrase_index.cpp:468-471`, `:483-484`). After a
    mismatch stop, the walk never reaches the over-read, and the records
    before the stop stay applied.

  `merge_log` walks the log once and reports `MergeEnd::OverRead` when it
  reaches the record. Only then, at init and at a reload, is the whole log
  skipped. The parent's `load_logs_with_mask` did the same when
  `validate_log_records` refused the log (`persistence.rs` at `7b1d40a2`). The
  `Merge` of the library is local until `finish`, so dropping it is the skip.
  No pre-scan reads ahead of the replay. A bounded pre-scan that stopped
  at the first fault or stop would need the merge's own state to find
  the mismatch stops, so it would be a second merge. Held by
  `a_short_add_before_an_over_read_is_the_fault`,
  `a_header_token_before_an_over_read_is_the_fault`,
  `a_mismatch_before_an_over_read_keeps_the_records_before_the_stop`,
  `an_over_read_the_walk_reaches_skips_the_whole_log_at_reload_too` and
  `a_payload_past_the_log_skips_the_whole_log_as_before`, and by
  `replay-short-add-before-over-read-*` (init) and
  `replay-reload-short-add-before-over-read-*` (reload).

  At `mask_out`, `_peek_header` walks to the over-read before any later walk
  runs. Thus `masked_log` keeps the records before it, as the parent's
  `validate_system_logs` snapshot of `decode_log_records_partial` kept them
  (`store.rs` at `7b1d40a2`). The mask reads of rows 91, 92, 97 and 108 before
  the over-read still fault. `mask_out_faults_before_an_over_read_come_first`
  holds this.
- Row 107's class (b) handling (a REMOVE of an absent token, a stale
  MODIFY, a read past an item) is unchanged in `replay.rs`.
- The library loop is per library (`library_loop::run`, over
  `persistence::replay_log`). Thus the row judgement and mapping of each row
  are at the pin's position ([record](pin-aborts-remaining.md#the-init-library-loop-2026-11-06-utc)).

## Fuzzing

No fuzz target feeds a `.dbin` to the replay:

- `capi_commands.rs` opens a new user directory, and its alphabet has no
  save, mask or library load.
- `user_store_ops.rs` drives a standalone store, which has no system libraries.
- `dict_loader.rs`, `phrase_library.rs`, `fixture_model.rs` and
  `table_conf.rs` read system files only.

No harness change is necessary.

## Verification

The commands and results are in the change's report.
