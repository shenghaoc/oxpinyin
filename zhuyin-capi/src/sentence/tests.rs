#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::candidates::{zhuyin_get_candidate, zhuyin_get_candidate_type, zhuyin_get_n_candidate};

use crate::parse::zhuyin_parse_more_chewings;

use crate::sentence::{
    zhuyin_guess_candidates_after_cursor, zhuyin_guess_candidates_before_cursor,
    zhuyin_guess_sentence,
};

use crate::test_support::{assert_aborts, chewing, close, cstr, n_candidate, open};

use crate::types::{LookupCandidate, ZhuyinContext, ZhuyinInstance, lookup_candidate_type_t};

use std::ptr;

/// su3cl3 with the facade's unigram total wrapped to zero through
/// `zhuyin_token_add_unigram_frequency`.
///
/// # Safety
///
/// `context` is NULL or a live context: `zhuyin_init` returned it and `zhuyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn zero_total(context: *mut ZhuyinContext) -> *mut ZhuyinInstance {
    // SAFETY: `context` meets this function's `# Safety` contract; only this thread calls into it.
    let instance = unsafe { chewing(context, "su3cl3") };
    // SAFETY: `instance` is live; the reference ends in this block.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    let total = u32::try_from(total).expect("a guint32 total");
    assert!(unsafe {
        crate::dict::zhuyin_token_add_unigram_frequency(instance, 0x0100_0001, total.wrapping_neg())
    });
    instance
}

/// Before offset 0 the before-cursor guess searches no span, so a zero
/// total ranks nothing: true with no row.
#[test]
fn a_zero_facade_total_before_offset_zero_ranks_nothing() {
    let (context, _) = open();
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { zero_total(context) };
    assert!(unsafe { zhuyin_guess_candidates_before_cursor(instance, 0) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { n_candidate(instance) }, (true, 0));
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A zero facade total asserts while the after-cursor guess ranks a row
/// (`zhuyin.cpp:1261`).
#[test]
fn a_zero_facade_total_aborts_the_after_cursor_ranking() {
    assert_aborts(
        "sentence::tests::a_zero_facade_total_aborts_the_after_cursor_ranking",
        || {
            let (context, _) = open();
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
            let instance = unsafe { zero_total(context) };
            unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) };
        },
    );
}

/// The before-cursor guess also asserts when a span reaches ranking.
#[test]
fn a_zero_facade_total_aborts_the_before_cursor_ranking() {
    assert_aborts(
        "sentence::tests::a_zero_facade_total_aborts_the_before_cursor_ranking",
        || {
            let (context, _) = open();
            let instance = unsafe { zero_total(context) };
            unsafe { zhuyin_guess_candidates_before_cursor(instance, 6) };
        },
    );
}

/// Every row's type in the current list.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `zhuyin_alloc_instance` returned it and
/// `zhuyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn kinds(instance: *mut crate::types::ZhuyinInstance) -> Vec<lookup_candidate_type_t> {
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

/// S6, the noguess protocol: no sentence guess since the parse, so the n-best results
/// are empty and nothing is prepended (`zhuyin.cpp:1277-1278` at 074a2219). Before offset 0 the
/// span loop does not run (`:1563`); inside a key the span is skipped (`:1590-1591`;
/// `phonetic_key_matrix.cpp:422-423`). A toneless `su` places no key under FORCE_TONE
/// (`zhuyin_parser2.cpp:178-179`), so the empty matrix answers false (`zhuyin.cpp:1549-1552`).
/// Pin observations
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
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { kinds(instance) }, [], "{keys}@{offset}");
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
    }
}

/// S6: at a key end every unguessed row is a phrase of the spans ending there,
/// NORMAL_CANDIDATE_BEFORE_CURSOR (`zhuyin.cpp:1594`); after a guess the sentence is
/// prepended as the one BEST_MATCH row (`:1283`, `:1626`), and before offset 0 it is the only
/// row. Pin observations
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
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let unguessed = unsafe { kinds(instance) };
        assert!(!unguessed.is_empty());
        assert!(
            unguessed
                .iter()
                .all(|kind| *kind == lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR),
            "{offset}: {unguessed:?}"
        );
        assert!(unsafe { zhuyin_guess_sentence(instance) });
        assert!(unsafe { zhuyin_guess_candidates_before_cursor(instance, offset) });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let guessed = unsafe { kinds(instance) };
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
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
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
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { kinds(instance) },
        [lookup_candidate_type_t::BEST_MATCH_CANDIDATE]
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}
