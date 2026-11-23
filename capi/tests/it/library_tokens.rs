//! The library bits of a token: tokens under high bits, two tokens of one
//! library and five libraries that hold one string.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    PinyinContext, TempUserDir, assert_aborts, import_batch, pinyin_get_character_offset,
    pinyin_guess_predicted_candidates, set_phrase_table_row, *,
};

/// Imports `phrase` under `reading` into libraries 1 to 5, so the phrase
/// table search holds five tokens.
///
/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn hold_in_five_libraries(context: *mut PinyinContext, phrase: &str, reading: &str) {
    for library in 1..=5 {
        assert_eq!(
            // SAFETY: `context` meets this function's `# Safety` contract; only this thread calls
            // into it.
            unsafe { import_batch(context, library, &[(phrase, reading, 100_000)]) },
            [true],
            "library {library}"
        );
    }
}

/// An import aborts where the user phrase table maps the phrase to two
/// tokens of one library.
#[test]
fn import_aborts_on_two_tokens_of_one_library() {
    assert_aborts(
        "library_tokens::import_aborts_on_two_tokens_of_one_library",
        || {
            let user = TempUserDir::new("import-duplicate-token");
            let (context, instance) = open_at(&user.path);
            // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above
            // and is not yet freed; nothing uses either handle after this call; only this thread
            // calls into them.
            unsafe { close(context, instance) };
            set_phrase_table_row(&user.path, "你好", &[0x0700_0001, 0x0700_0002]);
            let (context, _) = open_at(&user.path);
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { import_batch(context, 7, &[("你好", "ni'hao", 5)]) };
        },
    );
}

/// A reading of the wrong length is refused before the five-token search.
#[test]
fn an_import_refuses_a_short_reading_before_the_token_count() {
    let user = TempUserDir::new("combined-invalid-reading");
    let (context, instance) = open_at(&user.path);
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    unsafe { hold_in_five_libraries(context, "你好", "ni3'hao3") };
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { import_batch(context, 6, &[("你好", "ni3", 100_000)]) },
        [false]
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The prefix guess aborts on a prefix of five tokens (`compute_prefixes`).
#[test]
fn the_prefix_guess_aborts_on_five_tokens() {
    assert_aborts(
        "library_tokens::the_prefix_guess_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-prefix-guess");
            let (context, instance) = open_at(&user.path);
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { hold_in_five_libraries(context, "你好", "ni3'hao3") };
            unsafe {
                crate::support::pinyin_guess_sentence_with_prefix(instance, c"你好".as_ptr())
            };
        },
    );
}

/// The prediction aborts on a prefix of five tokens.
#[test]
fn the_prediction_aborts_on_five_tokens() {
    assert_aborts(
        "library_tokens::the_prediction_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-prediction");
            let (context, instance) = open_at(&user.path);
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { hold_in_five_libraries(context, "你好", "ni3'hao3") };
            unsafe { pinyin_guess_predicted_candidates(instance, c"你好".as_ptr()) };
        },
    );
}

/// The character offset aborts on a phrase of five tokens.
#[test]
fn the_character_offset_aborts_on_five_tokens() {
    assert_aborts(
        "library_tokens::the_character_offset_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-character-offset");
            let (context, instance) = open_at(&user.path);
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { hold_in_five_libraries(context, "你", "ni3") };
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            assert_eq!(unsafe { parse(instance, "ni") }, 2);
            let mut length = 0_usize;
            unsafe { pinyin_get_character_offset(instance, c"你".as_ptr(), 0, &raw mut length) };
        },
    );
}

/// `remember_user_input` aborts on a phrase of five tokens.
#[test]
fn remember_aborts_on_five_tokens() {
    assert_aborts("library_tokens::remember_aborts_on_five_tokens", || {
        let user = TempUserDir::new("combined-remember");
        let (context, instance) = open_at(&user.path);
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { hold_in_five_libraries(context, "你", "ni3") };
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { parse(instance, "ni") }, 2);
        unsafe { crate::support::pinyin_remember_user_input(instance, c"你".as_ptr(), 1) };
    });
}
