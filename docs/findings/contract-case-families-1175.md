# Contract case-family extraction inventory (#1175)

This inventory describes `contract-diff.py` at main
`b363ef9af5c4e6ae811e2a98a74c024834942d41`, before the first family move.
The sections group the driver's adjacent issue-specific definitions; the two
remember-tone portions and the three axis-C builders are grouped together.
The zhuyin allocation-after-fini twin remains in the zhuyin facade twins
section. The final C-string cases register after the CLI definitions.

The 67 sections account for all 4,184 registered IDs. Counts assign each ID
to the section defining its case function or generated probe, including
partials and closures; they are not counts of the `--family` selector. The
first example ID identifies the section. Builders/helpers lists named
definitions in the section, including named case functions and shared helpers;
the latter are not necessarily safe to move with a family. Needs lists global
names read from outside that section, including `case`, imported ABI aliases,
standard-library modules, fixtures, and cross-section helpers. Local loop and
closure values are not driver dependencies. The worker's `Kit` is the case
argument for every section.

The order is a planning heuristic, not a promise that every section can be
moved as one patch. It ranks by case count plus five for each named helper and
each dependency outside the already-extracted ABI, fixture and verdict
modules, standard-library imports and the `case` registration callback.
Smaller, less coupled sections therefore tend to come first. Cross-section
helpers, driver state, interleaved registrations and new main changes must be
rechecked when each section is moved. Rows marked moved register from their
own module through the driver's `case` at their original position; the
remaining rows keep the proposed order. The remaining rows are planned as a
linear stack on main `1e2736984d1a909828642b526c90d77e668cd15e`; "PR n" is its
n-th member. PR 1 moves no rows: it moves the 47 helpers that several sections
read into `contract_case_helpers`, and the driver re-exports them by
assignment. Helpers that read the driver's `CELL`, directly or through one
another (`_craft_db`, `_craft_hash`, `index_file_rows`, `token_reuse_files`,
`token_reuse_sequence`, `crafted_bigram_context`, `predicted_bigram_context`,
`addon_promote_sequence`, `items_system` and `phrase_text_system`), and
`Rest`, which moved families capture, stay in the driver and are passed to the
sections that use them. PR 7 moves the last section and settles what is left.
Axis C remains one family: its three builders, five observation helpers and
export case; its driver registration also owns `AXIS_C_GROUPS`,
`AXIS_C_DEFINITION` and `AXIS_C_LAYOUT_COVERED`.

| Order | Section (example case) | Cases | Builders/helpers | Needs from outside the section | Status |
| --- | --- | ---: | --- | --- | --- |
| 1 | allocation after fini (`alloc-instance-after-fini`) | 1 | none | `P`, `case` | moved (`contract_alloc_after_fini`) |
| 2 | input length cap (`input-length-cap`) | 2 | none | `B`, `P`, `S`, `Z`, `case`, `sentence_out` | moved (`contract_input_length_cap`) |
| 3 | default options (`default-option-parse`) | 4 | none | `B`, `P`, `U`, `case`, `consumed`, `key_out` | moved (`contract_default_options`) |
| 4 | double fini (`fini-twice`) | 2 | `fini_twice` | `P`, `case` | moved (`contract_double_fini`) |
| 5 | invalid input bytes (`bytes-parse-more-full`) | 7 | none | `B`, `C`, `I`, `P`, `S`, `U`, `case`, `consumed`, `key_out` | moved (`contract_invalid_input_bytes`) |
| 6 | non-UTF8 directories (`non-utf8-directory-system`) | 4 | `non_utf8_dirs` | `B`, `C`, `P`, `S`, `Z`, `case`, `os` | moved (`contract_non_utf8_directories`) |
| 7 | axis J (`j-choose-nbest-diff-result`) | 5 | `j_multi_chewing` | `B`, `C`, `I`, `P`, `S`, `Z`, `case`, `guessed`, `row_of` | moved (`contract_axis_j`) |
| 8 | negative remember counts (`remember-count-2`) | 5 | `remember_negative` | `B`, `C`, `I`, `P`, `S`, `U`, `Z`, `case` | moved (`contract_negative_remember`) |
| 9 | predicted candidate type (`choose-predicted-types`) | 1 | `predicted_rows`, `choose_predicted_of` | `B`, `C`, `I`, `P`, `S`, `U`, `case` | moved (`contract_predicted_types`) |
| 10 | facade total (`facade-total-zero-ranks-nothing-end-sort-3`) | 4 | `wrap_total_to_zero`, `_no_rank` | `B`, `C`, `P`, `S`, `U`, `Z`, `case` | moved (`contract_facade_total`) |
| 11 | stored count headers (`stored-count-headers-system-pinyin`) | 4 | `stored_count_headers` | `B`, `C`, `P`, `Path`, `U`, `UNTOUCHED`, `_chunk`, `_write_user_conf`, `case`, `conf_system` | moved (`contract_stored_count_headers`) |
| 12 | remaining issue 542 (`issue542-phrase-segment-invalid-utf8-pinyin`) | 7 | `phrase_segment_invalid_utf8`, `train_no_choose_save` | `B`, `C`, `P`, `S`, `U`, `UNTOUCHED`, `Z`, `case`, `full_inst`, `key_out`, `os`, `sentence_out` | moved (`contract_issue542_remaining`) |
| 13 | pinyin out parameters and returns (`empty-parse-sentence`) | 12 | `Rest` | `Arr`, `B`, `C`, `I`, `P`, `S`, `U`, `UNTOUCHED`, `Z`, `case`, `sentence_out`, `tokens_of` | moved (`contract_pinyin_out_parameters`) |
| 14 | candidate lookup offsets (`guess-candidates-lookup-offset`) | 3 | `candidate_rows`, `guess_rows`, `zhuyin_guess_rows` | `B`, `C`, `I`, `P`, `S`, `U`, `Z`, `case`, `full_inst` | moved (`contract_candidate_lookup_offsets`) |
| 15 | remember tone (`remember-tones-pinyin`) | 4 | `remember_export`, `remember_tones`, `remember_review` | `B`, `C`, `I`, `P`, `S`, `U`, `Z`, `case` | moved (`contract_remember_tone`) |
| 16 | allocator register (`alloc-register-pinyin_get_pinyin-zero-key`) | 14 | `alloc_register_case` | `B`, `C`, `P`, `S`, `U`, `UNTOUCHED`, `Z`, `case`, `owned_text` | moved (`contract_allocator_register`) |
| 17 | cursor and sentence refusals (`abort-get-sentence-past-rows`) | 15 | `leading_apostrophe_aux` | `B`, `C`, `P`, `S`, `UNTOUCHED`, `Z`, `case`, `full_inst`, `sentence_out` | moved (`contract_cursor_sentence_refusals`) |
| 18 | duplicate phrase add (`abort-zhuyin-add-phrase-duplicate-library-token`) | 6 | `_ucs4_key`, `_persist_pinyin_phrase`, `_add_pinyin_phrase_after_index` | `B`, `I`, `P`, `S`, `U`, `_craft_db`, `_write_user_conf`, `case`, `os`, `tokens_of` | moved (`contract_duplicate_phrase_add`) |
| 19 | zhuyin facade twins (`user-library-token-unload-pinyin`) | 11 | `user_library_token_unload`, `zhuyin_candidates_at_start` | `Arr`, `B`, `C`, `I`, `P`, `Rest`, `S`, `U`, `UNTOUCHED`, `Z`, `case`, `tokens_of`, `zhuyin_chewing`, `zhuyin_sentence_out` | moved (`contract_zhuyin_facade_twins`) |
| 20 | scheme and library refusals (`abort-set-full-pinyin-scheme-0`) | 23 | `ctx_call` | `B`, `C`, `I`, `P`, `U`, `UNTOUCHED`, `case` | moved (`contract_scheme_library_refusals`) |
| 21 | zero consumption (`zero-consumption-pinyin-full_pinyins-n`) | 24 | `zero_consumption_sentence` | `B`, `C`, `P`, `S`, `U`, `UNTOUCHED`, `Z`, `case` | moved (`contract_zero_consumption`) |
| 22 | row 54 empty and forcing (`row54-empty-pinyin-full_pinyins`) | 16 | `row54_empty`, `row54_unload`, `row54_forcing` | `B`, `C`, `I`, `P`, `S`, `U`, `UNTOUCHED`, `Z`, `case` | moved (`contract_row54_empty_forcing`) |
| 23 | GLib arrays (`glib-924-array-lifecycle-pinyin`) | 12 | `glib_924_array_lifecycle`, `glib_924_segment_critical`, `glib_924_oom_array`, `glib_924_null_array` | `Arr`, `B`, `C`, `I`, `P`, `Path`, `S`, `U`, `UNTOUCHED`, `__file__`, `case`, `subprocess`, `tokens_of` | moved (`contract_glib_arrays`) |
| 24 | 256th reading (`wrap-256th-reading`) | 4 | `_wrap_pinyin_readings`, `_wrap_zhuyin_readings`, `_wrap_distinct`, `_wrap_item_reads`, `_wrap_export_rows`, `wrap_256th_reading` | `Arr`, `B`, `C`, `I`, `P`, `S`, `U`, `UNTOUCHED`, `case`, `hashlib`, `json`, `tempfile`, `tokens_of`, `unigram_of` | moved (`contract_reading_256th`) |
| 25 | auxiliary text (`aux-text-full-parse`) | 14 | `aux_matrix`, `chewing_cursors`, `aux_cursors`, `aux_options` | `B`, `C`, `P`, `S`, `U`, `UNTOUCHED`, `Z`, `case` | moved (`contract_auxiliary_text`) |
| 26 | combined import and nbest order (`combined-library-sixth-add-pinyin`) | 10 | `combined_import_bound`, `import_review`, `nbest_user_table_order` | `B`, `I`, `P`, `S`, `U`, `Z`, `case`, `remember_export`, `sentence_out`, `tokens_of`, `transient_files`, `unigram_of`, `zhuyin_sentence_out` | moved (`contract_combined_import_nbest`) |
| 27 | stored total overflow (`stored-total-overflow-below`) | 6 | `stored_total_overflow`, `log_headers` | `B`, `FACADE_TOTAL`, `P`, `U`, `_facade_total_out`, `_modify_header`, `_stored_library_total`, `_write_log`, `_write_user_conf`, `case`, `import_rows`, `tokens_of`, `unigram_of` | moved (`contract_stored_total_overflow`) |
| 28 | zhuyin per-entry contracts (`zhuyin-parse-chewing-key`) | 16 | `zy_strv`, `parse_chewing_probe`, `in_chewing_probe`, `key_rest_positions_probe`, `phrase_tokens_probe` | `B`, `C`, `I`, `P`, `S`, `U`, `UNTOUCHED`, `Z`, `case`, `owned_text`, `tokens_of`, `zhuyin_chewing` | moved (`contract_zhuyin_per_entry`) |
| 29 | before-cursor and longer dedup (`before-cursor-frequency-dedup`) | 8 | `before_cursor_frequency_listing`, `longer_frequency_listing` | planned: PR 2 |
| 30 | token unigram and training (`system-zhuyin-only-readings`) | 14 | `token_unigram_system`, `token_unigram_user`, `zhuyin_only_readings`, `token_unigram_user_libraries`, `train_selected_top`, `token_unigram_trained` | planned: PR 2 |
| 31 | bigram removal (`removal-bigram-reported-mask-out-pinyin`) | 23 | `removal_bigram` | planned: PR 2 |
| 32 | candidate removals and refusals (`abort-get-candidate-nbest-index-normal-row`) | 15 | `predicted`, `_add_user_phrase`, `_user_candidate_rows`, `_guessed_user_phrase`, `_removable_user_bigram_context`, `choose_without_sentence` | planned: PR 2 |
| 33 | null user and library operations (`null-user-session-pinyin`) | 30 | `transient_files`, `null_user_session`, `transient_library` | planned: PR 2 |
| 34 | bigram mask and prediction (`issue1114-collision-load-save-pinyin`) | 14 | `crafted_bigram_context`, `predicted_bigram_context`, `issue1114_collision_saves`, `predicted_bigram_row_context`, `issue677_sequence` | planned: PR 2 |
| 35 | sort and pinyin length (`glib-sort-predicted`) | 15 | `glib_sort_zhuyin_options`, `glib_sort_tails`, `glib_sort_candidates`, `pinyin_length_rows` | planned: PR 2 |
| 36 | system mask out (`mask-out-system-unsaved-pinyin`) | 20 | `token_phrase_of`, `mask_out_system_reload`, `mask_out_system_modify` | planned: PR 2 |
| 37 | phrase-index logger (`abort-init-log-header-token-pinyin`) | 16 | `_chunk`, `_modify_header`, `_write_log`, `logger_init_probe`, `logger_mask_out_probe`, `logger_mask_out_unloaded_probe`, `logger_mask_out_untouched` | planned: PR 2 |
| 38 | phrase table mask (`phrase-table-mask-long-short`) | 7 | `phrase_table_lookup`, `phrase_table_reopen`, `phrase_table_mask_records`, `phrase_table_raw_key_prediction` | planned: PR 2 |
| 39 | punctuation (`punct-decode/valid`) | 22 | `punct_words`, `punct_fixture`, `punct_rows`, `punct_decode_case`, `punct_dedup_case`, `punct_alias_case` | planned: PR 2 |
| 40 | training and constraints (`abort-zhuyin-train-stale-forcing`) | 19 | `forced_sentence`, `constraint_phrases`, `candidate_heads`, `constraint_instance`, `constraint_survives`, `constraint_survives_reconcile`, `constraint_stale_item`, `reparse_after_choose` | planned: PR 2 |
| 41 | user stored words (`user-stored-words/pinyin-1-valid`) | 32 | `user_stored_words` | planned: PR 2 |
| 42 | toned keys and zhuyin refusals (`abort-is-incomplete-toned-initial`) | 24 | `toned_initial`, `offset_call`, `guess_call`, `key_call`, `char_offset_call`, `_no_rank_z` | planned: PR 2 |
| 43 | final step surface (`final-step-pinyin-ba'kua-20`) | 50 | `final_step_surface` | planned: PR 2 |
| 44 | zero-key guess (`zero-key-guess/pinyin/default/'/guess_sentence`) | 63 | `zero_key_guess_case` | planned: PR 2 |
| 45 | token reuse and addon import (`issue762-token-reuse-no-mask-pinyin`) | 30 | `token_reuse_sequence`, `index_file_rows`, `token_reuse_files`, `addon_promote_sequence`, `addon_promoted_listing` | planned: PR 3 |
| 46 | system phrase text (`phrase-text/pinyin-valid-candidate`) | 32 | `phrase_text_system`, `items_system`, `phrase_text_bytes`, `phrase_text_rows`, `phrase_text_choose`, `phrase_text_probe` | planned: PR 3 |
| 47 | prefix conversion (`prefix-convert/predicted-valid`) | 28 | `glib_utf8`, `prefix_system`, `prefix_argument`, `prefix_prediction_case`, `prefix_sentence_case` | planned: PR 3 |
| 48 | facade totals and counts (`import-wrapped-facade-total-guess`) | 10 | `import_facade_total`, `stored_header_total`, `system_add_reload`, `_stored_library_total`, `_facade_total_out`, `bumped_item_guess`, `longer_choices_guess`, `unload_accepted_bump` | planned: PR 3 |
| 49 | changed parse constraints (`parse-constraints-pinyin-changed-suffix-direct`) | 80 | `changed_parse_constraints` | planned: PR 3 |
| 50 | stored user gram (`user-gram-unsorted-train-save-pinyin`) | 26 | `_gram_bytes`, `_gram_items`, `_plant_user_grams`, `_close_and_read_grams`, `_mark_modified_by_unconstrained_train`, `user_gram_layout`, `user_gram_predicted`, `train_unsorted_set_freq_miss`, `itemless_gram_untouched`, `itemless_user_total` | planned: PR 3 |
| 51 | phrase export and training (`issue790-export-after-train-system`) | 14 | `export_rows`, `export_open_across_import_system`, `export_open_across_import_user`, `issue790_export`, `issue790_readings`, `issue790_bounded_counts`, `issue790_imported` | planned: PR 3 |
| 52 | user index and phrase mask (`issue784-index-keys-no-mask-pinyin`) | 62 | `index_key_sequence` | planned: PR 3 |
| 53 | GLib path construction (`init-system-empty-pinyin`) | 49 | `init_system_argument`, `spell_dir`, `dir_spelling_session`, `fini_write_failed`, `save_write_failed`, `spelled_system`, `row_spelling_session`, `row_spelling_missing`, `table_conf_line` | planned: PR 3 |
| 54 | matrix resplit windows (`matrix-resplit-pinyin-banangang-plain-exact`) | 96 | `matrix_resplit_windows` | planned: PR 3 |
| 55 | library reload (`reload-drops-session-pinyin-train`) | 32 | `_reload_pron_keys`, `_reload_candidates`, `_reload_snapshot`, `_reload_probe`, `_reload_train`, `_reload_import`, `_reload_user`, `_reload_reuse_setup`, `_reload_dangling`, `_reload_total_window`, `mask_out_reload` | planned: PR 3 |
| 56 | user configuration and overlong index (`user-conf-920-nul-database-token`) | 29 | `_craft_db`, `_craft_btree`, `_craft_hash`, `_user_conf_values`, `_replace_conf_value`, `_edit_user_conf_versions`, `_edit_user_conf_counter`, `_edit_user_conf_nul`, `_edit_user_conf_missing_database`, `_edit_user_conf_missing_counter`, `_edit_user_conf_negative_overflow`, `_user_conf_920_session`, `_user_conf_920_case`, `overlong_index_context`, `system_overlong_index_context` | planned: PR 4 |
| 57 | row 58 ranking (`row58-bigram-scoring-off`) | 43 | `row58_rows`, `row58_seed_bigrams`, `row58_scoring`, `row58_choose`, `row58_prefix`, `row58_bigram_choices`, `row58_conversion`, `row58_lambda`, `row58_prefix_overflow`, `row58_signed_score`, `row58_zero_total`, `row58_prefix_zero_total`, `row58_lambda_rounding`, `row58_system_import` | planned: PR 4 |
| 58 | zhuyin null arguments (`zhuyin-add-phrase-library-16`) | 20 | `null_argument_case`, `zy_parse_chewing_null_instance`, `zy_parse_chewing_null_text`, `zy_parse_chewing_null_key`, `zy_in_chewing_null_instance`, `zy_in_chewing_null_symbols`, `zy_pinyin_string_null_instance`, `zy_zhuyin_string_null_key`, `zy_zhuyin_string_null_out`, `zy_key_rest_positions_null_rest`, `zy_n_phrase_null_instance`, `zy_n_phrase_null_out`, `zy_phrase_token_null_instance`, `zy_phrase_token_null_out`, `zy_first_rest`, `zy_first_candidate`, `zy_key_rest_length_null_rest`, `zy_key_rest_length_null_length`, `zy_candidate_type_null_candidate`, `zy_candidate_type_null_out`, `zy_candidate_string_null_candidate`, `zy_candidate_string_null_out`, `zy_import_index_out_of_range` | planned: PR 4 |
| 59 | save failures and stderr (`stderr-save-one-tmp-blocked`) | 40 | `save_after_training`, `blocked_save`, `save_929_failure`, `save_929_roundtrip`, `save_929_sync_rejecting`, `save_929_physical_files`, `save_929_import`, `save_929_tokens`, `save_929_marker_interrupt`, `save_929_late_boundary`, `save_929_logical_roundtrip`, `_dirty_for_save`, `save_header_dev_full`, `save_header_rlimit` | planned: PR 4 |
| 60 | integer limits D18 (`d18-remember-reported`) | 63 | `d18_export_phrases`, `d18_remember`, `remember_nul`, `d18_unigram`, `d18_input`, `d18_bigram_db`, `d18_gram`, `d18_bigram_fixture`, `d18_merge_observation`, `d18_merge`, `d18_bigram_export`, `d18_bigram_update`, `stale_index_tmp` | planned: PR 4 |
| 61 | C string inputs (`c-string-input/pinyin-full-key-valid`) | 90 | `c_string_full_key_apostrophe`, `c_string_full_key`, `c_string_words`, `c_string_direct_reading`, `c_string_null_state`, `c_string_parse_null`, `c_string_character_offset` | planned: PR 4 |
| 62 | gram lookup (`user-gram-unsorted-guess`) | 30 | `_start_gram`, `_replace_user_bigram`, `_replace_system_start_row`, `_guess_out`, `_dynamic_adjust_out`, `_segment_out`, `_system_start_tokens`, `_pin_lower_bound`, `_interleave_miss_layout`, `user_gram_lookup`, `system_gram_lookup`, `short_start_guess`, `short_start_dynamic_adjust`, `short_start_dynamic_adjust_parse`, `phrase_prefix_gram`, `phrase_segment_nonscalar_raw_key` | planned: PR 4 |
| 63 | library loading and replacement (`stderr-library-init-missing`) | 155 | `sparse_chunk`, `chunk_stats`, `nonzero_layout_chunk`, `break_library`, `train_once`, `library_probe`, `library_expected`, `library_directory_probe`, `library_directory_guard`, `library_control`, `chunk_limits`, `chunk_init_probe`, `chunk_addon_probe`, `chunk_load_probe`, `chunk_save_probe`, `chunk_mask_probe`, `chunk_reinit_probe`, `chunk_candidates_probe`, `chunk_token_probe`, `nonzero_total_candidates_probe`, `nonzero_total_save_probe`, `crash_layout_init_probe`, `log_walk`, `save_replacement_probe`, `save_changed_with_override_probe`, `save_missing_system_probe`, `save_unloaded_replacement_probe` | planned: PR 4 |
| 64 | table.conf and scanner (`hash-path/system-directory-pinyin`) | 222 | `conf_system_bytes`, `conf_guess_rows`, `conf_sentence`, `conf_view`, `conf_session`, `hash_path_fixture`, `hash_path_session`, `addon_session`, `conf_case`, `conf_abort`, `header_probe`, `conf_919_case`, `one_line_conf`, `header_919`, `scanner_initialization`, `absolute_user_file`, `stderr_conf_case`, `candidates_probe` | planned: PR 5 |
| 65 | journal replay and removed items (`replay-unigram-add-live-item-pinyin`) | 177 | `_log_record`, `_item_size`, `_system_items`, `_item_with`, `_flip`, `_item_text`, `_observe_system_item`, `system_replay`, `_replay_case`, `_watch`, `unigram_lifecycle`, `unigram_null_user_save`, `_add_fresh`, `_matched_remove`, `_compare_case`, `_touched_order`, `_seed_mixed`, `_touched_sparse_edits`, `_modify_grow`, `_modify_in_place`, `_modify_empty`, `_watch_absent`, `_modify_absent_stale`, `_modify_absent_default`, `_cross_item_pair`, `_cross_item_records`, `_cross_item_records_past_area`, `_observe_replayed_item`, `_observe_cross_item`, `_watch_last`, `touched_many_invalid`, `_short_add`, `_pron_entries`, `_pron_shape`, `pron_lifecycle`, `pron_mixed`, `_removed_extension`, `_removed_candidate`, `_removed_predict_setup`, `_predicted_surface`, `_predicted_sole`, `_predicted_choose`, `_predicted_class_b`, `_predicted_stale_skip` | planned: PR 5 |
| 66 | axis C cases (`axis-c-export-user`) | 888 | `axis_c_isolated`, `axis_c_surface`, `axis_c_options`, `axis_c_lookup`, `axis_c_import`, `axis_c_export_user`, `axis_c_encoding`, `axis_c_keyboard`, `axis_c_layout` | planned: PR 6 |
| 67 | separator and before-cursor (`separator/pinyin/0/parse/0`) | 1360 | `borrowed_text`, `separator_case` | planned: PR 7 |
