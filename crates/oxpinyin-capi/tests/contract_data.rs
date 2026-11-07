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
        fn is_user_candidate(instance: P, candidate: P) -> bool;
        fn remove_user_candidate(instance: P, candidate: P) -> bool;
    }

    /// Trains n-best result 0 (`pinyin_train(instance, 0)`).
    #[allow(dead_code)]
    pub fn train_top(instance: P) -> bool {
        train(instance, 0)
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
    }
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
    pub mod axis_j;
    pub mod before_cursor_dedup;
    pub mod candidate_lookup_offsets;
    pub mod case_helpers;
    pub mod default_options;
    pub mod duplicate_phrase_add;
    pub mod facade_total;
    pub mod glib_arrays;
    pub mod input_length_cap;
    pub mod invalid_input_bytes;
    pub mod issue542_remaining;
    pub mod key_positions_past_65535;
    pub mod negative_remember;
    pub mod non_utf8_directories;
    pub mod phrase_table_mask;
    pub mod predicted_types;
    pub mod reading_256th;
    pub mod remember_tone;
    pub mod stored_count_headers;
    pub mod stored_total_overflow;
}

#[path = "contract_data/fixtures/types.rs"]
mod fixture_types;

#[path = "contract_data/alloc_after_fini.rs"]
mod alloc_after_fini;
#[path = "contract_data/axis_j.rs"]
mod axis_j;
#[path = "contract_data/before_cursor_dedup.rs"]
mod before_cursor_dedup;
#[path = "contract_data/candidate_lookup_offsets.rs"]
mod candidate_lookup_offsets;
#[path = "contract_data/case_helpers.rs"]
mod case_helpers;
#[path = "contract_data/default_options.rs"]
mod default_options;
#[path = "contract_data/duplicate_phrase_add.rs"]
mod duplicate_phrase_add;
#[path = "contract_data/facade_total.rs"]
mod facade_total;
#[path = "contract_data/glib_arrays.rs"]
mod glib_arrays;
#[path = "contract_data/input_length_cap.rs"]
mod input_length_cap;
#[path = "contract_data/invalid_input_bytes.rs"]
mod invalid_input_bytes;
#[path = "contract_data/issue542_remaining.rs"]
mod issue542_remaining;
#[path = "contract_data/key_positions_past_65535.rs"]
mod key_positions_past_65535;
#[path = "contract_data/negative_remember.rs"]
mod negative_remember;
#[path = "contract_data/non_utf8_directories.rs"]
mod non_utf8_directories;
#[path = "contract_data/phrase_table_mask.rs"]
mod phrase_table_mask;
#[path = "contract_data/predicted_types.rs"]
mod predicted_types;
#[path = "contract_data/reading_256th.rs"]
mod reading_256th;
#[path = "contract_data/remember_tone.rs"]
mod remember_tone;
#[path = "contract_data/stored_count_headers.rs"]
mod stored_count_headers;
#[path = "contract_data/stored_total_overflow.rs"]
mod stored_total_overflow;
