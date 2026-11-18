#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::config::pinyin_set_options;

use crate::test_support::with_store;

use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
    pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_has_next_phrase,
};

use crate::parse::pinyin_parse_more_full_pinyins;

use crate::test_support::{TempUserDir, cstr, open};

use crate::types::PinyinInstance;

use crate::user_data::pinyin_remember_user_input;

use oxpinyin::user::{FIRST_USER_TOKEN, UserStore};

/// Imports `phrase` into library 7 once per reading and returns its token.
///
/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// `instance` is a live, non-NULL instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn import_readings(
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
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    (unsafe { with_store(instance, |store| store.token_for_phrase(phrase)) })
        .expect("readable store")
        .expect("the import gave the phrase a token")
}

/// The stored readings of `token` as `(packed first key, frequency)`.
///
/// # Safety
///
/// `instance` is a live, non-NULL instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn stored_readings(instance: *mut PinyinInstance, token: u32) -> Vec<(u16, u32)> {
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    (unsafe { with_store(instance, |store| store.item_pronunciations(token)) })
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

    let ni3 = oxpinyin::ChewingKey::from_pinyin("ni")
        .expect("ni is a key")
        .with_tone(3)
        .to_packed();
    for (input, remembered) in [("ni3", true), ("ni2", false)] {
        let user_dir = TempUserDir::new("remember-double-tone");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; only this thread calls into them.
        let token = unsafe { import_readings(context, instance, "农", &["ni3"]) };
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
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { stored_readings(instance, token) },
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
        assert_eq!(
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { with_store(instance, next_user_token) },
            FIRST_USER_TOKEN
        );
        let export = unsafe { pinyin_begin_get_phrases(context, 7) };
        assert!(!export.is_null());
        assert!(!unsafe { pinyin_iterator_has_next_phrase(export) });
        unsafe { pinyin_end_get_phrases(export) };
    }

    // Known characters remain eligible.
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("你好").as_ptr(), 1) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { with_store(instance, |store| store.token_for_phrase("你好")) }.unwrap(),
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

    for length in [0, oxpinyin::user::MAX_PHRASE_LENGTH, 65_536] {
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
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { with_store(instance, next_user_token) },
        FIRST_USER_TOKEN
    );
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

/// The preflight runs only where `_remember_phrase_recur` reaches
/// `_add_phrase`. With the crafted 你好 row and only `ni` parsed, the one
/// path has one key for a two-character phrase and is refused at
/// `pinyin.cpp:3600-3601`, so the call answers `false`.
#[test]
fn remember_user_input_refuses_a_short_path_before_the_preflight() {
    let user_dir = TempUserDir::new("remember-short-path");
    crate::test_support::set_phrase_table_row(&user_dir.path, "你好", &[0x0700_0001, 0x0700_0002]);
    let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("ni").as_ptr()) },
        2
    );
    assert!(!unsafe { pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1) });
}

/// A key the next character's item does not pronounce ends its branch
/// (`pinyin.cpp:3640-3645`): 好 does not pronounce `ni`, so 好你 reaches no
/// path under `nihao`, whatever its crafted row holds.
#[test]
fn remember_user_input_refuses_a_path_the_item_does_not_pronounce() {
    let user_dir = TempUserDir::new("remember-unpronounced");
    crate::test_support::set_phrase_table_row(&user_dir.path, "好你", &[0x0700_0001, 0x0700_0002]);
    let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()) },
        5
    );
    assert!(!unsafe { pinyin_remember_user_input(instance, cstr("好你").as_ptr(), -1) });
}

/// 弄 with its `nong` and `long` readings, parsed as `ns` in Microsoft
/// double pinyin under `L_N`. Returns the readings before any remember.
fn parsed_nong_under_l_n(
    user_dir: &TempUserDir,
) -> (
    *mut crate::types::PinyinContext,
    *mut PinyinInstance,
    u32,
    Vec<(u16, u32)>,
) {
    use crate::config::pinyin_set_double_pinyin_scheme;
    use crate::parse::pinyin_parse_more_double_pinyins;

    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; only this thread calls into them.
    let token = unsafe { import_readings(context, instance, "弄", &["nong", "long"]) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let before = unsafe { stored_readings(instance, token) };
    assert_eq!(before.len(), 2);
    assert!(unsafe { pinyin_set_options(context, 0x8 | (1 << 15)) });
    assert!(unsafe { pinyin_set_double_pinyin_scheme(context, 2) });
    assert_eq!(
        unsafe { pinyin_parse_more_double_pinyins(instance, cstr("ns").as_ptr()) },
        2
    );
    (context, instance, token, before)
}

/// Both readings' frequencies rose by the count of 3.
///
/// # Safety
///
/// `instance` is a live, non-NULL instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn assert_both_readings_rose(
    instance: *mut PinyinInstance,
    token: u32,
    before: &[(u16, u32)],
) {
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    let after = unsafe { stored_readings(instance, token) };
    assert_eq!(after.len(), 2);
    for ((_, was), (_, now)) in before.iter().zip(&after) {
        assert_eq!(*now, *was + 3);
    }
}

/// The matrix comes from the parse, not from the options now: turning `L_N`
/// off after the parse leaves `long` in it.
#[test]
fn remember_user_input_keeps_the_parse_time_fuzzy_alternatives() {
    let user_dir = TempUserDir::new("remember-parse-time-fuzzy");
    let (context, instance, token, before) = parsed_nong_under_l_n(&user_dir);
    assert!(unsafe { pinyin_set_options(context, 0x8) });
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("弄").as_ptr(), 3) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    unsafe { assert_both_readings_rose(instance, token, &before) };
}

/// `_remember_phrase_recur` calls `_add_phrase` for every complete path and
/// ORs the answers (`pinyin.cpp:3654-3659`). With `L_N` the composition
/// `nong` holds two keys over one span, `nong` and `long`, and a character
/// whose item pronounces both is added along both paths: each reading's
/// frequency rises by the count.
#[test]
fn remember_user_input_adds_along_every_complete_path() {
    let user_dir = TempUserDir::new("remember-two-paths");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let import = unsafe { pinyin_begin_add_phrases(context, 7) };
    for reading in ["nong", "long"] {
        assert!(unsafe {
            pinyin_iterator_add_phrase(import, cstr("弄").as_ptr(), cstr(reading).as_ptr(), 1)
        });
    }
    unsafe { pinyin_end_add_phrases(import) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { with_store(instance, |store| store.token_for_phrase("弄")) }
        .expect("readable store")
        .expect("the import gave 弄 a token");
    let readings = |instance| {
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        (unsafe { with_store(instance, |store| store.item_pronunciations(token)) })
            .expect("readable store")
            .expect("an item")
            .iter()
            .map(|(keys, freq)| (keys.len(), *freq))
            .collect::<Vec<_>>()
    };
    let before = readings(instance);
    assert_eq!(before.len(), 2, "two imported readings");

    assert!(unsafe { pinyin_set_options(context, 0x8 | (1 << 15)) });
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("nong").as_ptr()) },
        4
    );
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("弄").as_ptr(), 3) });
    let after = readings(instance);
    assert_eq!(after.len(), before.len());
    for ((_, was), (_, now)) in before.iter().zip(&after) {
        assert_eq!(*now, *was + 3, "each path adds its reading once");
    }
}

/// Every complete path calls _add_phrase, including repeated keys
/// (pinyin.cpp:3594-3606). Repeated paths therefore repeat the count write.
#[test]
fn remember_user_input_writes_again_for_an_identical_complete_path() {
    let user = TempUserDir::new("remember-identical-paths");
    let (context, instance, token, before) = parsed_nong_under_l_n(&user);
    // This fixture adds an identical column entry to the parsed matrix,
    // as PhoneticTable::append does (phonetic_key_matrix.h:92-100).
    let duplicate = {
        // SAFETY: the live instance is exclusively borrowed before remember.
        let mut inst = unsafe { crate::state::instance_mut(instance) };
        let first = inst.core.parse_matrix[0];
        inst.core.parse_matrix.push(first);
        first.key().to_packed()
    };
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("弄").as_ptr(), 3) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let after = unsafe { stored_readings(instance, token) };
    for ((key, was), (after_key, now)) in before.iter().zip(&after) {
        assert_eq!(key, after_key);
        assert_eq!(*now, *was + if *key == duplicate { 6 } else { 3 });
    }
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}
