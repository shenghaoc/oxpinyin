//! The zhuyin facade's frozen pin contract cases whose references hold the tkrzw cell only:
//! the pin's observations with the pinned libpinyin-data package,
//! at pin 074a2219, formerly verified by `tools/bisection/contract-diff.py`.
//!
//! Each case is one test named by its case ID. The expected values are the pin's
//! frozen observations, or oxpinyin's for a policy-accepted difference, whose
//! record the case's comment names. `common/contract/support.rs` in
//! `oxpinyin-capi` runs each case in a fresh child process.

// Links the facade: the cases call its exported C symbols, not Rust paths.
use zhuyin_capi as _;
// Links libpinyin: the cases export a saved profile through its export iterator
// (`common/contract/libpinyin.rs`).
use pinyin_capi as _;

#[macro_use]
#[path = "../../oxpinyin-capi/tests/common/contract/support.rs"]
mod support;

#[macro_use]
#[path = "../../oxpinyin-capi/tests/common/contract/abi.rs"]
mod abi;

/// The zhuyin facade's C symbols under the names the case bodies use.
mod facade {
    use crate::support::P;
    use std::ffi::c_char;

    /// Whether the bodies run on the zhuyin facade.
    #[allow(dead_code)]
    pub const ZHUYIN: bool = true;

    shared_abi! { "zhuyin_";
        fn get_sentence(instance: P, sentence: *mut *mut c_char) -> bool;
        fn guess_candidates_after_cursor(instance: P, offset: usize) -> bool;
        fn guess_candidates_before_cursor(instance: P, offset: usize) -> bool;
        fn train(instance: P) -> bool;
        fn get_zhuyin_key(instance: P, index: usize, key: *mut P) -> bool;
        fn get_zhuyin_key_rest(instance: P, index: usize, key_rest: *mut P) -> bool;
        fn get_zhuyin_key_rest_positions(instance: P, key_rest: P, begin: *mut u16, end: *mut u16) -> bool;
        fn get_zhuyin_key_rest_length(instance: P, key_rest: P, length: *mut u16) -> bool;
        fn set_chewing_scheme(context: P, scheme: i32) -> bool;
        fn get_character_offset(instance: P, phrase: *const c_char, offset: usize, length: *mut usize) -> bool;
        fn get_zhuyin_offset(instance: P, cursor: usize, offset: *mut usize) -> bool;
        fn get_left_zhuyin_offset(instance: P, offset: usize, left: *mut usize) -> bool;
        fn get_right_zhuyin_offset(instance: P, offset: usize, right: *mut usize) -> bool;
        fn reset(instance: P) -> bool;
    }

    /// Trains the n-best result (`zhuyin_train` takes no index).
    #[allow(dead_code)]
    pub fn train_top(instance: P) -> bool {
        train(instance)
    }

    /// The zhuyin facade's train takes no index.
    #[allow(dead_code, unused_variables)]
    pub fn train_at(instance: P, index: u8) -> bool {
        unreachable!("the zhuyin facade's train takes no index")
    }

    /// The zhuyin facade's one sentence.
    #[allow(dead_code)]
    pub fn get_sentence_only(instance: P, sentence: *mut *mut c_char) -> bool {
        get_sentence(instance, sentence)
    }

    /// The zhuyin facade has one sentence and no indexed accessor.
    #[allow(dead_code, unused_variables)]
    pub fn get_sentence_at(instance: P, index: u8, sentence: *mut *mut c_char) -> bool {
        unreachable!("the zhuyin facade exports no indexed get_sentence")
    }
    absent_abi! { "zhuyin";
        fn guess_candidates(instance: P, offset: usize, sort_option: u32) -> bool;
        fn parse_more_double_pinyins(instance: P, pinyins: *const c_char) -> usize;
        fn remember_user_input(instance: P, phrase: *const c_char, count: i32) -> bool;
        fn begin_get_phrases(context: P, index: u32) -> P;
        fn iterator_has_next_phrase(iter: P) -> bool;
        fn iterator_get_next_phrase(iter: P, phrase: *mut *mut c_char, pinyin: *mut *mut c_char, count: *mut i32) -> bool;
        fn end_get_phrases(iter: P);
        fn guess_predicted_candidates(instance: P, prefix: *const c_char) -> bool;
        fn guess_predicted_candidates_with_punctuations(instance: P, prefix: *const c_char) -> bool;
        fn choose_predicted_candidate(instance: P, candidate: P) -> bool;
        fn get_candidate_nbest_index(instance: P, candidate: P, index: *mut u8) -> bool;
        fn get_pinyin_key(instance: P, offset: usize, key: *mut P) -> bool;
        fn get_pinyin_key_rest(instance: P, offset: usize, key_rest: *mut P) -> bool;
        fn get_pinyin_key_rest_positions(instance: P, key_rest: P, begin: *mut u16, end: *mut u16) -> bool;
        fn get_pinyin_key_rest_length(instance: P, key_rest: P, length: *mut u16) -> bool;
        fn set_zhuyin_scheme(context: P, scheme: i32) -> bool;
        fn set_double_pinyin_scheme(context: P, scheme: i32) -> bool;
        fn load_addon_phrase_library(context: P, index: u8) -> bool;
        fn unload_addon_phrase_library(context: P, index: u8) -> bool;
        fn is_user_candidate(instance: P, candidate: P) -> bool;
        fn remove_user_candidate(instance: P, candidate: P) -> bool;
        fn get_pinyin_offset(instance: P, cursor: usize, offset: *mut usize) -> bool;
        fn get_left_pinyin_offset(instance: P, offset: usize, left: *mut usize) -> bool;
        fn get_right_pinyin_offset(instance: P, offset: usize, right: *mut usize) -> bool;
        fn get_luoma_pinyin_string(instance: P, key: P, utf8_str: *mut *mut c_char) -> bool;
        fn get_secondary_zhuyin_string(instance: P, key: P, utf8_str: *mut *mut c_char) -> bool;
        fn get_pinyin_strings(instance: P, key: P, shengmu: *mut *mut c_char, yunmu: *mut *mut c_char) -> bool;
        fn parse_double_pinyin(instance: P, onepinyin: *const c_char, onekey: P) -> bool;
        fn get_pinyin_is_incomplete(instance: P, key: P) -> bool;
        fn begin_get_bigram_phrases(context: P) -> P;
        fn bigram_iterator_has_next_phrase(iter: P) -> bool;
        fn bigram_iterator_get_next_phrase(iter: P, phrase: *mut *mut c_char, pinyin: *mut *mut c_char, count: *mut i32) -> bool;
        fn end_get_bigram_phrases(iter: P);
    }
}

#[path = "../../oxpinyin-capi/tests/common/contract/libpinyin.rs"]
mod libpinyin;
#[path = "../../oxpinyin-capi/tests/common/contract/readers.rs"]
mod readers;

#[path = "../../oxpinyin-capi/tests/common/contract/fixtures.rs"]
mod fixtures;

/// The case bodies, shared by both facades' targets: each target uses
/// the bodies of its own facade's cases.
#[allow(dead_code)]
#[path = "../../oxpinyin-capi/tests/common/contract/bodies"]
mod bodies {
    pub mod allocator_register;
    pub mod axis_c;
    pub mod axis_j;
    pub mod before_cursor_dedup;
    pub mod bigram_mask_prediction;
    pub mod bigram_removal;
    pub mod c_string_inputs;
    pub mod candidate_lookup_offsets;
    pub mod case_helpers;
    pub mod changed_parse_constraints;
    pub mod combined_import_nbest;
    pub mod duplicate_phrase_add;
    pub mod final_step_surface;
    pub mod glib_arrays;
    pub mod glib_path_construction;
    pub mod gram_lookup;
    pub mod integer_limits_d18;
    pub mod invalid_input_bytes;
    pub mod issue542_remaining;
    pub mod journal_replay;
    pub mod key_positions_past_65535;
    pub mod library_loading;
    pub mod library_reload;
    pub mod matrix_resplit_windows;
    pub mod missing_system_dbm;
    pub mod non_utf8_directories;
    pub mod null_user_library;
    pub mod phrase_export_training;
    pub mod phrase_index_logger;
    pub mod prefix_conversion;
    pub mod reading_256th;
    pub mod row54_empty_forcing;
    pub mod row58_ranking;
    pub mod save_failures_stderr;
    pub mod scheme_library_refusals;
    pub mod separator_before_cursor;
    pub mod sort_pinyin_length;
    pub mod stored_count_headers;
    pub mod stored_user_gram;
    pub mod system_mask_out;
    pub mod system_phrase_text;
    pub mod table_conf_scanner;
    pub mod token_reuse_addon_import;
    pub mod token_unigram_training;
    pub mod toned_keys_zhuyin_refusals;
    pub mod training_constraints;
    pub mod user_conf_overlong_index;
    pub mod user_index_phrase_mask;
    pub mod user_key_spellings;
    pub mod user_stored_words;
    pub mod zero_consumption;
    pub mod zero_key_guess;
    pub mod zhuyin_facade_twins;
    pub mod zhuyin_null_arguments;
    pub mod zhuyin_per_entry;
}

#[path = "contract_data/fixtures/types.rs"]
mod fixture_types;

#[path = "contract_data/allocator_register.rs"]
mod allocator_register;
#[path = "contract_data/axis_c.rs"]
mod axis_c;
#[path = "contract_data/axis_j.rs"]
mod axis_j;
#[path = "contract_data/before_cursor_dedup.rs"]
mod before_cursor_dedup;
#[path = "contract_data/bigram_mask_prediction.rs"]
mod bigram_mask_prediction;
#[path = "contract_data/bigram_removal.rs"]
mod bigram_removal;
#[path = "contract_data/c_string_inputs.rs"]
mod c_string_inputs;
#[path = "contract_data/candidate_lookup_offsets.rs"]
mod candidate_lookup_offsets;
#[path = "contract_data/case_helpers.rs"]
mod case_helpers;
#[path = "contract_data/changed_parse_constraints.rs"]
mod changed_parse_constraints;
#[path = "contract_data/combined_import_nbest.rs"]
mod combined_import_nbest;
#[path = "contract_data/duplicate_phrase_add.rs"]
mod duplicate_phrase_add;
#[path = "contract_data/final_step_surface.rs"]
mod final_step_surface;
#[path = "contract_data/glib_arrays.rs"]
mod glib_arrays;
#[path = "contract_data/glib_path_construction.rs"]
mod glib_path_construction;
#[path = "contract_data/gram_lookup.rs"]
mod gram_lookup;
#[path = "contract_data/integer_limits_d18.rs"]
mod integer_limits_d18;
#[path = "contract_data/invalid_input_bytes.rs"]
mod invalid_input_bytes;
#[path = "contract_data/issue542_remaining.rs"]
mod issue542_remaining;
#[path = "contract_data/journal_replay.rs"]
mod journal_replay;
#[path = "contract_data/key_positions_past_65535.rs"]
mod key_positions_past_65535;
#[path = "contract_data/library_loading.rs"]
mod library_loading;
#[path = "contract_data/library_reload.rs"]
mod library_reload;
#[path = "contract_data/matrix_resplit_windows.rs"]
mod matrix_resplit_windows;
#[path = "contract_data/missing_system_dbm.rs"]
mod missing_system_dbm;
#[path = "contract_data/non_utf8_directories.rs"]
mod non_utf8_directories;
#[path = "contract_data/null_user_library.rs"]
mod null_user_library;
#[path = "contract_data/phrase_export_training.rs"]
mod phrase_export_training;
#[path = "contract_data/phrase_index_logger.rs"]
mod phrase_index_logger;
#[path = "contract_data/prefix_conversion.rs"]
mod prefix_conversion;
#[path = "contract_data/reading_256th.rs"]
mod reading_256th;
#[path = "contract_data/row54_empty_forcing.rs"]
mod row54_empty_forcing;
#[path = "contract_data/row58_ranking.rs"]
mod row58_ranking;
#[path = "contract_data/save_failures_stderr.rs"]
mod save_failures_stderr;
#[path = "contract_data/scheme_library_refusals.rs"]
mod scheme_library_refusals;
#[path = "contract_data/separator_before_cursor.rs"]
mod separator_before_cursor;
#[path = "contract_data/sort_pinyin_length.rs"]
mod sort_pinyin_length;
#[path = "contract_data/stored_count_headers.rs"]
mod stored_count_headers;
#[path = "contract_data/stored_user_gram.rs"]
mod stored_user_gram;
#[path = "contract_data/system_mask_out.rs"]
mod system_mask_out;
#[path = "contract_data/system_phrase_text.rs"]
mod system_phrase_text;
#[path = "contract_data/table_conf_scanner.rs"]
mod table_conf_scanner;
#[path = "contract_data/token_reuse_addon_import.rs"]
mod token_reuse_addon_import;
#[path = "contract_data/token_unigram_training.rs"]
mod token_unigram_training;
#[path = "contract_data/toned_keys_zhuyin_refusals.rs"]
mod toned_keys_zhuyin_refusals;
#[path = "contract_data/training_constraints.rs"]
mod training_constraints;
#[path = "contract_data/user_conf_overlong_index.rs"]
mod user_conf_overlong_index;
#[path = "contract_data/user_index_phrase_mask.rs"]
mod user_index_phrase_mask;
#[path = "contract_data/user_key_spellings.rs"]
mod user_key_spellings;
#[path = "contract_data/user_stored_words.rs"]
mod user_stored_words;
#[path = "contract_data/zero_consumption.rs"]
mod zero_consumption;
#[path = "contract_data/zero_key_guess.rs"]
mod zero_key_guess;
#[path = "contract_data/zhuyin_facade_twins.rs"]
mod zhuyin_facade_twins;
#[path = "contract_data/zhuyin_null_arguments.rs"]
mod zhuyin_null_arguments;
#[path = "contract_data/zhuyin_per_entry.rs"]
mod zhuyin_per_entry;
