//! The pinyin facade's frozen pin contract cases whose references hold the tkrzw cell only:
//! the pin's observations with the pinned libpinyin-data package,
//! at pin 074a2219, formerly verified by `tools/bisection/contract-diff.py`.
//!
//! Each case is one test named by its case ID. The expected values are the pin's
//! frozen observations, or oxpinyin's for a policy-accepted difference, whose
//! record the case's comment names. `common/contract/support.rs` in
//! `oxpinyin-capi` runs each case in a fresh child process.

// Links the facade: the cases call its exported C symbols, not Rust paths.
use pinyin_capi as _;

#[macro_use]
#[path = "common/contract/support.rs"]
mod support;

#[macro_use]
#[path = "common/contract/abi.rs"]
mod abi;

/// The pinyin facade's C symbols under the names the case bodies use.
mod facade {
    use crate::support::P;
    use std::ffi::c_char;

    /// Whether the bodies run on the zhuyin facade.
    #[allow(dead_code)]
    pub const ZHUYIN: bool = false;

    shared_abi! { "pinyin_";
        fn get_sentence(instance: P, index: u8, sentence: *mut *mut c_char) -> bool;
        fn guess_candidates(instance: P, offset: usize, sort_option: u32) -> bool;
        fn parse_more_double_pinyins(instance: P, pinyins: *const c_char) -> usize;
        fn train(instance: P, index: u8) -> bool;
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
        fn get_character_offset(instance: P, phrase: *const c_char, offset: usize, length: *mut usize) -> bool;
        fn get_pinyin_offset(instance: P, cursor: usize, offset: *mut usize) -> bool;
        fn get_left_pinyin_offset(instance: P, offset: usize, left: *mut usize) -> bool;
        fn get_right_pinyin_offset(instance: P, offset: usize, right: *mut usize) -> bool;
        fn get_luoma_pinyin_string(instance: P, key: P, utf8_str: *mut *mut c_char) -> bool;
        fn get_secondary_zhuyin_string(instance: P, key: P, utf8_str: *mut *mut c_char) -> bool;
        fn get_pinyin_strings(instance: P, key: P, shengmu: *mut *mut c_char, yunmu: *mut *mut c_char) -> bool;
        fn parse_double_pinyin(instance: P, onepinyin: *const c_char, onekey: P) -> bool;
        fn get_full_pinyin_auxiliary_text(instance: P, cursor: usize, aux_text: *mut *mut c_char) -> bool;
        fn get_double_pinyin_auxiliary_text(instance: P, cursor: usize, aux_text: *mut *mut c_char) -> bool;
        fn get_chewing_auxiliary_text(instance: P, cursor: usize, aux_text: *mut *mut c_char) -> bool;
        fn reset(instance: P) -> bool;
        fn get_pinyin_is_incomplete(instance: P, key: P) -> bool;
        fn begin_get_bigram_phrases(context: P) -> P;
        fn bigram_iterator_has_next_phrase(iter: P) -> bool;
        fn bigram_iterator_get_next_phrase(iter: P, phrase: *mut *mut c_char, pinyin: *mut *mut c_char, count: *mut i32) -> bool;
    }

    /// Trains n-best result 0 (`pinyin_train(instance, 0)`).
    #[allow(dead_code)]
    pub fn train_top(instance: P) -> bool {
        train(instance, 0)
    }

    /// Trains n-best result `index` (`pinyin_train(instance, index)`).
    #[allow(dead_code)]
    pub fn train_at(instance: P, index: u8) -> bool {
        train(instance, index)
    }

    /// The pinyin facade's indexed sentence accessor.
    #[allow(dead_code)]
    pub fn get_sentence_at(instance: P, index: u8, sentence: *mut *mut c_char) -> bool {
        get_sentence(instance, index, sentence)
    }

    /// The pinyin facade has no unindexed sentence accessor.
    #[allow(dead_code, unused_variables)]
    pub fn get_sentence_only(instance: P, sentence: *mut *mut c_char) -> bool {
        unreachable!("the pinyin facade's get_sentence takes an index")
    }
    absent_abi! { "pinyin";
        fn guess_candidates_after_cursor(instance: P, offset: usize) -> bool;
        fn guess_candidates_before_cursor(instance: P, offset: usize) -> bool;
        fn get_zhuyin_key(instance: P, index: usize, key: *mut P) -> bool;
        fn get_zhuyin_key_rest(instance: P, index: usize, key_rest: *mut P) -> bool;
        fn get_zhuyin_key_rest_positions(instance: P, key_rest: P, begin: *mut u16, end: *mut u16) -> bool;
        fn get_zhuyin_key_rest_length(instance: P, key_rest: P, length: *mut u16) -> bool;
        fn set_chewing_scheme(context: P, scheme: i32) -> bool;
        fn get_zhuyin_offset(instance: P, cursor: usize, offset: *mut usize) -> bool;
        fn get_left_zhuyin_offset(instance: P, offset: usize, left: *mut usize) -> bool;
        fn get_right_zhuyin_offset(instance: P, offset: usize, right: *mut usize) -> bool;
    }
}

/// libpinyin's export iterator for the export child (`support::libpinyin_export`):
/// this target links the pinyin facade itself.
mod libpinyin {
    #[allow(unused_imports)]
    pub use crate::facade::{
        begin_get_phrases, end_get_phrases, init, iterator_get_next_phrase,
        iterator_has_next_phrase,
    };
}
#[path = "common/contract/readers.rs"]
mod readers;

#[path = "common/contract/fixtures.rs"]
mod fixtures;

/// The case bodies, shared by both facades' targets: each target uses
/// the bodies of its own facade's cases.
#[allow(dead_code)]
#[path = "common/contract/bodies"]
mod bodies {
    pub mod alloc_after_fini;
    pub mod allocator_register;
    pub mod auxiliary_text;
    pub mod axis_c;
    pub mod axis_j;
    pub mod before_cursor_dedup;
    pub mod bigram_removal;
    pub mod c_string_inputs;
    pub mod candidate_lookup_offsets;
    pub mod candidate_removals;
    pub mod case_helpers;
    pub mod changed_parse_constraints;
    pub mod combined_import_nbest;
    pub mod cursor_sentence_refusals;
    pub mod default_options;
    pub mod duplicate_phrase_add;
    pub mod facade_total;
    pub mod facade_totals_counts;
    pub mod final_step_surface;
    pub mod glib_arrays;
    pub mod input_length_cap;
    pub mod invalid_input_bytes;
    pub mod issue542_remaining;
    pub mod journal_replay;
    pub mod key_positions_past_65535;
    pub mod library_loading;
    pub mod matrix_resplit_windows;
    pub mod negative_remember;
    pub mod non_utf8_directories;
    pub mod phrase_table_mask;
    pub mod pinyin_out_parameters;
    pub mod predicted_types;
    pub mod prefix_conversion;
    pub mod punctuation;
    pub mod reading_256th;
    pub mod remember_tone;
    pub mod row54_empty_forcing;
    pub mod row58_ranking;
    pub mod scheme_library_refusals;
    pub mod separator_before_cursor;
    pub mod sort_pinyin_length;
    pub mod stored_count_headers;
    pub mod stored_total_overflow;
    pub mod table_conf_scanner;
    pub mod toned_keys_zhuyin_refusals;
    pub mod training_constraints;
    pub mod user_key_spellings;
    pub mod zero_consumption;
    pub mod zero_key_guess;
    pub mod zhuyin_facade_twins;
}

#[path = "contract_data/fixtures/types.rs"]
mod fixture_types;

#[path = "contract_data/alloc_after_fini.rs"]
mod alloc_after_fini;
#[path = "contract_data/allocator_register.rs"]
mod allocator_register;
#[path = "contract_data/auxiliary_text.rs"]
mod auxiliary_text;
#[path = "contract_data/axis_c.rs"]
mod axis_c;
#[path = "contract_data/axis_j.rs"]
mod axis_j;
#[path = "contract_data/before_cursor_dedup.rs"]
mod before_cursor_dedup;
#[path = "contract_data/bigram_removal.rs"]
mod bigram_removal;
#[path = "contract_data/c_string_inputs.rs"]
mod c_string_inputs;
#[path = "contract_data/candidate_lookup_offsets.rs"]
mod candidate_lookup_offsets;
#[path = "contract_data/candidate_removals.rs"]
mod candidate_removals;
#[path = "contract_data/case_helpers.rs"]
mod case_helpers;
#[path = "contract_data/changed_parse_constraints.rs"]
mod changed_parse_constraints;
#[path = "contract_data/combined_import_nbest.rs"]
mod combined_import_nbest;
#[path = "contract_data/cursor_sentence_refusals.rs"]
mod cursor_sentence_refusals;
#[path = "contract_data/default_options.rs"]
mod default_options;
#[path = "contract_data/duplicate_phrase_add.rs"]
mod duplicate_phrase_add;
#[path = "contract_data/facade_total.rs"]
mod facade_total;
#[path = "contract_data/facade_totals_counts.rs"]
mod facade_totals_counts;
#[path = "contract_data/final_step_surface.rs"]
mod final_step_surface;
#[path = "contract_data/glib_arrays.rs"]
mod glib_arrays;
#[path = "contract_data/input_length_cap.rs"]
mod input_length_cap;
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
#[path = "contract_data/matrix_resplit_windows.rs"]
mod matrix_resplit_windows;
#[path = "contract_data/negative_remember.rs"]
mod negative_remember;
#[path = "contract_data/non_utf8_directories.rs"]
mod non_utf8_directories;
#[path = "contract_data/phrase_table_mask.rs"]
mod phrase_table_mask;
#[path = "contract_data/pinyin_out_parameters.rs"]
mod pinyin_out_parameters;
#[path = "contract_data/predicted_types.rs"]
mod predicted_types;
#[path = "contract_data/prefix_conversion.rs"]
mod prefix_conversion;
#[path = "contract_data/punctuation.rs"]
mod punctuation;
#[path = "contract_data/reading_256th.rs"]
mod reading_256th;
#[path = "contract_data/remember_tone.rs"]
mod remember_tone;
#[path = "contract_data/row54_empty_forcing.rs"]
mod row54_empty_forcing;
#[path = "contract_data/row58_ranking.rs"]
mod row58_ranking;
#[path = "contract_data/scheme_library_refusals.rs"]
mod scheme_library_refusals;
#[path = "contract_data/separator_before_cursor.rs"]
mod separator_before_cursor;
#[path = "contract_data/sort_pinyin_length.rs"]
mod sort_pinyin_length;
#[path = "contract_data/stored_count_headers.rs"]
mod stored_count_headers;
#[path = "contract_data/stored_total_overflow.rs"]
mod stored_total_overflow;
#[path = "contract_data/table_conf_scanner.rs"]
mod table_conf_scanner;
#[path = "contract_data/toned_keys_zhuyin_refusals.rs"]
mod toned_keys_zhuyin_refusals;
#[path = "contract_data/training_constraints.rs"]
mod training_constraints;
#[path = "contract_data/user_key_spellings.rs"]
mod user_key_spellings;
#[path = "contract_data/zero_consumption.rs"]
mod zero_consumption;
#[path = "contract_data/zero_key_guess.rs"]
mod zero_key_guess;
#[path = "contract_data/zhuyin_facade_twins.rs"]
mod zhuyin_facade_twins;
