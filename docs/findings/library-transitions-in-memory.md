# Library transitions in memory

Status: implemented, 2026-10-31 UTC. Rule: [compatibility policy, class (c)
retired](compatibility-policy.md#failure-behaviour).
Rows: 103, and row 62's store-read sub-shape. Pin: libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`, read from a `git archive` of
the pin. The other records of the stack are
[pin-aborts-pinyin-facade.md](pin-aborts-pinyin-facade.md),
[pin-aborts-zhuyin-facade.md](pin-aborts-zhuyin-facade.md) and
[pin-aborts-journal-replay.md](pin-aborts-journal-replay.md).

Neither row has a pin abort to reproduce. The pin's state for a library
unload or reload and for the phrase export is in memory and cannot fail.
oxpinyin also has no failing site there: no user-store write or read on
these paths can refuse.

## The pin

`pinyin_unload_phrase_library` does these steps (`src/pinyin.cpp:464-475`):

1. It asserts that the index is below 16.
2. It returns `false` for every library but `GBK_DICTIONARY`.
3. It calls `FacadePhraseIndex::unload`, and returns `true` whatever that call returned.

`zhuyin_unload_phrase_library` does the same for every library but
`TSI_DICTIONARY` (`src/zhuyin.cpp:379-390`). Thus the zhuyin facade can also
unload the `USER_FILE` libraries. `unload` deletes
the sub-index and takes its total out of the facade total
(`src/storage/phrase_index.cpp:261-268`). Nothing is read from or written
to disk.

`pinyin_load_phrase_library` returns `false` for an index of 16 or more.
It asserts that the row is `SYSTEM_FILE` or `USER_FILE` (`:446-459`), and calls
`_load_phrase_library` (`:234-322`; zhuyin `:178-266`). That function returns
`false` when `get_range` finds the sub-index loaded (`:242-244`).

- For a system library, it maps the system chunk and loads it as the new
  sub-index (`FacadePhraseIndex::load`, `phrase_index.cpp:237-248`). It then
  loads the user `.dbin` into a `MemoryChunk` and merges it (`:262-276`).
  `MemoryChunk::load` returns `false` and leaves the chunk empty when the
  file cannot be opened, is shorter than its header, or has a wrong length
  or checksum (`src/include/memory_chunk.h:416-464`). The merge of an empty
  log changes nothing.
- For a `USER_FILE` library, it loads the saved chunk. When the chunk does
  not load, it calls `create_sub_phrase` for an empty sub-index (`:303-321`).

The return value is `true`. Only the asserts of the merge (rows 91
and 108) and the mapping of a directory (row 96) can stop it.

`pinyin_save` and `zhuyin_save` write and rename the library files in
sub-index order. They skip a library whose `get_range` returns
`ERROR_NO_SUB_PHRASE_INDEX` (`pinyin.cpp:929-932` in `_write_files`,
`:1032-1035` in `_rename_files`; `zhuyin.cpp:562-565`). The saved file of an
unloaded library stays on disk as it was. Neither transition sets
`m_modified`.

The export iterator reads the sub-index through `get_range` and
`get_phrase_item` (`pinyin.cpp:662-768`), both in memory. Its only
failures are the asserts of row 62.

## oxpinyin

The values of a libpinyin session are process memory. `open_session` is the
only constructor of a libpinyin target, and it uses the in-memory
`SessionValues`. The session phrases live in per-library arenas (#1455).
A transition changes only that session state and the runtime's visibility
mask. A save skips an unloaded library, so it does not write an emptied
sub-index over the saved `.dbin` or user chunk.

- `ContextCore::unload_phrase_library` returns `bool`. It drops the
  arena, overrides, unigram deltas and facade share of the library. It sets
  the mask bit and returns the runtime's `true`.
- `ContextCore::load_phrase_library` returns `Result<bool,
  LibraryLoadFault>`, where the fault is only a pin assert: `Row`
  (`NotLoadable` at `pinyin.cpp:457`, `ChunkHeaderRead` for row 96) or
  `Journal` (rows 91 and 108). Before it clears the mask, it replaces the
  library with the system chunk plus the saved `.dbin`, or with the saved
  user chunk or an empty one.
  A system file that cannot be mapped again (row 76) replaces nothing.
  The library stays unloaded behind its mask, the call still returns
  `true`, and the next load maps the file again. The mask clears only
  for a library that the remap mapped. A `SYSTEM_FILE` row is mapped
  whether or not it names a user file (`pinyin.cpp:246-264`).
- `UserStore::unload_system_libraries`, `load_user_library` and
  `unload_user_library` return nothing. `load_system_libraries` and
  `reload_system_libraries` return only `JournalFault`. Their table writes
  go through `in_session` (`crates/oxpinyin-user/src/store.rs`). On the
  in-memory values of a session, these writes cannot fail. The transaction
  refuses only an empty or NUL table name or a re-entrant borrow, and the
  rows are the store's own encodings.
- `arena_replacement` keeps structurally valid empty persisted items
  within the current arena limits. It still leaves out an item that the arena
  cannot hold. The arena-past-`u32` omission is an interim **KNOWN
  DIVERGENCE** from the pin's wrap/overwrite behavior, tracked in
  [#1514](https://github.com/shenghaoc/oxpinyin/issues/1514).
- The save skips an unloaded library. `Runtime::save_disk_baseline`
  returns `SaveBaseline::Unloaded` for an unloaded system library. The
  session records an unloaded `USER_FILE` library in
  `SessionPhrases::unloaded`. The save does not write or rename either file.
- The export cursor reads are infallible. `phrase_export_cursor` returns
  the cursor, and `get_next_with` returns the `ExportStep`. A session
  serves both reads from its arenas. No C context holds a standalone row
  store. Such a store still maps an undecodable item to its assert site, and
  reads any other failure as no item.

The code has none of these: `store_first`, `StoreWriteFailed`,
`LibraryLoadFault::StoreWrite`, `fail_replace` and `fail_next_replace`, the
two refusal warnings in each C adapter, the NULL and `false` arms of the
export iterator with their warnings, and `store_first_tests`.

These paths add no abort. The fuzz harnesses need no filter:

- `capi_commands.rs` walks the export iterator, which has no refusal arm.
  Its alphabet has no library load or unload.
- `user_store_ops.rs` drives a standalone store, which has no library transitions.

## Tests and cases

Unit tests:

- `library_transition_tests` in `crates/oxpinyin-facade/src/context.rs`:
  `unload_then_reload_restores_the_saved_library`,
  `unload_then_save_leaves_the_unloaded_librarys_log_alone` and
  `reload_after_an_unreadable_log_loads_the_system_chunk_alone` (a missing
  and a short `.dbin`).
- `a_save_skips_an_unloaded_user_library` and
  `library_transitions_keep_the_independent_index` in `crates/oxpinyin-user/src/store_libpinyin.rs`.
- `an_undecodable_item_is_an_assert_site_and_a_store_failure_is_not` in
  `crates/oxpinyin-facade/src/export_rows.rs`.

Contract cases (recorded evidence):

| Case | What it holds | Parent build |
|---|---|---|
| `unload-save-skips-library-{pinyin,zhuyin}` | save, unsaved change, unload GBK, save, load (`userdir`, `stderr`) | DIFFER |
| `unload-save-skips-user-library-zhuyin` | the same for the `USER_FILE` library 7 | DIFFER |
| `reload-unreadable-log-{missing,short}-{pinyin,zhuyin}` | reload over a missing or short `.dbin` | MATCH (controls) |

The earlier cases of unload, reload and candidates are
`reload-drops-session-*`, `reload-keeps-saved-*`, `reload-refused-*`,
`reload-user-zhuyin-*`, `reload-dangling-token-candidates{,-zhuyin}`,
`reload-total-while-unloaded`, `unload-drops-accepted-bump`, the
`library-*` and `stderr-library-control-*` loading cases (including
`stderr-library-control-save-unloaded`) and `save-unloaded-replacement{,-zhuyin}`.
`an_unloaded_library_leaves_the_model` (`crates/oxpinyin-data/tests/language_model.rs`)
holds `reload-total-while-unloaded`.
The export iterator cases are those whose names contain `iterator`.

## Open items

- Arena-limit behavior: see #1514 user content offset wrap.

The pin's `pinyin_mask_out` is sequential. It masks the tables and bigrams,
then loads and merges each library, then compacts
(`src/pinyin.cpp:1224-1295` at `074a2219`). The empty-item cases show
that items are kept within the current arena limits. They give no evidence
about failure ordering.

## Verification

The Rust tests named above hold the behaviour. The contract cases are
recorded evidence.
