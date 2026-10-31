# Library transitions in memory

Status: implemented, 2026-10-31 UTC (member 5b of the stack that retires
class (c)). Rule: [compatibility policy, class (c)
retired](compatibility-policy.md#amendment--class-c-retired-pin-aborts-reproduced-2026-10-31-utc).
Rows: 103, and row 62's store-read sub-shape. Pin: libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`, read from a `git archive` of
the pin. The other records of the stack are
[pin-aborts-pinyin-facade.md](pin-aborts-pinyin-facade.md),
[pin-aborts-zhuyin-facade.md](pin-aborts-zhuyin-facade.md) and
[pin-aborts-journal-replay.md](pin-aborts-journal-replay.md).

Neither row had a pin abort to reproduce. Both were oxpinyin refusals with
no pin counterpart: a user store write that could fail during a library
unload or reload, and a user store read that could fail during the phrase
export. The pin's state for both is in memory and cannot fail. The
refusals are deleted, and the implementation now has no failing site
either.

## The pin

`pinyin_unload_phrase_library` asserts the index is below 16, answers
`false` for every library but `GBK_DICTIONARY`, then calls
`FacadePhraseIndex::unload` and answers `true` whatever it returned
(`src/pinyin.cpp:464-475`). `zhuyin_unload_phrase_library` does the same
for every library but `TSI_DICTIONARY` (`src/zhuyin.cpp:379-390`), so the
zhuyin facade can also unload the `USER_FILE` libraries. `unload` deletes
the sub-index and takes its total out of the facade total
(`src/storage/phrase_index.cpp:261-268`). Nothing is read from or written
to disk.

`pinyin_load_phrase_library` answers `false` for an index of 16 or more,
asserts the row is `SYSTEM_FILE` or `USER_FILE` (`:446-459`), and calls
`_load_phrase_library` (`:234-322`; zhuyin `:178-266`). That answers
`false` when `get_range` finds the sub-index loaded (`:242-244`). For a
system library it maps the system chunk, loads it as the new sub-index
(`FacadePhraseIndex::load`, `phrase_index.cpp:237-248`), loads the user
`.dbin` into a `MemoryChunk` and merges it (`:262-276`). `MemoryChunk::load`
answers `false` and leaves the chunk empty when the file cannot be opened,
is shorter than its header, or has a wrong length or checksum
(`src/include/memory_chunk.h:416-464`). The merge of an empty log changes
nothing. For a `USER_FILE` library it loads the saved chunk, or calls
`create_sub_phrase` for an empty sub-index when the chunk does not load
(`:303-321`). The answer is `true`. Only the merge's own asserts (rows 91
and 108, member 5a) and the mapping of a directory (row 96, member 5c)
can stop it.

`pinyin_save` and `zhuyin_save` write and rename the library files in
sub-index order and skip a library whose `get_range` answers
`ERROR_NO_SUB_PHRASE_INDEX` (`pinyin.cpp:929-932` in `_write_files`,
`:1032-1035` in `_rename_files`; `zhuyin.cpp:562-565`). The saved file of an
unloaded library stays on disk as it was. Neither transition sets
`m_modified`.

The export iterator reads the sub-index through `get_range` and
`get_phrase_item` (`pinyin.cpp:662-768`), both in memory. Its only
failures are the asserts of row 62.

## oxpinyin before

`ContextCore::unload_phrase_library` and `load_phrase_library`
(`crates/oxpinyin-facade/src/context.rs`) ran each transition through
`store_first`. The user store's replacement was written first, and the
runtime's visibility mask changed only when the write succeeded. A failed
write answered `false` and emitted one `g_warning` in each C adapter
(`LibraryLoadFault::StoreWrite`, `StoreWriteFailed`). The user store's
transitions (`UserStore::{load,unload}_system_libraries`,
`{load,unload}_user_library`) returned `UserStoreError`, and a test-only
`fail_replace` flag injected the failure.

A save did not know a library was unloaded. It wrote the emptied sub-index
over the saved `.dbin` or user chunk and renamed it into place. The
`unload-save-skips-*` cases below DIFFER against the parent build for that
reason.

`ContextCore::phrase_export_cursor` and `PhraseExportCursor::get_next_with`
returned `Result<_, UserStoreError>`. `pinyin_begin_get_phrases` answered
NULL with one warning, and `pinyin_iterator_get_next_phrase` answered
`false` with one warning, for a store that could not be read.

## oxpinyin after

A libpinyin session's values are process memory. `open_session` is the
only constructor of a libpinyin target, and it uses the in-memory
`SessionValues`. Since #1455 the session phrases live in per-library
arenas. A transition now changes only that session state and the runtime's
visibility mask.

- `ContextCore::unload_phrase_library` returns `bool`. It drops the
  library's arena, overrides, unigram deltas and facade share, sets the
  mask bit and answers the runtime's `true`.
- `ContextCore::load_phrase_library` returns `Result<bool,
  LibraryLoadFault>`, where the fault is only a pin assert: `Row`
  (`NotLoadable` at `pinyin.cpp:457`, `ChunkHeaderRead` for row 96) or
  `Journal` (rows 91 and 108). It replaces the library with the system
  chunk plus the saved `.dbin`, or with the saved user chunk or an empty
  one, before it clears the mask.
- `UserStore::unload_system_libraries`, `load_user_library` and
  `unload_user_library` return nothing. `load_system_libraries` and
  `reload_system_libraries` return only `JournalFault`. Their table writes
  go through `in_session` (`crates/oxpinyin-user/src/store.rs`). On a
  session's in-memory values these writes cannot fail: the transaction
  refuses only an empty or NUL table name or a re-entrant borrow, and the
  rows are the store's own encodings.
- `arena_replacement` leaves out an item the arena cannot hold: a
  zero-length phrase that `encode_phrase_item` refuses, or an arena past
  `u32` offsets. See the open items.
- The save skips an unloaded library. `Runtime::save_disk_baseline`
  answers `SaveBaseline::Unloaded` for an unloaded system library, and the
  session records an unloaded `USER_FILE` library in
  `SessionPhrases::unloaded`. Neither file is written or renamed.
- The export cursor reads are infallible. `phrase_export_cursor` returns
  the cursor, and `get_next_with` returns the `ExportStep`. A session
  answers both reads from its arenas. A standalone row store, which no C
  context holds, still maps an undecodable item to its assert site and
  reads any other failure as no item.

Deleted: `store_first`, `StoreWriteFailed`, `LibraryLoadFault::StoreWrite`,
`fail_replace` and `fail_next_replace`, the two refusal warnings in each C
adapter, the export iterator's NULL and `false` arms with their warnings,
and `store_first_tests`.

No abort is added. The fuzz harnesses need no filter: `capi_commands.rs`
walks the export iterator, which now has no refusal arm, and has no
library load or unload in its alphabet. `user_store_ops.rs` drives a
standalone store, which has no library transitions.

## Tests and cases

Unit tests:

- `library_transition_tests` in `crates/oxpinyin-facade/src/context.rs`:
  `unload_then_reload_restores_the_saved_library`,
  `unload_then_save_leaves_the_unloaded_librarys_log_alone` and
  `reload_after_an_unreadable_log_loads_the_system_chunk_alone` (a missing
  and a short `.dbin`). These replace `store_first_tests`.
- `a_save_skips_an_unloaded_user_library` and
  `library_transitions_keep_the_independent_index` (which replaces the
  failure-injection test) in `crates/oxpinyin-user/src/store_libpinyin.rs`.
- `an_undecodable_item_is_an_assert_site_and_a_store_failure_is_not` in
  `crates/oxpinyin-facade/src/export_rows.rs`.

New cases in `tools/bisection/contract_library_reload.py`:

| Case | What it holds | Parent build |
|---|---|---|
| `unload-save-skips-library-{pinyin,zhuyin}` | save, unsaved change, unload GBK, save, load (`userdir`, `stderr`) | DIFFER |
| `unload-save-skips-user-library-zhuyin` | the same for the `USER_FILE` library 7 | DIFFER |
| `reload-unreadable-log-{missing,short}-{pinyin,zhuyin}` | reload over a missing or short `.dbin` | MATCH (controls) |

The existing cases already cover unload, reload and candidates:
`reload-drops-session-*`, `reload-keeps-saved-*`, `reload-refused-*`,
`reload-user-zhuyin-*`, `reload-dangling-token-candidates{,-zhuyin}`,
`reload-total-while-unloaded`, `unload-drops-accepted-bump`, the
`library-*` and `stderr-library-control-*` loading cases (including
`stderr-library-control-save-unloaded`) and `save-unloaded-replacement{,-zhuyin}`.
The export iterator cases are those whose names contain `iterator`.

## Open items

- `reload-user-zhuyin-reuse-same` DIFFERs on all three cells, on this
  member and on its parent: both sides die of SIGABRT, but the case still
  declares the class (c) `abort=False` matcher. Member 4 reproduced the
  pin's abort at `zhuyin.cpp:454` (row 104); the case needs `sigabrt=True`
  on that member.
- `arena_replacement` leaves out an item the arena cannot hold, while
  `open_session` refuses the same log and the pin keeps the item. Only a
  `.dbin` written outside the library can hold such an item.

## Verification

The commands and results are in the change's report.
