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
- **oxpinyin:** `BigramExportWalk` (`src/facade/export_rows.rs`) builds each bigram row’s pronunciation strings by pairing the two tokens’ readings and joining each pair with an apostrophe. The C iterator duplicates the current pronunciation for the caller to free with `g_free`.
- **Decision:** accepted, class (b) (policy row 1): no safe construction reproduces a read past an allocation.
- **Tests:** `oxpinyin::facade::export_rows::tests::native::user_gram_unsorted_export_follows_the_stored_order`, `oxpinyin::facade::export_rows::tests::native::user_gram_duplicate_export_lists_every_stored_item`, `oxpinyin::facade::export_rows::tests::native::the_bigram_export_crosses_both_items_readings`, `oxpinyin::facade::export_rows::tests::native::bigram_export_reads_a_new_pronunciation_when_the_next_pair_loads`.

### Chewing auxiliary substring over-read

- **Pin:** `pinyin_get_chewing_auxiliary_text` calls `g_utf8_substring(zhuyin, len, end)` with the absolute raw offset `m_raw_end` as `end_pos` (`src/pinyin.cpp:3555-3560`). GLib steps `end_pos - len` scalars with no terminator check (GLib 2.89.4 `glib/gutf8.c:298-322`, `:376-386`), so for an interior cursor the copy passes the NUL and can read past the allocation.
- **oxpinyin:** `split_zhuyin` (`capi/src/aux_matrix.rs`) and `chewing_aux_text` (`text.rs`) cut the spelling at scalar `len`, with an empty right half at or past the scalar count. For `len` up to the scalar count the visible bytes are the pin's.
- **Decision:** accepted, class (b) (policy row 140): no safe, deterministic reproduction of the over-read exists.
- **Tests:** `parse_keys::chewing_cuts_the_zhuyin_spelling_by_scalar`, `parse_keys::chewing_auxiliary_text_clamps_a_cursor_past_the_input`.

### Unterminated punctuation value over-read

- **Pin:** `PunctTableEntry::unescape` reads the word after the value's last whole word to test for the terminator (`src/storage/punct_table.cpp:57-72`, from `get_all_punctuations`, `:74-94`, and `src/pinyin.cpp:2474`). Both outcomes of that read return the same run.
- **oxpinyin:** `punct::decode_puncts` (`src/data/row_format.rs`) ends the run at the value's last whole word without the read. The visible punctuation is the pin's.
- **Decision:** accepted, class (b) (policy row 141): the out-of-bounds read and its possible fault are not reproduced.
- **Tests:** `row_format` `punct_stream_ends_an_unterminated_run`.

### Aliased prediction prefix use-after-free

- **Pin:** `pinyin_guess_predicted_candidates` frees every candidate string and empties `m_prefixes` (`src/pinyin.cpp:2420-2423`, `:1335-1344`), then reads `prefix` (`:2424`, `:1389-1398`). When `prefix` is one of the instance's candidate strings, that read is a use after free; the punctuation entry calls it first and returns `true` (`:2459`, `:2498`).
- **oxpinyin:** `clear_borrowed_prefix` (`capi/src/predict.rs`) finds the alias by pointer identity before the read, empties the candidates and prefixes, and gives the no-prefix outcome: `false` from the plain entry, `true` with an empty list from the punctuation entry.
- **Decision:** accepted, class (b) (policy row 131): the read of freed storage is not reproduced; the defined parts are the pin's.
- **Tests:** `guess_predict::borrowed_prediction_prefix_clears_candidates_and_owned_text_recovers`, `guess_predict::a_borrowed_punctuation_prefix_answers_true_with_no_row`.

### NULL prediction prefix

- **Pin:** `_compute_prefixes` passes `prefix` to `g_utf8_to_ucs4` with no NULL guard (`src/pinyin.cpp:1389-1398`; GLib 2.84.4 `gutf8.c:950`), after the defined resets of its callers (`src/pinyin.cpp:2420-2423`, `:2459`, `:1431-1432`; `src/zhuyin.cpp:912-918`, `:949-953`). A NULL `prefix` dies of SIGSEGV.
- **oxpinyin:** `ucs4::prefix_text` (`oxpinyin`) turns a NULL, invalid or empty prefix into `None`: the plain entry answers `false`, the punctuation entry `true` with an empty list, and both sentence entries guess on `[sentence_start]`.
- **Decision:** accepted, class (b) (policy row 132), by maintainer ruling: a NULL prefix is a prefix that adds no token.
- **Tests:** `sentence::a_null_sentence_prefix_is_no_prefix`.

### NULL pointer arguments: the pin dereferences, oxpinyin null-guards (policy row 41)

- **Pin:** 68 `pinyin_*` exports dereference a NULL argument before any check: the instance (`src/pinyin.cpp:1359`), the context or iterator (`:657`, `:672`, `:779`, `:1196`, `:1322`), an out-parameter (`:2847`, `:2876`, `:2982`), a candidate in an `assert` (`:2507`, `:2593`), a key (`:2711`, `:2722`, `:2733`, `:2744`). The libzhuyin exports have the same shape.
- **oxpinyin:** guards the NULL arguments covered by row 41 and returns the entry point’s prescribed refusal. Other NULL cases retain their own rules.
- **Decision:** accepted, class (b): a NULL dereference is undefined.
- **Tests:** `null_arguments::null_handles_answer_false`; `oxzhuyin-capi` `null_arguments::null_arguments_answer_the_declared_value`.

### A guess on an instance whose context was finalised (policy row 42)

- **Pin:** `pinyin_fini` deletes the context but not its instances (`src/pinyin.cpp:1194-1222`), and `pinyin_guess_sentence` then reads `instance->m_context->m_pinyin_lookup` (`:1372-1380`): a use after free.
- **oxpinyin:** fini closes every native file attachment, as the pin's deletes do, and each instance keeps the context's in-memory state (`capi/src/state.rs`). A read through a closed attachment is a miss with no native call, as through a finished failed attachment. The guess completes, answers `true` and decodes no sentence; a following train answers `false`.
- **Decision:** accepted, class (b), by ruling: the use after free stays unreproduced, and a closed attachment reads as a miss (maintainer ruling 2026-11-15).
- **Tests:** `instance_outlives_context` and `fini_closes_the_system_files_while_an_instance_lives` (both C facades' `live.rs`).

### Zhuyin import into library index 16 (policy row 43)

- **Pin:** `zhuyin_begin_add_phrases` stores any index (`src/zhuyin.cpp:392-398`), and `_add_phrase` reads `m_sub_phrase_indices[16]` past the 16-entry array (`:475`; `src/storage/phrase_index.cpp:611`, `phrase_index.h:441`) and calls through it: SIGSEGV.
- **oxpinyin:** `zhuyin_begin_add_phrases` stores any index; `zhuyin_iterator_add_phrase` (`zhuyin-capi/src/iterators.rs`) answers `false` when index 16 cannot be used by the phrase store.
- **Decision:** accepted, class (b): an out-of-bounds read through a wild pointer.
- **Tests:** `null_arguments::null_arguments_answer_the_declared_value` (includes import at index 16).

### `pinyin_token_get_nth_pronunciation` past the last reading (policy row 51)

- **Pin:** for an `nth` past the last reading, `get_nth_pronunciation` writes nothing and its result is ignored, so the function appends `phrase_length` keys of the uninitialised `ChewingKey buffer[MAX_PHRASE_LENGTH]` and returns `true` (`src/pinyin.cpp:2801-2819`; `src/storage/phrase_index.cpp:33-44`).
- **oxpinyin:** returns `true` and appends `phrase_length` zeroed keys (`capi/src/dict.rs`).
- **Decision:** accepted, class (b): an uninitialised read; the return value and key count are the pin's.
- **Tests:** `phrase::token_pronunciation_surface` (the return value, the key count and the zeroed keys).

### `zhuyin_token_get_nth_pronunciation` past the last reading (policy row 70)

- **Pin:** the zhuyin twin of row 51: it appends `phrase_length` keys of an uninitialised buffer and answers `true` (`src/zhuyin.cpp:1793-1811`; `src/storage/phrase_index.cpp:33-44`).
- **oxpinyin:** answers `true` and appends `phrase_length` zeroed keys (`zhuyin-capi/src/dict.rs`).
- **Decision:** accepted, class (b): an uninitialised read.
- **Tests:** `token_getters::token_getters_answer_the_pins_ranges_and_outputs` (out-of-range return value and key count).

### `pinyin_alloc_instance` on a finalised context (policy row 60)

- **Pin:** `pinyin_alloc_instance` reads the deleted context's `m_phrase_index` (`src/pinyin.cpp:1310-1333`, at `:1322`; `pinyin_fini` at `:1194-1222`) and returns an instance with a dangling pointer. `zhuyin_alloc_instance` does the same (`src/zhuyin.cpp:845-857`).
- **oxpinyin:** `pinyin_init` and `zhuyin_init` record each live context and `fini` forgets it (`live.rs` in both C facades); allocation on an address that is not live answers NULL with no log.
- **Decision:** accepted, class (b): a use after free.
- **Tests:** `live::tests::a_context_is_live_from_init_to_fini`.

### `pinyin_bigram_iterator_get_next_phrase` after a walk that ends on a real predecessor (policy row 75)

- **Pin:** a walk's `false` row leaves `m_pinyins` empty (`src/pinyin.cpp:798-801`, `:885-886`). If the last loaded predecessor is a real phrase token, a further `get_next` passes the assert at `:902` and indexes the empty array at `:905`; on GLib 2.90 that read goes through NULL and dies of SIGSEGV.
- **oxpinyin:** answers `false` with `phrase`, `pinyin` and `count` untouched and no log (`capi/src/iterators.rs`; `BigramStep::Undefined`, `src/facade/export_rows.rs`).
- **Decision:** accepted, class (b): undefined behaviour.
- **Tests:** `export_rows::tests::the_walk_is_the_pins_state_machine` (facade walk with fixed predecessor order).

### A second `pinyin_fini` / `zhuyin_fini` on one context (policy row 105, class (b))

- **Pin:** a second `pinyin_fini` (`src/pinyin.cpp:1194-1222`) or `zhuyin_fini` (`src/zhuyin.cpp:741-757`) reads the freed context and frees its members again: a double free.
- **oxpinyin:** both return at once, with no log, for a context the live-context registry does not hold.
- **Decision:** accepted, class (b): a double free.
- **Rejected:** a registry of live instances for a second `*_free_instance` (`pinyin.cpp:1347-1356`, `zhuyin.cpp:876-884`), which would cost time and space on every alloc and free; that call stays caller UB.
- **Tests:** `init_and_save::fini_twice_returns_silently` (`oxpinyin-capi`) and `init_and_save::fini_twice_returns_silently` (`oxzhuyin-capi`).

### Candidate and prediction suggestions for an absent phrase item (policy rows 106 and 106a)

- **Pin:** candidate construction can leave the phrase string NULL after a failed item lookup (`src/pinyin.cpp:1635–1637,2010–2022,2389–2405`). Prediction can also keep the previous item's length. Multiple rows reach a NULL comparison (`:2041–2068,2082`), while a sole row can survive.
- **oxpinyin:** a missing item supplies no suggestion. A previously published live handle still follows the receiver's choice and training rules.
- **Decision:** accepted under the language-artefact rule for stale fields and cardinality-dependent NULL publication, and class (b) for a NULL comparison.
- **Tests:** `a_journal_removed_item_adds_no_candidate`, `a_sole_removed_prefix_item_is_omitted` and the C facades' `unreadable_candidates` tests.

### NULL string conversion after defined prior work

- **Pin:** With a NULL sentence, `pinyin_phrase_segment` reaches the `g_utf8_strlen` critical, then dereferences NULL in `g_utf8_to_ucs4` (`src/pinyin.cpp:1450-1452`).
- **oxpinyin:** `pinyin_phrase_segment` calls `g_utf8_strlen` for the critical and passes empty input to the segmenter (`capi/src/phrase.rs`).
- **Decision:** accepted, class (b) (policy row 142), under the no-input ruling of [NULL prediction prefix](#null-prediction-prefix).
- **Tests:** `parse_keys::null_string_arguments_after_prior_work`.

### Training a token whose library is unloaded raises a stale cached item's reading counts (policy row 118)

- **Pin:** after an unload deletes a sub-index (`src/storage/phrase_index.cpp:261-269`), `train_result3` reads the item unchecked and raises the reading counts of `m_cached_phrase_item`, which the failed read left as the last successful read set it (`src/lookup/phonetic_lookup.h:924-927`; `phrase_index.h:649-650`, `phrase_index.cpp:204`). The write is a use after free when that item was in the unloaded library, and a defined write into a live item otherwise.
- **oxpinyin:** trains no reading of a token whose library has no live buffer; the bigram trains, the unigram add moves no total and `*_train` answers `true`, as at the pin.
- **Decision:** accepted: the freed-chunk write is class (b), and the live-item write is declined under the [unloaded-token pronunciation ruling](compatibility-policy.md#unloaded-token-pronunciation-cache).
- **Tests:** `store_libpinyin::tests::unloaded_training_keeps_bigram_but_never_materializes_an_item` (`src/user/store_libpinyin/tests.rs`), `unloaded` (`src/user/store_libpinyin/tests/unloaded.rs`), `training::training_an_unloaded_token_answers_true` (`oxpinyin-capi`; the `true` answer of the training in the two-library witness, through the C entries).

### An addon row chosen on a context whose addon dictionary has no item for its token (class (b))

- **Pin:** the failed `get_phrase_item` leaves the default `PhraseItem` (`src/storage/phrase_index.h:81-84`, `:646-652`; `phrase_index.cpp:180-205`), and `pinyin_choose_candidate` calls `add_index(0, phrase, token)` (`src/pinyin.cpp:2534-2558`, at `:2557`). Its prefix walk starts at `size_t len = phrase_length - 1`, which wraps to `SIZE_MAX`, and reads past the key buffer on every backend (`phrase_large_table3_tkrzwdb.cpp:215-218`, `phrase_large_table3_kyotodb.cpp:280-283`, `phrase_large_table3_bdb.cpp:298-305`). The empty-key branch that skips the walk is reachable only after an earlier promotion made that read.
- **oxpinyin:** promotes nothing and rewrites nothing; the choose continues with the addon token and records no text (`try_promote_addon`, `capi/src/candidates.rs`). This covers a row from a context with other addon data and a same-instance row whose addon library was unloaded since the guess.
- **Decision:** accepted, class (b) (policy row 143): no part of the path has a defined pin result.
- **Tests:** none for the undefined path.

### Token getters on a phrase longer than sixteen characters (policy row 124, class (b))

- **Pin:** `_token_get_phrase` copies the stored words into `ucs4_t buffer[MAX_PHRASE_LENGTH]` (`src/pinyin.cpp:1633`, `:1639`; `src/zhuyin.cpp:1101`, `:1107`), and both `*_token_get_nth_pronunciation` getters copy the reading into `ChewingKey buffer[MAX_PHRASE_LENGTH]` (`src/pinyin.cpp:2808`, `:2815`; `src/zhuyin.cpp:1800`, `:1807`). A stored phrase length of 17 to 255 writes past the 16-entry stack buffer.
- **oxpinyin:** answers true and returns the whole item. `*_token_get_phrase` writes the stored length and the UTF-8 of the words before the first zero. `*_token_get_nth_pronunciation` appends all of the reading's keys (`capi/src/dict.rs`, `zhuyin-capi/src/dict.rs`).
- **Decision:** accepted, class (b), by maintainer ruling (2026-11-15): the pin's behaviour is undefined, so oxpinyin gives a defined result. It needs a crafted chunk or user library; an import refuses a phrase of sixteen characters or more (`src/pinyin.cpp:643-644`).
- **Tests:** `token_getters_return_a_phrase_longer_than_sixteen_whole` (`capi/tests/it/token_getters.rs`, `zhuyin-capi/tests/it/token_getters.rs`). Nothing runs on the pin: its result is undefined.

### Shipped item with a reading past the content end (policy row 130, class (b))

- **Pin:** the nth getter copies a reading out of the header-derived view (`src/storage/phrase_index.cpp:33–44,180–205`; `src/pinyin.cpp:2801–2818`; `src/zhuyin.cpp:1793–1810`). A reading past the allocation is undefined.
- **oxpinyin:** the getters return the available header fields, text and complete readings inside the selected record. The nth getter returns false for an unavailable reading.
- **Decision:** accepted, class (b), for an out-of-bounds read. The record boundary also applies when later records are present.
- **Tests:** `a_shipped_item_with_a_reading_past_the_content_end_keeps_its_fields` and `an_unreadable_in_range_pronunciation_returns_false_and_an_empty_array`.

### Constraint checks use the current token's item (policy row 164)

- **Pin:** constraint validation can keep the prior item's readings when a failed lookup leaves the view unchanged (`src/lookup/phonetic_lookup.cpp:141,150–164`; `src/storage/phrase_index.h:649–650`; `src/storage/phrase_index.cpp:189–193`).
- **oxpinyin:** the constraint needs readings for its own token. An absent item clears its constraint.
- **Decision:** accepted under the language-artefact rule. The previous token does not supply readings for the current one.
- **Tests:** `constraint_spelling_sums_duplicate_paths_at_the_f32_boundary`.

### Punctuation constraint abort stays at the clear or write (policy row 165)

- **Pin:** the zero-length punctuation choice replaces the head cell of the span at column zero and leaves its continuations. A later clear or span write through a continuation asserts when the head no longer owns it (`src/pinyin.cpp:2577–2590`, `src/lookup/phonetic_lookup.cpp:61–118`).
- **oxpinyin:** returns the caller's offset and records the affected span bounds in one marker. It trims the bounds when a later write covers them. A clear or write through a marked span aborts if the head is gone. Reset discards the marker.
- **Decision:** accepted under the language-artefact rule. The marker preserves the later clear or write assertion without retaining a zero-length constraint or any orphaned cells.
- **Tests:** `choose::a_clear_through_a_freed_run_start_aborts`, `choose::choosing_across_a_freed_run_start_aborts`, `choose::punctuation_allows_clear_until_a_live_head_is_freed`, `choose::reset_discards_punctuation_constraint_history`.

## Accepted divergences — store and data files

### The pinyin index DBMs carry uninitialized struct padding

- **Pin:** `ChewingTableEntry::add_index` copies a whole stack `PinyinIndexItem2<N>` into the DBM value (`src/storage/chewing_large_table2.h:208-232`, at `:229`), and its constructor sets only `m_token` and `m_keys` (`src/storage/pinyin_phrase3.h:181-196`). For an odd `N` the item's two bytes of tail padding are indeterminate; no reader reads them.
- **oxpinyin:** `row_format::pinyin_index::encode_item` (`src/data/row_format.rs`) zeroes the padding, and `pinyin_index::decode_items` reads only the token and keys. The tool comparison masks only the two tail bytes of each odd-`N` item.
- **Decision:** accepted, class (b) (policy row 18): the pin's bytes are an indeterminate read that no program can reproduce.
- **Tests:** `gbf-toned-model`; controls `gbf-mini-fixture`, `kmm-estimate-good`; `pinyin_index_mask_clears_only_odd_item_padding`.

### `mmap %s failed!`: a system library the pin cannot map, and the empty chunk it dies on (policy row 76)

- **Pin:** At init and mask-out, the pin prints `mmap %s failed!` before loading the empty chunk and reading past it (`src/pinyin.cpp:256`, `:1265`; `src/zhuyin.cpp:200`, `:800`; `src/storage/phrase_index.cpp:335-363`).
- **oxpinyin:** Pinyin and Zhuyin init report an earlier missing library and fail at the later unmappable library they reach. Pinyin mask-out with a removed `gb_char.bin` emits the mmap diagnostic and returns `true`, preserving the tested lookups, unigram count and profile files.
- **Decision:** accepted, class (b): the read through the empty chunk is not reproduced.
- **Tests:** `context::tests::library_directory::init_reports_only_what_the_library_loop_reached`, `oxpinyin::facade::context::tests::native::mask_out_over_an_unmappable_library_changes_nothing` (init and mask-out). At the C boundary: `library_loading::addon_load_of_an_empty_library_writes_the_mmap_line`, `init_and_save::init_answers_for_the_two_library_layout_faults` and `init_and_save::a_save_retries_after_a_failed_remap` (`oxpinyin-capi`); `init_and_save::a_reload_and_a_mask_out_report_a_library_that_cannot_be_mapped` and `init_and_save::a_save_stops_at_an_unmappable_library_after_the_earlier_renames` (`oxzhuyin-capi`).

### A sparse chunk's whole mapping is retained after `fini` (#865; policy row 109)

- **Pin:** `MemoryChunk::mmap` maps the whole `file_size` (`src/include/memory_chunk.h:475-524`), but `freemem` unmaps only `header + capacity()` (`:72-78`). When `guint32 data_len` wraps, as for a 2^32+8 file, most of the mapping stays after `fini`.
- **oxpinyin:** `oxpinyin::input::chunk` owns one private whole-file mapping per loaded chunk, and its `Drop` releases all of it.
- **Decision:** the mapping is reproduced; the wrapped-size residual is accepted under the external API misuse ruling: the pin's `munmap` length contradicts its own mapping.
- **Tests:** `chunk::tests::sparse_file_has_one_whole_writable_private_mapping_and_no_residual` (`src/input/chunk.rs`).

### `table.conf` follows its rows; malformed ones fail the triggering call (policy rows 78, 79, 80 and 83)

- **Pin:** a missing `source table format:` line leaves `str` unwritten and compares it (`src/storage/table_info.cpp:209-229`): undefined. A `table.conf` that points a loaded library at another file leaves tokens with no item, and the formatter leaves the string NULL (`src/pinyin.cpp:1635-1637`, `:2010-2016`). The string comparison is undefined if it reaches that NULL (`:2053`, `:2082`). A sole ordinary row survives when the facade total is positive and the prepends add no row. At zero the ranking assertion fires before formatting (`:1855-1865`, row 61).
- **oxpinyin:** the missing line is `TableConfError::UninitialisedSourceFormat`: init answers NULL with one warning. Candidate guesses omit the unreadable item and return true without a warning on the recorded comparison paths. For a sole searched row with a positive total and no prepended rows, both facades publish it with a NULL string. At zero total, both facades reproduce the ranking abort for an unreadable searched row. Row 106 records the choose and zero-total boundaries.
- **Decision:** accepted, class (b) (row 80); rows 78 and 83 read the rows and λ as the pin does, and row 79 reproduces the pin.
- **Tests:** `table_info::tests::header_misses_are_the_ordinary_false_or_the_uninitialised_read` (header parsing), `oxpinyin::facade::context::tests::native_files::{pinyin,zhuyin}_table_conf_919_short_row_merged` (row handling), `unreadable_candidates::{a_sole_unreadable_after_cursor_row_has_a_null_string,two_unreadable_rows_are_omitted,a_sole_unreadable_row_survives_the_user_table_fallback,zero_total_aborts_after_cursor}` in both C facades and `unreadable_candidates::choosing_a_sole_unreadable_row_reports_the_span_and_clears_the_constraint` in `oxpinyin-capi` (candidate rows).

### Journal operations use their own item (policy row 107)

- **Pin:** `SubPhraseIndex::merge` compares the view left by an earlier read when an absent token leaves the item untouched, and can write through that stale view (`src/storage/phrase_index.cpp:439–517`, `src/storage/phrase_index.h:646–652`); absent REMOVE can dereference NULL.
- **oxpinyin:** an absent MODIFY stops at that record. An absent REMOVE is a no-op. Ordered replay compares only the addressed item's old bytes. The matching prefix is kept, except on a truncated journal payload, which rolls the replay back.
- **Decision:** accepted under the language-artefact rule for stale views and as class (b) for the NULL dereference.
- **Tests:** `an_absent_modify_stops_with_the_previous_item_intact`, `an_incomplete_modify_cannot_borrow_bytes_from_other_records`, `rollback_restores_the_buffer_for_every_kind_of_mutation`.

### Bigram export iterator reads past a partial user-gram item (policy row 98, class (b))

- **Pin:** `SingleGram::retrieve_all` walks with `cur != end` and reads a whole item (`src/storage/ngram.cpp:133-143`, from `src/pinyin.cpp:789-889`), so a value of `4 + 8k + r` bytes, `1 <= r <= 7`, is read past its end.
- **oxpinyin:** the export walk returns `false` with one `libpinyin` warning when it reaches a predecessor with a partial tail. A mask or candidate removal over such a surviving predecessor answers `false` with one warning; the raw value is kept for save.
- **Decision:** accepted, class (b): an out-of-bounds read.
- **Tests:** `import_export::bigram_export_ends_at_a_partial_predecessor`, `user_store::mask_out_refuses_a_partial_bigram_tail`.

### Training on a malformed user-gram value reads past a partial item (policy row 100, class (b))

- **Pin:** `SingleGram::insert_freq`, `get_freq` and `set_freq` walk with `cur_item != end` (`src/storage/ngram.cpp:178`, `:190`, `:231`, `:241`, `:252`, `:261`), reached from `train_result3` (`src/lookup/phonetic_lookup.h:889`) and `pinyin_choose_predicted_candidate` (`src/pinyin.cpp:2630-2633`); a partial tail is read past.
- **oxpinyin:** `*_train` and `pinyin_choose_predicted_candidate` answer `false` with one facade-domain warning when the step would load such a predecessor; the raw value survives save and reopen byte for byte.
- **Decision:** accepted, class (b): the same out-of-bounds read as row 98, on the training path.
- **Tests:** `training_over_a_partial_tail_refuses_and_keeps_the_raw_value`, `a_partial_tail_created_in_session_refuses_training` (`src/user/store_libpinyin.rs`).

### A partial phrase-table token tail cannot terminate the entry walk (policy row 101, class (b))

- **Pin:** `PhraseTableEntry` walks four-byte tokens with `iter != end` over the raw byte end (`src/storage/phrase_large_table3.cpp:32-39`, `:57-65`, `:74-81`, `:103-117`), so a value of `4k + r` bytes, `1 <= r <= 3`, is read past the record.
- **oxpinyin:** The native tree keeps the raw record across load and save. `UserPhraseTable::tokens` reads it on demand, and `phrase_table::search_bytes` refuses it with `StoreError::InvalidInput`.
- **Decision:** accepted, class (b): an out-of-bounds read.
- **Tests:** `a_partial_phrase_table_record_is_refused_before_any_change` (`src/user/store_libpinyin.rs`).

### A reloaded library's dropped item stays in the user index tables, and a candidate walk dereferences it (policy row 102, class (b))

- **Pin:** `FacadePhraseIndex::unload` frees the session's items (`src/storage/phrase_index.cpp:261-268`), the reload merges only the saved `.dbin` (`src/pinyin.cpp:264-277`, `src/zhuyin.cpp:208-221`), and the user index tables keep the dropped token; `*_guess_candidates` then reads the absent item and dies of SIGSEGV.
- **oxpinyin:** Reloading a pinyin system library after an unsaved reading was added discards that reading and restores the saved readings from the journal.
- **Decision:** accepted, class (b): a dereference of an item the sub-index no longer owns.
- **Tests:** `reloading_a_system_library_drops_the_unsaved_session` (store reload and saved readings).

### Phrase reads and writes stay inside their record (policy rows 107, 111 and 130)

- **Pin:** `SubPhraseIndex` uses header-derived item extents and narrowed content offsets (`src/storage/phrase_index.cpp:150–236,439–517,624–675`). Item views can cross into later records, retain prior payload bytes, overlap after offset wrap, or write through shared backing storage (`src/include/memory_chunk.h:102–130,372–390`).
- **oxpinyin:** reads stop at the next indexed item or the end of content. Journal payloads stand alone. Each write changes only its addressed slot, even when two loaded offsets were equal. Partial last-item getters still return the fields present in that record.
- **Decision:** accepted under the language-artefact rule. Reads that would leave the allocation retain the class (b) outcome.
- **Tests:** `count_checks_both_header_bytes_within_the_record`, `a_read_crossing_the_item_boundary_answers_no_item`, `a_truncated_system_record_cannot_modify_the_next_record`, `aliased_offsets_keep_independent_values_through_save_and_reopen`, `a_shipped_item_with_a_reading_past_the_content_end_keeps_its_fields`.

### An oversized user chunk or journal bulk save (policy row 112)

- **Pin:** user saves and utility chunk writes both call `SubPhraseIndex::store`. Buffer growth narrows to `int`, so the content copy is unsafe from 2^31 bytes (`src/storage/phrase_index.cpp:365-390`, `src/include/memory_chunk.h:95-98,290-296`). The same limit applies to journal copies (`src/storage/phrase_index.cpp:271-283`, `src/storage/phrase_index_logger.h:115-117`).
- **oxpinyin:** `*_save` stops at the oversized copy with `PersistenceError::SaveTooLarge` and returns `false`. Earlier file changes stay and the session stays dirty. When an item's offset in the chunk content exceeds `u32::MAX`, `build_chunk` returns `None` (`src/data/phrase_library/write.rs`) and the utility aborts (`gen_binary_files`, `gen_unigram`, `gen_ngram`, `import_interpolation`).
- **Decision:** accepted, class (b): the copy past the allocation is not performed.
- **Tests:** `a_user_chunk_copy_at_the_limit_stops_the_save_where_it_is`, `a_system_journal_copy_at_the_limit_stops_the_save_and_retries`, `the_store_copy_threshold_follows_the_compacted_content`.

### A negative `MemoryChunk::load` payload read (policy row 114)

- **Pin:** `MemoryChunk::load` converts a negative `read` result, `EINTR` included, to `guint32` and checksums that length past the allocation (`src/include/memory_chunk.h:452-453`).
- **oxpinyin:** returns the I/O error before checksumming, releases the buffer and descriptor, and does not retry.
- **Decision:** accepted, class (b), by maintainer ruling: an out-of-bounds read.
- **Tests:** `chunk::tests::interrupted_payload_read_is_not_retried` (`src/input/chunk.rs`).

### Berkeley DB handles whose open failed (policy row 113)

- **Pin:** keeps a Berkeley DB handle whose `DB->open` failed and calls it again, so libdb prints `BDB1565 DB-><method>: method not permitted before handle's open method` ([policy row 113](compatibility-policy.md#bdb-failed-handles)).
- **oxpinyin:** On Berkeley DB, an open that fails closes its handle (`src/store/bdb/mod.rs`), so no later call reaches libdb and no `BDB1565` line is written. The frozen cases show it for `estimate_k_mixture_model`, `export_k_mixture_model`, `validate_k_mixture_model`, `prune_k_mixture_model`, `gen_ngram`, `gen_deleted_ngram`, `estimate_interpolation`, `eval_correction_rate` and `export_interpolation`.
- **Decision:** accepted under the external API misuse ruling: Berkeley DB requires `DB->close` after a failed open.
- **Tests:** the Berkeley DB arm of each frozen case that cites this entry or policy row 113: `kmm_estimate_existing_empty`, `kmm_estimate_no_models`, `kmm_export_absent_file`, `kmm_export_existing_empty`, `kmm_validate_existing_empty`, `kmm_validate_missing_file`, `kmm_prune_existing_empty` (`utils/tests/it/kmm_tools.rs`); `estimate_interpolation_missing_bigram_file`, `estimate_interpolation_missing_deleted_bigram`, `eval_correction_rate_missing_bigram_file`, `gen_deleted_ngram_{empty_stdin,missing_bigram_file,ordinary,too_many_arguments}`, `gen_ngram_{empty_stdin,missing_bigram_file,ordinary,skip_pi_gram,too_many_arguments}` (`utils/tests/it/training.rs`); `exp_missing_bigram` (`utils/tests/it/storage_tools.rs`).

### Pinyin-table row with a partial record (policy row 128, class (b))

- **Pin:** search and suggestion pass the raw row bounds as `IndexItem` pointers to `std_lite::equal_range` (`src/storage/chewing_large_table2.h:145-150`, `:197-202`). The pin counts whole 8-, 16- and 32-byte records. The pointer difference is undefined for the other record sizes when the row has a partial tail.
- **oxpinyin:** reads the whole records and drops the partial tail at every record size.
- **Decision:** accepted, class (b), for 12-, 20-, 24-, 28- and 36-byte records. The other sizes reproduce the pin.
- **Rejected:** treating the whole row as a miss discards the complete records the pin can read.
- **Tests:** `chewing_table::tests::a_misaligned_pinyin_table_row_keeps_its_whole_records` (`oxpinyin`) and `decode_items_drops_a_partial_record_and_rejects_zero_length` (`oxpinyin`).

### User pinyin-index row with a partial record (policy row 129, class (b))

- **Pin:** the user table uses the same `ChewingLargeTable2` search and suggestion bounds as row 128 (`src/storage/facade_chewing_table2.h:125-126`, `:151-152`; `src/storage/chewing_large_table2.h:145-150`, `:197-202`). When a partial tail is present, the pin counts whole 8-, 16- and 32-byte records, and the pointer difference is undefined for the other sizes.
- **oxpinyin:** search, suggestion and reading membership use the whole records at every size. Writes preserve the tail bytes.
- **Decision:** accepted, class (b), for 12-, 20-, 24-, 28- and 36-byte records. The other sizes reproduce the pin.
- **Rejected:** reporting a miss for the entire row loses complete records.
- **Tests:** `user_index::tests::a_misaligned_row_answers_its_whole_records` (`oxpinyin`).

### Native database objects after a failed open (policy row 157)

- **Pin:** system lookups and utilities can call a native object whose open failed (`src/pinyin.cpp:357,369,396,421,429,440`; `src/storage/ngram_kyotodb.cpp:43–50,110–153,186–196`; `src/storage/ngram_tkrzwdb.cpp:40–46,86–133,160–172`).
- **oxpinyin:** drops failed handles and represents the attachment as unavailable. Lookups miss, utility walks collect no keys and writes refuse where they previously refused. Successful files keep their sync and close order, and the Kyoto Cabinet CREATE retry uses the same object. An ignored walk failure after that retry still gives a successful empty result. In-memory tkrzw stores drop without a file close (`tkrzw_dbm_tiny.cc:256–260`, `tkrzw_dbm_baby.cc:394–398`).
- **Decision:** accepted under the language-artefact rule. Object lifetime after a failed open is an implementation detail.
- **Tests:** `a_failed_bigram_attach_attempts_the_open_and_releases_the_object`, `a_create_retry_reopens_the_same_hashdb_and_a_failed_open_releases_it`, `a_failed_create_retry_preserves_the_backend_walk_result`, `a_system_tree_synchronizes_before_close_and_a_hash_does_not`.

### Assigned configuration values (policy row 79)

- **Pin:** the two format directives reuse a C buffer even after a failed scan (`src/storage/table_info.cpp:227–233`).
- **oxpinyin:** validates each format using its assigned value and keeps the prescribed missing-format outcomes.
- **Decision:** accepted under the language-artefact rule. The configuration specifies format names.
- **Tests:** `a_missing_directive_does_not_supply_the_previous_word`.

### Incomplete journal records stop at the missing field (policy row 159)

- **Pin:** short token, length and header-total reads can leave zero in their destinations (`src/storage/phrase_index_logger.h:158–215`; `src/include/memory_chunk.h:372–377`; `src/storage/phrase_index.cpp:503–514,721–746`).
- **oxpinyin:** stops before applying an incomplete record, keeps the complete prefix and retains rollback for a truncated payload. A fully framed header counts even when its totals are unavailable. In ordinary replay it leaves a zero live total alone and stops on a nonzero total. Masking checks the record shape and can emit a replacement journal only from decoded totals.
- **Decision:** accepted under the language-artefact rule. A field absent from the wire record does not supply zero.
- **Tests:** `incomplete_records_stop_after_the_last_complete_record`, `bounded_short_header_runs_keep_later_journal_faults_reachable`.

## Accepted divergences — utilities

### gen_pinyin_table invalid UTF-8 phrase (class (b))

- **Pin:** a phrase that is not valid UTF-8 can pass `feed_line`'s checks and enter the tree with a NULL `uniphrase` (`utils/storage/gen_pinyin_table.cpp:160-232`, at `:174`); `gen_phrase_file` opens the output (`:261`) and then reads through the NULL (`:280-281`), or `phrase_item_compare` does earlier (`:85-94`): SIGSEGV. `gen_zhuyin_table` is the same (`utils/storage/gen_zhuyin_table.cpp:168-192`, `:90-98`, `:286-287`).
- **oxpinyin:** prints the node count, opens the output at the pin's point, writes the rows before the invalid one, then refuses with `<utility>: invalid UTF-8 phrase: <bytes>` and exit 1.
- **Decision:** accepted, class (b) (policy row 144): a NULL read; the pin's lost stdout buffers are not reproduced.
- **Tests:** `pinyin-invalid-utf8`, `pinyin-invalid-utf8-after-row`, `zhuyin-invalid-utf8`, `zhuyin-invalid-utf8-after-row` (`utils/tests/it/gen_pinyin_table.rs`, `gen_zhuyin_table.rs`).

### gen_zhuyin_table missing frequency (class (b))

- **Pin:** `atoi(strs[1])` runs before the field count check (`utils/storage/gen_zhuyin_table.cpp:148-159`, `:153` before `:156`), so an empty or one-field line dereferences NULL: SIGSEGV.
- **oxpinyin:** refuses with `gen_zhuyin_table: missing frequency: <line>` and exit 1, before it opens the output.
- **Decision:** accepted, class (b) (policy row 133): a NULL read.
- **Tests:** `zhuyin-missing-frequency`; control `zhuyin-normal` (`utils/tests/it/gen_zhuyin_table.rs`).

### gen_zhuyin_table truncated final lead byte (policy row 125, class (b))

- **Pin:** a phrase field that ends in a truncated UTF-8 lead byte, such as the line `"\xE3 1 ㄚ"`, makes `g_utf8_strlen` step past the end of the `g_strsplit_set` allocation (`utils/storage/gen_zhuyin_table.cpp:150`, `:168`): a heap over-read, so the length is undefined.
- **oxpinyin:** counts bounded UTF-8 chunks inside the field (`utils/src/gen_zhuyin_table.rs`), so the example has length 1. Its reading `ㄚ` parses to one key, so the row enters the tree and takes the invalid UTF-8 phrase path above: the output opens at the pin's point, the earlier rows are written, and the utility refuses with exit 1. A reading that does not parse to the counted length takes the pin's `Invalid pinyin` skip (`utils/storage/gen_zhuyin_table.cpp:191-195`), as `ㄅ` does: it is an incomplete syllable (`src/storage/pinyin_parser_table.h:1493`), and the generator parses without `ZHUYIN_INCOMPLETE` (`gen_zhuyin_table.cpp:187`).
- **Decision:** accepted, class (b), by maintainer ruling (2026-11-15): the pin's behaviour is undefined, so oxpinyin gives a defined result.
- **Tests:** `a_phrase_ending_in_a_truncated_lead_byte_is_refused` (`utils/tests/it/gen_zhuyin_table.rs`). Nothing runs on the pin: its result is undefined.

### gen_pinyin_table truncated lead byte on the first line (policy row 126, class (b))

- **Pin:** the first truncated UTF-8 field can read indeterminate bytes after its terminator. A later row can read bytes left by an earlier row (`utils/storage/gen_pinyin_table.cpp:135,162`).
- **oxpinyin:** counts bounded UTF-8 chunks in the current field and retains the invalid-UTF-8 refusal.
- **Decision:** accepted, class (b), for the indeterminate read and under the language-artefact rule for stale bytes in later rows.
- **Tests:** `a_truncated_phrase_is_refused` (`utils/tests/it/gen_pinyin_table.rs`).

### Generator phrase index of 256 or more, or negative (policy row 127, class (b))

- **Pin:** `phrase_index` is a `gint` (`utils/storage/gen_pinyin_table.cpp:36`, `:41`; `utils/storage/gen_zhuyin_table.cpp:42`, `:47`), and `PHRASE_INDEX_MAKE_TOKEN` shifts it left by 24 (`src/include/novel_types.h:45-46`). The pin builds at C++11 or later (`configure.ac:51`). Under CWG 1457, which compilers also apply to C++11, a non-negative shift whose result fits the unsigned type is defined, and GCC and Clang wrap the conversion back to `int`. A value from 128 to 255 therefore gives the library nibble as the value modulo 16: pin behaviour reproduced, held by the frozen cases `pinyin-phrase-index-131` and `zhuyin-phrase-index-131`. A value of 256 or more, or a negative value, is undefined behaviour.
- **oxpinyin:** reduces the library index modulo 16 and uses the persisted token encoder (`utils/src/gen_pinyin_table.rs`), and the tokens print through `%d`.
- **Decision:** accepted, class (b), by maintainer ruling (2026-11-15): the pin's behaviour is undefined, so oxpinyin gives a defined result.
- **Tests:** `a_phrase_index_outside_the_int_shift_range_wraps_into_the_library_nibble` (`utils/tests/it/gen_pinyin_table.rs`, `utils/tests/it/gen_zhuyin_table.rs`), with 259 and −1. Nothing runs on the pin: its result is undefined.

### k_mixture_model_to_interpolation undefined lines

- **Pin:** `taglib_read` reads `tokens[0]` as the line tag (`src/storage/tag_utility.cpp:179-190`): an empty line, a re-read empty last line (`utils/training/k_mixture_model_to_interpolation.cpp:86-88`) and a trailing escape that makes `split_line` return NULL (`tag_utility.cpp:137`) read through NULL: SIGSEGV. A truncated UTF-8 sequence at the line end reads past the string.
- **oxpinyin:** makes the pin's calls up to that read, then writes `k_mixture_model_to_interpolation: <reason>: <line>` and exits 1.
- **Decision:** accepted, class (b) (policy row 134): a NULL read or an over-read.
- **Tests:** `kti-blank-line`, `kti-end-missing`, `kti-trailing-escape` (`utils/tests/it/k_mixture_model_to_interpolation.rs`).

### An exhausted utility stream supplies no previous line (policy rows 134 and 162)

- **Pin:** the converter reuses the line after a failed C++ stream read. A final unterminated item reaches the outer tag assertion. A final section marker can be read repeatedly (`utils/training/k_mixture_model_to_interpolation.cpp:86–88`, C++ `[string.io]`).
- **oxpinyin:** processes each line once. After an unterminated final item, it closes the section and aborts at the outer checked read. It refuses a final section marker whose body is missing.
- **Decision:** accepted for repeated reads under the language-artefact rule. The pin’s abort and preceding output are reproduced.
- **Tests:** `getline_reads_the_final_line_once_and_clears_on_eof`, `kti_last_line_reread_assert`.

### g_utf8_next_char from a byte that leads no sequence

- **Pin:** `split_line` uses `g_utf8_next_char` (`src/storage/tag_utility.cpp:118–169`), whose lead-byte table can skip valid bytes after a malformed sequence.
- **oxpinyin:** walks bounded Rust UTF-8 chunks, keeping the later valid bytes and the prescribed refusal for a truncated final sequence. A checked read aborts on an unreadable tag name in the bounded tokenizer. The tag grammar and GLib character classifiers still apply.
- **Decision:** accepted under the language-artefact rule (policy row 135). The documented macro expects valid UTF-8.
- **Tests:** `split_line_keeps_bytes_after_a_malformed_sequence`, `a_checked_read_refuses_an_unreadable_tag`, `kti_unreadable_tag_aborts`, `text_calls_keep_their_format_contracts`.

### Utility check macros write no message

- **Pin:** In the reference build, `check_result` is `assert` (`src/include/pinyin_utils.h:26-31`). The malformed interpolation input checks can emit GLib criticals (`src/storage/tag_utility.cpp:202`, `:246`) and the glibc assertion line before SIGABRT.
- **oxpinyin:** `k_mixture_model_to_interpolation` aborts without the check-macro diagnostics in the unknown-line, missing-value, bigram and final-item cases.
- **Decision:** accepted, language artefact (policy row 136), by maintainer ruling: oxpinyin does not imitate the macros' text.
- **Tests:** `kti_unknown_line_assert`, `kti_missing_value_assert`, `kti_bigram_assert`, `kti_last_line_reread_assert` (`utils/tests/it/k_mixture_model_to_interpolation.rs`).

### KMM tool asserts write no macro text

- **Pin:** The reference build emits assertion text and aborts at the missing merge result file, zero estimation total and generator document assertion sites (`utils/training/merge_k_mixture_model.cpp:127`, `estimate_k_mixture_model.cpp:45`, `gen_k_mixture_model.cpp:264`).
- **oxpinyin:** The KMM merge, estimate and generate tools abort without the assertion text in the missing-result-file, zero-total and good-document cases.
- **Decision:** accepted under the check-macro ruling above (policy row 137).
- **Tests:** `kmm_merge_no_result_file`, `kmm_estimate_zero_total`, `kmm_gen_good_document` (`utils/tests/it/kmm_tools.rs`).

### FlexibleBigram malformed record lengths

- **Pin:** the `FlexibleBigram` backends hand a whole stored value to `FlexibleSingleGram` (`src/storage/flexible_ngram_bdb.h:165-187`, `flexible_ngram_kyotodb.h:192-216`, `flexible_ngram_tkrzwdb.h:200-225`). A value shorter than the header asserts in `get_array_header` (`flexible_ngram_bdb.h:388`, `flexible_ngram_kyotodb.h:371`, `flexible_ngram_tkrzwdb.h:388`) or is read past its end (`flexible_single_gram.h:88-105`, `:151-181`, `:300-306`); a partial item tail is read past when walked.
- **oxpinyin:** aborts at the pin's assert sites; where the pin reads past a short value, the tools exit 1 with `flexible bigram single gram is shorter than its array header`; a partial item tail loads as a missing row; merge copies a short source row the target lacks with `copy_single_gram`.
- **Decision:** accepted, class (b) (policy row 145): the reads past a record are not reproduced.
- **Tests:** `oxpinyin_tool_support::flexible_ngram::tests::a_truncated_single_gram_is_refused_by_length` (store decoding) and `oxpinyin_tool_support::flexible_ngram::tests::flexible_bigram::a_short_array_header_aborts_at_the_getter` (getter abort). In `oxpinyin-utils`, `k_mixture_model::merge::tests::a_short_target_row_aborts_at_the_targets_header` (merge abort), `k_mixture_model::database::tests::a_checked_load_of_a_short_row_takes_the_store_error_exit`, `k_mixture_model::estimate::tests::a_short_candidate_row_takes_the_store_error_exit` and `k_mixture_model::generate::tests::a_short_first_word_row_takes_the_store_error_exit` (exit 1), `k_mixture_model::merge::tests::a_short_source_row_the_target_lacks_is_copied_unchanged` (the copy).

### Consumed invalid UTF-8 in utility text rows or phrase-table queries (policy row 115)

- **Pin:** `FacadePhraseIndex::load_text` passes a failed `g_utf8_to_ucs4` to `set_phrase_string(written, NULL)` with `written` uninitialised (`src/storage/phrase_index.cpp:541-586`); the phrase-table test searches the same failed output (`tests/storage/test_phrase_table.cpp:45-59`; `src/storage/phrase_large_table3_tkrzwdb.cpp:133-136`, `phrase_large_table3.cpp:131-135`). In `gen_binary_files`, the phrase-table pass precedes the phrase-index pass and both saves (`utils/storage/gen_binary_files.cpp:68-83`). The input domain of `gen_binary_files` is valid UTF-8 tables, as produced by libpinyin’s own data and trainer. A phrase that fails conversion falls outside that domain. The pin does not handle this case. It passes the NULL conversion result to the backend as a key of nonzero size (`src/storage/phrase_large_table3.cpp:131-135`). The backend receives the key (`src/storage/phrase_large_table3_bdb.cpp:278-283`, `phrase_large_table3_kyotodb.cpp:264-267`, `phrase_large_table3_tkrzwdb.cpp:199-202`).
- **oxpinyin:** the REPL modes load strictly (`refuse_invalid`; `repl/src/main.rs`, `repl/src/tables.rs`): they stop the consuming failures with `exit(EINVAL)` and no added diagnostic, keeping earlier output; the defined same-token path continues as in the pin. The utility calls `tool::abort()` on the first failed conversion in the phrase-table pass.
- **Decision:** Class (b). For safe handling of this input, oxpinyin aborts at that record, before the phrase-index pass and both library saves. The strict REPL modes retain `exit(EINVAL)`. The pin is not run on this input.
- **Tests:** `invalid_utf8_aborts_before_the_phrase_index_pass` in `utils/tests/it/storage_tools.rs` checks SIGABRT, the absence of a phrase-index diagnostic and both unwritten library files. `phrase_table_invalid_text` and `invalid_text_for_a_new_item_is_refused_at_the_table_reader` in `utils/src/storage/gen_binary_files/tests.rs` check strict loading. The REPL's `chewing_table_invalid_text` checks strict item loading through the mode. `invalid_phrase_query_keeps_the_previous_result` in `repl/tests/it/table_guards.rs` checks query refusal and the preceding output.

### More than sixteen keys passed to the chewing-table test search (policy row 116)

- **Pin:** the test passes every parsed key to a search whose local `ChewingKey index[MAX_PHRASE_LENGTH]` has sixteen slots (`tests/storage/test_chewing_table.cpp:71-86`; `src/storage/chewing_large_table2.cpp:161-172`; `src/storage/pinyin_phrase3.h:160-177`): a stack-buffer write.
- **oxpinyin:** after the first incomplete key's tone check (`pinyin_phrase3.h:146-157`), stops before the copy with `exit(EINVAL)`, keeping earlier output.
- **Decision:** accepted, class (b), by maintainer approval.
- **Tests:** `overlong_chewing_query_keeps_the_previous_result` in `repl/tests/it/table_guards.rs` checks refusal after prior output. `chewing_table_overlong_toned_first` checks tone validation before the length check.

## Approved product and ruling exceptions

### Pinyin trellis adjusted cost — S1/S2 repair

- **Pin:** the trellis comparator lets an adjacent longer sentence win by `LONG_SENTENCE_PENALTY` and then says it loses in reverse (`src/lookup/phonetic_lookup.h:39`, `:65-91`), which violates the strict weak ordering of the standard heap algorithms at S1 beam selection (`:770-771`) and S2 final-tail selection (`:331-335`).
- **oxpinyin:** `nbest.rs` selects by `S = poss - length * P`, `P = 1.2_f32.ln()`, in f32, larger S first, NaN last, then earlier input order. Node admission uses adjusted score alone, so a tie keeps the incumbent. The two-slot front-eviction algorithm follows the pin.
- **Decision:** accepted under the [S1/S2 ruling](compatibility-policy.md#s1-and-s2-one-ordered-adjusted-score) (policy row 146).
- **Tests:** `nbest::tests::adjusted_score_rounds_product_then_difference_and_ties`, `nbest::tests::float_adjusted_comparison_is_a_strict_weak_order`.

### Pinyin tail sort — S3 repair

- **Pin:** `trellis_value_compare` returns the negated `gfloat` difference as `gint` (`src/lookup/phonetic_lookup.h:174-178`) and is passed to `g_ptr_array_sort` (`:337`); its equality is not transitive, against the consistent total order a qsort-style comparator needs.
- **oxpinyin:** `tail_merge` (`src/engine/nbest.rs`) sorts the tails stably by raw possibility descending, NaN last, with no integer truncation.
- **Decision:** accepted under the [S3 ruling](compatibility-policy.md#s3-stable-descending-possibility) (policy row 147).
- **Tests:** `nbest::tests::ieee_order_and_stable_raw_tails` checks ties and special values. `nbest::tests::raw_tails_order_subunit_score_differences` checks two finite scores 0.75 apart.

### Phrase segmentation writes no invalid-UTF-8 critical

- **Pin:** `pinyin_phrase_segment` and `zhuyin_phrase_segment` check `g_return_val_if_fail(num_of_chars == ucs4_len, FALSE)` after their conversions (`src/pinyin.cpp:1450-1454`, `src/zhuyin.cpp:969-973`). A sentence that `g_utf8_to_ucs4` refuses, such as `FF FE`, logs GLib's critical and returns `FALSE`; under fatal criticals the pin dies. A sequence that GLib converts, such as the surrogate `ED A0 80`, passes the check and is segmented (GLib 2.90.1 `glib/gutf8.c:665-747`, `:940-997`).
- **oxpinyin:** for a refused conversion, returns `false`, keeps the earlier result and makes no log call. A converted sequence is segmented as at the pin.
- **Decision:** accepted, maintainer decision D1 (policy row 148): the check tests results, not arguments, so oxpinyin implements its early return without the critical.
- **Tests:** `surrogate_segment_matches_pin_length_and_no_critical` (both facades): the surrogate result and the `FF FE` refusal.

### Phrase-library layout faults write no critical

- **Pin:** `SubPhraseIndex::load` checks the three separators and the end with `g_return_val_if_fail` (`src/storage/phrase_index.cpp:354-356`, `:361`). The pinyin save path ignores the failed layout check and uses an empty sub-index (`src/pinyin.cpp:955-965`; `FacadePhraseIndex::diff`, `src/storage/phrase_index.cpp:271-284`, at `:277-278`).
- **oxpinyin:** When pinyin save reloads a system library with an invalid separator, it writes the journal against an empty sub-index with the local total intact and emits no critical.
- **Decision:** accepted, maintainer decision D1 (policy row 149): the check tests file data inside an internal method, so oxpinyin implements its early return without the critical.
- **Tests:** `oxpinyin::facade::context::tests::native_files::a_save_over_a_failed_layout_journals_against_an_empty_index` (the journal), `oxpinyin_capi::context::tests::saving_against_a_failed_layout_writes_no_glib_critical` (no critical).

### Reproduced library aborts write no assertion line

- **Pin:** at an `assert` site glibc's `__assert_fail` writes `<progname>: <file>:<line>: <function>: Assertion '<expr>' failed.` and raises SIGABRT; in the reference build `check_result` is `assert` (`src/include/pinyin_utils.h:27-31`).
- **oxpinyin:** does the pin's preceding work, then calls `std::process::abort()` with no assertion line; an `abort()` site writes nothing on either side.
- **Decision:** accepted for the stderr line only, under [the `assert` line](compatibility-policy.md#failure-behaviour) (policy row 150): the line is compiler-generated text; the SIGABRT status, the earlier stderr and the files match.
- **Tests:** `context::tests::a_user_session_fault_aborts_the_init` (`oxpinyin-capi`) and `candidates::tests::a_store_into_the_failed_user_bigram_aborts_training` (both C facades) assert an abort with empty stderr.

## Language-artefact records

### Candidate sorting guards the wrapping frequency order

- **Pin:** the candidate comparator subtracts two `guint32` frequencies and converts to `gint` (`src/pinyin.cpp:1678-1709`, at `:1701-1705`; `src/zhuyin.cpp:1129-1144`), then sorts stably with GLib (`pinyin.cpp:2286`, `:2440-2445`; `zhuyin.cpp:1529`, `:1610`). The wrapping order is inconsistent in cyclic frequency groups, including groups with an antipodal pair.
- **oxpinyin:** `candidate::rank_by_frequency` groups stably by enabled length and span, checks each group's wrapping relation, and sorts a consistent group by `rhs.wrapping_sub(lhs) as i32` and a cyclic group by stable numeric descending frequency. A consistent group fits in a circular interval strictly narrower than 2^31.
- **Decision:** accepted under the [candidate-frequency ruling](compatibility-policy.md#no-ported-external-library-internals) (policy row 151): std sorting needs a consistent order; the fallback applies only within inconsistent equal-length, equal-span groups.
- **Rejected:** transcribing GLib's sort algorithm.
- **Tests:** `candidate::tests::frequency_sort_guards_each_leading_key_group_and_preserves_ties`, `candidate::tests::frequency_sort_has_a_defined_antipodal_fallback`, `guess_predict::a_prediction_list_past_gint_max_keeps_the_wrapping_order` (`oxpinyin-capi`; a consistent group past `gint`'s maximum at the C boundary).

### Zhuyin selection uses length, raw score and input order

- **Pin:** zhuyin compares equal-length raw possibilities with no tie key and runs the standard heap algorithms (`src/lookup/phonetic_lookup.h:66-91`, `:141-172`, `:331-335`; `src/zhuyin.cpp:50`), so an equal tie goes by the heap schedule; for λ > 1 the unigram paths carry NaN (`phonetic_lookup.h:651-663`, `:712-713`).
- **oxpinyin:** `nbest.rs` `selection_compare` orders by length ascending, raw f32 possibility descending, NaN last, signed zeros equal, then earlier input order. Single-slot admission uses the pin’s length check and raw `<` for equal lengths.
- **Decision:** accepted under the [no-ported-internals ruling](compatibility-policy.md#no-ported-external-library-internals) (policy row 152): libstdc++'s choice among equal elements is not a product key.
- **Tests:** `nbest::tests::selection_uses_batch_ties_and_zhuyin_length_first_nan_last`, `nbest::tests::the_zhuyin_shape_keeps_one_tail_with_batch_ties`.

### Punctuation values contain whole words

- **Pin:** `get_all_punctuations` compares byte pointers and passes a shifted byte difference as the word count (`src/storage/punct_table.cpp:56-94`), so 1 to 3 bytes after a terminated run add one empty punctuation.
- **oxpinyin:** `punct::decode_puncts` walks whole four-byte words only, so a misaligned value gets no extra empty punctuation.
- **Decision:** accepted, [internal language artefact](compatibility-policy.md#internal-language-artefacts) (policy row 153): system writers emit whole words, and valid values are identical.
- **Tests:** `row_format` `punct_stream_ignores_partial_words`, `punct_stream_keeps_non_scalar_words`.

### Stored gram partitions: phrase segmentation

- **Pin:** `SingleGram` searches stored records with the standard lower bound before its walks (`src/storage/ngram.cpp:153-271`), and `merge_single_gram` interleaves without sorting (`:308-349`), so a foreign value can violate the partition precondition for the searched token. Segmentation reads that gram (`src/lookup/phrase_lookup.cpp:268-290`).
- **oxpinyin:** `single_gram::lower_bound_records` uses `slice::partition_point` on the stored order.
- **Decision:** accepted under the [no-ported-internals ruling](compatibility-policy.md#no-ported-external-library-internals) (policy row 154). `std::lower_bound` requires the records to be partitioned for the searched token. A call outside that precondition has undefined behaviour. Segmentation reads the separate facade total for the unigram term and does not change it (`phrase_lookup.cpp:307-345`). [C++ partition precondition](https://timsong-cpp.github.io/cppwp/n4950/lower.bound), [effect of a violated precondition](https://timsong-cpp.github.io/cppwp/n4950/structure.specifications).
- **Tests:** `oxpinyin::runtime::tests::the_segmenter_reads_an_unsorted_user_gram_as_stored`, `oxpinyin::facade::export_rows::tests::native::user_gram_unsorted_export_follows_the_stored_order`, `oxpinyin::facade::export_rows::tests::native::user_gram_duplicate_export_lists_every_stored_item`.

### Stored gram partitions: training

- **Pin:** in the recorded unpartitioned-gram case, `get_freq` misses, `insert_freq` appends and the next search misses again (`src/storage/ngram.cpp:178-205`, `:231-271`), so `check_result(user->set_freq(...))` aborts (`src/lookup/phonetic_lookup.h:888-903`).
- **oxpinyin:** in that case `partition_point` finds the inserted item, and training answers `true` with no warning.
- **Decision:** accepted under the [no-ported-internals ruling](compatibility-policy.md#no-ported-external-library-internals) and row 85. The unpartitioned search has undefined behaviour. The pin’s own code adds to the facade total only when training reaches the unigram leg and the token’s sub-index is loaded (`phonetic_lookup.h:888-929`, `phrase_index.h:628-635`).
- **Tests:** `store_libpinyin::tests::training_over_an_unsorted_start_gram_answers_true`, `store_libpinyin::tests::training_preserves_a_partitioned_unsorted_gram_and_its_duplicates`.

### The full-pinyin parse's 16-bit steps

- **Pin:** all three full-pinyin schemes use the same `gint16` steps (`src/storage/pinyin_parser2.cpp:67-70,276,290,383-396`). The parser returns the last step whose parsed length equals its index, at most 32,767 bytes (`:333,346-380`).
- **oxpinyin:** the graph uses `u32` positions and spans and accepts input up to `MAX_GRAPH_POSITION`. After `pinyin_set_full_pinyin_scheme` selects Luoma or secondary Zhuyin, `pinyin_parse_more_full_pinyins` uses `usize` steps and can return a parsed prefix longer than 32,767 bytes (`src/full_pinyin_index.rs:114-139,196-297`).
- **Decision:** accepted, language artefact (policy row 59), by maintainer decision: the stop is a C++ storage width.
- **Tests:** `session::tests::input_past_the_pins_gint16_bound_is_kept_and_parsed_whole`, `session::tests::candidates_come_from_keys_past_the_pins_gint16_bound`, `oxpinyin::graph::tests::an_import_reading_parses_past_long_separator_runs`, `oxpinyin_capi::iterators::tests::an_import_reading_parses_past_long_separator_runs`. `keys::a_scheme_parse_keys_render_as_luoma_and_secondary_zhuyin` covers both alternate indexes on `nihao`.

### Oversized journal record payloads (policy row 123)

- **Pin:** `PhraseIndexLogger::append_record` narrows lengths modulo 65536 and copies the full payload for ADD and REMOVE (`src/storage/phrase_index_logger.h:248-263`). For `LOG_MODIFY_RECORD`, it copies each full item and advances by the narrowed `oldlen` and `newlen` (`:269-278`). The bytes then extend past the boundaries a reader derives from the length fields (`:169-199`).
- **oxpinyin:** the checked encoder refuses a payload above 65535 bytes (`src/data/user_files.rs:886-926`). `PersistenceError::Codec` becomes `UserStoreError::Persistence`, which the facade maps to `SaveOutcome::NotSaved` (`src/user/persistence.rs:2276-2281`, `src/user/store_libpinyin.rs:148-150`, `src/facade/context.rs:576-595`).
- **Decision:** accepted under the [internal language artefact rule](compatibility-policy.md#internal-language-artefacts). The journal format prescribes a 16-bit length field and leaves oversized payload handling unspecified. oxpinyin keeps that width and writes records whose lengths fit it. A language-neutral journal requirement does not prescribe wrapping a length so that the payload crosses the reader's record boundary. The rule names unsigned wrap-around as a language artefact.
- **Tests:** `oxpinyin::facade::context::tests::native::oversized_journal_record_payloads_refuse_save_and_keep_modified`, `oxpinyin::facade::context::tests::native::an_oversized_journal_stops_a_zhuyin_save_after_the_earlier_renames` (the `zhuyin_save` path, with the oversized item in the second library) and the `POLICY-ACCEPTED` capture case `oversized-journal-record-payloads` in [pin_capture.py](../../tools/oracle/pin_capture.py). Both use crafted system data because import accepts at most 15 keys (`src/pinyin.cpp:643`, `src/zhuyin.cpp:528`). An item with 128 characters and 255 readings occupies `6 + 128 * 4 + 255 * (128 * 2 + 4) = 66818` bytes (`src/storage/phrase_index.cpp:180-205`). Its stored length wraps to 1282.

| Save effect | Pin | oxpinyin |
| --- | --- | --- |
| Journal on disk, `pinyin_save` | `_write_files` stages the full payload under the wrapped length. `_rename_files` can publish a journal with broken record boundaries (`src/pinyin.cpp:963-971,1047-1062`). | The encoder refuses before writing any bytes for that record. The save stops before staging that journal, running the rename pass or writing the version marker (`src/user/persistence.rs:2146-2164,2352-2356,2408-2415`). |
| Journal on disk, `zhuyin_save` | `zhuyin_save` writes each journal under the wrapped length and renames it at once, which can publish the same journal (`src/zhuyin.cpp:596-609`). | The encoder refuses in the same way. A zhuyin save renames each file as it writes it (`src/user/store.rs:1929`, `src/user/persistence.rs:2192-2209`), so the files of the earlier libraries are already renamed. The save stops before staging that journal, writing the later files or writing the version marker (`src/user/persistence.rs:2146,2163,2408-2415`). |
| Return and modified state | The save can return `true`. It calls `mark_version` after the write and rename sequence, then clears `m_modified` (`src/pinyin.cpp:220-231,1141-1146`; `src/zhuyin.cpp:695-698`). | The C call returns `false` and the store stays modified (`capi/src/context.rs:176-177,195-196`, `zhuyin-capi/src/context.rs:160,181-182`, `src/facade/context.rs:90-95`, `src/user/store.rs:1934-1946`). |

### Key positions past byte 65535

- **Pin:** the double-pinyin and keyboard zhuyin parsers store `int` positions into the `guint16`s of `ChewingKeyRest` (`src/storage/pinyin_parser2.cpp:565`, `zhuyin_parser2.cpp:259`, `chewing_key.h:102-103`), and `fill_matrix` puts each key at its wrapped begin (`phonetic_key_matrix.cpp:51-55`), so the getters answer `false` from column 65536 (`src/pinyin.cpp:2930-2931`).
- **oxpinyin:** stores the positions `as u16`, as the pin does, but keeps the facade's columns at real offsets.
- **Decision:** accepted under the [key-positions ruling](compatibility-policy.md#key-positions-past-byte-65535) (policy row 155): no IME composition reaches 65536 bytes.
- **Tests:** `keys::key_rest_positions_past_byte_65535_wrap`, `oxzhuyin-capi` `parse_keys::key_rest_positions_past_byte_65535_wrap`.
- **Candidate bounds:** a candidate's `m_begin` and `m_end` are `guint16` too (`src/pinyin.cpp:103-104`, `src/zhuyin.cpp:83-84`). oxpinyin stores them `as u16` and answers them from `zhuyin_choose_candidate` as the pin does, while a choose reads them against real-offset columns; the same ruling covers it.

### A candidate pointer that is not a row of a live published list

- **Pin:** `pinyin_choose_candidate`, `zhuyin_choose_candidate`, `pinyin_choose_predicted_candidate` and `pinyin_remove_user_candidate` dereference the candidate (`src/pinyin.cpp:2501-2588`, `:2591-2640`, `:3726-3768`; `src/zhuyin.cpp:1635-1666`). A pointer past a regenerated, shorter list reads a stale row only because `GArray` keeps its capacity when the list shrinks.
- **oxpinyin:** all four read the row directly, as the candidate getters do; a pointer that is not a row of a live instance's published list is a caller precondition violation.
- **Decision:** accepted under the [no-ported-internals ruling](compatibility-policy.md#no-ported-external-library-internals) (policy row 139): GLib's retained capacity is a library internal, and in Rust the read is undefined. A row of another live instance's list follows the pin.
- **Tests:** `tests/it/foreign_candidates.rs` (`oxpinyin-capi`, `oxzhuyin-capi`) for rows of other instances; none for an invalid pointer.

### FlexibleBigram unsorted-record search

- **Pin:** `FlexibleSingleGram` searches with the standard lower bound on the unsigned token order (`src/storage/flexible_single_gram.h:63-65`, `:118-139`), and backend loads do not check the order (`flexible_ngram_bdb.h:165-187`, `flexible_ngram_kyotodb.h:192-215`, `flexible_ngram_tkrzwdb.h:196-219`); on an unsorted value the result is unspecified.
- **oxpinyin:** `SingleGram::lower_bound` (`tool-support/src/flexible_ngram.rs`) uses `slice::partition_point` and does not sort or clean loaded records.
- **Decision:** accepted under the [no-ported-internals ruling](compatibility-policy.md#no-ported-external-library-internals) (policy row 156): no contract fixes a result on unpartitioned input; sorted records match.
- **Tests:** `kmm_estimate_good` (unsorted Kyoto Cabinet fixture, sorted Berkeley DB and tkrzw controls).

## Outstanding upstream defects (report-back)

These entries are pin defects that oxpinyin collects for the libpinyin report-back batch; the drafts are in [upstream report drafts](upstream-report-drafts.md), and none is filed.

### `pinyin_init`/`zhuyin_init` leave the process `LC_NUMERIC` at "C"

- **Pin:** `UserTableInfo::load` and `SystemTableInfo2::load` call `setlocale(LC_NUMERIC, "C")` and keep its return, the name of the locale just installed, which is `"C"` (`src/storage/table_info.cpp:328`, `:197`). On success they pass that name back, which installs `"C"` again (`:372`, `:291`); an early return skips that call. `save` does the same (`:378`, `:394`). So after any init `LC_NUMERIC` is `"C"`.
- **oxpinyin:** installs `"C"` with the C library's `setlocale` at the pin's points.
- **Decision:** reproduced under the [`LC_NUMERIC` ruling](compatibility-policy.md#lc_numeric-after-init-row-39); item 6 of the report drafts.
- **Tests:** `lc_numeric_follows_the_pins_table_info_sites` (`capi/tests/it/locale.rs`), `a_failed_init_leaves_lc_numeric_at_c` (`src/input/config/tests.rs`).

### Pinyin candidate frequency: out-of-range `float`-to-`guint32` store (class (b))

- **Pin:** `_compute_frequency_of_items` converts a `gfloat` score to `guint32` (`src/pinyin.cpp:1794-1868`, at `:1821-1823`, `:1838-1840`, `:1862-1865`; `_prepend_longer_candidates` at `:1919-1921`). For an out-of-range score, the pin’s result depends on the CPU architecture.
- **oxpinyin:** `amplified_frequency_with_bigram` uses Rust’s defined `as u32` cast.
- **Decision:** accepted, class (b) (policy row 83): C leaves the conversion undefined, and processor behaviour is not reproduced; item 14 of the report drafts.
- **Tests:** `oxpinyin::engine::session::tests::candidate_scores_follow_rusts_saturating_conversion`, `guess_predict::candidates_follow_a_lambda_outside_zero_to_one` (`oxpinyin-capi`; the order at λ = 6).

### Zhuyin candidate frequency: `float`-to-`guint32` store of a score that `guint32` cannot hold (class (b))

- **Pin:** `_compute_frequency_of_items` converts `(lambda * bigram_poss + (1 - lambda) * unigram / total) * 2^24` to `guint32` (`src/zhuyin.cpp:1264-1268`). The conversion is undefined when the score is non-finite or its truncated value cannot be represented as `guint32`. Those cases use the recorded class (b) result.
- **oxpinyin:** Uses Rust’s defined `as u32` cast.
- **Decision:** accepted, class (b) (policy row 83): C leaves the conversion undefined; item 15 of the report drafts.
- **Tests:** `sentence_and_candidates::candidates_follow_a_lambda_outside_zero_to_one` (`oxzhuyin-capi`; the order at λ = 6).

## Pin aborts reproduced

Where the pin asserts or aborts, oxpinyin does the pin's preceding work and calls `std::process::abort()` ([failure behaviour](compatibility-policy.md#failure-behaviour)).
An `abort()` site is MATCH; an `assert` site differs only by the assertion line ([Reproduced library aborts write no assertion line](#reproduced-library-aborts-write-no-assertion-line)).
Pin cites are relative to `src/`; `*` in a case name is `pinyin` and `zhuyin`.

| Site | Pin cite | Test |
|---|---|---|
| Toned initial-only key in a table search, both facades (row 4) | `storage/pinyin_phrase3.h:152`; `pinyin.cpp:2769` | `guess_predict::guess_candidates_aborts_on_a_toned_initial`, `sentence::guess_sentence_aborts_on_a_toned_initial`, `keys::toned_initial_only_key_aborts` |
| `pinyin_set_double_pinyin_scheme(30)` (row 5a) | `storage/pinyin_parser2.cpp:611` | `contract::double_customized_aborts` |
| Zhuyin scheme 7 (row 5c) | `storage/zhuyin_parser2.cpp:295` | `contract::zhuyin_standard_dvorak_aborts`, `parse_keys::the_dvorak_chewing_scheme_aborts` |
| Zhuyin scheme outside the enum; full-pinyin scheme outside 1 to 3 (row 5d) | `pinyin.cpp:1189`; `zhuyin.cpp:736`; `storage/pinyin_parser2.cpp:398` | `contract::zhuyin_out_of_enum_aborts`, `contract::full_pinyin_out_of_enum_aborts`, `parse_keys::an_out_of_enum_chewing_scheme_aborts`, `parse_keys::an_out_of_range_full_pinyin_scheme_aborts` |
| `*_train` on a stale forcing (row 6) | `lookup/phonetic_lookup.h:868` | `candidates::tests::training_a_stale_forcing_aborts`, `pinned_data::training_a_stale_forced_choice_aborts` |
| `pinyin_get_sentence` past the rows (row 10) | `pinyin.cpp:1474` | `sentence::get_sentence_aborts_past_the_rows` |
| Pinyin left and right offset after a separator or past the matrix (row 14) | `pinyin.cpp:3035`, `:3057`, `:3067`, `:3092`; `storage/phonetic_key_matrix.h:103` | `parse_keys::left_offset_aborts_after_a_separator`, `parse_keys::right_offset_aborts_after_a_separator`, `matrix_bounds::the_left_move_past_an_early_stopped_parse_aborts`, `matrix_bounds::the_right_move_from_past_the_last_column_aborts` |
| `*_get_character_offset` past the matrix or after a separator (row 19) | `pinyin.cpp:3147`, `:3161`, `:3203`, `:3204`; `zhuyin.cpp:2110`, `:2158`, `:1453` | `parse_keys::character_offset_aborts_on_an_empty_leading_column`, `matrix_bounds::the_character_offset_past_the_last_column_aborts`, `sentence::tests::the_character_offset_after_a_separator_aborts`, `matrix_bounds::a_null_phrase_past_the_last_column_aborts` (both facades), `parse_keys::character_offset_on_a_leading_separator_aborts` |
| `*_parse_full_pinyin` with `'`; pinyin library unload at index 16 or more (row 21) | `storage/pinyin_parser2.cpp:170`; `pinyin.cpp:466`, `:499` | `parse_keys::full_key_parse_aborts_on_an_apostrophe_after_invalid_bytes`, `parse_keys::a_single_key_apostrophe_aborts`, `null_arguments::null_context_unload_phrase_library_orders_assert_first`, `null_arguments::null_context_unload_addon_phrase_library_aborts_on_index_16` |
| A pinyin clear or span write reaches a continuation whose head is gone (row 165) | `lookup/phonetic_lookup.cpp:61–118`; `pinyin.cpp:2577–2590,2641–2647` | `choose::a_clear_through_a_freed_run_start_aborts`, `choose::choosing_across_a_freed_run_start_aborts` |
| `pinyin_train` past nonempty results (row 38) | `pinyin.cpp:2684` | `pinned_data::training_one_past_the_nbest_results_aborts` |
| `*_init` on an unknown `user.conf` database format (row 44) | `storage/table_info.cpp:132`, from `:353-354` | `init_and_save::init_aborts_on_an_unknown_user_conf_database_format`, `init_and_save::an_unknown_user_database_format_aborts_and_keeps_user_conf` |
| `pinyin_choose_candidate` at a nonzero offset without sentence candidates (row 45) | `pinyin.cpp:2566` | `choose::choose_candidate_aborts_at_offset_one_without_a_sentence` |
| `pinyin_choose_candidate` of an `NBEST_MATCH_CANDIDATE` row on an instance whose results hold no row 0 or no row at its rank (row 138) | `pinyin.cpp:2517`, `:2518` | `foreign_candidates::an_nbest_row_on_a_receiver_without_results_aborts` |
| A searched phrase row with a zero facade total, before the dedup (row 61) | `pinyin.cpp:1859`; `zhuyin.cpp:1261` | `sentence::tests::guess_candidates_aborts_on_a_zero_facade_total`, `predict::tests::predicted_bigram_rows_abort_on_a_zero_facade_total`, `sentence::tests::a_zero_facade_total_aborts_the_after_cursor_ranking` |
| Pinyin library load at index 0 or 8 and export iterator reads (row 62). `pinyin_bigram_iterator_has_next_phrase` checks each stored key’s tone, then its initial, middle and final, and aborts on the first out-of-range field. `pinyin_bigram_iterator_get_next_phrase` writes phrase, pinyin and count before its trailing probe can abort. | `pinyin.cpp:457`, `:709`, `:721`, `:846`, `:859`, `:902`, `:904-910`; `storage/chewing_key.cpp:30-32`, `:48`, `:50` | `library_loading::load_of_the_reserved_library_aborts`, `import_export::bigram_get_next_aborts_on_an_empty_export`, `oxpinyin::facade::export_rows::tests::native::export_refuses_an_invalid_system_reading::case_1_tone`, `oxpinyin::facade::export_rows::tests::native::export_refuses_an_invalid_system_reading::case_2_fields`; `iterators::tests::export_iterator_aborts_on_a_reading_it_cannot_spell`, `import_export::bigram_has_next_aborts_on_a_stored_key_outside_the_table`, `import_export::bigram_get_next_writes_its_row_before_the_key_assert_aborts` |
| Pinyin key getters on an empty matrix, below `SIZE_MAX`; auxiliary text (row 63) | `storage/phonetic_key_matrix.h:103`; `pinyin.cpp:3311`, `:3488` | `parse_keys::key_rest_aborts_on_an_empty_matrix`, `parse_keys::full_auxiliary_text_aborts_on_a_leading_apostrophe`, `parse_keys::double_auxiliary_text_aborts_three_bytes_into_a_key`; `parse_keys::the_key_getters_answer_false_at_size_max_on_an_empty_matrix`, `parse_keys::key_aborts_on_an_empty_matrix`, `parse_keys::double_auxiliary_text_aborts_on_a_leading_apostrophe`, `parse_keys::chewing_auxiliary_text_aborts_on_a_leading_apostrophe` |
| A candidate of the wrong row type or a non-user token (row 64) | `pinyin.cpp:2507`, `:2593`, `:2883`, `:3734`, `:3738` | `choose::choose_candidate_aborts_on_a_predicted_prefix_row`, `choose::choose_predicted_candidate_aborts_on_a_normal_row`, `choose::nbest_index_aborts_on_a_normal_row`, `choose::remove_user_candidate_aborts_on_a_system_token`, `choose::remove_user_candidate_aborts_on_a_sentence_row`, `choose::choose_candidate_aborts_on_a_predicted_bigram_row` |
| Pinyin right offset on an empty matrix; candidates two or more past the reserved slot (row 66) | `storage/phonetic_key_matrix.h:103`, through `pinyin.cpp:3075` and `:2226` | `matrix_bounds::right_offset_zero_over_a_keyless_parse_aborts`, `matrix_bounds::guess_candidates_past_an_early_stopped_parse_aborts` |
| Pinyin remember-input on an empty matrix after resolving the phrase tokens (row 66) | `pinyin.cpp:3590–3601,3685–3695`; `storage/phonetic_key_matrix.h:103` | `user_store::remember_user_input_aborts_on_a_keyless_parse` |
| Zhuyin library load and unload, key accessors below `SIZE_MAX`, offsets and cursor guesses past the matrix (row 66) | `zhuyin.cpp:372`, `:381`, `:1453`, `:1261`; `storage/phonetic_key_matrix.h:103` | `init_and_save::loading_library_zero_aborts`, `null_arguments::null_context_unload_phrase_library_orders_assert_first`, `parse_keys::key_rest_on_an_empty_matrix_aborts`, `parse_keys::left_offset_past_the_matrix_aborts`, `parse_keys::right_offset_past_the_matrix_aborts`, `parse_keys::left_offset_just_past_a_separator_aborts`, `parse_keys::right_offset_just_past_a_separator_aborts`, `parse_keys::key_rest_just_past_a_separator_aborts`, `sentence_and_candidates::after_cursor_guess_one_past_the_reserved_slot_aborts`, `sentence::tests::a_zero_facade_total_aborts_the_after_cursor_ranking`, `matrix_bounds::a_cursor_over_a_keyless_parse_aborts`; `parse_keys::the_key_accessors_answer_false_at_size_max_on_an_empty_matrix` |
| More than four libraries hold a searched string (row 74) | `storage/phrase_large_table3.h:95`, `:106`; `pinyin.cpp:541`, `:1415`, `:2664`, `:3117`; `zhuyin.cpp:427`, `:933`, `:1690`, `:2080` | `library_tokens::the_prefix_guess_aborts_on_five_tokens`, `library_tokens::the_prediction_aborts_on_five_tokens`, `library_tokens::the_character_offset_aborts_on_five_tokens`, `library_tokens::remember_aborts_on_five_tokens`, `pinned_data::a_prefix_guess_over_five_library_tokens_aborts`; `oxpinyin::facade::import::tests::an_import_refuses_five_tokens_before_the_library_refusal` |
| `*_save` on a chunk header or checksum write (row 77) | `include/memory_chunk.h:543`, `:547` | `init_and_save::save_aborts_where_the_chunk_header_write_fails`, `init_and_save::a_save_cut_after_the_chunk_length_word_aborts` |
| `table.conf` rows and library loads the pin refuses (row 79) | `storage/table_info.cpp:119`, `:132`, `:142`, `:156`, `:175`, `:276`; `pinyin.cpp:388`, `:457`, `:491`; `zhuyin.cpp:330`, `:372`; `include/memory_chunk.h:434`, `:493` | `init_and_save::init_aborts_on_an_unknown_table_conf_database_format`, `init_and_save::an_unknown_table_database_format_aborts_before_any_user_file`, `library_loading::addon_load_aborts_on_a_row_that_is_not_a_dictionary`, `init_and_save::init_aborts_on_a_default_dictionary_row_after_user_conf` (`oxpinyin-capi`), `init_and_save::a_default_dictionary_row_aborts_the_init_with_no_user_conf` (`oxzhuyin-capi`) |
| `pinyin_remove_user_candidate` on a removal the store refuses (row 81) | `pinyin.cpp:3743`, `:3750`, `:3759` | `choose::remove_user_candidate_aborts_without_the_phrase_table`; `a_removal_assert_comes_before_the_bigram_fault`, `choose::remove_user_candidate_aborts_on_a_reading_the_index_lacks` |
| `*_train` after a re-parse past the matrix (row 82) | `storage/phonetic_key_matrix.cpp:661`, `:663`, `:664` | `session::tests::train_after_a_shortening_reparse_refuses_a_span_past_the_matrix`, `session::tests::train_after_an_apostrophe_reparse_refuses_an_empty_start_column`, `session::tests::train_after_a_reparse_onto_an_empty_end_column_refuses`; `session::tests::train_after_a_reparse_and_a_choose_stops_the_walk_at_the_store_length` |
| `*_mask_out` on an over-long user index key (rows 84, 94) | `storage/chewing_large_table2_bdb.cpp:529` (kc `:499`, tkrzw `:466`) | `user_store::mask_out_aborts_on_an_overlong_index_key_before_the_reload`, `oxpinyin::user::store::tests::mask_out_aborts_on_an_empty_index_key` (owner error), `import_and_user_store::mask_out_over_an_overlong_user_index_key_aborts` |
| `pinyin_guess_candidates` LONGER walk on an over-long index key (rows 84, 95) | `storage/chewing_large_table2_bdb.cpp:282` (kc `:269`, tkrzw `:252`) | `user_store::guess_candidates_aborts_on_an_overlong_index_key`; `import_and_user_store::cursor_guesses_ignore_an_overlong_user_index_key` |
| A user-bigram key that is not a token (row 86) | `storage/ngram_bdb.cpp:199` (kc `:173`, tkrzw `:150`) | `user_store::mask_out_aborts_on_a_non_token_bigram_key`, `user_store::bigram_export_aborts_on_a_non_token_key`, `import_and_user_store::mask_out_over_a_non_token_bigram_key_aborts` |
| A residual user-bigram gram at mask-out or removal (row 87) | `storage/ngram.cpp:70`, through `ngram_bdb.cpp:243` | `user_store::mask_out_aborts_on_a_residual_bigram_gram`, `choose::remove_user_candidate_aborts_on_a_residual_bigram_gram`, `import_and_user_store::mask_out_over_a_residual_bigram_gram_aborts` |
| An empty bigram gram in a prediction (row 88) | `storage/ngram.cpp:70`, through `pinyin.cpp:2332` | `guess_predict::prediction_aborts_on_an_item_less_gram_with_a_total` |
| Zhuyin import token checks (rows 89, 90, 104) | `zhuyin.cpp:440`, `:454`, `:457` | `import_and_user_store::adding_a_phrase_with_two_library_tokens_aborts`, `oxpinyin::facade::import::tests::an_index_token_with_another_item_refuses_import::case_3_zhuyin_text`, `oxpinyin::facade::import::tests::adding_a_user_phrase_again_after_a_reload_refuses_the_missing_item` |
| Pinyin import token checks (row 93) | `pinyin.cpp:554`, `:568`, `:571` | `library_tokens::import_aborts_on_two_tokens_of_one_library`, `user_store::remember_user_input_aborts_on_two_same_library_tokens`, `oxpinyin::facade::import::tests::an_index_token_with_another_item_refuses_import::case_1_pinyin_length`, `oxpinyin::facade::import::tests::an_index_token_with_another_item_refuses_import::case_2_pinyin_text` |
| A journal header record with a token, at init, reload or mask-out (row 91) | `storage/phrase_index_logger.h:202` | `library_loading::reload_aborts_on_a_log_header_token` (`oxpinyin-capi`), `init_and_save::reload_aborts_on_a_log_header_token` (`oxzhuyin-capi`), `init_and_save::init_aborts_on_a_log_header_token` (`oxpinyin-capi`); `a_bad_log_aborts_before_a_later_library_that_cannot_map` |
| More than one journal header at mask-out (row 92) | `storage/phrase_index.cpp:745` | `user_store::mask_out_aborts_on_two_log_headers_and_leaves_the_files` |
| A kept ADD or REMOVE record at mask-out (row 97) | `storage/phrase_index_logger.h:245`, `:257`, through `storage/phrase_index.cpp:838` | `mask_out_aborts_on_a_surviving_add` and `mask_out_aborts_on_a_surviving_remove` in both C facades |
| A journal ADD payload under six bytes (row 108) | `include/memory_chunk.h:390`, through `storage/phrase_index.cpp:457`, `:215` and `:778`-`:795` | `a_short_add_before_an_over_read_is_the_fault` |
| A system or user bigram value under four bytes. With a user gram present, the merge reads the short system total first and the pin's assert fires; oxpinyin aborts in the same way. With a system gram only, the n-best ranged step calls `lower_bound` on a range whose end comes before its start, before `get_total_freq`: the pin's result is undefined, and oxpinyin aborts there. User paths include mask-out, removal, prediction, training, lookup and export (row 85) | `include/memory_chunk.h:390`, through `storage/ngram.cpp:48-51`, `:80`, `:62-70`, `:133-141`, `:155-161`, `:163-165`, `:303-304`; `lookup/phonetic_lookup.h:884` | `import_export::bigram_get_next_writes_its_row_before_the_short_predecessor_aborts`, `import_export::bigram_export_aborts_at_a_short_predecessor_before_a_partial_one`; `mask_out_aborts_on_a_short_bigram_value`; `sentence::a_short_sentence_start_gram_aborts_the_guess_after_parsing`, `sentence::tests::a_short_system_gram_merged_with_a_user_gram_aborts_the_segment`, `sentence::tests::a_short_system_gram_aborts_the_ranged_step` |
| A system library that is a directory, from 8 bytes of reported size (row 96) | `include/memory_chunk.h:493`, through `pinyin.cpp:255`, `:289`, `:955`, `:1264`; `zhuyin.cpp:199`, `:588`, `:799` | `{save,mask_out,reload}_aborts_on_a_directory_library`, `addon_load_aborts_on_a_directory_library`; init: `oxpinyin::facade::context::tests::library_directory::init_returns_the_fault_where_its_library_loop_maps_the_directory` |
| A Kyoto Cabinet system bigram read that fails after its size check (row 110) | `storage/ngram_kyotodb.cpp:138-139` | `sentence::tests::a_read_failure_after_the_size_check_aborts` (`oxpinyin-capi`), `a_checked_read_failure_is_the_predecessors_read_after_check_fault` |
| A Kyoto Cabinet pinyin- or phrase-table body read that fails after its size check (row 110); the phrase table takes the pinyin table's checked read (`RawChewingDbm::get_phrase_row`) | `storage/chewing_large_table2_kyotodb.cpp:170-174`, `storage/phrase_large_table3_kyotodb.cpp:174-187` | `oxpinyin::data::chewing_table::tests::a_pinyin_table_body_read_failure_after_the_check_aborts` |

## Open

None.

## Resolved by source reading

`pinyin_choose_candidate`, `zhuyin_choose_candidate`, `pinyin_choose_predicted_candidate` and `pinyin_remove_user_candidate` with a candidate from another live instance read the row's scalars and act on the receiving instance and its context, as the pin does (`src/pinyin.cpp:2501-2588`, `:2591-2640`, `:3726-3768`; `src/zhuyin.cpp:1635-1666`); a pointer that is no published row is the [caller precondition](#a-candidate-pointer-that-is-not-a-row-of-a-live-published-list).

A system library’s user `.dbin` that is a directory matches on the traced Unix path. Failed open, failed end seek or a reported size below eight leaves an empty journal. If the descriptor reports at least eight bytes and a header read fails, both abort. The pin’s callers continue to merge an empty log on ordinary load failure (`memory_chunk.h:417-438`, `pinyin.cpp:264-277`, `zhuyin.cpp:208-221`). oxpinyin takes the same paths through `src/input/{native,chunk}.rs`, `src/data/user_files.rs` and `src/user/persistence.rs`.
