//! The zhuyin facade's frozen pin contract cases with references on every backend cell,
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
    }

    /// Trains the n-best result (`zhuyin_train` takes no index).
    #[allow(dead_code)]
    pub fn train_top(instance: P) -> bool {
        train(instance)
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
        fn is_user_candidate(instance: P, candidate: P) -> bool;
        fn remove_user_candidate(instance: P, candidate: P) -> bool;
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
    pub mod remember_tone;
    pub mod zhuyin_null_arguments;
}

#[path = "contract/remember_tone.rs"]
mod remember_tone;
#[path = "contract/zhuyin_null_arguments.rs"]
mod zhuyin_null_arguments;
