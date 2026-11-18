//! Choosing a candidate, the candidate-type asserts and removing a user
//! candidate.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    DEFAULT_SORT, LookupCandidate, PinyinContext, PinyinInstance, TempUserDir, assert_aborts, cstr,
    gram_bytes, open, pinyin_choose_candidate, pinyin_choose_predicted_candidate,
    pinyin_get_candidate, pinyin_get_candidate_nbest_index, pinyin_guess_candidates,
    pinyin_guess_predicted_candidates_with_punctuations, pinyin_guess_sentence,
    pinyin_remove_user_candidate, *,
};

use std::ptr;

/// Parses `nihao`, guesses the sentence and the candidates at 0 with sort 0.
fn guessed_nihao(context: *mut PinyinContext) -> *mut PinyinInstance {
    let instance = alloc(context);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    instance
}

/// `choose_candidate` asserts on a predicted prefix row.
#[test]
fn choose_candidate_aborts_on_a_predicted_prefix_row() {
    assert_aborts(
        "choose::choose_candidate_aborts_on_a_predicted_prefix_row",
        || {
            let user = TempUserDir::new("choose-prefix-row");
            let (_context, instance) = open_at(&user.path);
            assert!(unsafe {
                pinyin_guess_predicted_candidates_with_punctuations(instance, c"你".as_ptr())
            });
            unsafe { pinyin_choose_candidate(instance, 0, row_of(instance, PREDICTED_PREFIX)) };
        },
    );
}

/// `choose_predicted_candidate` asserts on a NORMAL row.
#[test]
fn choose_predicted_candidate_aborts_on_a_normal_row() {
    assert_aborts(
        "choose::choose_predicted_candidate_aborts_on_a_normal_row",
        || {
            let user = TempUserDir::new("choose-predicted-normal");
            let (context, _) = open_at(&user.path);
            let instance = guessed_nihao(context);
            unsafe { pinyin_choose_predicted_candidate(instance, row_of(instance, NORMAL)) };
        },
    );
}

/// `get_candidate_nbest_index` asserts on a NORMAL row.
#[test]
fn nbest_index_aborts_on_a_normal_row() {
    assert_aborts("choose::nbest_index_aborts_on_a_normal_row", || {
        let user = TempUserDir::new("nbest-normal");
        let (context, _) = open_at(&user.path);
        let instance = guessed_nihao(context);
        let mut index = 0xAB_u8;
        unsafe {
            pinyin_get_candidate_nbest_index(instance, row_of(instance, NORMAL), &raw mut index)
        };
    });
}

/// `remove_user_candidate` asserts on a sentence row.
#[test]
fn remove_user_candidate_aborts_on_a_sentence_row() {
    assert_aborts(
        "choose::remove_user_candidate_aborts_on_a_sentence_row",
        || {
            let user = TempUserDir::new("remove-sentence-row");
            let (context, _) = open_at(&user.path);
            let instance = guessed_nihao(context);
            unsafe { pinyin_remove_user_candidate(instance, row_of(instance, NBEST)) };
        },
    );
}

/// `remove_user_candidate` asserts on a NORMAL system token.
#[test]
fn remove_user_candidate_aborts_on_a_system_token() {
    assert_aborts(
        "choose::remove_user_candidate_aborts_on_a_system_token",
        || {
            let user = TempUserDir::new("remove-system-token");
            let (context, _) = open_at(&user.path);
            let instance = guessed_nihao(context);
            unsafe { pinyin_remove_user_candidate(instance, row_of(instance, NORMAL)) };
        },
    );
}

/// `remove_user_candidate` asserts where the user phrase table is gone
///.
#[test]
fn remove_user_candidate_aborts_without_the_phrase_table() {
    assert_aborts(
        "choose::remove_user_candidate_aborts_without_the_phrase_table",
        || {
            let user = TempUserDir::new("remove-no-phrase-table");
            saved_user_phrase(&user.path);
            std::fs::rename(
                user.path.join("user_phrase_index.bin"),
                user.path.join("user_phrase_index.bin.gone"),
            )
            .expect("move the phrase table");
            let (instance, cand) = user_row_of_dada(&user.path);
            unsafe { pinyin_remove_user_candidate(instance, cand) };
        },
    );
}

/// `remove_user_candidate` asserts on a residual user gram: a total of 7
/// over one item of 3.
#[test]
fn remove_user_candidate_aborts_on_a_residual_bigram_gram() {
    assert_aborts(
        "choose::remove_user_candidate_aborts_on_a_residual_bigram_gram",
        || {
            let user = TempUserDir::new("remove-residual-gram");
            let token = saved_user_phrase(&user.path);
            replace_user_bigram(
                &user.path,
                &[(1_u32.to_le_bytes().to_vec(), gram_bytes(7, &[(token, 3)]))],
            );
            let (instance, cand) = user_row_of_dada(&user.path);
            unsafe { pinyin_remove_user_candidate(instance, cand) };
        },
    );
}

/// `choose_candidate` at offset 1 asserts on a list guessed without the
/// sentence row.
#[test]
fn choose_candidate_aborts_at_offset_one_without_a_sentence() {
    assert_aborts(
        "choose::choose_candidate_aborts_at_offset_one_without_a_sentence",
        || {
            let user = TempUserDir::new("choose-without-sentence");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "nihao"), 5);
            // SORT_WITHOUT_SENTENCE_CANDIDATE.
            assert!(unsafe { pinyin_guess_candidates(instance, 0, 0x1) });
            unsafe { pinyin_choose_candidate(instance, 1, row_of(instance, NORMAL)) };
        },
    );
}

/// A punctuation row of a prediction chooses: true, twice.
#[test]
fn a_predicted_punctuation_row_chooses() {
    let user = TempUserDir::new("choose-punctuation");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, c"好".as_ptr())
    });
    let row = row_of(instance, PREDICTED_PUNCTUATION);
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, row) });
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, row) });
    close(context, instance);
}

/// An NBEST row chosen after a reparse that places no key answers the
/// empty matrix's `size() - 1`, -1 (`pinyin.cpp:2514-2520`,
/// `phonetic_key_matrix.cpp:34-38` at 074a2219), not the consumed length.
#[test]
fn an_nbest_choose_after_a_keyless_reparse_answers_minus_one() {
    let user = TempUserDir::new("keyless-nbest-choose");
    let (context, instance) = open_at(&user.path);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    let row = row_of(instance, NBEST);
    assert_eq!(parse(instance, "'''"), 3);
    assert_eq!(
        unsafe { crate::support::pinyin_choose_candidate(instance, 0, row) },
        -1
    );
    close(context, instance);
}

/// A choose with a caller offset other than the row's `m_begin` forces the
/// row's own span and answers the caller's offset plus that span's length:
/// `guint8 len = add_constraint(m_begin, m_end, token); return offset + len;`
/// (074a2219 `pinyin.cpp:2582-2588`, `phonetic_lookup.cpp:61-86`). The row
/// came from a guess at 2, so its span is `[2, 5)` whatever offset the
/// choose names.
#[test]
fn a_choose_at_another_offset_answers_that_offset_plus_the_row_span() {
    let user = TempUserDir::new("choose-other-offset");
    let (context, instance) = open_at(&user.path);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe { pinyin_guess_candidates(instance, 2, crate::support::DEFAULT_SORT) });
    let row = row_with(instance, NORMAL, "好");
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, row) }, 3);
    // The forcing is the row's `[2, 5)`, not one from the caller's 0.
    assert!(!unsafe { crate::support::pinyin_clear_constraint(instance, 0) });
    assert!(unsafe { crate::support::pinyin_clear_constraint(instance, 2) });
    close(context, instance);
}

/// The span length the pin adds is a `guint8` (`pinyin.cpp:2582`): a row
/// across a separator run longer than 255 bytes answers its length modulo
/// 256.
#[test]
fn a_choose_over_a_long_separator_run_wraps_the_span_length() {
    let user = TempUserDir::new("choose-long-run");
    let (context, instance) = open_at(&user.path);
    let input = format!("ni{}hao", "'".repeat(300));
    assert_eq!(parse(instance, &input), input.len());
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::support::DEFAULT_SORT) });
    let row = row_with(instance, NORMAL, "你好");
    assert_eq!(
        unsafe { pinyin_choose_candidate(instance, 0, row) },
        i32::try_from(input.len() % 256).expect("small")
    );
    close(context, instance);
}

/// Register row 37 for the transformed input schemes (maintainer ruling
/// 2026-10-03): the caller's offsets are in the ORIGINAL input's
/// coordinates and the pin builds every window from `start = offset`
/// (`pinyin.cpp:2224-2262`), so under double pinyin a lookup at 0 behind a
/// chosen 你好 shows the offset-0 window (a lookup that used the cached,
/// composition-anchored list showed none of it), and choosing 你 from it
/// answers the span's end in original coordinates, `0 + len = 2`.
#[test]
fn choosing_behind_the_composition_under_double_pinyin_answers_the_chosen_span() {
    fn row(instance: *mut PinyinInstance, text: &str) -> *mut LookupCandidate {
        let index = capture_rows(instance)
            .iter()
            .position(|(_, row, _)| row == text)
            .unwrap_or_else(|| panic!("{text} is offered"));
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(unsafe {
            pinyin_get_candidate(
                instance,
                u32::try_from(index).expect("small candidate index"),
                &raw mut cand,
            )
        });
        cand
    }
    let user_dir = TempUserDir::new("row37-double-behind-choose");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // Microsoft double pinyin (scheme 2): n+i, then h+ao (`k`).
    assert!(unsafe { crate::support::pinyin_set_double_pinyin_scheme(context, 2) });
    let keys = cstr("nihk");
    assert_eq!(
        unsafe { crate::support::pinyin_parse_more_double_pinyins(instance, keys.as_ptr()) },
        4
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    assert_eq!(
        unsafe { pinyin_choose_candidate(instance, 0, row(instance, "你好")) },
        4,
        "the chosen phrase consumes both keys, in original coordinates"
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    assert_eq!(
        unsafe { pinyin_choose_candidate(instance, 0, row(instance, "你")) },
        2,
        "the pin's offset + len for the key-wide choose behind the composition"
    );
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}
