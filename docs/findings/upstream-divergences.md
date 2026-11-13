# Upstream divergences

Status: **register**.

## Purpose and entry form

This register holds every difference between oxpinyin and the pin, libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`: each accepted divergence, each pin abort that oxpinyin reproduces, and each pin defect to report upstream.
The [compatibility policy](compatibility-policy.md) gives each difference's class and policy row.
Pin citations use the pinned revision.

Each difference is one entry of ten lines at most, under its own heading:

- **Pin:** what the pin does, with the file and line cite.
- **oxpinyin:** what oxpinyin does.
- **Decision:** accepted or reproduced, with the reason in one sentence.
- **Rejected:** the alternative, if there was one.
- **Tests:** the test names.

An entry holds nothing else.

## Accepted divergences — C facades (class (b))

### Bigram export iterator's pinyin buffer

- **Pin:** `pinyin_bigram_iterator_has_next_phrase` builds each pronunciation in a `GPtrArray` that it never NULL-terminates, then passes it to `g_strjoinv` and `g_strfreev` (`src/pinyin.cpp:844-850`, `:857-863`; the unigram export adds the NULL at `:730`). Both read past the array, and after one train the first export cycle can die of SIGSEGV.
- **oxpinyin:** `BigramExportWalk` (`crates/oxpinyin-facade/src/export_rows.rs`) builds each bigram row’s pronunciation strings by pairing the two tokens’ readings and joining each pair with an apostrophe. The C iterator duplicates the current pronunciation for the caller to free with `g_free`.
- **Decision:** accepted, class (b) (policy row 1): no safe construction reproduces a read past an allocation.
- **Tests:** `user-gram-unsorted-export`, `user-gram-duplicate-export`; `e2e_tests::the_bigram_export_crosses_both_items_readings`, `e2e_tests::bigram_export_reads_a_new_pronunciation_when_the_next_pair_loads`.

### Chewing auxiliary substring over-read

- **Pin:** `pinyin_get_chewing_auxiliary_text` calls `g_utf8_substring(zhuyin, len, end)` with the absolute raw offset `m_raw_end` as `end_pos` (`src/pinyin.cpp:3555-3560`). GLib steps `end_pos - len` scalars with no terminator check (GLib 2.89.4 `glib/gutf8.c:298-322`, `:376-386`), so for an interior cursor the copy passes the NUL and can read past the allocation.
- **oxpinyin:** `split_zhuyin` (`crates/oxpinyin-capi/src/aux_matrix.rs`) and `chewing_aux_text` (`text.rs`) cut the spelling at scalar `len`, with an empty right half at or past the scalar count. For `len` up to the scalar count the visible bytes are the pin's.
- **Decision:** accepted, class (b): no safe, deterministic reproduction of the over-read exists.
- **Tests:** `aux_matrix::tests::chewing_cuts_the_zhuyin_spelling_by_scalar`, `boundary_tests::chewing_auxiliary_text_clamps_a_cursor_past_the_input`.

### Unterminated punctuation value over-read

- **Pin:** `PunctTableEntry::unescape` reads the word after the value's last whole word to test for the terminator (`src/storage/punct_table.cpp:57-72`, from `get_all_punctuations`, `:74-94`, and `src/pinyin.cpp:2474`). Both outcomes of that read return the same run.
- **oxpinyin:** `punct::decode_puncts` (`crates/oxpinyin-data/src/row_format.rs`) ends the run at the value's last whole word without the read. The visible punctuation is the pin's.
- **Decision:** accepted, class (b): the out-of-bounds read and its possible fault are not reproduced.
- **Tests:** `row_format` `punct_stream_ends_an_unterminated_run`.

### Aliased prediction prefix use-after-free

- **Pin:** `pinyin_guess_predicted_candidates` frees every candidate string and empties `m_prefixes` (`src/pinyin.cpp:2420-2423`, `:1335-1344`), then reads `prefix` (`:2424`, `:1389-1398`). When `prefix` is one of the instance's candidate strings, that read is a use after free; the punctuation entry calls it first and returns `true` (`:2459`, `:2498`).
- **oxpinyin:** `clear_borrowed_prefix` (`crates/oxpinyin-capi/src/predict.rs`) finds the alias by pointer identity before the read, empties the candidates and prefixes, and gives the no-prefix outcome: `false` from the plain entry, `true` with an empty list from the punctuation entry.
- **Decision:** accepted, class (b): the read of freed storage is not reproduced; the defined parts are the pin's.
- **Tests:** `e2e_tests::borrowed_prediction_prefix_clears_candidates_and_owned_text_recovers`, `boundary_tests::a_borrowed_punctuation_prefix_answers_true_with_no_row`.

### NULL prediction prefix

- **Pin:** `_compute_prefixes` passes `prefix` to `g_utf8_to_ucs4` with no NULL guard (`src/pinyin.cpp:1389-1398`; GLib 2.84.4 `gutf8.c:950`), after the defined resets of its callers (`:2420-2423`, `:2459`, `:1431-1432`; `src/zhuyin.cpp:912-918`, `:949-953`). A NULL `prefix` dies of SIGSEGV.
- **oxpinyin:** `ucs4::prefix_text` (`oxpinyin-capi-marshal`) turns a NULL, invalid or empty prefix into `None`: the plain entry answers `false`, the punctuation entry `true` with an empty list, and both sentence entries guess on `[sentence_start]`.
- **Decision:** accepted, class (b), by maintainer ruling: a NULL prefix is a prefix that adds no token.
- **Tests:** `boundary_tests::a_null_sentence_prefix_is_no_prefix`.

### NULL pointer arguments: the pin dereferences, oxpinyin null-guards (policy row 41)

- **Pin:** 68 `pinyin_*` exports dereference a NULL argument before any check: the instance (`src/pinyin.cpp:1312`), the context or iterator (`:509`, `:665`, `:777`, `:1196`), an out-parameter (`:2847`, `:2876`, `:2982`), a candidate in an `assert` (`:2507`, `:2593`), a key (`:2711`, `:2722`, `:2733`, `:2744`). The libzhuyin exports have the same shape.
- **oxpinyin:** guards the NULL arguments covered by row 41 and returns the entry point’s prescribed refusal. Other NULL cases retain their own rules.
- **Decision:** accepted, class (b): a NULL dereference is undefined.
- **Tests:** `e2e_tests::null_handles_answer_false`; `oxpinyin-zhuyin-capi` `boundary_tests::null_arguments_answer_the_declared_value`.

### A guess on an instance whose context was finalised (policy row 42)

- **Pin:** `pinyin_fini` deletes the context but not its instances (`src/pinyin.cpp:1194-1222`), and `pinyin_guess_sentence` then reads `instance->m_context->m_pinyin_lookup` (`:1372-1380`): a use after free.
- **oxpinyin:** each instance holds its own handles to the shared engine state (`crates/oxpinyin-capi/src/state.rs`), so the guess completes and answers `true`.
- **Decision:** accepted, class (b), by ruling: the use after free stays unreproduced.
- **Tests:** `instance_outlives_context` (both C facades' `live.rs`).

### Zhuyin import into library index 16 (policy row 43)

- **Pin:** `zhuyin_begin_add_phrases` stores any index (`src/zhuyin.cpp:392-398`), and `_add_phrase` reads `m_sub_phrase_indices[16]` past the 16-entry array (`:475`; `src/storage/phrase_index.cpp:611`, `phrase_index.h:441`) and calls through it: SIGSEGV.
- **oxpinyin:** `zhuyin_begin_add_phrases` stores any index; `zhuyin_iterator_add_phrase` (`crates/oxpinyin-zhuyin-capi/src/iterators.rs`) answers `false` when index 16 cannot be used by the phrase store.
- **Decision:** accepted, class (b): an out-of-bounds read through a wild pointer.
- **Tests:** `boundary_tests::null_arguments_answer_the_declared_value` (includes import at index 16).

### `pinyin_token_get_nth_pronunciation` past the last reading (policy row 51)

- **Pin:** for an `nth` past the last reading, `get_nth_pronunciation` writes nothing and its result is ignored, so the function appends `phrase_length` keys of the uninitialised `ChewingKey buffer[MAX_PHRASE_LENGTH]` and returns `true` (`src/pinyin.cpp:2801-2819`; `src/storage/phrase_index.cpp:33-44`).
- **oxpinyin:** returns `true` and appends `phrase_length` zeroed keys (`crates/oxpinyin-capi/src/dict.rs`).
- **Decision:** accepted, class (b): an uninitialised read; the return value and key count are the pin's.
- **Tests:** none.

### `zhuyin_token_get_nth_pronunciation` past the last reading (policy row 70)

- **Pin:** the zhuyin twin of row 51: it appends `phrase_length` keys of an uninitialised buffer and answers `true` (`src/zhuyin.cpp:1793-1811`; `src/storage/phrase_index.cpp:33-44`).
- **oxpinyin:** answers `true` and appends `phrase_length` zeroed keys (`crates/oxpinyin-zhuyin-capi/src/dict.rs`).
- **Decision:** accepted, class (b): an uninitialised read.
- **Tests:** `boundary_tests::token_getters_answer_the_pins_ranges_and_outputs` (out-of-range return value).

### `pinyin_alloc_instance` on a finalised context (policy row 60)

- **Pin:** `pinyin_alloc_instance` reads the deleted context's `m_phrase_index` (`src/pinyin.cpp:1310-1333`, at `:1322`; `pinyin_fini` at `:1194-1222`) and returns an instance with a dangling pointer. `zhuyin_alloc_instance` does the same (`src/zhuyin.cpp:845-857`).
- **oxpinyin:** `pinyin_init` and `zhuyin_init` record each live context and `fini` forgets it (`live.rs` in both C facades); allocation on an address that is not live answers NULL with no log.
- **Decision:** accepted, class (b): a use after free.
- **Tests:** `live::tests::a_context_is_live_from_init_to_fini`.

### `pinyin_bigram_iterator_get_next_phrase` after a walk that ends on a real predecessor (policy row 75)

- **Pin:** a walk's `false` row leaves `m_pinyins` empty (`src/pinyin.cpp:798-801`, `:885-886`). If the last loaded predecessor is a real phrase token, a further `get_next` passes the assert at `:902` and indexes the empty array at `:905`; on GLib 2.90 that read goes through NULL and dies of SIGSEGV.
- **oxpinyin:** answers `false` with `phrase`, `pinyin` and `count` untouched and no log (`crates/oxpinyin-capi/src/iterators.rs`; `BigramStep::Undefined`, `crates/oxpinyin-facade/src/export_rows.rs`).
- **Decision:** accepted, class (b): undefined behaviour.
- **Tests:** `export_rows::tests::the_walk_is_the_pins_state_machine` (facade walk with fixed predecessor order).

### A second `pinyin_fini` / `zhuyin_fini` on one context (policy row 105, class (b))

- **Pin:** a second `pinyin_fini` (`src/pinyin.cpp:1194-1222`) or `zhuyin_fini` (`src/zhuyin.cpp:741-757`) reads the freed context and frees its members again: a double free.
- **oxpinyin:** both return at once, with no log, for a context the live-context registry does not hold.
- **Decision:** accepted, class (b): a double free.
- **Rejected:** a registry of live instances for a second `*_free_instance` (`pinyin.cpp:1347-1356`, `zhuyin.cpp:876-884`), which would cost time and space on every alloc and free; that call stays caller UB.
- **Tests:** `e2e_tests::fini_twice_returns_silently` (both C facades).

### A prediction candidate whose phrase item a journal REMOVE took out (policy row 106a, class (b))

- **Pin:** `_compute_predicted_prefix_candidates` ignores `get_phrase_item`'s result and gates on the last successful read's length (`src/pinyin.cpp:2389-2405`). A missing token keeps a NULL string (`:2018-2022`, `:1635-1637`): with two or more pre-dedup rows the dedup `strcmp` dies of SIGSEGV (`:2041-2068`, `:2082`); a sole row survives, and choosing it adds to the facade total (`:2608-2616`; `src/storage/phrase_index.h:628-634`).
- **oxpinyin:** `select_prefix_rows` (`crates/oxpinyin-capi/src/predict.rs`) keeps `last_read_length` in the pin's order, keeps a sole missing row as `TextSpan::NULL` and drops missing rows beside any other row; `pinyin_choose_predicted_candidate` advances the loaded facade total and returns `true`.
- **Decision:** accepted, class (b): the NULL `strcmp` is not reproduced; the defined single-row path is the pin's.
- **Tests:** `choosing_a_sole_missing_prefix_row_advances_the_facade_total`.

### NULL string conversion after defined prior work

- **Pin:** With a NULL sentence, `pinyin_phrase_segment` reaches the `g_utf8_strlen` critical, then dereferences NULL in `g_utf8_to_ucs4` (`src/pinyin.cpp:1450-1452`).
- **oxpinyin:** `pinyin_phrase_segment` calls `g_utf8_strlen` for the critical and passes empty input to the segmenter (`crates/oxpinyin-capi/src/phrase.rs`).
- **Decision:** accepted, class (b), under the no-input ruling of [NULL prediction prefix](#null-prediction-prefix).
- **Tests:** none.

### Training a token whose library is unloaded raises a stale cached item's reading counts (policy row 118)

- **Pin:** after an unload deletes a sub-index (`src/storage/phrase_index.cpp:261-269`), `train_result3` reads the item unchecked and raises the reading counts of `m_cached_phrase_item`, which the failed read left as the last successful read set it (`src/lookup/phonetic_lookup.h:924-927`; `phrase_index.h:649-650`, `phrase_index.cpp:204`). The write is a use after free when that item was in the unloaded library, and a defined write into a live item otherwise.
- **oxpinyin:** trains no reading of a token whose library has no live buffer; the bigram trains, the unigram add moves no total and `*_train` answers `true`, as at the pin.
- **Decision:** accepted: the freed-chunk write is class (b), and the live-item write is declined under the [unloaded-token pronunciation ruling](compatibility-policy.md#unloaded-token-pronunciation-cache).
- **Tests:** `unloaded_library_tests` (`crates/oxpinyin-capi/src/candidates.rs`), `unloaded` (`crates/oxpinyin-user/src/store_libpinyin/unloaded.rs`).

## Accepted divergences — store and data files

### The pinyin index DBMs carry uninitialized struct padding

- **Pin:** `ChewingTableEntry::add_index` copies a whole stack `PinyinIndexItem2<N>` into the DBM value (`src/storage/chewing_large_table2.h:208-232`, at `:229`), and its constructor sets only `m_token` and `m_keys` (`src/storage/pinyin_phrase3.h:181-196`). For an odd `N` the item's two bytes of tail padding are indeterminate; no reader reads them.
- **oxpinyin:** `row_format::pinyin_index::encode_item` (`crates/oxpinyin-data/src/row_format.rs`) zeroes the padding, and `pinyin_index::decode_items` reads only the token and keys. The tool comparison masks only the two tail bytes of each odd-`N` item.
- **Decision:** accepted, class (b) (policy row 18): the pin's bytes are an indeterminate read that no program can reproduce.
- **Tests:** `gbf-toned-model`; controls `gbf-mini-fixture`, `kmm-estimate-good`; `pinyin_index_mask_clears_only_odd_item_padding`.

### `mmap %s failed!`: a system library the pin cannot map, and the empty chunk it dies on (policy row 76)

- **Pin:** At init and mask-out, the pin prints `mmap %s failed!` before loading the empty chunk and reading past it (`src/pinyin.cpp:256`, `:956`; `src/zhuyin.cpp:200`; `src/storage/phrase_index.cpp:335-363`).
- **oxpinyin:** Pinyin and Zhuyin init report an earlier missing library and fail at the later unmappable library they reach. On Unix, pinyin mask-out with a removed `gb_char.bin` emits the mmap diagnostic and returns `true`, preserving the tested lookups, unigram count and profile files.
- **Decision:** accepted, class (b): the read through the empty chunk is not reproduced.
- **Tests:** `library_directory::init_reports_only_what_the_library_loop_reached`, `boundary_tests::mask_out_over_an_unmappable_library_changes_nothing` (init and mask-out).

### A sparse chunk's whole mapping is retained after `fini` (#865; policy row 109)

- **Pin:** `MemoryChunk::mmap` maps the whole `file_size` (`src/include/memory_chunk.h:475-524`), but `freemem` unmaps only `header + capacity()` (`:72-78`). When `guint32 data_len` wraps, as for a 2^32+8 file, most of the mapping stays after `fini`.
- **oxpinyin:** `oxpinyin-input::chunk` owns one private whole-file mapping per loaded chunk, and its `Drop` releases all of it.
- **Decision:** the mapping is reproduced; the wrapped-size residual is accepted under the external API misuse ruling: the pin's `munmap` length contradicts its own mapping.
- **Tests:** `chunk::tests::sparse_file_has_one_whole_writable_private_mapping_and_no_residual` (`crates/oxpinyin-input/src/chunk.rs`).

### `table.conf` follows its rows; malformed ones fail the triggering call (policy rows 78, 79, 80 and 83)

- **Pin:** a missing `source table format:` line leaves `str` unwritten and compares it (`src/storage/table_info.cpp:209-229`): undefined. A `table.conf` that points a loaded library at another file leaves tokens with no item, and `_remove_duplicated_items_by_phrase_string` compares the unset string and dies of SIGSEGV in `strcmp` (`src/pinyin.cpp:1635-1637`, `:2001-2007`, `:2053`).
- **oxpinyin:** the missing line is `TableConfError::UninitialisedSourceFormat`: init answers NULL with one warning. Candidate guesses omit tokens whose live item cannot be read and return true without a warning, as in register row 106.
- **Decision:** accepted, class (b) (row 80); rows 78 and 83 read the rows and λ as the pin does, and row 79 reproduces the pin.
- **Tests:** `table_info::tests::header_misses_are_the_ordinary_false_or_the_uninitialised_read` (header parsing), `e2e_tests::table_conf_919_short_row_merged` (row handling).

### A system journal record names an absent token or leaves an over-long item header (policy row 107)

- **Pin:** in `SubPhraseIndex::merge` (`src/storage/phrase_index.cpp:439-517`, from `src/pinyin.cpp:276`), a REMOVE of an absent token dereferences the NULL item (`:460-475`, `:219-234`), and a growing MODIFY of an absent token asserts through the same NULL (`:477-501`, at `:489`): SIGSEGV. A MODIFY or ADD payload is not checked against its header, so a later `get_phrase_item` (`:180-206`) can read past the content buffer.
- **oxpinyin:** the absent-token REMOVE is a no-op; the growing stale MODIFY stops the replay with the earlier records applied; a stored header that names a read past the content buffer makes that item unreadable to every getter, with the other changes kept. A payload past the log's end reverts the whole replay.
- **Decision:** accepted, class (b), by maintainer ruling: a NULL dereference and an over-read have no safe reproduction.
- **Tests:** `an_over_read_item_answers_no_item_and_the_records_around_it_stay`, `a_mismatch_stop_keeps_every_applied_record`, `rollback_restores_the_buffer_for_every_kind_of_mutation`.

### Bigram export iterator reads past a partial user-gram item (policy row 98, class (b))

- **Pin:** `SingleGram::retrieve_all` walks with `cur != end` and reads a whole item (`src/storage/ngram.cpp:133-143`, from `src/pinyin.cpp:789-889`), so a value of `4 + 8k + r` bytes, `1 <= r <= 7`, is read past its end.
- **oxpinyin:** the export walk returns `false` with one `libpinyin` warning when it reaches a predecessor with a partial tail. A mask or candidate removal over such a surviving predecessor answers `false` with one warning; the raw value is kept for save.
- **Decision:** accepted, class (b): an out-of-bounds read.
- **Tests:** `e2e_tests::bigram_export_ends_at_a_partial_predecessor`, `e2e_tests::mask_out_refuses_a_partial_bigram_tail`.

### Training on a malformed user-gram value reads past a partial item (policy row 100, class (b))

- **Pin:** `SingleGram::insert_freq`, `get_freq` and `set_freq` walk with `cur_item != end` (`src/storage/ngram.cpp:178`, `:190`, `:231`, `:241`, `:252`, `:261`), reached from `train_result3` (`src/lookup/phonetic_lookup.h:889`) and `pinyin_choose_predicted_candidate` (`src/pinyin.cpp:2630-2633`); a partial tail is read past.
- **oxpinyin:** `*_train` and `pinyin_choose_predicted_candidate` answer `false` with one facade-domain warning when the step would load such a predecessor; the raw value survives save and reopen byte for byte.
- **Decision:** accepted, class (b): the same out-of-bounds read as row 98, on the training path.
- **Tests:** `training_over_a_partial_tail_refuses_and_keeps_the_raw_value`, `a_partial_tail_created_in_session_refuses_training` (`crates/oxpinyin-user/src/store_libpinyin.rs`).

### A partial phrase-table token tail cannot terminate the entry walk (policy row 101, class (b))

- **Pin:** `PhraseTableEntry` walks four-byte tokens with `iter != end` over the raw byte end (`src/storage/phrase_large_table3.cpp:32-39`, `:57-65`, `:74-81`, `:103-117`), so a value of `4k + r` bytes, `1 <= r <= 3`, is read past the record.
- **oxpinyin:** `PhraseTableEntry::from_bytes` refuses the whole record with `StoreError::InvalidInput`; the lookup cache holds an empty entry for its key, and the raw bytes are saved unchanged.
- **Decision:** accepted, class (b): an out-of-bounds read.
- **Tests:** `a_partial_phrase_table_record_is_refused_before_any_change` (`crates/oxpinyin-user/src/store_libpinyin.rs`).

### A reloaded library's dropped item stays in the user index tables, and a candidate walk dereferences it (policy row 102, class (b))

- **Pin:** `FacadePhraseIndex::unload` frees the session's items (`src/storage/phrase_index.cpp:261-268`), the reload merges only the saved `.dbin` (`src/pinyin.cpp:264-277`, `src/zhuyin.cpp:298-312`), and the user index tables keep the dropped token; `*_guess_candidates` then reads the absent item and dies of SIGSEGV.
- **oxpinyin:** Reloading a pinyin system library after an unsaved reading was added discards that reading and restores the saved readings from the journal.
- **Decision:** accepted, class (b): a dereference of an item the sub-index no longer owns.
- **Tests:** `reloading_a_system_library_drops_the_unsaved_session` (store reload and saved readings).

### A wrapped phrase-content item extends past the content (policy row 111)

- **Pin:** `SubPhraseIndex::get_phrase_item` builds a header-derived view without checking its extent (`src/storage/phrase_index.cpp:180-206`); after the content offset wraps, a read can pass the content end (compaction at `:624-675`, `src/pinyin.cpp:655-659`).
- **oxpinyin:** `UserLibrary::item` and the replay buffer answer a read inside the content with the following bytes and answer no item past the content end; `append_offset` reproduces the wrap modulo 2^32, and compaction runs at the pin's points only.
- **Decision:** accepted, class (b): the read past the content is unsafe.
- **Tests:** `a_read_crossing_the_item_end_returns_the_next_bytes_and_one_past_the_content_is_none` (`crates/oxpinyin-user/src/user_library.rs`).

### An oversized user chunk or journal bulk save (policy row 112)

- **Pin:** a `USER_FILE` `SubPhraseIndex::store` narrows the compacted content length to `int`, so the copy is unsafe from 2^31 bytes (`src/storage/phrase_index.cpp:365-390`; `src/include/memory_chunk.h:95-98`, `:290-296`); `PhraseIndexLogger::store` copies a journal of 2^31 bytes or more the same way (`phrase_index.cpp:271-283`, `src/storage/phrase_index_logger.h:115-117`).
- **oxpinyin:** reproduces the save up to the oversized copy, then stops with `PersistenceError::SaveTooLarge`: earlier file changes stay, `*_save` answers `false`, and the session stays dirty.
- **Decision:** accepted, class (b): the copy past the allocation is not performed.
- **Tests:** `a_user_chunk_copy_at_the_limit_stops_the_save_where_it_is`, `a_system_journal_copy_at_the_limit_stops_the_save_and_retries`, `the_store_copy_threshold_follows_the_compacted_content`.

### A negative `MemoryChunk::load` payload read (policy row 114)

- **Pin:** `MemoryChunk::load` converts a negative `read` result, `EINTR` included, to `guint32` and checksums that length past the allocation (`src/include/memory_chunk.h:452-453`).
- **oxpinyin:** takes the checksum-failure path: it frees the buffer, closes the descriptor and answers `false`, with no retry.
- **Decision:** accepted, class (b), by maintainer ruling: an out-of-bounds read.
- **Tests:** `chunk::tests::interrupted_payload_read_is_not_retried` (`crates/oxpinyin-input/src/chunk.rs`).

### Berkeley DB handles whose open failed (policy row 113)

- **Pin:** keeps a Berkeley DB handle whose `DB->open` failed and calls it again, so libdb prints `BDB1565 DB-><method>: method not permitted before handle's open method` ([policy row 113](compatibility-policy.md#bdb-failed-handles)).
- **oxpinyin:** On Berkeley DB, the KMM estimator closes failed handles and omits the `BDB1565` diagnostic in the existing-empty database and no-models cases.
- **Decision:** accepted under the external API misuse ruling: Berkeley DB requires `DB->close` after a failed open.
- **Tests:** `kmm_estimate_existing_empty`, `kmm_estimate_no_models` (`crates/oxpinyin-utils/tests/kmm_tools.rs`, Berkeley DB).

## Accepted divergences — utilities

### gen_pinyin_table invalid UTF-8 phrase (class (b))

- **Pin:** a phrase that is not valid UTF-8 can pass `feed_line`'s checks and enter the tree with a NULL `uniphrase` (`utils/storage/gen_pinyin_table.cpp:160-232`, at `:174`); `gen_phrase_file` opens the output (`:261`) and then reads through the NULL (`:280-281`), or `phrase_item_compare` does earlier (`:85-94`): SIGSEGV. `gen_zhuyin_table` is the same (`utils/storage/gen_zhuyin_table.cpp:168-192`, `:90-98`, `:286-287`).
- **oxpinyin:** prints the node count, opens the output at the pin's point, writes the rows before the invalid one, then refuses with `<utility>: invalid UTF-8 phrase: <bytes>` and exit 1.
- **Decision:** accepted, class (b): a NULL read; the pin's lost stdout buffers are not reproduced.
- **Tests:** `pinyin-invalid-utf8`, `pinyin-invalid-utf8-after-row`, `zhuyin-invalid-utf8`, `zhuyin-invalid-utf8-after-row` (`crates/oxpinyin-utils/tests/gen_pinyin_table.rs`, `gen_zhuyin_table.rs`).

### gen_zhuyin_table missing frequency (class (b))

- **Pin:** `atoi(strs[1])` runs before the field count check (`utils/storage/gen_zhuyin_table.cpp:148-159`, `:153` before `:156`), so an empty or one-field line dereferences NULL: SIGSEGV.
- **oxpinyin:** refuses with `gen_zhuyin_table: missing frequency: <line>` and exit 1, before it opens the output.
- **Decision:** accepted, class (b): a NULL read.
- **Tests:** `zhuyin-missing-frequency`; control `zhuyin-normal` (`crates/oxpinyin-utils/tests/gen_zhuyin_table.rs`).

### k_mixture_model_to_interpolation undefined lines

- **Pin:** `taglib_read` reads `tokens[0]` as the line tag (`src/storage/tag_utility.cpp:179-190`): an empty line, a re-read empty last line (`utils/training/k_mixture_model_to_interpolation.cpp:86-88`) and a trailing escape that makes `split_line` return NULL (`tag_utility.cpp:137`) read through NULL: SIGSEGV. A truncated UTF-8 sequence at the line end reads past the string.
- **oxpinyin:** makes the pin's calls up to that read, then writes `k_mixture_model_to_interpolation: <reason>: <line>` and exits 1.
- **Decision:** accepted, class (b): a NULL read or an over-read.
- **Tests:** `kti-blank-line`, `kti-end-missing`, `kti-trailing-escape` (`crates/oxpinyin-utils/tests/k_mixture_model_to_interpolation.rs`).

### g_utf8_next_char from a byte that leads no sequence

- **Pin:** `split_line` steps with `g_utf8_next_char` (`src/storage/tag_utility.cpp:118-169`), whose GLib table steps 5 from `0xF8..=0xFB` and 6 from `0xFC..=0xFD` (`glib/gutf8.c:115-124`, GLib 2.84.4), outside the macro's valid-UTF-8 precondition.
- **oxpinyin:** `oxpinyin_input::tool::utf8_next_char_step` steps the lead length of 2, 3 or 4, else 1; from such a byte it steps 1 and tokenizes the bytes after it.
- **Decision:** accepted under the [no-ported-internals ruling](compatibility-policy.md#no-ported-external-library-internals): GLib's internal table sets the step outside the precondition.
- **Rejected:** reading GLib's table, which needs a hand-declared data symbol that MSVC cannot import.
- **Tests:** `split_line_steps_one_byte_from_a_byte_that_leads_no_sequence` (`crates/oxpinyin-utils/src/utils_helper/taglib.rs`); the `tool.rs` boundary test of `oxpinyin-input`.

### Utility check macros write no message

- **Pin:** In the reference build, `check_result` is `assert` (`src/include/pinyin_utils.h:26-31`). The malformed interpolation input checks can emit GLib criticals (`src/storage/tag_utility.cpp:202`, `:246`) and the glibc assertion line before SIGABRT.
- **oxpinyin:** `k_mixture_model_to_interpolation` aborts without the check-macro diagnostics in the unknown-line, missing-value, bigram and last-line-reread cases.
- **Decision:** accepted, language artefact, by maintainer ruling: oxpinyin does not imitate the macros' text.
- **Tests:** `kti_unknown_line_assert`, `kti_missing_value_assert`, `kti_bigram_assert`, `kti_last_line_reread_assert` (`crates/oxpinyin-utils/tests/k_mixture_model_to_interpolation.rs`).

### KMM tool asserts write no macro text

- **Pin:** The reference build emits assertion text and aborts at the missing merge result file, zero estimation total and generator document assertion sites (`utils/training/merge_k_mixture_model.cpp:127`, `estimate_k_mixture_model.cpp:45`, `gen_k_mixture_model.cpp:380`).
- **oxpinyin:** The KMM merge, estimate and generate tools abort without the assertion text in the missing-result-file, zero-total and good-document cases.
- **Decision:** accepted under the check-macro ruling above.
- **Tests:** `kmm_merge_no_result_file`, `kmm_estimate_zero_total`, `kmm_gen_good_document` (`crates/oxpinyin-utils/tests/kmm_tools.rs`).

### FlexibleBigram malformed record lengths

- **Pin:** the `FlexibleBigram` backends hand a whole stored value to `FlexibleSingleGram` (`src/storage/flexible_ngram_bdb.h:165-187`, `flexible_ngram_kyotodb.h:192-216`, `flexible_ngram_tkrzwdb.h:200-225`). A value shorter than the header asserts in `get_array_header` (`flexible_ngram_bdb.h:388`, `flexible_ngram_kyotodb.h:371`, `flexible_ngram_tkrzwdb.h:388`) or is read past its end (`flexible_single_gram.h:88-105`, `:151-181`, `:300-306`); a partial item tail is read past when walked.
- **oxpinyin:** aborts at the pin's assert sites; where the pin reads past a short value, the tools exit 1 with `flexible bigram single gram is shorter than its array header`; a partial item tail loads as a missing row; merge copies a short source row the target lacks with `copy_single_gram`.
- **Decision:** accepted, class (b): the reads past a record are not reproduced.
- **Tests:** `flexible_ngram::tests::a_truncated_single_gram_is_refused_by_length` (store decoding only).

### Consumed invalid UTF-8 in utility text rows or phrase-table queries (policy row 115)

- **Pin:** `FacadePhraseIndex::load_text` passes a failed `g_utf8_to_ucs4` to `set_phrase_string(written, NULL)` with `written` uninitialised (`src/storage/phrase_index.cpp:541-586`); the phrase-table test searches the same failed output (`tests/storage/test_phrase_table.cpp:45-59`; `src/storage/phrase_large_table3_tkrzwdb.cpp:133-136`, `phrase_large_table3.cpp:131-135`).
- **oxpinyin:** stops the consuming failures with `exit(EINVAL)` and no added diagnostic, keeping earlier output; the defined same-token path continues as in the pin.
- **Decision:** accepted, class (b), by maintainer approval: undefined behaviour.
- **Tests:** `phrase_table_invalid_text`, `phrase_table_invalid_utf8` (`crates/oxpinyin-repl/tests/interactive.rs`).

### More than sixteen keys passed to the chewing-table test search (policy row 116)

- **Pin:** the test passes every parsed key to a search whose local `ChewingKey index[MAX_PHRASE_LENGTH]` has sixteen slots (`tests/storage/test_chewing_table.cpp:71-86`; `src/storage/chewing_large_table2.cpp:161-172`; `src/storage/pinyin_phrase3.h:160-177`): a stack-buffer write.
- **oxpinyin:** after the first incomplete key's tone check (`pinyin_phrase3.h:146-157`), stops before the copy with `exit(EINVAL)`, keeping earlier output.
- **Decision:** accepted, class (b), by maintainer approval.
- **Tests:** `chewing-table-overlong-query`.

### The logger test ignores a failed required phrase-chunk load (policy row 117)

- **Pin:** the three required `../../data/gb_char.bin` loads ignore `MemoryChunk::load` returning false and reach row 76's NULL read (`tests/storage/test_phrase_index_logger.cpp:28-30`, `:45-48`, `:52-54`; `src/storage/phrase_index.cpp:271-282`, `:335-354`).
- **oxpinyin:** The shared utility loader `Libraries::load` exits with `ENOENT` when a required system phrase chunk cannot be loaded (`crates/oxpinyin-utils/src/storage/libraries.rs`).
- **Decision:** accepted, class (b), by maintainer approval.
- **Tests:** none.

## Approved product and ruling exceptions

### Pinyin trellis adjusted cost — S1/S2 repair

- **Pin:** the trellis comparator lets an adjacent longer sentence win by `LONG_SENTENCE_PENALTY` and then says it loses in reverse (`src/lookup/phonetic_lookup.h:39`, `:65-91`), which violates the strict weak ordering of the standard heap algorithms at S1 beam selection (`:770-771`) and S2 final-tail selection (`:331-335`).
- **oxpinyin:** `nbest.rs` selects by `S = poss - length * P`, `P = 1.2_f32.ln()`, in f32, larger S first, NaN last, then earlier input order. Node admission uses adjusted score alone, so a tie keeps the incumbent. The two-slot front-eviction algorithm follows the pin.
- **Decision:** accepted under the [S1/S2 ruling](compatibility-policy.md#s1-and-s2-one-ordered-adjusted-score).
- **Tests:** `nbest::tests::adjusted_score_rounds_product_then_difference_and_ties`, `nbest::tests::float_adjusted_comparison_is_a_strict_weak_order`.

### Pinyin tail sort — S3 repair

- **Pin:** `trellis_value_compare` returns the negated `gfloat` difference as `gint` (`src/lookup/phonetic_lookup.h:174-178`) and is passed to `g_ptr_array_sort` (`:337`); its equality is not transitive, against the consistent total order a qsort-style comparator needs.
- **oxpinyin:** `tail_merge` (`crates/oxpinyin-engine/src/nbest.rs`) sorts the tails stably by raw possibility descending, NaN last, with no integer truncation.
- **Decision:** accepted under the [S3 ruling](compatibility-policy.md#s3-stable-descending-possibility).
- **Tests:** `nbest::tests::ieee_order_and_stable_raw_tails`.

### Phrase segmentation writes no invalid-UTF-8 critical

- **Pin:** `pinyin_phrase_segment` and `zhuyin_phrase_segment` check `g_return_val_if_fail(num_of_chars == ucs4_len, FALSE)` after their conversions (`src/pinyin.cpp:1450-1454`, `src/zhuyin.cpp:969-973`), so invalid UTF-8 logs GLib's critical and returns `FALSE`; under fatal criticals the pin dies.
- **oxpinyin:** returns `false`, keeps the earlier result and makes no log call.
- **Decision:** accepted, maintainer decision D1: the check tests results, not arguments, so oxpinyin implements its early return without the critical.
- **Tests:** `surrogate_segment_matches_pin_length_and_no_critical` (both facades).

### Phrase-library layout faults write no critical

- **Pin:** `SubPhraseIndex::load` checks the three separators and the end with `g_return_val_if_fail` (`src/storage/phrase_index.cpp:354-356`, `:361`). The pinyin save path ignores the failed layout check and uses an empty sub-index (`src/pinyin.cpp:955-965`).
- **oxpinyin:** When pinyin save reloads a system library with an invalid separator, it writes the journal against an empty sub-index with the local total intact and emits no critical.
- **Decision:** accepted, maintainer decision D1: the check tests file data inside an internal method, so oxpinyin implements its early return without the critical.
- **Tests:** `pinned_data_tests::a_save_over_a_failed_layout_journals_against_an_empty_index`.

### Reproduced library aborts write no assertion line

- **Pin:** at an `assert` site glibc's `__assert_fail` writes `<progname>: <file>:<line>: <function>: Assertion '<expr>' failed.` and raises SIGABRT; in the reference build `check_result` is `assert` (`src/include/pinyin_utils.h:27-31`).
- **oxpinyin:** does the pin's preceding work, then calls `std::process::abort()` with no assertion line; an `abort()` site writes nothing on either side.
- **Decision:** accepted for the stderr line only, by the ruling that retires class (c): the line is compiler-generated text; the SIGABRT status, the earlier stderr and the files match.
- **Tests:** none.

## Language-artefact records

### Candidate sorting guards the wrapping frequency order

- **Pin:** the candidate comparator subtracts two `guint32` frequencies and converts to `gint` (`src/pinyin.cpp:1678-1709`, at `:1701-1705`; `src/zhuyin.cpp:1129-1144`), then sorts stably with GLib (`pinyin.cpp:2286`, `:2440-2445`; `zhuyin.cpp:1529`, `:1610`). The wrapping order is inconsistent in cyclic frequency groups, including groups with an antipodal pair.
- **oxpinyin:** `candidate::rank_by_frequency` groups stably by enabled length and span, checks each group's wrapping relation, and sorts a consistent group by `rhs.wrapping_sub(lhs) as i32` and a cyclic group by stable numeric descending frequency. A consistent group fits in a circular interval strictly narrower than 2^31.
- **Decision:** accepted under the [candidate-frequency ruling](compatibility-policy.md#no-ported-external-library-internals): std sorting needs a consistent order; the fallback applies only within inconsistent equal-length, equal-span groups.
- **Rejected:** transcribing GLib's sort algorithm.
- **Tests:** `candidate::tests::frequency_sort_guards_each_leading_key_group_and_preserves_ties`, `candidate::tests::frequency_sort_has_a_defined_antipodal_fallback`.

### Zhuyin selection uses length, raw score and input order

- **Pin:** zhuyin compares equal-length raw possibilities with no tie key and runs the standard heap algorithms (`src/lookup/phonetic_lookup.h:66-91`, `:141-172`, `:331-335`; `src/zhuyin.cpp:50`), so an equal tie goes by the heap schedule; for λ > 1 the unigram paths carry NaN (`phonetic_lookup.h:651-663`, `:712-713`).
- **oxpinyin:** `nbest.rs` `selection_compare` orders by length ascending, raw f32 possibility descending, NaN last, signed zeros equal, then earlier input order. Single-slot admission uses the pin’s length check and raw `<` for equal lengths.
- **Decision:** accepted under the [no-ported-internals ruling](compatibility-policy.md#no-ported-external-library-internals): libstdc++'s choice among equal elements is not a product key.
- **Tests:** `nbest::tests::selection_uses_batch_ties_and_zhuyin_length_first_nan_last`, `nbest::tests::the_zhuyin_shape_keeps_one_tail_with_batch_ties`.

### Punctuation values contain whole words

- **Pin:** `get_all_punctuations` compares byte pointers and passes a shifted byte difference as the word count (`src/storage/punct_table.cpp:56-94`), so 1 to 3 bytes after a terminated run add one empty punctuation.
- **oxpinyin:** `punct::decode_puncts` walks whole four-byte words only, so a misaligned value gets no extra empty punctuation.
- **Decision:** accepted, [internal language artefact](compatibility-policy.md#internal-language-artefacts): system writers emit whole words, and valid values are identical.
- **Tests:** `row_format` `punct_stream_ignores_partial_words`, `punct_stream_keeps_non_scalar_words`.

### Stored gram partitions: phrase segmentation

- **Pin:** `SingleGram` searches stored records with the standard lower bound before its walks (`src/storage/ngram.cpp:153-271`), and `merge_single_gram` interleaves without sorting (`:308-349`), so a foreign unsorted value breaks the partition precondition; segmentation reads it (`src/lookup/phrase_lookup.cpp:268-290`).
- **oxpinyin:** `single_gram::lower_bound_records` uses `slice::partition_point` on the stored order; on unpartitioned input its result is std's.
- **Decision:** accepted under the [no-ported-internals ruling](compatibility-policy.md#no-ported-external-library-internals): the lower-bound result is unspecified when the stored records do not satisfy the partition precondition.
- **Tests:** `pinned_data_tests::the_segmenter_reads_an_unsorted_user_gram_as_stored`, `e2e_tests::user_gram_unsorted_export_follows_the_stored_order`, `e2e_tests::user_gram_duplicate_export_lists_every_stored_item`.

### Stored gram partitions: training

- **Pin:** on an unsorted user gram `get_freq` misses, `insert_freq` appends and the next search misses again (`src/storage/ngram.cpp:178-205`, `:231-271`), so `check_result(user->set_freq(...))` aborts (`src/lookup/phonetic_lookup.h:888-903`).
- **oxpinyin:** the `partition_point` search finds the inserted item, and training answers `true` with no warning.
- **Decision:** accepted under the [no-ported-internals ruling](compatibility-policy.md#no-ported-external-library-internals): oxpinyin does not force a successful std search to reproduce another implementation's miss.
- **Tests:** `boundary_tests::training_over_an_unsorted_start_gram_answers_true`, `store_libpinyin::tests::training_preserves_a_partitioned_unsorted_gram_and_its_duplicates`.

### The full-pinyin parse's 16-bit steps

- **Pin:** `FullPinyinParser2` keeps its steps in `gint16` (`src/storage/pinyin_parser2.cpp:67-70`, `:276`, `:290`); `final_step` returns the last step whose `m_parsed_len` equals its index (`:349`, `:353-359`), never above 32767, and `parse` returns it (`:333`).
- **oxpinyin:** keeps graph positions and spans in `u32` and parses the whole input; it refuses only past `MAX_GRAPH_POSITION`.
- **Decision:** accepted, language artefact (policy row 59), by maintainer decision: the stop is a C++ storage width.
- **Tests:** `session::tests::input_past_the_pins_gint16_bound_is_kept_and_parsed_whole`, `session::tests::candidates_come_from_keys_past_the_pins_gint16_bound`, `boundary_tests::an_import_reading_parses_past_long_separator_runs`.

### Key positions past byte 65535

- **Pin:** the double-pinyin and keyboard zhuyin parsers store `int` positions into the `guint16`s of `ChewingKeyRest` (`src/storage/pinyin_parser2.cpp:565`, `zhuyin_parser2.cpp:259`, `chewing_key.h:102-103`), and `fill_matrix` puts each key at its wrapped begin (`phonetic_key_matrix.cpp:51-55`), so the getters answer `false` from column 65536 (`src/pinyin.cpp:2930-2931`).
- **oxpinyin:** stores the positions `as u16`, as the pin does, but keeps the facade's columns at real offsets.
- **Decision:** accepted under the [key-positions ruling](compatibility-policy.md#key-positions-past-byte-65535): no IME composition reaches 65536 bytes.
- **Tests:** `cursor::preedit_key_tests::key_rest_positions_past_byte_65535_wrap`, `oxpinyin-zhuyin-capi` `e2e_tests::key_rest_positions_past_byte_65535_wrap`.

### FlexibleBigram unsorted-record search

- **Pin:** `FlexibleSingleGram` searches with the standard lower bound on the unsigned token order (`src/storage/flexible_single_gram.h:63-65`, `:118-139`), and backend loads do not check the order (`flexible_ngram_bdb.h:165-187`, `flexible_ngram_kyotodb.h:192-215`, `flexible_ngram_tkrzwdb.h:196-219`); on an unsorted value the result is unspecified.
- **oxpinyin:** `SingleGram::lower_bound` (`crates/oxpinyin-store/src/flexible_ngram.rs`) uses `slice::partition_point` and does not sort or clean loaded records.
- **Decision:** accepted under the [no-ported-internals ruling](compatibility-policy.md#no-ported-external-library-internals): no contract fixes a result on unpartitioned input; sorted records match.
- **Tests:** `kmm_estimate_good` (unsorted Kyoto Cabinet fixture, sorted Berkeley DB and tkrzw controls).

## Outstanding upstream defects (report-back)

These entries are pin defects that oxpinyin collects for the libpinyin report-back batch; the drafts are in [upstream report drafts](upstream-report-drafts.md), and none is filed.

### `pinyin_init`/`zhuyin_init` leave the process `LC_NUMERIC` at "C" (policy row 39)

- **Pin:** `UserTableInfo::load` and `SystemTableInfo2::load` set `LC_NUMERIC` to `"C"` and restore it only on success (`src/storage/table_info.cpp:328`, `:372`, `:197`, `:291`; `save` at `:378`, `:394`), so after any init `LC_NUMERIC` is `"C"`.
- **oxpinyin:** installs `"C"` with the C library's `setlocale` at the pin's points.
- **Decision:** reproduced under the [`LC_NUMERIC` ruling](compatibility-policy.md#lc_numeric-after-init-row-39); item 6 of the report drafts.
- **Tests:** `lc_numeric_follows_the_pins_table_info_sites` (`crates/oxpinyin-capi/tests/locale.rs`), `a_failed_init_leaves_lc_numeric_at_c` (`crates/oxpinyin-zhuyin-capi/tests/locale.rs`).

### Pinyin candidate frequency: out-of-range `float`-to-`guint32` store (class (b))

- **Pin:** `_compute_frequency_of_items` converts a `gfloat` score to `guint32` (`src/pinyin.cpp:1794-1868`, at `:1824`, `:1841`, `:1866`). For an out-of-range score, the pin’s result depends on the CPU architecture.
- **oxpinyin:** `amplified_frequency_with_bigram` uses Rust’s defined `as u32` cast.
- **Decision:** accepted, class (b): C leaves the conversion undefined, and processor behaviour is not reproduced.
- **Tests:** none.

### Zhuyin candidate frequency: negative `float`-to-`guint32` store at λ > 1 (class (b))

- **Pin:** `_compute_frequency_of_items` converts `(1 - lambda) * unigram / total * 2^24` to `guint32` (`src/zhuyin.cpp:1264-1268`). For an out-of-range negative score at λ > 1, the pin’s result depends on the CPU architecture.
- **oxpinyin:** Uses Rust’s defined `as u32` cast.
- **Decision:** accepted, class (b): C leaves the conversion undefined.
- **Tests:** none.

## Pin aborts reproduced

Where the pin asserts or aborts, oxpinyin does the pin's preceding work and calls `std::process::abort()` ([failure behaviour](compatibility-policy.md#failure-behaviour)).
An `abort()` site is MATCH; an `assert` site differs only by the assertion line ([Reproduced library aborts write no assertion line](#reproduced-library-aborts-write-no-assertion-line)).
Pin cites are relative to `src/`; `*` in a case name is `pinyin` and `zhuyin`.

| Site | Pin cite | Test |
|---|---|---|
| Toned initial-only key in a table search, both facades (row 4) | `storage/pinyin_phrase3.h:152`; `pinyin.cpp:2769` | `boundary_tests::guess_candidates_aborts_on_a_toned_initial`, `boundary_tests::guess_sentence_aborts_on_a_toned_initial`, `boundary_tests::is_incomplete_aborts_on_a_toned_initial`, `keys::toned_initial_only_key_aborts` |
| `pinyin_set_double_pinyin_scheme(30)` (row 5a) | `storage/pinyin_parser2.cpp:611` | `contract::double_customized_aborts` |
| Zhuyin scheme 7 (row 5c) | `storage/zhuyin_parser2.cpp:295` | `contract::zhuyin_standard_dvorak_aborts`, `e2e_tests::the_dvorak_chewing_scheme_aborts` |
| Zhuyin scheme outside the enum; full-pinyin scheme outside 1 to 3 (row 5d) | `pinyin.cpp:1189`; `zhuyin.cpp:736`; `storage/pinyin_parser2.cpp:398` | `contract::zhuyin_out_of_enum_aborts`, `contract::full_pinyin_out_of_enum_aborts`, `e2e_tests::an_out_of_enum_chewing_scheme_aborts`, `e2e_tests::an_out_of_range_full_pinyin_scheme_aborts` |
| `*_train` on a stale forcing (row 6) | `lookup/phonetic_lookup.h:868` | `e2e_tests::training_a_stale_forcing_aborts`, `pinned_data_tests::training_a_stale_forced_choice_aborts` |
| `pinyin_get_sentence` past the rows (row 10) | `pinyin.cpp:1474` | `boundary_tests::get_sentence_aborts_past_the_rows` |
| Pinyin left and right offset after a separator or past the matrix (row 14) | `pinyin.cpp:3035`, `:3057`, `:3067`, `:3092`; `storage/phonetic_key_matrix.h:103` | `boundary_tests::left_offset_aborts_after_a_separator`, `boundary_tests::right_offset_aborts_after_a_separator`, `matrix_bound_tests::the_left_move_past_an_early_stopped_parse_aborts`, `matrix_bound_tests::the_right_move_from_past_the_last_column_aborts` |
| `*_get_character_offset` past the matrix or after a separator (row 19) | `pinyin.cpp:3147`, `:3161`, `:3203`, `:3204`; `zhuyin.cpp:2110`, `:2158`, `:1453` | `boundary_tests::character_offset_aborts_on_an_empty_leading_column`, `matrix_bound_tests::the_character_offset_past_the_last_column_aborts`, `matrix_bound_tests::the_character_offset_after_a_separator_aborts`, `matrix_bound_tests::a_null_phrase_past_the_last_column_aborts` (both facades), `boundary_tests::character_offset_on_a_leading_separator_aborts` |
| `*_parse_full_pinyin` with `'`; pinyin library unload at index 16 or more (row 21) | `storage/pinyin_parser2.cpp:170`; `pinyin.cpp:466`, `:499` | `boundary_tests::full_key_parse_aborts_on_an_apostrophe_after_invalid_bytes`, `e2e_tests::a_single_key_apostrophe_aborts`, `e2e_tests::null_context_unload_phrase_library_orders_assert_first`, `e2e_tests::null_context_unload_addon_phrase_library_aborts_on_index_16` |
| `pinyin_train` past nonempty results (row 38) | `pinyin.cpp:2684` | `pinned_data_tests::training_one_past_the_nbest_results_aborts` |
| `*_init` on an unknown `user.conf` database format (row 44) | `storage/table_info.cpp:132`, from `:353-354` | `boundary_tests::init_aborts_on_an_unknown_user_conf_database_format`, `boundary_tests::an_unknown_user_database_format_aborts_and_keeps_user_conf` |
| `pinyin_choose_candidate` at a nonzero offset without sentence candidates (row 45) | `pinyin.cpp:2566` | `boundary_tests::choose_candidate_aborts_at_offset_one_without_a_sentence` |
| A searched phrase row with a zero facade total, before the dedup (row 61) | `pinyin.cpp:1859`; `zhuyin.cpp:1261` | `boundary_tests::guess_candidates_aborts_on_a_zero_facade_total`, `boundary_tests::predicted_bigram_rows_abort_on_a_zero_facade_total`, `boundary_tests::a_zero_facade_total_aborts_the_after_cursor_ranking` |
| Pinyin library load at index 0 or 8; export iterator reads (row 62) | `pinyin.cpp:457`, `:709`, `:714`, `:721`, `:724`, `:902`; `storage/chewing_key.cpp:48`, `:50` | `boundary_tests::load_of_the_reserved_library_aborts`, `boundary_tests::bigram_get_next_aborts_on_an_empty_export`; `e2e_tests::export_iterator_aborts_on_a_reading_it_cannot_spell` |
| Pinyin key getters on an empty matrix, below `SIZE_MAX`; auxiliary text (row 63) | `storage/phonetic_key_matrix.h:103`; `pinyin.cpp:3311`, `:3488` | `boundary_tests::key_rest_aborts_on_an_empty_matrix`, `boundary_tests::full_auxiliary_text_aborts_on_a_leading_apostrophe`, `boundary_tests::double_auxiliary_text_aborts_three_bytes_into_a_key`; `e2e_tests::the_key_getters_answer_false_at_size_max_on_an_empty_matrix` |
| A candidate of the wrong row type or a non-user token (row 64) | `pinyin.cpp:2507`, `:2593`, `:2883`, `:3734`, `:3738` | `boundary_tests::choose_candidate_aborts_on_a_predicted_prefix_row`, `boundary_tests::choose_predicted_candidate_aborts_on_a_normal_row`, `boundary_tests::nbest_index_aborts_on_a_normal_row`, `boundary_tests::remove_user_candidate_aborts_on_a_system_token`, `boundary_tests::remove_user_candidate_aborts_on_a_sentence_row` |
| Pinyin right offset on an empty matrix; candidates two or more past the reserved slot (row 66) | `storage/phonetic_key_matrix.h:103`, through `pinyin.cpp:3085` and `:2226` | `matrix_bound_tests::guess_candidates_past_an_early_stopped_parse_aborts` |
| Zhuyin library load and unload, key accessors below `SIZE_MAX`, offsets and cursor guesses past the matrix (row 66) | `zhuyin.cpp:372`, `:381`, `:1453`, `:1261`; `storage/phonetic_key_matrix.h:103` | `boundary_tests::loading_library_zero_aborts`, `e2e_tests::an_unload_index_past_the_sub_indices_aborts`, `e2e_tests::null_context_unload_phrase_library_orders_assert_first`, `boundary_tests::key_rest_on_an_empty_matrix_aborts`, `boundary_tests::left_offset_past_the_matrix_aborts`, `boundary_tests::right_offset_past_the_matrix_aborts`, `boundary_tests::left_offset_just_past_a_separator_aborts`, `boundary_tests::right_offset_just_past_a_separator_aborts`, `boundary_tests::key_rest_just_past_a_separator_aborts`, `boundary_tests::after_cursor_guess_one_past_the_reserved_slot_aborts`, `boundary_tests::a_zero_facade_total_aborts_the_after_cursor_ranking`, `matrix_bound_tests::a_cursor_over_a_keyless_parse_aborts`; `e2e_tests::the_key_accessors_answer_false_at_size_max_on_an_empty_matrix` |
| More than four libraries hold a searched string (row 74) | `storage/phrase_large_table3.h:95`, `:106`; `pinyin.cpp:541`, `:1415`, `:2664`, `:3117`; `zhuyin.cpp:427`, `:933`, `:1690`, `:2080` | `boundary_tests::the_prefix_guess_aborts_on_five_tokens`, `boundary_tests::the_prediction_aborts_on_five_tokens`, `boundary_tests::the_character_offset_aborts_on_five_tokens`, `boundary_tests::remember_aborts_on_five_tokens`, `pinned_data_tests::an_import_into_an_unused_library_after_five_aborts`, `pinned_data_tests::a_prefix_guess_over_five_library_tokens_aborts`; `boundary_tests::an_import_aborts_on_five_tokens_before_the_library_refusal` |
| `*_save` on a chunk header or checksum write (row 77) | `include/memory_chunk.h:543`, `:547` | `boundary_tests::save_aborts_where_the_chunk_header_write_fails`, `boundary_tests::a_save_cut_after_the_chunk_length_word_aborts` |
| `table.conf` rows and library loads the pin refuses (row 79) | `storage/table_info.cpp:119`, `:132`, `:142`, `:156`, `:175`, `:276`; `pinyin.cpp:388`, `:457`, `:491`; `zhuyin.cpp:330`, `:372`; `include/memory_chunk.h:434`, `:493` | `boundary_tests::init_aborts_on_an_unknown_table_conf_database_format`, `boundary_tests::an_unknown_table_database_format_aborts_before_any_user_file` |
| `pinyin_remove_user_candidate` on a removal the store refuses (row 81) | `pinyin.cpp:3743`, `:3750`, `:3759` | `boundary_tests::remove_user_candidate_aborts_without_the_phrase_table`; `a_removal_assert_comes_before_the_bigram_fault` |
| `*_train` after a re-parse past the matrix (row 82) | `storage/phonetic_key_matrix.cpp:661`, `:663`, `:664` | `session::tests::train_after_a_reparse_and_a_choose_stops_the_walk_at_the_store_length` |
| `*_mask_out` on an over-long user index key (rows 84, 94) | `storage/chewing_large_table2_bdb.cpp:529` (kc `:499`, tkrzw `:466`) | `boundary_tests::mask_out_aborts_on_an_overlong_index_key_before_the_reload`, `boundary_tests::mask_out_aborts_on_an_empty_index_key`, `boundary_tests::mask_out_over_an_overlong_user_index_key_aborts` |
| `pinyin_guess_candidates` LONGER walk on an over-long index key (rows 84, 95) | `storage/chewing_large_table2_bdb.cpp:282` (kc `:269`, tkrzw `:252`) | `boundary_tests::guess_candidates_aborts_on_an_overlong_index_key`; `boundary_tests::cursor_guesses_ignore_an_overlong_user_index_key` |
| A user-bigram key that is not a token (row 86) | `storage/ngram_bdb.cpp:199` (kc `:173`, tkrzw `:150`) | `boundary_tests::mask_out_aborts_on_a_non_token_bigram_key`, `boundary_tests::bigram_export_aborts_on_a_non_token_key`, `boundary_tests::mask_out_over_a_non_token_bigram_key_aborts` |
| A residual user-bigram gram at mask-out or removal (row 87) | `storage/ngram.cpp:70`, through `ngram_bdb.cpp:243` | `boundary_tests::mask_out_aborts_on_a_residual_bigram_gram`, `boundary_tests::remove_user_candidate_aborts_on_a_residual_bigram_gram`, `boundary_tests::mask_out_over_a_residual_bigram_gram_aborts` |
| An empty bigram gram in a prediction (row 88) | `storage/ngram.cpp:70`, through `pinyin.cpp:2332` | `boundary_tests::prediction_aborts_on_an_item_less_gram_with_a_total` |
| Zhuyin import token checks (rows 89, 90, 104) | `zhuyin.cpp:440`, `:454`, `:457` | `boundary_tests::adding_a_phrase_with_two_library_tokens_aborts`, `boundary_tests::adding_a_phrase_whose_index_token_has_other_text_aborts`, `boundary_tests::adding_a_user_phrase_again_after_a_reload_aborts` |
| Pinyin import token checks (row 93) | `pinyin.cpp:554`, `:568`, `:571` | `boundary_tests::import_aborts_on_two_tokens_of_one_library`, `e2e_tests::remember_user_input_aborts_on_two_same_library_tokens`, `e2e_tests::remember_user_input_aborts_on_an_item_length_mismatch`, `e2e_tests::remember_user_input_aborts_on_an_item_text_mismatch` |
| A journal header record with a token, at init, reload or mask-out (row 91) | `storage/phrase_index_logger.h:202` | `e2e_tests::reload_aborts_on_a_log_header_token` (both facades), `e2e_tests::init_aborts_on_a_log_before_an_unmappable_library` (both facades); `init_aborts_on_a_log_header_token` |
| More than one journal header at mask-out (row 92) | `storage/phrase_index.cpp:745` | `boundary_tests::mask_out_aborts_on_two_log_headers_and_leaves_the_files` |
| A kept ADD or REMOVE record at mask-out (row 97) | `storage/phrase_index_logger.h:245`, `:257`, through `storage/phrase_index.cpp:838` | `mask_out_aborts_on_a_surviving_add` |
| A journal ADD payload under six bytes (row 108) | `include/memory_chunk.h:390`, through `storage/phrase_index.cpp:457`, `:215` and `:778`-`:795` | `e2e_tests::init_aborts_on_a_short_add_payload` (both facades); `a_short_add_payload_is_the_load_fault` |
| A user-bigram value under four bytes, at mask-out, removal, prediction, training, lookup or export (row 85) | `include/memory_chunk.h:390`, through `storage/ngram.cpp:48-51`, `:80`, `:62-70`, `:133-141`, `:165`, `:303-304`; `lookup/phonetic_lookup.h:884`, `:903` | `e2e_tests::bigram_export_aborts_where_the_walk_loads_a_short_predecessor`, `e2e_tests::bigram_get_next_writes_its_row_before_the_short_predecessor_aborts`, `e2e_tests::bigram_export_aborts_at_a_short_predecessor_before_a_partial_one`; `mask_out_aborts_on_a_short_bigram_value` |
| A system library that is a directory, from 8 bytes of reported size (row 96) | `include/memory_chunk.h:493`, through `pinyin.cpp:255`, `:289`, `:955`, `:1264`; `zhuyin.cpp:199`, `:588`, `:799` | `{init,save,mask_out,reload}_aborts_on_a_directory_library`, `addon_load_aborts_on_a_directory_library` |
| A Kyoto Cabinet system bigram read that fails after its size check (row 110) | `storage/ngram_kyotodb.cpp:136-137` | `phrase::tests::a_read_failure_after_the_size_check_aborts` (both facades), `row_read_names_only_the_checked_read_failure` |

## Open

- Stored gram partitions, phrase segmentation: the facade total awaits a ruling.
- Stored gram partitions, training: the facade total awaits a ruling.
- Promotion of an addon candidate with `table.conf` row 5 `NOT_USED`: the pin reads `range.m_range_end` unwritten (`pinyin.cpp:2541-2545`; `phrase_index.cpp:612-613`), a pin defect. oxpinyin reads its own unwritten local, so the token is not defined on either side.
- The same promotion, where the unwritten token names an unloaded system library: the pin makes a new sub-index there (`phrase_index.h:665-668`), and oxpinyin refuses the promotion. oxpinyin has no state for a live sub-index in a library whose chunk is unloaded.
- `pinyin_choose_candidate` with a candidate pointer outside the current list (oxpinyin returns `-1` before row 45's check): reading has not settled it.
- A system library's user `.dbin` that is a directory: reading has not settled it.
