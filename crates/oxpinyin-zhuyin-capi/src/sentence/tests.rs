#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::candidates::{zhuyin_get_candidate, zhuyin_get_candidate_type, zhuyin_get_n_candidate};

use crate::dict::zhuyin_token_add_unigram_frequency;

use crate::instance::zhuyin_alloc_instance;

use crate::parse::zhuyin_parse_more_chewings;

use crate::sentence::{
    zhuyin_guess_candidates_after_cursor, zhuyin_guess_candidates_before_cursor,
    zhuyin_guess_sentence,
};

use crate::test_support::{assert_aborts, close, cstr, open};

use crate::types::{LookupCandidate, ZhuyinContext, ZhuyinInstance, lookup_candidate_type_t};

use std::ptr;

/// The out-parameter seed that shows an untouched output.
const UNTOUCHED: u32 = 0x00AB_CDEF;

/// A fresh instance on `context` with `keys` parsed as chewing keystrokes.
fn chewing(context: *mut ZhuyinContext, keys: &str) -> *mut ZhuyinInstance {
    let instance = unsafe { zhuyin_alloc_instance(context) };
    assert!(!instance.is_null());
    unsafe { zhuyin_parse_more_chewings(instance, cstr(keys).as_ptr()) };
    instance
}

/// The candidate count of the current list.
fn n_candidate(instance: *mut ZhuyinInstance) -> (bool, u32) {
    let mut count = UNTOUCHED;
    let ret = unsafe { zhuyin_get_n_candidate(instance, &raw mut count) };
    (ret, count)
}

/// su3cl3 with the facade's unigram total wrapped to zero through
/// `zhuyin_token_add_unigram_frequency`.
fn zero_total(context: *mut ZhuyinContext) -> *mut ZhuyinInstance {
    let instance = chewing(context, "su3cl3");
    // SAFETY: `instance` is live; the reference ends in this block.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    let total = u32::try_from(total).expect("a guint32 total");
    assert!(unsafe {
        zhuyin_token_add_unigram_frequency(instance, 0x0100_0001, total.wrapping_neg())
    });
    instance
}

/// Before offset 0 the before-cursor guess searches no span, so a zero
/// total ranks nothing: true with no row.
#[test]
fn a_zero_facade_total_before_offset_zero_ranks_nothing() {
    let (context, _) = open();
    let instance = zero_total(context);
    assert!(unsafe { zhuyin_guess_candidates_before_cursor(instance, 0) });
    assert_eq!(n_candidate(instance), (true, 0));
    close(context, instance);
}

#[test]
fn a_read_failure_after_the_size_check_aborts() {
    // Kyoto Cabinet's `Bigram::load` asserts where the body read fails
    // after `check` found the record (`ngram_kyotodb.cpp:136-137`). No
    // differential can make a read fail after a successful check, so
    // this death test holds the abort.
    crate::test_support::assert_aborts(
        "sentence::tests::a_read_failure_after_the_size_check_aborts",
        || {
            let error = oxpinyin_engine::EngineError::ReadAfterCheck { token: 0x0100_0001 };
            crate::sentence::refuse_lookup_gram("zhuyin_phrase_segment", &error);
        },
    );
}

/// Every row's type in the current list.
fn kinds(instance: *mut crate::types::ZhuyinInstance) -> Vec<lookup_candidate_type_t> {
    let mut count = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    (0..count)
        .map(|index| {
            let mut cand: *mut LookupCandidate = ptr::null_mut();
            assert!(unsafe { zhuyin_get_candidate(instance, index, &raw mut cand) });
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(unsafe { zhuyin_get_candidate_type(instance, cand, &raw mut kind) });
            kind
        })
        .collect()
}

/// The defensive matrix failure reaches the entry point as an owned
/// refusal. Exercise its diagnostic without manufacturing a bad matrix.
#[test]
fn candidate_offset_refusal_logs_once() {
    let logs = crate::test_support::LogCapture::new();
    assert!(!super::candidate_guess_answer(Err(
        super::CandidateGuessRefusal::InvalidOffset,
    )));
    assert_eq!(
        logs.take_with_messages(),
        [(
            (Some("libzhuyin".to_owned()), glib_sys::G_LOG_LEVEL_WARNING),
            "zhuyin_guess_candidates: the lookup failed".to_owned(),
        )]
    );
}

/// S6, the noguess protocol: no sentence guess since the parse, so the n-best results
/// are empty and nothing is prepended (`zhuyin.cpp:1277-1278` at 074a2219). Before offset 0 the
/// span loop does not run (`:1563`); inside a key the span is skipped (`:1590-1591`;
/// `phonetic_key_matrix.cpp:422-423`). A toneless `su` places no key under FORCE_TONE
/// (`zhuyin_parser2.cpp:178-179`), so the empty matrix answers false (`zhuyin.cpp:1549-1552`).
/// Pin capture of 2026-11-07, runs 1 and 2 identical, observations
/// `zhuyin-noguess-before/su3cl3@0,1,2,4,5` and `zhuyin-noguess-before/su@0`.
#[test]
fn an_unguessed_before_cursor_window_with_no_span_is_empty() {
    for (keys, consumed, offset, answer) in [
        ("su3cl3", 6, 0, true),
        ("su3cl3", 6, 1, true),
        ("su3cl3", 6, 2, true),
        ("su3cl3", 6, 4, true),
        ("su3cl3", 6, 5, true),
        ("su", 0, 0, false),
    ] {
        let (context, instance) = open();
        let text = cstr(keys);
        assert_eq!(
            unsafe { zhuyin_parse_more_chewings(instance, text.as_ptr()) },
            consumed
        );
        assert_eq!(
            unsafe { zhuyin_guess_candidates_before_cursor(instance, offset) },
            answer,
            "{keys}@{offset}"
        );
        assert_eq!(kinds(instance), [], "{keys}@{offset}");
        close(context, instance);
    }
}

/// S6: at a key end every unguessed row is a phrase of the spans ending there,
/// NORMAL_CANDIDATE_BEFORE_CURSOR (`zhuyin.cpp:1594`); after a guess the sentence is
/// prepended as the one BEST_MATCH row (`:1283`, `:1626`), and before offset 0 it is the only
/// row. Pin capture of 2026-11-07, runs 1 and 2 identical, observations
/// `zhuyin-noguess-before/su3cl3@3,6` and `zhuyin-guess-before/su3cl3@0,3,6`; the row counts
/// are the pinned data's and are not asserted.
#[test]
fn only_a_guess_puts_a_sentence_row_in_the_before_cursor_window() {
    for offset in [3, 6] {
        let (context, instance) = open();
        let text = cstr("su3cl3");
        assert_eq!(
            unsafe { zhuyin_parse_more_chewings(instance, text.as_ptr()) },
            6
        );
        assert!(unsafe { zhuyin_guess_candidates_before_cursor(instance, offset) });
        let unguessed = kinds(instance);
        assert!(!unguessed.is_empty());
        assert!(
            unguessed
                .iter()
                .all(|kind| *kind == lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR),
            "{offset}: {unguessed:?}"
        );
        assert!(unsafe { zhuyin_guess_sentence(instance) });
        assert!(unsafe { zhuyin_guess_candidates_before_cursor(instance, offset) });
        let guessed = kinds(instance);
        assert_eq!(
            guessed[0],
            lookup_candidate_type_t::BEST_MATCH_CANDIDATE,
            "{offset}"
        );
        assert!(
            guessed[1..]
                .iter()
                .all(|kind| *kind == lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR),
            "{offset}: {guessed:?}"
        );
        close(context, instance);
    }
    let (context, instance) = open();
    let text = cstr("su3cl3");
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, text.as_ptr()) },
        6
    );
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_guess_candidates_before_cursor(instance, 0) });
    assert_eq!(
        kinds(instance),
        [lookup_candidate_type_t::BEST_MATCH_CANDIDATE]
    );
    close(context, instance);
}

/// A zero facade total asserts while the after-cursor guess ranks a row
/// (`zhuyin.cpp:1261`, register rows 61 and 66).
#[test]
fn a_zero_facade_total_aborts_the_after_cursor_ranking() {
    assert_aborts(
        "sentence::tests::a_zero_facade_total_aborts_the_after_cursor_ranking",
        || {
            let (context, _) = open();
            unsafe { zhuyin_guess_candidates_after_cursor(zero_total(context), 0) };
        },
    );
}
