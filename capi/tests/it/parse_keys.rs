//! Parsing, the key and offset getters, the auxiliary texts and the
//! keyboard wrapper.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{PARITY, parsed_key_and_rest};

use crate::support::{
    ChewingKey, ChewingKeyRest, DEFAULT_SORT, GChar, PinyinInstance, TempUserDir, assert_aborts,
    cstr, key_slot_guard, open, pinyin_fini, pinyin_free_instance, pinyin_get_character_offset,
    pinyin_get_chewing_auxiliary_text, pinyin_get_double_pinyin_auxiliary_text,
    pinyin_get_full_pinyin_auxiliary_text, pinyin_get_left_pinyin_offset, pinyin_get_n_candidate,
    pinyin_get_parsed_input_length, pinyin_get_pinyin_key, pinyin_get_pinyin_key_rest,
    pinyin_get_pinyin_key_rest_positions, pinyin_get_pinyin_string, pinyin_get_right_pinyin_offset,
    pinyin_guess_candidates, pinyin_guess_sentence, pinyin_parse_full_pinyin,
    pinyin_parse_more_chewings, pinyin_parse_more_full_pinyins, pinyin_set_options, *,
};

use rstest::rstest;

use std::ffi::c_char;

use std::ptr;

/// The symbols `in_chewing_keyboard` answers for `key`, freed.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn keyboard(instance: *mut PinyinInstance, key: u8) -> (bool, Option<Vec<String>>) {
    let mut symbols: *mut *mut c_char = UNTOUCHED as usize as *mut *mut c_char;
    let ret = unsafe {
        crate::support::pinyin_in_chewing_keyboard(
            instance,
            c_char::from_ne_bytes([key]),
            &raw mut symbols,
        )
    };
    if symbols.is_null() {
        return (ret, None);
    }
    assert!(
        symbols as usize != UNTOUCHED as usize,
        "the out-parameter is written"
    );
    let mut texts = Vec::new();
    // SAFETY: a NULL-terminated vector of GLib strings the caller owns.
    unsafe {
        let mut at = symbols;
        while !(*at).is_null() {
            texts.push(std::ffi::CStr::from_ptr(*at).to_string_lossy().into_owned());
            at = at.add(1);
        }
        glib_sys::g_strfreev(symbols);
    }
    (ret, Some(texts))
}

/// The luoma and secondary-zhuyin renderers answer false on the zero key
/// of a column and write NULL to the out-parameter.
#[test]
fn luoma_and_secondary_zhuyin_refuse_the_zero_key() {
    let _slots = crate::support::key_slot_guard();
    let user = TempUserDir::new("zero-key-renderers");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, 0x1aa) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "xi'") }, 3);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    let mut key: *mut ChewingKey = ptr::null_mut();
    assert!(unsafe { pinyin_get_pinyin_key(instance, 2, &raw mut key) });
    for render in [
        crate::support::pinyin_get_luoma_pinyin_string,
        crate::support::pinyin_get_secondary_zhuyin_string,
    ] {
        let mut out = UNTOUCHED as usize as *mut c_char;
        assert!(!unsafe { render(instance, key, &raw mut out) });
        assert!(out.is_null());
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// Init seeds USE_TONE alone: a toned key parses, an initial does not
/// (no PINYIN_INCOMPLETE).
#[test]
fn init_options_parse_tones_but_not_initials() {
    let user = TempUserDir::new("default-options");
    let (context, _) = open_at(&user.path);
    for (text, parsed) in [("n", 0), ("ni3", 3), ("zzz", 0), ("ni2hao", 6)] {
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { alloc(context) };
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { parse(instance, text) }, parsed, "{text}");
        unsafe { pinyin_free_instance(instance) };
    }
    // The single-key parser reads the same word (`pinyin.cpp:1484-1495`).
    // It clears the key first, takes a last character of `1` to `5` as the
    // tone under `USE_TONE`, and refuses a spelling the index lacks: an
    // initial alone needs `PINYIN_INCOMPLETE` (`pinyin_parser2.cpp:174-213`).
    // The key word is `m_initial:5`, `m_middle:2`, `m_final:5`, `m_tone:3`
    // from bit 0 (`chewing_key.h:43-46`), with `CHEWING_N` 11, `CHEWING_ZH`
    // 23, `CHEWING_I` 1, `CHEWING_A` 1 and `CHEWING_ANG` 4
    // (`chewing_enum.h`).
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    for (text, parsed, word) in [
        (c"n", false, 0),
        (c"zzz", false, 0),
        (c"ni", true, 11 | 1 << 5),
        (c"ni3", true, 11 | 1 << 5 | 3 << 12),
        (c"a1", true, 1 << 7 | 1 << 12),
        (c"zhang", true, 23 | 4 << 7),
    ] {
        let mut key = 0xCDEF_u16;
        assert_eq!(
            unsafe { pinyin_parse_full_pinyin(instance, text.as_ptr(), (&raw mut key).cast()) },
            parsed,
            "{text:?}"
        );
        assert_eq!(key, word, "{text:?}");
    }
    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_fini(context) };
}

/// `parse_full_pinyin` asserts on an apostrophe after bytes that are not
/// UTF-8 (#1279).
#[test]
fn full_key_parse_aborts_on_an_apostrophe_after_invalid_bytes() {
    assert_aborts(
        "parse_keys::full_key_parse_aborts_on_an_apostrophe_after_invalid_bytes",
        || {
            let user = TempUserDir::new("full-key-invalid");
            let (_context, instance) = open_at(&user.path);
            let mut key = 0xCDEF_u16;
            unsafe {
                pinyin_parse_full_pinyin(instance, c"ni\xff'".as_ptr(), (&raw mut key).cast())
            };
        },
    );
}

/// The full-pinyin auxiliary text asserts on a leading apostrophe.
#[test]
fn full_auxiliary_text_aborts_on_a_leading_apostrophe() {
    assert_aborts(
        "parse_keys::full_auxiliary_text_aborts_on_a_leading_apostrophe",
        || {
            let user = TempUserDir::new("aux-leading-apostrophe");
            let (_context, instance) = open_at(&user.path);
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            assert_eq!(unsafe { parse(instance, "'nihao") }, 6);
            let mut render_aux: *mut c_char = ptr::null_mut();
            unsafe { pinyin_get_full_pinyin_auxiliary_text(instance, 2, &raw mut render_aux) };
        },
    );
}

/// The double-pinyin auxiliary text asserts on a cut more than two bytes
/// into a key.
#[test]
fn double_auxiliary_text_aborts_three_bytes_into_a_key() {
    assert_aborts(
        "parse_keys::double_auxiliary_text_aborts_three_bytes_into_a_key",
        || {
            let user = TempUserDir::new("aux-double-cut");
            let (_context, instance) = open_at(&user.path);
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            assert_eq!(unsafe { parse(instance, "zhong") }, 5);
            let mut render_aux: *mut c_char = ptr::null_mut();
            unsafe { pinyin_get_double_pinyin_auxiliary_text(instance, 3, &raw mut render_aux) };
        },
    );
}

/// The double-pinyin and chewing auxiliary texts share the prefix walk of
/// the full-pinyin one (`pinyin.cpp:3450`, `:3528`). After a leading
/// apostrophe no key begins at column 0 and `fill_matrix` puts a zero key
/// only between two keys (`phonetic_key_matrix.cpp:70-80`), so the column is
/// empty and `assert(matrix.get_column_size(offset) >= 1)` fails
/// (`pinyin.cpp:3311`).
#[test]
fn double_auxiliary_text_aborts_on_a_leading_apostrophe() {
    assert_aborts(
        "parse_keys::double_auxiliary_text_aborts_on_a_leading_apostrophe",
        || {
            let user = TempUserDir::new("aux-double-leading-apostrophe");
            let (_context, instance) = open_at(&user.path);
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            assert_eq!(unsafe { parse(instance, "'nihao") }, 6);
            let mut render_aux: *mut c_char = ptr::null_mut();
            unsafe { pinyin_get_double_pinyin_auxiliary_text(instance, 2, &raw mut render_aux) };
        },
    );
}

/// The chewing twin of
/// [`double_auxiliary_text_aborts_on_a_leading_apostrophe`].
#[test]
fn chewing_auxiliary_text_aborts_on_a_leading_apostrophe() {
    assert_aborts(
        "parse_keys::chewing_auxiliary_text_aborts_on_a_leading_apostrophe",
        || {
            let user = TempUserDir::new("aux-chewing-leading-apostrophe");
            let (_context, instance) = open_at(&user.path);
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            assert_eq!(unsafe { parse(instance, "'nihao") }, 6);
            let mut render_aux: *mut c_char = ptr::null_mut();
            unsafe {
                crate::support::pinyin_get_chewing_auxiliary_text(instance, 2, &raw mut render_aux)
            };
        },
    );
}

/// `get_pinyin_key` asserts on an empty matrix: `matrix.size() - 1` wraps,
/// the offset test passes and `get_column_size(0)` asserts
/// (`pinyin.cpp:2927-2930`, `phonetic_key_matrix.h:103`).
#[test]
fn key_aborts_on_an_empty_matrix() {
    assert_aborts("parse_keys::key_aborts_on_an_empty_matrix", || {
        let user = TempUserDir::new("key-empty");
        let (_context, instance) = open_at(&user.path);
        let mut key = ptr::null_mut();
        unsafe { crate::support::pinyin_get_pinyin_key(instance, 0, &raw mut key) };
    });
}

/// `get_pinyin_key_rest` asserts on an empty matrix.
#[test]
fn key_rest_aborts_on_an_empty_matrix() {
    assert_aborts("parse_keys::key_rest_aborts_on_an_empty_matrix", || {
        let user = TempUserDir::new("key-rest-empty");
        let (_context, instance) = open_at(&user.path);
        let mut rest = ptr::null_mut();
        unsafe { pinyin_get_pinyin_key_rest(instance, 0, &raw mut rest) };
    });
}

/// The left offset asserts on the zero key after a separator.
#[test]
fn left_offset_aborts_after_a_separator() {
    assert_aborts("parse_keys::left_offset_aborts_after_a_separator", || {
        let user = TempUserDir::new("left-after-separator");
        let (_context, instance) = open_at(&user.path);
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { parse(instance, "ni'hao") }, 6);
        let mut left = 0_usize;
        unsafe { pinyin_get_left_pinyin_offset(instance, 3, &raw mut left) };
    });
}

/// The right offset asserts on the zero key after a separator.
#[test]
fn right_offset_aborts_after_a_separator() {
    assert_aborts("parse_keys::right_offset_aborts_after_a_separator", || {
        let user = TempUserDir::new("right-after-separator");
        let (_context, instance) = open_at(&user.path);
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { parse(instance, "ni'hao") }, 6);
        let mut right = 0_usize;
        unsafe { pinyin_get_right_pinyin_offset(instance, 3, &raw mut right) };
    });
}

/// The character offset asserts on the empty leading column of a
/// leading separator (`PinAssert`).
#[test]
fn character_offset_aborts_on_an_empty_leading_column() {
    assert_aborts(
        "parse_keys::character_offset_aborts_on_an_empty_leading_column",
        || {
            let user = TempUserDir::new("character-leading-separator");
            let (context, instance) = open_at(&user.path);
            assert!(unsafe { pinyin_set_options(context, SEPARATOR_OPTIONS) });
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            assert_eq!(unsafe { parse(instance, "'ni") }, 3);
            unsafe { pinyin_guess_sentence(instance) };
            let mut length = 0_usize;
            unsafe { pinyin_get_character_offset(instance, c"你".as_ptr(), 0, &raw mut length) };
        },
    );
}

/// The right offset at the first of two leading separators answers false
/// and leaves the out-parameter alone.
#[test]
fn right_offset_at_a_leading_separator_is_refused() {
    let user = TempUserDir::new("right-leading-separator");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, SEPARATOR_OPTIONS) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "''ni") }, 4);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    let mut right = UNTOUCHED as usize;
    assert!(!unsafe { pinyin_get_right_pinyin_offset(instance, 0, &raw mut right) });
    assert_eq!(right, UNTOUCHED as usize);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// Invalid bytes in pinyin input: an import reading keeps its parsed
/// prefix, an import text is refused, and the double and chewing parsers
/// stop at the byte.
#[test]
fn invalid_input_bytes_stop_each_pinyin_parser() {
    let user = TempUserDir::new("invalid-bytes");
    let (context, instance) = open_at(&user.path);
    let iter = unsafe { crate::support::pinyin_begin_add_phrases(context, 7) };
    let add = |text: &[u8], reading: &[u8]| {
        let text = std::ffi::CString::new(text).expect("no NUL");
        let reading = std::ffi::CString::new(reading).expect("no NUL");
        unsafe {
            crate::support::pinyin_iterator_add_phrase(iter, text.as_ptr(), reading.as_ptr(), 1)
        }
    };
    assert!(add("你好".as_bytes(), b"ni'ha\xffo"));
    assert!(add("世界".as_bytes(), b"shi'jie\xff"));
    assert!(!add(b"\xe4\xbd\xa0\xe5\xa5\xbd\xff", b"ni'hao"));
    unsafe { crate::support::pinyin_end_add_phrases(iter) };
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { crate::support::export_of(context, 7) },
        [
            ("你好".to_owned(), "ni'ha".to_owned(), 1),
            ("世界".to_owned(), "shi'jie".to_owned(), 1),
        ]
    );
    let consumed = |parser: unsafe extern "C" fn(*mut PinyinInstance, *const c_char) -> usize,
                    text: &[u8]| {
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let fresh = unsafe { alloc(context) };
        let text = std::ffi::CString::new(text).expect("no NUL");
        let parsed = unsafe { parser(fresh, text.as_ptr()) };
        unsafe { pinyin_free_instance(fresh) };
        parsed
    };
    let double = crate::support::pinyin_parse_more_double_pinyins;
    assert_eq!(consumed(double, b"ni\xffhk"), 2);
    assert_eq!(consumed(double, b"nihk\xff"), 4);
    assert_eq!(consumed(double, b"\xffnihk"), 0);
    let chewings = crate::support::pinyin_parse_more_chewings;
    assert_eq!(consumed(chewings, b"su\xff3"), 2);
    assert_eq!(consumed(chewings, b"su3\xff"), 3);
    assert_eq!(consumed(chewings, b"\xffsu3"), 0);
    // The single-key parser hands the whole argument to the index search,
    // which finds no such spelling: `false`, with the key cleared first
    // (`pinyin.cpp:1491-1494`, `pinyin_parser2.cpp:174`, `:196-200`).
    let mut key = 0xCDEF_u16;
    assert!(!unsafe {
        pinyin_parse_full_pinyin(instance, c"ni\xff".as_ptr(), (&raw mut key).cast())
    });
    assert_eq!(key, 0);
    // `pinyin_lookup_tokens` searches the `g_utf8_to_ucs4` result, which is
    // NULL with length 0 for a text that is not UTF-8 (`pinyin.cpp:2657-2667`).
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let tokens = unsafe { glib_sys::g_array_new(0, 0, 4) };
    assert!(!unsafe {
        crate::support::pinyin_lookup_tokens(instance, c"\xe4\xbd\xa0\xff".as_ptr(), tokens.cast())
    });
    // SAFETY: `tokens` is the live GArray allocated above.
    assert_eq!(unsafe { (*tokens).len }, 0);
    // SAFETY: allocated above and freed once.
    unsafe { glib_sys::g_array_free(tokens, 1) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { lookup(instance, "你") }.len(), 1);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The pinyin wrapper of `in_chewing_keyboard`: a key of the
/// scheme answers its symbol, a tone key needs USE_TONE, and other keys
/// answer false with NULL.
#[test]
fn the_pinyin_chewing_keyboard_wrapper_answers_per_key() {
    let user = TempUserDir::new("chewing-keyboard");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, 0) });
    assert!(unsafe { crate::support::pinyin_set_zhuyin_scheme(context, 1) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { keyboard(instance, b's') },
        (true, Some(vec!["ㄋ".to_owned()]))
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { keyboard(instance, b'3') }, (false, None));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { keyboard(instance, b'!') }, (false, None));
    assert!(unsafe { pinyin_set_options(context, 0x60) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { keyboard(instance, b'3') },
        (true, Some(vec!["ˇ".to_owned()]))
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

const USE_TONE: u32 = 0x20;

const FORCE_TONE: u32 = 0x40;

/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn parse_len(
    context: *mut crate::support::PinyinContext,
    instance: *mut crate::support::PinyinInstance,
    word: u32,
    input: &str,
) -> usize {
    assert!(unsafe { pinyin_set_options(context, word) });
    let text = cstr(input);
    unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) }
}

#[test]
fn force_tone_rejects_toneless_input_under_use_tone() {
    // C1: under USE_TONE|FORCE_TONE (the 0x60 profile) the pin parses
    // toneless input to 0 bytes — measured on the rebuilt pin:
    // opt:0x60-nihao@0:parsed=0, opt:0x60-zai6@0:parsed=0, while the
    // toned opt:0x60-ni3hao3@0:parsed=7.
    let user_dir = TempUserDir::new("c1-force");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let word = PARITY | USE_TONE | FORCE_TONE;

    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; only this thread calls into them.
    assert_eq!(unsafe { parse_len(context, instance, word, "nihao") }, 0);
    assert!(
        !unsafe { pinyin_guess_sentence(instance) },
        "no sentence on an empty parse"
    );
    assert!(
        !unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) },
        "the pin's empty-matrix early return (pinyin.cpp:2193): \
             no rows and no engine fallback row on an empty parse"
    );
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; only this thread calls into them.
        unsafe { parse_len(context, instance, word, "zai6") },
        0,
        "'6' is not a tone digit"
    );

    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; only this thread calls into them.
        unsafe { parse_len(context, instance, word, "ni3hao3") },
        7,
        "toned input parses"
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let mut n = 0;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut n) });
    assert!(n > 0, "the toned window is non-empty");

    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; only this thread calls into them.
    assert_eq!(unsafe { parse_len(context, instance, word, "zai4") }, 4);

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

#[test]
fn stop_bytes_terminate_the_parse() {
    // B2: the pin stops consuming at the first byte no key matches;
    // the capi parse seam must let those bytes reach the decoder.
    // Measured: punctparse-space-mid:parsed=2,
    // punctparse-fullwidth:parsed=0, punctparse-apostrophe-mid:parsed=6.
    let user_dir = TempUserDir::new("b2-stop");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; only this thread calls into them.
    assert_eq!(unsafe { parse_len(context, instance, PARITY, "ni hao") }, 2);
    assert!(
        unsafe { pinyin_guess_sentence(instance) },
        "the ni window still decodes"
    );
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; only this thread calls into them.
        unsafe { parse_len(context, instance, PARITY, "\u{ff0c}nihao") },
        0
    );
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; only this thread calls into them.
        unsafe { parse_len(context, instance, PARITY, "ni'hao") },
        6,
        "internal run unchanged"
    );

    // The inherited apostrophe class, folded in: trailing and
    // standalone runs are consumed by the pin's propagation
    // (the pin parses ni' → 3; nihao' → 6 by the same law).
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; only this thread calls into them.
    assert_eq!(unsafe { parse_len(context, instance, PARITY, "nihao'") }, 6);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; only this thread calls into them.
    assert_eq!(unsafe { parse_len(context, instance, PARITY, "ni'") }, 3);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; only this thread calls into them.
    assert_eq!(unsafe { parse_len(context, instance, PARITY, "'''") }, 3);
    // Apostrophe-only parses hold no keys but are NOT empty parses: no
    // candidate and never the engine's raw-input fallback row. The
    // sentence guess answers `false` there: `get_nbest_match` returns
    // before it clears anything when the matrix has no step
    // (`phonetic_lookup.h:743-745`; measured at 074a2219 on bdb for one,
    // two and three apostrophes under the default and the parity word).
    assert!(!unsafe { pinyin_guess_sentence(instance) });
    // `pinyin_guess_candidates` frees the list and answers `false` for
    // the same step-less matrix (`pinyin.cpp:2193-2196`).
    assert!(!unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let mut n = 0;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut n) });
    assert_eq!(
        n, 0,
        "no candidates and no fallback row for a keyless parse"
    );

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// on a fresh instance the pin's
/// `offset >= matrix.size() - 1` is `offset >= SIZE_MAX`
/// (`pinyin.cpp:2927-2928`, `:2950-2951`), so `SIZE_MAX` answers `false`
/// with the out-param NULLed before `get_column_size` could assert.
#[test]
fn the_key_getters_answer_false_at_size_max_on_an_empty_matrix() {
    let _slots = crate::support::key_slot_guard();
    let user_dir = TempUserDir::new("key-size-max");
    let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let mut key = ptr::NonNull::<crate::support::ChewingKey>::dangling().as_ptr();
    assert!(!unsafe { crate::support::pinyin_get_pinyin_key(instance, usize::MAX, &raw mut key) });
    assert!(key.is_null());
    let mut rest = ptr::NonNull::<crate::support::ChewingKeyRest>::dangling().as_ptr();
    assert!(!unsafe {
        crate::support::pinyin_get_pinyin_key_rest(instance, usize::MAX, &raw mut rest)
    });
    assert!(rest.is_null());
}

/// `pinyin_get_pinyin_key_rest_positions` never reads `instance`, and reads
/// `key_rest` only to fill a non-NULL `begin` or `end`
/// (`pinyin.cpp:2967-2977`).
#[test]
fn key_rest_positions_follow_the_pins_reads() {
    let _slots = crate::support::key_slot_guard();
    let user = TempUserDir::new("key-rest-positions-null");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let (_, rest) = unsafe { parsed_key_and_rest(instance, "ni3") };
    let (mut begin, mut end) = (u16::MAX, u16::MAX);
    // NULL instance: answered (`:2967-2977` never reads it).
    assert!(unsafe {
        crate::support::pinyin_get_pinyin_key_rest_positions(
            ptr::null_mut(),
            rest,
            &raw mut begin,
            &raw mut end,
        )
    });
    assert_eq!((begin, end), (0, 3));
    // NULL `key_rest` with both outs NULL: no read, `true` (`:2970-2975`).
    assert!(unsafe {
        crate::support::pinyin_get_pinyin_key_rest_positions(
            instance,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    });
    // NULL `key_rest` with an out: the pin reads it (row 41), so this stays guarded.
    begin = u16::MAX;
    assert!(!unsafe {
        crate::support::pinyin_get_pinyin_key_rest_positions(
            instance,
            ptr::null_mut(),
            &raw mut begin,
            ptr::null_mut(),
        )
    });
    assert!(!unsafe {
        crate::support::pinyin_get_pinyin_key_rest_positions(
            instance,
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut end,
        )
    });
    assert_eq!(begin, u16::MAX);
}

/// One offset of a key walk as `tools/oracle/pin_capture.py`'s `key_walk` observes it: the
/// key's packed word (`None` when the query answers false), the key rest's raw span (asked only
/// of a non-null rest) and the key's pinyin string (`None` when the renderer answers false).
#[derive(Debug, PartialEq)]
struct Step {
    key: Option<u16>,
    span: Option<(u16, u16)>,
    pinyin: Option<String>,
}

fn step(key: Option<u16>, span: Option<(u16, u16)>, pinyin: Option<&str>) -> Step {
    Step {
        key,
        span,
        pinyin: pinyin.map(str::to_owned),
    }
}

/// Every offset from 0 to the parsed length, the calls in `key_walk`'s order.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn walk(instance: *mut crate::support::PinyinInstance, parsed: usize) -> Vec<Step> {
    (0..=parsed)
        .map(|offset| {
            let mut key: *mut ChewingKey = ptr::null_mut();
            let found = unsafe { pinyin_get_pinyin_key(instance, offset, &raw mut key) };
            assert_eq!(
                found,
                !key.is_null(),
                "offset {offset}: answer and out-param"
            );
            let mut rest: *mut ChewingKeyRest = ptr::null_mut();
            let rest_found = unsafe { pinyin_get_pinyin_key_rest(instance, offset, &raw mut rest) };
            assert_eq!(
                rest_found,
                !rest.is_null(),
                "offset {offset}: rest and out-param"
            );
            let span = (!rest.is_null()).then(|| {
                let (mut begin, mut end) = (0_u16, 0_u16);
                assert!(unsafe {
                    pinyin_get_pinyin_key_rest_positions(
                        instance,
                        rest,
                        &raw mut begin,
                        &raw mut end,
                    )
                });
                (begin, end)
            });
            let (packed, pinyin) = if key.is_null() {
                (None, None)
            } else {
                // SAFETY: a true answer points `key` at the instance's key slot.
                let packed = unsafe { (*key).packed };
                let mut text: *mut GChar = ptr::null_mut();
                let rendered = unsafe { pinyin_get_pinyin_string(instance, key, &raw mut text) };
                (
                    Some(packed),
                    // SAFETY: `text` is the string the library just returned; this code owns it and
                    // frees it only here.
                    rendered.then(|| unsafe { crate::support::take_owned_cstr(text) }),
                )
            };
            Step {
                key: packed,
                span,
                pinyin,
            }
        })
        .collect()
}

// At 074a2219, pinyin.cpp:1497-1609,2183-2196,2693-2704,2873-2878: parsing
// leaves candidate storage alone; guessing and reset free it. Keep the
// original C pointers, not just copied text or a row fetched after parse.
#[rstest]
#[case::full(crate::support::pinyin_parse_more_full_pinyins, "ni")]
#[case::double(crate::support::pinyin_parse_more_double_pinyins, "ni")]
#[case::chewing(crate::support::pinyin_parse_more_chewings, "su3")]
fn reparse_preserves_candidate_strings_until_guess_or_reset(
    #[case] parse: unsafe extern "C" fn(
        *mut crate::support::PinyinInstance,
        *const std::ffi::c_char,
    ) -> usize,
    #[case] input: &str,
) {
    use crate::support::{
        pinyin_get_candidate, pinyin_get_candidate_string, pinyin_get_n_candidate,
    };
    use std::ffi::CStr;
    use std::ptr;

    let user_dir = TempUserDir::new("candidate-lifetime");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()) },
        5
    );
    assert!(unsafe { crate::support::pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let mut count = 0;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut count) });
    assert!(count > 0);
    let mut original = ptr::null_mut();
    assert!(unsafe { pinyin_get_candidate(instance, 0, &raw mut original) });
    let mut string = ptr::null();
    assert!(unsafe { pinyin_get_candidate_string(instance, original, &raw mut string) });
    assert!(!string.is_null());
    // SAFETY: the freshly guessed candidate owns a NUL-terminated string.
    let expected = unsafe { CStr::from_ptr(string) }.to_owned();

    assert_eq!(
        unsafe { parse(instance, cstr(input).as_ptr()) },
        input.len()
    );
    let mut after = 0;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut after) });
    assert_eq!(after, count, "a reparse preserves the original list");
    let mut current = ptr::null_mut();
    assert!(unsafe { pinyin_get_candidate(instance, 0, &raw mut current) });
    assert_eq!(current, original);
    let mut current_string = ptr::null();
    assert!(unsafe { pinyin_get_candidate_string(instance, original, &raw mut current_string) });
    assert_eq!(current_string, string);
    // SAFETY: parsing has preserved the original snapshot, as checked above;
    // no guess or reset has invalidated either saved pointer.
    assert_eq!(unsafe { CStr::from_ptr(string) }, expected.as_c_str());

    // A successful guess replaces the old list with the reparsed input's
    // candidates. Read only newly obtained pointers after this boundary.
    assert!(unsafe { crate::support::pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    assert!(unsafe { pinyin_get_candidate(instance, 0, &raw mut current) });
    assert!(unsafe { pinyin_get_candidate_string(instance, current, &raw mut current_string) });
    assert_ne!(
        // SAFETY: this pointer comes from the new candidate list.
        unsafe { CStr::from_ptr(current_string) },
        expected.as_c_str()
    );

    // Even a refused empty-input guess invalidates the preceding list.
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut count) });
    assert!(count > 0);
    assert_eq!(unsafe { parse(instance, cstr("").as_ptr()) }, 0);
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut after) });
    assert_eq!(after, count);
    assert!(!unsafe { crate::support::pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut after) });
    assert_eq!(after, 0);
    assert!(!unsafe { pinyin_get_candidate(instance, 0, &raw mut current) });
    assert!(current.is_null());

    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()) },
        5
    );
    assert!(unsafe { crate::support::pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    assert!(unsafe { crate::support::pinyin_reset(instance) });
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut after) });
    assert_eq!(after, 0);
    assert!(!unsafe { pinyin_get_candidate(instance, 0, &raw mut current) });
    assert!(current.is_null());
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// The pin hands the parser the raw bytes (`pinyin.cpp:1498-1509`), so
/// valid pinyin before an invalid sequence is consumed and the count is
/// the prefix's; the bytes behind the sequence are never reached.
#[test]
fn invalid_utf8_after_valid_pinyin_keeps_the_parsed_prefix() {
    let user_dir = TempUserDir::new("invalid-utf8-prefix");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    for (bytes, consumed) in [
        (&b"ni\xffhao"[..], 2),
        (b"nihao\xff", 5),
        (b"ni\xe4\xbd", 2),
        (b"\xffnihao", 0),
    ] {
        let text = std::ffi::CString::new(bytes).expect("no interior NUL");
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) },
            consumed,
            "{bytes:?}"
        );
        assert_eq!(
            unsafe { pinyin_get_parsed_input_length(instance) },
            consumed
        );
    }

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// A key query at offset 0 of an apostrophe-only parse aborts. The column holds the zero key,
/// so `_compute_pinyin_start` (`pinyin.cpp:2903-2919` at 074a2219) walks to the reserved
/// slot, where `get_item` asserts `row < column->len` (`storage/phonetic_key_matrix.h:115`,
/// reached from `pinyin.cpp:2933-2938`). Pin observation
/// observations `key-abort/'`, `key-abort/''` and `key-abort/'''`: the parse and the parsed
/// length answer the input's length, then the key query ends in SIGABRT.
///
/// The abort is an `assert()` on user input, the same root cause as ibus-libpinyin #570.
/// A lowercase letter in the parsed prefix avoids the abort, so `ni'`, `'ni`, `a'a` and
/// `ni''hao` do not abort.
#[rstest]
#[case::one("'")]
#[case::two("''")]
#[case::three("'''")]
fn a_key_query_on_an_apostrophe_only_parse_aborts(#[case] input: &str) {
    let case = ["one", "two", "three"][input.len() - 1];
    let name = format!(
        "parse_keys::a_key_query_on_an_apostrophe_only_parse_aborts::case_{}_{case}",
        input.len()
    );
    crate::support::assert_aborts(&name, || {
        let _slots = key_slot_guard();
        let user_dir = TempUserDir::new("apostrophe-key-abort");
        let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        let text = cstr(input);
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) },
            input.len()
        );
        assert_eq!(
            unsafe { pinyin_get_parsed_input_length(instance) },
            input.len()
        );
        let mut key: *mut ChewingKey = ptr::null_mut();
        unsafe { pinyin_get_pinyin_key(instance, 0, &raw mut key) };
    });
}

/// The key walk of `ni` with a trailing apostrophe, then junk or a vowel the parse leaves.
/// Pin observations `key-walk/ni'`,
/// `key-walk/ni'!` and `key-walk/ni'i`, on the default options:
/// - the parse consumes `ni'`, 3 bytes: `ni`, then the separator, which places the zero key
///   (`storage/pinyin_parser2.cpp:238-250`);
/// - offset 0 is `ni`, packed 43, raw span 0..2;
/// - offset 1 is inside the key: no key and no rest (`pinyin.cpp:2930-2931`);
/// - offset 2 is the zero key's column: `_compute_pinyin_start` (`:2903-2919`) walks past it
///   to the reserved slot, so the key is the zero key, its rest spans 3..4, and the zero key
///   renders nothing (`storage/chewing_key.cpp:91-116`);
/// - offset 3 is the reserved slot, `offset >= matrix.size() - 1` (`:2927-2928`).
#[rstest]
#[case::trailing("ni'")]
#[case::junk("ni'!")]
#[case::vowel("ni'i")]
fn a_trailing_apostrophe_walks_as_the_pin_does(#[case] input: &str) {
    let _slots = key_slot_guard();
    let user_dir = TempUserDir::new("apostrophe-key-walk");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let text = cstr(input);
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) },
        3
    );
    assert_eq!(unsafe { pinyin_get_parsed_input_length(instance) }, 3);
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { walk(instance, 3) },
        vec![
            step(Some(43), Some((0, 2)), Some("ni")),
            step(None, None, None),
            step(Some(0), Some((3, 4)), None),
            step(None, None, None),
        ]
    );
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

mod chewing_batch_force_tone {
    use super::*;

    /// # Safety
    ///
    /// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not
    /// taken it.
    ///
    /// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
    /// `pinyin_free_instance` has not freed it.
    ///
    /// Calls are serial across the context, its sibling instances and its iterators: no other call
    /// through any of them, and no borrow of their state, overlaps this call (the crate's Serial
    /// calls rule).
    unsafe fn chewing_len(
        context: *mut crate::support::PinyinContext,
        instance: *mut crate::support::PinyinInstance,
        word: u32,
        input: &str,
    ) -> usize {
        assert!(unsafe { pinyin_set_options(context, word) });
        let text = cstr(input);
        unsafe { pinyin_parse_more_chewings(instance, text.as_ptr()) }
    }

    #[test]
    fn force_tone_crosses_the_pinyin_facade_seam() {
        // Under USE_TONE|FORCE_TONE the pin consumes 0 on toneless `su`
        // (STANDARD ㄋㄧ). A toned syllable parses.
        let user_dir = TempUserDir::new("row30-force");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        let word = PARITY | USE_TONE | FORCE_TONE;

        assert_eq!(
            // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above
            // and is not yet freed; only this thread calls into them.
            unsafe { chewing_len(context, instance, word, "su") },
            0,
            "toneless syllable refuses under FORCE_TONE"
        );
        assert_eq!(
            // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above
            // and is not yet freed; only this thread calls into them.
            unsafe { chewing_len(context, instance, word, "su3") },
            3,
            "toned syllable parses under FORCE_TONE"
        );

        unsafe { crate::support::pinyin_free_instance(instance) };
        unsafe { crate::support::pinyin_fini(context) };
    }
}

/// `(true, text)` for an answer, `(false, "")` for the empty matrix.
///
/// # Safety
///
/// `render` is one of the crate's auxiliary-text renderers.
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn render_aux(
    render: unsafe extern "C" fn(*mut PinyinInstance, usize, *mut *mut GChar) -> bool,
    instance: *mut PinyinInstance,
    cursor: usize,
) -> (bool, String) {
    let mut text: *mut GChar = std::ptr::null_mut();
    // SAFETY: the caller supplies an auxiliary renderer and valid handle; text is a writable slot.
    let answered = unsafe { render(instance, cursor, &raw mut text) };
    // SAFETY: `text` is the string the library just returned; this code owns it and frees it only
    // here.
    (answered, unsafe { crate::support::take_owned_cstr(text) })
}

/// The chewing split is the pin's `g_utf8_substring` cut: the raw byte
/// delta is used as a scalar index into the zhuyin spelling. At the last
/// scalar the right half is empty; past it the pin's pointer is already
/// off the allocation, so oxpinyin keeps the right half
/// empty as well.
#[test]
fn chewing_cuts_the_zhuyin_spelling_by_scalar() {
    let user_dir = TempUserDir::new("aux-chewing-scalar");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()) },
        5
    );
    // "hao" spans bytes 2..5 and spells two scalars: cursor 3 cuts after
    // the first, cursor 4 after the second (the terminator).
    assert_eq!(
        // SAFETY: `pinyin_get_chewing_auxiliary_text` is one of the crate's renderers; `instance`
        // was opened above and is not yet freed; only this thread calls into it.
        unsafe { render_aux(pinyin_get_chewing_auxiliary_text, instance, 3) },
        (true, "ㄋㄧ ㄏ|ㄠ ".to_owned())
    );
    assert_eq!(
        // SAFETY: `pinyin_get_chewing_auxiliary_text` is one of the crate's renderers; `instance`
        // was opened above and is not yet freed; only this thread calls into it.
        unsafe { render_aux(pinyin_get_chewing_auxiliary_text, instance, 4) },
        (true, "ㄋㄧ ㄏㄠ| ".to_owned())
    );
    // An overrun cursor clamps to the parsed length, the end boundary.
    assert_eq!(
        // SAFETY: `pinyin_get_chewing_auxiliary_text` is one of the crate's renderers; `instance`
        // was opened above and is not yet freed; only this thread calls into it.
        unsafe { render_aux(pinyin_get_chewing_auxiliary_text, instance, 99) },
        (true, "ㄋㄧ ㄏㄠ |".to_owned())
    );

    // "zhong" spells three scalars in five raw bytes, so cursor 4 is a
    // scalar past the spelling.
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("zhong").as_ptr()) },
        5
    );
    assert_eq!(
        // SAFETY: `pinyin_get_chewing_auxiliary_text` is one of the crate's renderers; `instance`
        // was opened above and is not yet freed; only this thread calls into it.
        unsafe { render_aux(pinyin_get_chewing_auxiliary_text, instance, 4) },
        (true, "ㄓㄨㄥ| ".to_owned())
    );

    // A tone-1 key keeps its digit in the raw span but not in the
    // zhuyin spelling: cursor 2 lands exactly on the terminator.
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("ni1").as_ptr()) },
        3
    );
    assert_eq!(
        // SAFETY: `pinyin_get_chewing_auxiliary_text` is one of the crate's renderers; `instance`
        // was opened above and is not yet freed; only this thread calls into it.
        unsafe { render_aux(pinyin_get_chewing_auxiliary_text, instance, 2) },
        (true, "ㄋㄧ| ".to_owned())
    );

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

type Render = unsafe extern "C" fn(*mut PinyinInstance, usize, *mut *mut c_char) -> bool;

const FULL_AUX: Render = pinyin_get_full_pinyin_auxiliary_text;

const DOUBLE_AUX: Render = pinyin_get_double_pinyin_auxiliary_text;

const CHEWING_AUX: Render = crate::support::pinyin_get_chewing_auxiliary_text;

/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule). `render` is FULL_AUX, DOUBLE_AUX or CHEWING_AUX. Each returns NULL or an owned string
/// that the caller releases with g_free.
unsafe fn aux(instance: *mut PinyinInstance, render: Render, cursor: usize) -> Option<String> {
    let mut out = UNTOUCHED as usize as *mut c_char;
    // SAFETY: the caller supplies an auxiliary renderer and valid handle; out is a writable slot.
    let ret = unsafe { render(instance, cursor, &raw mut out) };
    assert!(
        out as usize != UNTOUCHED as usize,
        "the renderer writes its out-parameter"
    );
    // SAFETY: `out` is the string the library just returned; this code owns it and frees it only
    // here.
    ret.then(|| unsafe { crate::support::take_owned_cstr(out) })
}

/// The double-pinyin renderer cuts at two bytes into a key with a tone
/// after the cut; the full and chewing renderers read the same matrix.
#[test]
fn auxiliary_text_cuts_before_a_tone() {
    let user = TempUserDir::new("aux-tones");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        unsafe { crate::support::pinyin_parse_more_double_pinyins(instance, c"ni3hk4".as_ptr()) },
        6
    );
    for (render, cursor, text) in [
        (DOUBLE_AUX, 2, "ni|3 hao4 "),
        (DOUBLE_AUX, 4, "ni3 h|ao4 "),
        (DOUBLE_AUX, 5, "ni3 hao|4 "),
        (FULL_AUX, 5, "ni3 ha|o4 "),
        (CHEWING_AUX, 2, "ㄋㄧ|ˇ ㄏㄠˋ "),
        (CHEWING_AUX, 5, "ㄋㄧˇ ㄏㄠ|ˋ "),
    ] {
        assert_eq!(
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { aux(instance, render, cursor) }.as_deref(),
            Some(text),
            "{cursor}"
        );
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The getter reads the keys made by the last parse. Setting options only
/// stores the context word (074a2219 pinyin.cpp:1299-1306, 1497-1524,
/// 3381-3438). A candidate guess may apply those options to the Rust
/// session without changing the parse that the getter should display.
#[rstest::rstest]
fn full_auxiliary_text_uses_the_parse_options(#[values(false, true)] after_guess: bool) {
    let user = TempUserDir::new("aux-parse-options");
    let (context, instance) = open_at(&user.path);
    let incomplete = oxpinyin::OptionBits::PINYIN_INCOMPLETE;
    assert!(unsafe {
        pinyin_set_options(
            context,
            (incomplete | oxpinyin::OptionBits::USE_TONE).bits(),
        )
    });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "ni3hao3") }, 7);
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { aux(instance, FULL_AUX, 7) }.as_deref(),
        Some("ni3 hao3 |")
    );

    assert!(unsafe { pinyin_set_options(context, incomplete.bits()) });
    if after_guess {
        assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::support::DEFAULT_SORT) });
    }
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { aux(instance, FULL_AUX, 7) }.as_deref(),
        Some("ni3 hao3 |")
    );

    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "ni3hao3") }, 2);
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { aux(instance, FULL_AUX, 7) }.as_deref(),
        Some("ni |")
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The chewing renderer clamps a cursor past the input to the end.
#[test]
fn chewing_auxiliary_text_clamps_a_cursor_past_the_input() {
    let user = TempUserDir::new("aux-chewing-overrun");
    let (context, _) = open_at(&user.path);
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let full = unsafe { alloc(context) };
    // SAFETY: `full` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(full, "nihao") }, 5);
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let chewing = unsafe { alloc(context) };
    assert_eq!(
        unsafe { crate::support::pinyin_parse_more_chewings(chewing, c"su3cl3".as_ptr()) },
        6
    );
    for (instance, cursors, text) in [
        (full, [6, 12, usize::MAX], "ㄋㄧ ㄏㄠ |"),
        (chewing, [7, 13, usize::MAX], "ㄋㄧˇ ㄏㄠˇ |"),
    ] {
        for cursor in cursors {
            assert_eq!(
                // SAFETY: `instance` is `full` or `chewing`, which the test opened above and has
                // not freed; only this thread calls into it.
                unsafe { aux(instance, CHEWING_AUX, cursor) }.as_deref(),
                Some(text)
            );
        }
        unsafe { pinyin_free_instance(instance) };
    }
    unsafe { pinyin_fini(context) };
}

/// Under USE_DIVIDED_TABLE the renderers show the parsed keys, not the
/// option alternates.
#[test]
fn auxiliary_text_shows_the_parsed_keys_under_the_divided_table() {
    let user = TempUserDir::new("aux-divided");
    let (context, _) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, 0xA0) });
    for (input, expected) in [
        (
            "xianan",
            [
                (FULL_AUX, 3, "xia |nan "),
                (CHEWING_AUX, 3, "ㄒㄧㄚ |ㄋㄢ "),
                (DOUBLE_AUX, 2, "xia| nan "),
            ],
        ),
        (
            "xian",
            [
                (FULL_AUX, 2, "xi|an "),
                (CHEWING_AUX, 2, "ㄒㄧ|ㄢ "),
                (DOUBLE_AUX, 2, "xian| "),
            ],
        ),
    ] {
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { alloc(context) };
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { parse(instance, input) }, input.len());
        for (render, cursor, text) in expected {
            assert_eq!(
                // SAFETY: `instance` was opened above and is not yet freed; only this thread calls
                // into it.
                unsafe { aux(instance, render, cursor) }.as_deref(),
                Some(text),
                "{input} {cursor}"
            );
        }
        unsafe { pinyin_free_instance(instance) };
    }
    unsafe { pinyin_fini(context) };
}

/// The resplit and fuzzy option bits do not change the text. The parser's
/// keys do not depend on them: its resplit pass is compiled out
/// (`pinyin_parser2.cpp:335-340`) and its index check masks the fuzzy bits
/// (`:49-50`). `fill_matrix` appends those keys first
/// (`phonetic_key_matrix.cpp:51-55`); `resplit_step`, `inner_split_step` and
/// `fuzzy_syllable_step` run after it (`pinyin.cpp:1514-1520`) and append
/// their alternates (`phonetic_key_matrix.cpp:150`, `:155`, `:211`, `:216`,
/// `:256`, `:295`). The renderers read item 0 of a column and move to that
/// key's end (`pinyin.cpp:3312`, `:3331`, `:3353`, `:3413`, `:3427`), so
/// they meet the parser's keys only: a key that a resplit puts in an empty
/// column is never visited.
#[test]
fn auxiliary_text_ignores_the_resplit_and_fuzzy_alternates() {
    const USE_TONE: u32 = 1 << 5;
    const USE_DIVIDED_TABLE: u32 = 1 << 7;
    const USE_RESPLIT_TABLE: u32 = 1 << 8;
    // PINYIN_AMB_Z_ZH | PINYIN_AMB_L_N | PINYIN_AMB_AN_ANG.
    const FUZZY: u32 = 1 << 12 | 1 << 15 | 1 << 17;
    let user = TempUserDir::new("aux-alternates");
    let (context, _) = open_at(&user.path);
    let texts = |input: &str| {
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { alloc(context) };
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { parse(instance, input) }, input.len());
        let mut rendered = Vec::new();
        for cursor in 0..input.len() + 2 {
            // The double-pinyin renderer aborts three bytes into a key.
            let renders: &[Render] = if matches!(cursor, 0..=2) || cursor >= input.len() {
                &[FULL_AUX, CHEWING_AUX, DOUBLE_AUX]
            } else {
                &[FULL_AUX, CHEWING_AUX]
            };
            for &render in renders {
                // SAFETY: `instance` was opened above and is not yet freed; only this thread calls
                // into it.
                rendered.push(unsafe { aux(instance, render, cursor) }.expect("a parsed matrix"));
            }
        }
        unsafe { pinyin_free_instance(instance) };
        rendered
    };
    for (word, inputs) in [
        (
            USE_RESPLIT_TABLE,
            &["zhengfu", "xianan", "dianxin", "changan"][..],
        ),
        (
            USE_DIVIDED_TABLE | USE_RESPLIT_TABLE,
            &["xian", "zhengfu", "xianan"],
        ),
        (FUZZY, &["zongguo", "nan", "lan", "zi", "banggan"]),
    ] {
        for input in inputs {
            assert!(unsafe { pinyin_set_options(context, USE_TONE) });
            let plain = texts(input);
            assert!(unsafe { pinyin_set_options(context, USE_TONE | word) });
            assert_eq!(texts(input), plain, "{input} under {word:#x}");
        }
    }
    // The texts themselves, for the parse the divided-table test names.
    assert!(unsafe { pinyin_set_options(context, USE_TONE | USE_RESPLIT_TABLE) });
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "xianan") }, 6);
    for (render, cursor, text) in [
        (FULL_AUX, 3, "xia |nan "),
        (CHEWING_AUX, 3, "ㄒㄧㄚ |ㄋㄢ "),
        (DOUBLE_AUX, 2, "xia| nan "),
    ] {
        assert_eq!(
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls
            // into it.
            unsafe { aux(instance, render, cursor) }.as_deref(),
            Some(text),
            "{cursor}"
        );
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// With no parse each renderer answers false and writes an empty string.
#[test]
fn auxiliary_text_without_a_parse_is_an_empty_string() {
    let user = TempUserDir::new("aux-no-parse");
    let (context, instance) = open_at(&user.path);
    for render in [FULL_AUX, DOUBLE_AUX, CHEWING_AUX] {
        let mut out = UNTOUCHED as usize as *mut c_char;
        assert!(!unsafe { render(instance, 0, &raw mut out) });
        assert!(!out.is_null() && out as usize != UNTOUCHED as usize);
        // SAFETY: `out` is the string the library just returned; this code owns it and frees it
        // only here.
        assert_eq!(unsafe { crate::support::take_owned_cstr(out) }, "");
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// NULL string arguments after defined prior work (#1279,
/// docs/findings/upstream-divergences.md, "NULL string conversion after defined
/// prior work"): the segment and parse entries answer as for an empty string,
/// the others refuse.
#[test]
fn null_string_arguments_after_prior_work() {
    let user = TempUserDir::new("null-strings");
    let (context, instance) = open_at(&user.path);
    let logs = crate::support::LogCapture::new();

    assert!(unsafe { crate::support::pinyin_phrase_segment(instance, c"你好".as_ptr()) });
    logs.take();
    assert!(unsafe { crate::support::pinyin_phrase_segment(instance, ptr::null()) });
    let mut count = UNTOUCHED;
    assert!(unsafe { crate::support::pinyin_get_n_phrase(instance, &raw mut count) });
    assert_eq!(count, 0);
    assert_eq!(logs.take(), [(Some("GLib".to_owned()), CRITICAL)]);

    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, ptr::null()) },
        0
    );
    assert_eq!(
        unsafe { crate::support::pinyin_get_parsed_input_length(instance) },
        0
    );

    let mut key = 0xCDEF_u16;
    assert!(!unsafe { pinyin_parse_full_pinyin(instance, ptr::null(), (&raw mut key).cast()) });
    assert_eq!(key, 0xCDEF);

    let iter = unsafe { crate::support::pinyin_begin_add_phrases(context, 7) };
    assert!(!unsafe {
        crate::support::pinyin_iterator_add_phrase(iter, ptr::null(), c"da1'da1'da1".as_ptr(), 5)
    });
    unsafe { crate::support::pinyin_end_add_phrases(iter) };

    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "da1da1da1") }, 9);
    assert!(!unsafe { crate::support::pinyin_remember_user_input(instance, ptr::null(), 5) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}
