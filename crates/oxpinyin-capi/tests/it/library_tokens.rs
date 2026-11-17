//! The library bits of a token: tokens under high bits, two tokens of one
//! library and five libraries that hold one string.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    PinyinContext, TempUserDir, assert_aborts, cstr, import_batch, open, pinyin_begin_add_phrases,
    pinyin_end_add_phrases, pinyin_get_character_offset, pinyin_guess_predicted_candidates,
    pinyin_iterator_add_phrase, set_phrase_table_row, *,
};

use std::ptr;

/// Imports `phrase` under `reading` into libraries 1 to 5, so the phrase
/// table search holds five tokens.
fn hold_in_five_libraries(context: *mut PinyinContext, phrase: &str, reading: &str) {
    for library in 1..=5 {
        assert_eq!(
            import_batch(context, library, &[(phrase, reading, 100_000)]),
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
            close(context, instance);
            set_phrase_table_row(&user.path, "你好", &[0x0700_0001, 0x0700_0002]);
            let (context, _) = open_at(&user.path);
            import_batch(context, 7, &[("你好", "ni'hao", 5)]);
        },
    );
}

/// A reading of the wrong length is refused before the five-token search.
#[test]
fn an_import_refuses_a_short_reading_before_the_token_count() {
    let user = TempUserDir::new("combined-invalid-reading");
    let (context, instance) = open_at(&user.path);
    hold_in_five_libraries(context, "你好", "ni3'hao3");
    assert_eq!(
        import_batch(context, 6, &[("你好", "ni3", 100_000)]),
        [false]
    );
    close(context, instance);
}

/// The prefix guess aborts on a prefix of five tokens (`compute_prefixes`).
#[test]
fn the_prefix_guess_aborts_on_five_tokens() {
    assert_aborts(
        "library_tokens::the_prefix_guess_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-prefix-guess");
            let (context, instance) = open_at(&user.path);
            hold_in_five_libraries(context, "你好", "ni3'hao3");
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
            hold_in_five_libraries(context, "你好", "ni3'hao3");
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
            hold_in_five_libraries(context, "你", "ni3");
            assert_eq!(parse(instance, "ni"), 2);
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
        hold_in_five_libraries(context, "你", "ni3");
        assert_eq!(parse(instance, "ni"), 2);
        unsafe { crate::support::pinyin_remember_user_input(instance, c"你".as_ptr(), 1) };
    });
}

/// Imports 你好 into libraries `1..=libraries` of the fixture, which loads
/// libraries 1 to 7, so the phrase-table search holds `libraries` tokens.
fn hold_nihao_in_libraries(context: *mut crate::support::PinyinContext, libraries: u8) {
    for library in 1..=libraries {
        let iter = unsafe { pinyin_begin_add_phrases(context, library) };
        assert!(unsafe {
            pinyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("ni3'hao3").as_ptr(), 1)
        });
        unsafe { pinyin_end_add_phrases(iter) };
    }
}

/// Four tokens pass `reduce_tokens`' count, so the NULL-array lookup
/// answers instead of aborting.
#[test]
fn lookup_tokens_into_a_null_array_answers_with_four_tokens() {
    let user_dir = TempUserDir::new("lookup-null-four");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    hold_nihao_in_libraries(context, 4);
    assert!(unsafe {
        crate::support::pinyin_lookup_tokens(instance, cstr("你好").as_ptr(), ptr::null_mut())
    });
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}
