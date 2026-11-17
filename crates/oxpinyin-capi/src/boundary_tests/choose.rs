//! Choosing a candidate, the candidate-type asserts and removing a user
//! candidate.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

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
        "boundary_tests::choose::choose_candidate_aborts_on_a_predicted_prefix_row",
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
        "boundary_tests::choose::choose_predicted_candidate_aborts_on_a_normal_row",
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
    assert_aborts(
        "boundary_tests::choose::nbest_index_aborts_on_a_normal_row",
        || {
            let user = TempUserDir::new("nbest-normal");
            let (context, _) = open_at(&user.path);
            let instance = guessed_nihao(context);
            let mut index = 0xAB_u8;
            unsafe {
                pinyin_get_candidate_nbest_index(instance, row_of(instance, NORMAL), &raw mut index)
            };
        },
    );
}

/// `remove_user_candidate` asserts on a sentence row.
#[test]
fn remove_user_candidate_aborts_on_a_sentence_row() {
    assert_aborts(
        "boundary_tests::choose::remove_user_candidate_aborts_on_a_sentence_row",
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
        "boundary_tests::choose::remove_user_candidate_aborts_on_a_system_token",
        || {
            let user = TempUserDir::new("remove-system-token");
            let (context, _) = open_at(&user.path);
            let instance = guessed_nihao(context);
            unsafe { pinyin_remove_user_candidate(instance, row_of(instance, NORMAL)) };
        },
    );
}

// Removing a real imported candidate must mutate the retained store even
// after the public handle is gone; no pin call is valid in that state.
#[rstest::rstest]
fn removing_a_user_candidate_keeps_the_instance_owner(#[values(false, true)] finalized: bool) {
    let user = TempUserDir::new("remove-retained-owner");
    let token = saved_user_phrase(&user.path);
    let (instance, candidate) = user_row_of_dada(&user.path);
    let context = unsafe { crate::instance::pinyin_get_context(instance) };
    if finalized {
        crate::test_support::finalize_public_context(context, instance);
    }
    assert!(unsafe { pinyin_remove_user_candidate(instance, candidate) });
    // SAFETY: the instance remains live; the removal's exclusive borrow ended.
    let inst = unsafe { crate::state::instance_ref(instance) };
    assert!(inst.core.user.phrase(token).is_none());
    unsafe { crate::instance::pinyin_free_instance(instance) };
    if !finalized {
        unsafe { crate::context::pinyin_fini(context) };
    }
}

/// `remove_user_candidate` asserts where the user phrase table is gone
///.
#[test]
fn remove_user_candidate_aborts_without_the_phrase_table() {
    assert_aborts(
        "boundary_tests::choose::remove_user_candidate_aborts_without_the_phrase_table",
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
        "boundary_tests::choose::remove_user_candidate_aborts_on_a_residual_bigram_gram",
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
        "boundary_tests::choose::choose_candidate_aborts_at_offset_one_without_a_sentence",
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
        unsafe { crate::candidates::pinyin_choose_candidate(instance, 0, row) },
        -1
    );
    close(context, instance);
}

/// The tokens the instance's selection record holds, which follow its
/// forcings.
fn selected_tokens(instance: *mut PinyinInstance) -> Vec<u32> {
    // SAFETY: the live instance is read between serialized ABI calls.
    unsafe { crate::state::instance_ref(instance) }
        .core
        .session
        .selected_tokens()
        .iter()
        .map(|token| token.value())
        .collect()
}

/// A second choose of the same published row, after an accepted choose and
/// with no guess between, forces the row's own span and token again, as the
/// pin's reads `m_begin`, `m_end` and `m_token` off the row and
/// `add_constraint` clears and rewrites that same run (074a2219
/// `pinyin.cpp:2577-2590`, `phonetic_lookup.cpp:61-86`). It answers
/// `offset + len`, 2; the next sentence and the training are those of a
/// single choose.
#[test]
fn a_repeat_choose_of_an_accepted_row_forces_its_own_span_again() {
    const NI: u32 = 0x0100_1225;
    let start = |tag: &str| {
        let user = TempUserDir::new(tag);
        let (context, instance) = open_at(&user.path);
        assert_eq!(parse(instance, "nihao"), 5);
        assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) });
        let row = row_with(instance, NORMAL, "你");
        assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, row) }, 2);
        (user, context, instance, row)
    };
    let (_once_dir, once_ctx, once, _) = start("repeat-choose-once");
    let (_twice_dir, twice_ctx, twice, row) = start("repeat-choose-twice");
    assert_eq!(unsafe { pinyin_choose_candidate(twice, 0, row) }, 2);
    assert_eq!(selected_tokens(twice), [NI]);
    assert_eq!(selected_tokens(twice), selected_tokens(once));
    assert_eq!(sentence_after_guess(once).as_deref(), Some("你好"));
    assert_eq!(sentence_after_guess(twice).as_deref(), Some("你好"));
    let tokens: Vec<u32> = ["你", "好", "号"]
        .into_iter()
        .flat_map(|text| lookup(twice, text))
        .collect();
    let unigrams = |instance| {
        tokens
            .iter()
            .map(|&token| unigram(instance, token))
            .collect::<Vec<_>>()
    };
    let before = (unigrams(once), unigrams(twice));
    assert_eq!(before.0, before.1);
    assert!(unsafe { crate::candidates::pinyin_train(once, 0) });
    assert!(unsafe { crate::candidates::pinyin_train(twice, 0) });
    assert_ne!(
        unigrams(once),
        before.0,
        "the single choose trains its path"
    );
    assert_eq!(unigrams(twice), unigrams(once));
    close(once_ctx, once);
    close(twice_ctx, twice);
}

/// `pinyin_choose_candidate` on a predicted punctuation row, which carries
/// the pin's default `m_begin = m_end = 0` and `m_token = null_token`
/// (`pinyin.cpp:114`), in the three states the pin defines:
///
/// - While composing, the normal branch writes `add_constraint(0, 0,
///   null_token)` between its two validations and answers `0 + 0`
///   (`pinyin.cpp:2577-2590`). The write is a zero-length null-token
///   ONESTEP at 0 that clearing and validation leave in place: `clear_constraint`
///   walks `[0, 0)` and answers `true` (`phonetic_lookup.cpp:61-86`,
///   `:88-118`). Every later decode stops at 0 (`search_matrix` over
///   `[0, 0)` is no `SEARCH_OK`, `phonetic_lookup.h:776-790`), so a guess
///   answers `true` with no results, `pinyin_get_sentence` answers `false`
///   (`pinyin.cpp:1470`) and `pinyin_train` answers `false` (`:2678`).
/// - After `pinyin_reset`, the same write lands in the empty array's
///   terminator slot, which GLib allocates for a zero-terminated array
///   (2.84.4 `garray.c:349-354`, `:1051-1079`) and zeroes again before any
///   read, and the choose answers 0.
/// - Under `SORT_WITHOUT_SENTENCE_CANDIDATE`, `add_unigram_frequency` of the
///   null token meets the reserved library 0's missing sub-index
///   (`phrase_index.h:628-635`) and the choose answers 1 (`pinyin.cpp:2565-2575`).
#[rstest::rstest]
#[case::composing(crate::test_support::DEFAULT_SORT, false, 0, None, true)]
#[case::after_reset(crate::test_support::DEFAULT_SORT, true, 0, None, false)]
#[case::sort_without_sentence(0x1, false, 1, Some("你好"), false)]
fn a_predicted_punctuation_row_chooses_as_the_pin_does(
    #[case] sort: u32,
    #[case] reset: bool,
    #[case] answer: i32,
    #[case] after: Option<&str>,
    #[case] cleared: bool,
) {
    let user = TempUserDir::new("choose-punctuation-normal");
    let (context, instance) = open_at(&user.path);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, sort) });
    assert_eq!(sentence_after_guess(instance).as_deref(), Some("你好"));
    if reset {
        assert!(unsafe { pinyin_reset(instance) });
    }
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, c"好".as_ptr())
    });
    let row = row_of(instance, PREDICTED_PUNCTUATION);
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, row) }, answer);
    assert_eq!(selected_tokens(instance), Vec::<u32>::new());
    assert_eq!(sentence_after_guess(instance).as_deref(), after);
    assert_eq!(
        unsafe { crate::candidates::pinyin_train(instance, 0) },
        after.is_some()
    );
    assert_eq!(
        unsafe { crate::candidates::pinyin_clear_constraint(instance, 0) },
        cleared
    );
    if !reset && after.is_none() {
        // The cell survives its clear and a reparse. Reset clears it.
        assert_eq!(parse(instance, "nihao"), 5);
        assert_eq!(sentence_after_guess(instance), None);
        assert!(unsafe { pinyin_reset(instance) });
        assert_eq!(parse(instance, "nihao"), 5);
        assert_eq!(sentence_after_guess(instance).as_deref(), Some("你好"));
    }
    close(context, instance);
}

/// A choose of a row that starts at 0 overwrites the null step that a
/// predicted punctuation choose left there, and the sentence comes back.
/// `add_constraint(0, 2, token)` finds nothing to clear in the zero-length
/// run, whose clear walks `[0, 0)`, and writes its ONESTEP over cell 0
/// (074a2219 `phonetic_lookup.cpp:61-86`, `:88-118`). The choose answers
/// `0 + 2` (`pinyin.cpp:2582-2588`) and the next decode runs from that
/// forcing, so the sentence is that of the same choose with no punctuation
/// choose before it. The list the row comes from has no sentence rows,
/// because the blocked guess left no results (`pinyin.cpp:1936-1940`).
#[test]
fn a_choose_at_zero_after_a_predicted_punctuation_choose_brings_the_sentence_back() {
    let choose_first = |instance: *mut PinyinInstance| {
        assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) });
        assert!(
            capture_rows(instance)
                .iter()
                .all(|(kind, _, _)| *kind != NBEST)
        );
        let first = row_with(instance, NORMAL, "你");
        assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, first) }, 2);
        sentence_after_guess(instance)
    };
    let plain_dir = TempUserDir::new("null-step-overwrite-plain");
    let (plain_context, plain) = open_at(&plain_dir.path);
    assert_eq!(parse(plain, "nihao"), 5);
    let expected = choose_first(plain);
    assert_eq!(expected.as_deref(), Some("你好"));

    let user = TempUserDir::new("null-step-overwrite");
    let (context, instance) = open_at(&user.path);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, c"好".as_ptr())
    });
    let punctuation = row_of(instance, PREDICTED_PUNCTUATION);
    assert_eq!(
        unsafe { pinyin_choose_candidate(instance, 0, punctuation) },
        0
    );
    assert_eq!(sentence_after_guess(instance), None);
    assert_eq!(choose_first(instance), expected);
    close(plain_context, plain);
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
    assert!(unsafe { pinyin_guess_candidates(instance, 2, crate::test_support::DEFAULT_SORT) });
    let row = row_with(instance, NORMAL, "好");
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, row) }, 3);
    // The forcing is the row's `[2, 5)`, not one from the caller's 0.
    assert!(!unsafe { crate::candidates::pinyin_clear_constraint(instance, 0) });
    assert!(unsafe { crate::candidates::pinyin_clear_constraint(instance, 2) });
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
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) });
    let row = row_with(instance, NORMAL, "你好");
    assert_eq!(
        unsafe { pinyin_choose_candidate(instance, 0, row) },
        i32::try_from(input.len() % 256).expect("small")
    );
    close(context, instance);
}
