//! Choosing a candidate, the candidate-type asserts and removing a user
//! candidate.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

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
