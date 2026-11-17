#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::config::pinyin_set_options;

use crate::e2e_tests::with_store;

use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
    pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_has_next_phrase,
};

use crate::parse::pinyin_parse_more_full_pinyins;

use crate::test_support::{TempUserDir, cstr, open};

use crate::types::PinyinInstance;

use crate::user_data::pinyin_remember_user_input;

use oxpinyin_user::{FIRST_USER_TOKEN, UserStore};

/// Imports `phrase` into library 7 once per reading and returns its token.
fn import_readings(
    context: *mut crate::types::PinyinContext,
    instance: *mut PinyinInstance,
    phrase: &str,
    readings: &[&str],
) -> u32 {
    let import = unsafe { pinyin_begin_add_phrases(context, 7) };
    for reading in readings {
        assert!(unsafe {
            pinyin_iterator_add_phrase(import, cstr(phrase).as_ptr(), cstr(reading).as_ptr(), 1)
        });
    }
    unsafe { pinyin_end_add_phrases(import) };
    with_store(instance, |store| store.token_for_phrase(phrase))
        .expect("readable store")
        .expect("the import gave the phrase a token")
}

/// The stored readings of `token` as `(packed first key, frequency)`.
fn stored_readings(instance: *mut PinyinInstance, token: u32) -> Vec<(u16, u32)> {
    with_store(instance, |store| store.item_pronunciations(token))
        .expect("readable store")
        .expect("an item")
        .iter()
        .map(|(keys, freq)| (keys[0].to_packed(), *freq))
        .collect()
}

/// The remember walk reads the matrix the parse built. Double pinyin keeps
/// the parsed tone (`ni3` is the reading `ni3`), and a different tone finds no
/// path.
#[test]
fn remember_user_input_keeps_the_double_pinyin_tone() {
    use crate::config::pinyin_set_double_pinyin_scheme;
    use crate::parse::pinyin_parse_more_double_pinyins;

    let ni3 = oxpinyin_core::ChewingKey::from_pinyin("ni")
        .expect("ni is a key")
        .with_tone(3)
        .to_packed();
    for (input, remembered) in [("ni3", true), ("ni2", false)] {
        let user_dir = TempUserDir::new("remember-double-tone");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        let token = import_readings(context, instance, "农", &["ni3"]);
        assert!(unsafe { pinyin_set_options(context, 0x28) });
        assert!(unsafe { pinyin_set_double_pinyin_scheme(context, 2) });
        assert_eq!(
            unsafe { pinyin_parse_more_double_pinyins(instance, cstr(input).as_ptr()) },
            input.len()
        );
        assert_eq!(
            unsafe { pinyin_remember_user_input(instance, cstr("农").as_ptr(), 3) },
            remembered,
            "{input}"
        );
        let expected = if remembered { 1 + 3 } else { 1 };
        assert_eq!(
            stored_readings(instance, token),
            [(ni3, expected)],
            "{input}"
        );
    }
}

fn next_user_token(store: &UserStore) -> u32 {
    store
        .phrases()
        .iter()
        .map(|phrase| phrase.token())
        .max()
        .map_or(FIRST_USER_TOKEN, |token| token + 1)
}

/// #752: the pin's `_pre_compute_tokens` refuses a phrase if any character
/// has no dictionary token (074a2219 pinyin.cpp:3098-3130, 3685-3695).
/// NUL termination is covered differentially in contract-diff.py.
#[test]
fn remember_user_input_requires_known_characters() {
    let user_dir = TempUserDir::new("remember-known");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(unsafe { pinyin_set_options(context, 0x28) });
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()) },
        5
    );

    for phrase in [b"ni\0".as_slice(), "你i\0".as_bytes(), "😀好\0".as_bytes()] {
        assert!(!unsafe { pinyin_remember_user_input(instance, phrase.as_ptr().cast(), 1) });
        assert_eq!(with_store(instance, next_user_token), FIRST_USER_TOKEN);
        let export = unsafe { pinyin_begin_get_phrases(context, 7) };
        assert!(!export.is_null());
        assert!(!unsafe { pinyin_iterator_has_next_phrase(export) });
        unsafe { pinyin_end_get_phrases(export) };
    }

    // Known characters remain eligible.
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("你好").as_ptr(), 1) });
    assert_eq!(
        with_store(instance, |store| store.token_for_phrase("你好")).unwrap(),
        Some(FIRST_USER_TOKEN)
    );

    // Eligibility comes from the phrase table, including user tokens:
    // importing the previously unknown characters makes `ni` eligible.
    let import = unsafe { pinyin_begin_add_phrases(context, 7) };
    assert!(!import.is_null());
    for (character, reading) in [("n", "ni3"), ("i", "hao3")] {
        assert!(unsafe {
            pinyin_iterator_add_phrase(import, cstr(character).as_ptr(), cstr(reading).as_ptr(), 1)
        });
    }
    unsafe { pinyin_end_add_phrases(import) };
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("ni").as_ptr(), 1) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// The pin refuses an empty or oversized phrase before token lookup
/// (074a2219 pinyin.cpp:3677-3681). Invalid UTF-8 also has length zero.
#[test]
fn remember_user_input_rejects_empty_and_oversized_known_phrases() {
    let user_dir = TempUserDir::new("remember-length-gate");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(unsafe { pinyin_set_options(context, 0x28) });
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("ni").as_ptr()) },
        2
    );

    for length in [0, oxpinyin_user::MAX_PHRASE_LENGTH, 65_536] {
        assert!(!unsafe {
            pinyin_remember_user_input(instance, cstr("你".repeat(length)).as_ptr(), 1)
        });
    }
    for invalid in [
        b"\xff\0".as_slice(),
        b"\xe4\xbd\xa0\xff\0".as_slice(),
        b"\xe4\xbd\0".as_slice(),
        b"\xc0\x80\0".as_slice(),
    ] {
        assert!(!unsafe { pinyin_remember_user_input(instance, invalid.as_ptr().cast(), 1) });
    }
    assert_eq!(with_store(instance, next_user_token), FIRST_USER_TOKEN);
    let export = unsafe { pinyin_begin_get_phrases(context, 7) };
    assert!(!export.is_null());
    assert!(!unsafe { pinyin_iterator_has_next_phrase(export) });
    unsafe { pinyin_end_get_phrases(export) };

    // The repeated character is eligible; the oversized probes above do
    // not depend on an unknown-character rejection.
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("你").as_ptr(), 1) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}
