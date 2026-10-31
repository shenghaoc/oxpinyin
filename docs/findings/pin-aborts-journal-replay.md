# Pin aborts reproduced in the journal replay

Status: implemented, 2026-10-31 UTC (member 5a of the stack that retires
class (c)). Rule: [compatibility policy, class (c)
retired](compatibility-policy.md#amendment--class-c-retired-pin-aborts-reproduced-2026-10-31-utc).
Pin: libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`, read from a
`git archive` of the pin; `check_result` is `assert` in the reference build
(`src/include/pinyin_utils.h:27-31`). The facade records are
[pin-aborts-pinyin-facade.md](pin-aborts-pinyin-facade.md) and
[pin-aborts-zhuyin-facade.md](pin-aborts-zhuyin-facade.md).

A system library's user `.dbin` is a `MemoryChunk` of `PhraseIndexLogger`
records. oxpinyin used to validate every such file before it replayed any,
refuse an init or a mask with one warning at rows 91, 92 and 97, and stop
a replay with a warning at row 108. Those refusals are deleted. The log is
now read the way the pin reads it, record by record, and each assertion
the pin reaches is a typed error at the pin's point. The C adapters abort
on it with `std::process::abort()`.

## The pin's walk

`PhraseIndexLogger::next_record` (`src/storage/phrase_index_logger.h:158-215`)
reads each fixed field with `MemoryChunk::get_content`, which reads nothing
past the end (`memory_chunk.h:372-377`): a missing type stays
`LOG_INVALID_RECORD` and ends the walk through `m_error`, a missing token
stays `null_token`, a missing length stays 0, and the offset moves on
either way. An unknown type ends the walk. A `MODIFY_HEADER` asserts
`token == null_token` (`:202`) before its length is read. A payload whose
length runs past the log is copied from past the chunk, an over-read with
no defined result (class (b)).

`SubPhraseIndex::merge` (`src/storage/phrase_index.cpp:439-517`) applies
the records in order. ADD stores the payload and reads its unigram in
`add_phrase_item` (`:457` → `:215`), which asserts in
`get_unigram_frequency`'s `check_result` (`memory_chunk.h:390`) for a payload
of 0 to 5 bytes. REMOVE takes the item out, then returns at a mismatch
(`:468-471`); MODIFY returns at a mismatch before it changes anything
(`:483-484`); a header returns when its old total is not the current one
(`:509-510`). The records before a return stay applied.
`_load_phrase_library` ignores the answer (`src/pinyin.cpp:276`,
`src/zhuyin.cpp:219`).

`merge_with_mask` (`:301-330`) returns before it reads the log when the
library bits do not match, then builds a new logger
(`mask_out_phrase_index_logger`, `:844-869`) in three walks over the old
one: `_peek_header` (`:721-746`) walks every record, so `:202` fires for a
bad header anywhere, then asserts `1 >= header_count` (`:745`); no header
gives an empty logger. `_compute_new_header` (`:750-804`) reads the unigram
of every record the mask keeps (`:778`, `:785`, `:791`, `:795`), which
asserts at `memory_chunk.h:390` on a payload under six bytes.
`_mask_out_records` (`:817-842`) appends each kept record with both chunk
pointers (`:838`): `append_record` asserts `NULL == oldone` for an ADD
(`phrase_index_logger.h:245`) and `NULL == newone` for a REMOVE (`:257`).
The new logger is merged.

## Preceding work

Before these aborts the pin changes only in-memory state, except
`check_format`'s writes at init (libpinyin writes `user.conf`; both
facades unlink a non-conforming profile's files, `pinyin.cpp:172-218`,
`zhuyin.cpp:126-162`). Its user tables are in-memory DBMs written at save,
and the libraries before the faulty one are mapped and merged in memory.
The only stderr the pin can write first is an earlier library's
`mmap %s failed!` line, and such a library dies of SIGSEGV at
`phrase_index.cpp:354` (class (b), row 76) before a later library is
reached, so no abort is preceded by it. oxpinyin's init runs `check_format`
first, as before; the replay is the user-store open's last profile step and
stops at the first faulty library in sub-index order. `mask_out` masks the
user tables first (rows 84, 86 and 87 abort there), then reloads the
libraries in the mask loop's order (`pinyin.cpp:1234-1291`,
`zhuyin.cpp:769-826`); everything it changes is session memory.

## Sites

| Row | Pin site(s) | Paths | oxpinyin item that returns the fault | Cases | Disposition |
|---|---|---|---|---|---|
| 91 | `storage/phrase_index_logger.h:202` `assert`, through `merge` and `_peek_header` | init's library loop; reload (`*_load_phrase_library`, `pinyin.cpp:276`, `zhuyin.cpp:219`); `mask_out` | `LogReader` (`LogFault::HeaderToken`) → `JournalFault::HeaderToken` from `persistence::merge_log` and `masked_log`; `OpenError::Journal`, `LibraryLoadFault::Journal`, `MaskOutOutcome::Journal` | `abort-init-log-header-token-*` (`userdir`), `abort-mask-out-log-header-token-*`, `abort-reload-log-header-token-*` (`userdir`); `replay-stop-before-header-token-{remove,modify}-*` (init completes) | POLICY-ACCEPTED; the stop cases MATCH |
| 92 | `storage/phrase_index.cpp:745` `assert` | `mask_out` | `masked_log` → `JournalFault::MultipleHeaders` | `abort-mask-out-multiple-log-headers{,-bad-tail}-*`, `abort-mask-out-log-header-untouched-*` (`userdir`), `abort-mask-out-second-library-multiple-log-headers-*` (`userdir`); controls `init-multiple-log-headers-*`, `mask-out-log-header-bad-tail-*`, `mask-out-unloaded-library-log-*`, `mask-out-second-library-other-bits-*` | POLICY-ACCEPTED; controls MATCH |
| 97 | `storage/phrase_index_logger.h:245` (ADD) and `:257` (REMOVE) `assert`, through `phrase_index.cpp:838` | `mask_out` | `masked_log` → `JournalFault::SurvivingRecord` | `abort-mask-out-surviving-add-*` (`userdir`); control `mask-out-system-selected-add-*` | POLICY-ACCEPTED; control MATCH |
| 108 | `include/memory_chunk.h:390` `assert` (`check_result`), through `phrase_index.cpp:457` → `:215`, and through `_compute_new_header` (`:778`-`:795`) | init's library loop; reload; `mask_out` | `Merge::apply` (`Stop::ShortAdd`) → `JournalFault::ShortAdd`; `masked_log` → `JournalFault::ShortMaskedItem` | `replay-short-add-{one-byte,five-bytes}-*` (`userdir`), `abort-reload-short-add-*` (`userdir`) | POLICY-ACCEPTED |

`*` is `pinyin` and `zhuyin`. The death tests `init_aborts_on_a_log_header_token`,
`init_aborts_on_a_short_add_payload`, `reload_aborts_on_a_log_header_token`
and `mask_out_aborts_on_a_surviving_add` (both `e2e_tests.rs`) and the
control `init_completes_when_the_merge_stops_before_a_bad_header` hold the
adapters; the user crate's `a_header_token_the_merge_reaches_is_the_load_fault`,
`a_header_token_after_a_mismatch_is_never_read`,
`a_short_add_payload_is_the_load_fault` and
`mask_out_replays_the_log_it_reads_and_returns_its_faults` hold the
typed faults, and `user_files.rs` holds the walk.

Notes:

- The kept-REMOVE assert (`:257`) and `_compute_new_header`'s unigram read
  are the same `merge_with_mask` walks as rows 97 and 92; they had no
  oxpinyin arm before (a kept REMOVE was replayed, a short kept payload
  read as 0). They now return the fault where the pin asserts. No
  contract case builds them; the user crate's test does.
- An unknown record type or a header without its totals used to make
  oxpinyin skip the whole file; the pin applies the records before it and
  reads a missing field as its initial value. The walk now does the same,
  so `an_unknown_record_type_keeps_the_records_before_it` and the
  `user_files.rs` field tests hold the prefix.
- A payload past the log's end is the pin's over-read: class (b),
  unchanged by this stack, outcome as before. At init and at a reload the
  log is skipped whole, as the parent's `load_logs_with_mask` did when
  `validate_log_records` refused it (`persistence.rs` at `7b1d40a2`);
  `persistence::replay_logs` checks the walk for `LogFault::OverRead`
  before it merges, held by
  `a_payload_past_the_log_skips_the_whole_log_as_before`. At `mask_out`
  the records before it are kept, as the parent's `validate_system_logs`
  snapshot of `decode_log_records_partial` kept them (`store.rs` at
  `7b1d40a2`). A header token before the over-read still faults, as it
  did.
- Row 107's class (b) handling (a REMOVE of an absent token, a stale
  MODIFY, a read past an item) is unchanged in `replay.rs`.
- The library loop is per library (`persistence::replay_logs`), so the
  mapping check member 5c adds for row 96 can sit at the pin's position.

## Fuzzing

No fuzz target feeds a `.dbin` to the replay: `capi_commands.rs` opens a
fresh user directory and its alphabet has no save, mask or library load;
`user_store_ops.rs` drives a standalone store, which has no system
libraries; `dict_loader.rs`, `phrase_library.rs`, `fixture_model.rs` and
`table_conf.rs` read system files only. No harness change is needed.

## Verification

The commands and results are in the change's report.
