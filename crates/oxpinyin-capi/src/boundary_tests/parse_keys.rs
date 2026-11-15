//! Parsing, the key and offset getters, the auxiliary texts and the
//! keyboard wrapper.

use super::*;

/// The luoma and secondary-zhuyin renderers answer false on the zero key
/// of a column and write NULL to the out-parameter (register row 41).
#[test]
fn luoma_and_secondary_zhuyin_refuse_the_zero_key() {
    let _slots = crate::test_support::key_slot_guard();
    let user = TempUserDir::new("zero-key-renderers");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, 0x1aa));
    assert_eq!(parse(instance, "xi'"), 3);
    assert!(pinyin_guess_sentence(instance));
    let mut key: *mut ChewingKey = ptr::null_mut();
    assert!(pinyin_get_pinyin_key(instance, 2, &raw mut key));
    for render in [
        crate::keys::pinyin_get_luoma_pinyin_string,
        crate::keys::pinyin_get_secondary_zhuyin_string,
    ] {
        let mut out = UNTOUCHED as usize as *mut c_char;
        assert!(!render(instance, key, &raw mut out));
        assert!(out.is_null());
    }
    close(context, instance);
}

/// Init seeds USE_TONE alone: a toned key parses, an initial does not
/// (no PINYIN_INCOMPLETE).
#[test]
fn init_options_parse_tones_but_not_initials() {
    let user = TempUserDir::new("default-options");
    let (context, _) = open_at(&user.path);
    for (text, parsed) in [("n", 0), ("ni3", 3), ("zzz", 0), ("ni2hao", 6)] {
        let instance = alloc(context);
        assert_eq!(parse(instance, text), parsed, "{text}");
        pinyin_free_instance(instance);
    }
    pinyin_fini(context);
}

/// `is_incomplete` asserts on a toned initial.
#[test]
fn is_incomplete_aborts_on_a_toned_initial() {
    assert_aborts(
        "boundary_tests::parse_keys::is_incomplete_aborts_on_a_toned_initial",
        || {
            let user = TempUserDir::new("toned-incomplete");
            let (context, instance) = open_at(&user.path);
            assert!(pinyin_set_options(context, 0x28));
            let mut key = 0_u16;
            pinyin_parse_full_pinyin(instance, c"n4".as_ptr(), (&raw mut key).cast());
            pinyin_get_pinyin_is_incomplete(instance, (&raw mut key).cast());
        },
    );
}

/// `parse_full_pinyin` asserts on an apostrophe after bytes that are not
/// UTF-8 (#1279).
#[test]
fn full_key_parse_aborts_on_an_apostrophe_after_invalid_bytes() {
    assert_aborts(
        "boundary_tests::parse_keys::full_key_parse_aborts_on_an_apostrophe_after_invalid_bytes",
        || {
            let user = TempUserDir::new("full-key-invalid");
            let (_context, instance) = open_at(&user.path);
            let mut key = 0xCDEF_u16;
            pinyin_parse_full_pinyin(instance, c"ni\xff'".as_ptr(), (&raw mut key).cast());
        },
    );
}

/// The full-pinyin auxiliary text asserts on a leading apostrophe.
#[test]
fn full_auxiliary_text_aborts_on_a_leading_apostrophe() {
    assert_aborts(
        "boundary_tests::parse_keys::full_auxiliary_text_aborts_on_a_leading_apostrophe",
        || {
            let user = TempUserDir::new("aux-leading-apostrophe");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "'nihao"), 6);
            let mut aux: *mut c_char = ptr::null_mut();
            pinyin_get_full_pinyin_auxiliary_text(instance, 2, &raw mut aux);
        },
    );
}

/// The double-pinyin auxiliary text asserts on a cut more than two bytes
/// into a key.
#[test]
fn double_auxiliary_text_aborts_three_bytes_into_a_key() {
    assert_aborts(
        "boundary_tests::parse_keys::double_auxiliary_text_aborts_three_bytes_into_a_key",
        || {
            let user = TempUserDir::new("aux-double-cut");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "zhong"), 5);
            let mut aux: *mut c_char = ptr::null_mut();
            pinyin_get_double_pinyin_auxiliary_text(instance, 3, &raw mut aux);
        },
    );
}

/// `get_pinyin_key_rest` asserts on an empty matrix.
#[test]
fn key_rest_aborts_on_an_empty_matrix() {
    assert_aborts(
        "boundary_tests::parse_keys::key_rest_aborts_on_an_empty_matrix",
        || {
            let user = TempUserDir::new("key-rest-empty");
            let (_context, instance) = open_at(&user.path);
            let mut rest = ptr::null_mut();
            pinyin_get_pinyin_key_rest(instance, 0, &raw mut rest);
        },
    );
}

/// The left offset asserts on the zero key after a separator.
#[test]
fn left_offset_aborts_after_a_separator() {
    assert_aborts(
        "boundary_tests::parse_keys::left_offset_aborts_after_a_separator",
        || {
            let user = TempUserDir::new("left-after-separator");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "ni'hao"), 6);
            let mut left = 0_usize;
            pinyin_get_left_pinyin_offset(instance, 3, &raw mut left);
        },
    );
}

/// The right offset asserts on the zero key after a separator.
#[test]
fn right_offset_aborts_after_a_separator() {
    assert_aborts(
        "boundary_tests::parse_keys::right_offset_aborts_after_a_separator",
        || {
            let user = TempUserDir::new("right-after-separator");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "ni'hao"), 6);
            let mut right = 0_usize;
            pinyin_get_right_pinyin_offset(instance, 3, &raw mut right);
        },
    );
}

/// The character offset asserts on the empty leading column of a
/// leading separator (MatrixColumnAssert).
#[test]
fn character_offset_aborts_on_an_empty_leading_column() {
    assert_aborts(
        "boundary_tests::parse_keys::character_offset_aborts_on_an_empty_leading_column",
        || {
            let user = TempUserDir::new("character-leading-separator");
            let (context, instance) = open_at(&user.path);
            assert!(pinyin_set_options(context, SEPARATOR_OPTIONS));
            assert_eq!(parse(instance, "'ni"), 3);
            pinyin_guess_sentence(instance);
            let mut length = 0_usize;
            pinyin_get_character_offset(instance, c"你".as_ptr(), 0, &raw mut length);
        },
    );
}

/// The right offset at the first of two leading separators answers false
/// and leaves the out-parameter alone.
#[test]
fn right_offset_at_a_leading_separator_is_refused() {
    let user = TempUserDir::new("right-leading-separator");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, SEPARATOR_OPTIONS));
    assert_eq!(parse(instance, "''ni"), 4);
    assert!(pinyin_guess_sentence(instance));
    let mut right = UNTOUCHED as usize;
    assert!(!pinyin_get_right_pinyin_offset(instance, 0, &raw mut right));
    assert_eq!(right, UNTOUCHED as usize);
    close(context, instance);
}

type Render = extern "C" fn(*mut PinyinInstance, usize, *mut *mut c_char) -> bool;

const FULL_AUX: Render = pinyin_get_full_pinyin_auxiliary_text;

const DOUBLE_AUX: Render = pinyin_get_double_pinyin_auxiliary_text;

const CHEWING_AUX: Render = crate::text::pinyin_get_chewing_auxiliary_text;

fn aux(instance: *mut PinyinInstance, render: Render, cursor: usize) -> Option<String> {
    let mut out = UNTOUCHED as usize as *mut c_char;
    let ret = render(instance, cursor, &raw mut out);
    assert!(
        out as usize != UNTOUCHED as usize,
        "the renderer writes its out-parameter"
    );
    ret.then(|| crate::ffi::take_owned_cstr(out))
}

/// The double-pinyin renderer cuts at two bytes into a key with a tone
/// after the cut; the full and chewing renderers read the same matrix.
#[test]
fn auxiliary_text_cuts_before_a_tone() {
    let user = TempUserDir::new("aux-tones");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        crate::parse::pinyin_parse_more_double_pinyins(instance, c"ni3hk4".as_ptr()),
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
            aux(instance, render, cursor).as_deref(),
            Some(text),
            "{cursor}"
        );
    }
    close(context, instance);
}

/// The getter reads the keys made by the last parse. Setting options only
/// stores the context word (074a2219 pinyin.cpp:1299-1306, 1497-1524,
/// 3381-3438). A candidate guess may apply those options to the Rust
/// session without changing the parse that the getter should display.
#[rstest::rstest]
fn full_auxiliary_text_uses_the_parse_options(#[values(false, true)] after_guess: bool) {
    let user = TempUserDir::new("aux-parse-options");
    let (context, instance) = open_at(&user.path);
    let incomplete = oxpinyin_core::OptionBits::PINYIN_INCOMPLETE;
    assert!(pinyin_set_options(
        context,
        (incomplete | oxpinyin_core::OptionBits::USE_TONE).bits()
    ));
    assert_eq!(parse(instance, "ni3hao3"), 7);
    assert_eq!(aux(instance, FULL_AUX, 7).as_deref(), Some("ni3 hao3 |"));

    assert!(pinyin_set_options(context, incomplete.bits()));
    if after_guess {
        assert!(pinyin_guess_candidates(
            instance,
            0,
            crate::test_support::DEFAULT_SORT
        ));
    }
    assert_eq!(aux(instance, FULL_AUX, 7).as_deref(), Some("ni3 hao3 |"));

    assert_eq!(parse(instance, "ni3hao3"), 2);
    assert_eq!(aux(instance, FULL_AUX, 7).as_deref(), Some("ni |"));
    close(context, instance);
}

/// The chewing renderer clamps a cursor past the input to the end.
#[test]
fn chewing_auxiliary_text_clamps_a_cursor_past_the_input() {
    let user = TempUserDir::new("aux-chewing-overrun");
    let (context, _) = open_at(&user.path);
    let full = alloc(context);
    assert_eq!(parse(full, "nihao"), 5);
    let chewing = alloc(context);
    assert_eq!(
        crate::parse::pinyin_parse_more_chewings(chewing, c"su3cl3".as_ptr()),
        6
    );
    for (instance, cursors, text) in [
        (full, [6, 12, usize::MAX], "ㄋㄧ ㄏㄠ |"),
        (chewing, [7, 13, usize::MAX], "ㄋㄧˇ ㄏㄠˇ |"),
    ] {
        for cursor in cursors {
            assert_eq!(aux(instance, CHEWING_AUX, cursor).as_deref(), Some(text));
        }
        pinyin_free_instance(instance);
    }
    pinyin_fini(context);
}

/// Under USE_DIVIDED_TABLE the renderers show the parsed keys, not the
/// option alternates.
#[test]
fn auxiliary_text_shows_the_parsed_keys_under_the_divided_table() {
    let user = TempUserDir::new("aux-divided");
    let (context, _) = open_at(&user.path);
    assert!(pinyin_set_options(context, 0xA0));
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
        let instance = alloc(context);
        assert_eq!(parse(instance, input), input.len());
        for (render, cursor, text) in expected {
            assert_eq!(
                aux(instance, render, cursor).as_deref(),
                Some(text),
                "{input} {cursor}"
            );
        }
        pinyin_free_instance(instance);
    }
    pinyin_fini(context);
}

/// With no parse each renderer answers false and writes an empty string
///.
#[test]
fn auxiliary_text_without_a_parse_is_an_empty_string() {
    let user = TempUserDir::new("aux-no-parse");
    let (context, instance) = open_at(&user.path);
    for render in [FULL_AUX, DOUBLE_AUX, CHEWING_AUX] {
        let mut out = UNTOUCHED as usize as *mut c_char;
        assert!(!render(instance, 0, &raw mut out));
        assert!(!out.is_null() && out as usize != UNTOUCHED as usize);
        assert_eq!(crate::ffi::take_owned_cstr(out), "");
    }
    close(context, instance);
}

/// NULL string arguments after defined prior work (#1279,
/// docs/findings/upstream-divergences.md, "NULL string conversion after defined
/// prior work"): the segment and parse entries answer as for an empty string,
/// the others refuse.
#[test]
fn null_string_arguments_after_prior_work() {
    let user = TempUserDir::new("null-strings");
    let (context, instance) = open_at(&user.path);
    let logs = crate::test_support::LogCapture::new();

    assert!(crate::phrase::pinyin_phrase_segment(
        instance,
        c"你好".as_ptr()
    ));
    logs.take();
    assert!(crate::phrase::pinyin_phrase_segment(instance, ptr::null()));
    let mut count = UNTOUCHED;
    assert!(crate::phrase::pinyin_get_n_phrase(instance, &raw mut count));
    assert_eq!(count, 0);
    assert_eq!(logs.take(), [(Some("GLib".to_owned()), CRITICAL)]);

    assert_eq!(parse(instance, "nihao"), 5);
    assert_eq!(pinyin_parse_more_full_pinyins(instance, ptr::null()), 0);
    assert_eq!(crate::parse::pinyin_get_parsed_input_length(instance), 0);

    let mut key = 0xCDEF_u16;
    assert!(!pinyin_parse_full_pinyin(
        instance,
        ptr::null(),
        (&raw mut key).cast()
    ));
    assert_eq!(key, 0xCDEF);

    let iter = crate::iterators::pinyin_begin_add_phrases(context, 7);
    assert!(!crate::iterators::pinyin_iterator_add_phrase(
        iter,
        ptr::null(),
        c"da1'da1'da1".as_ptr(),
        5
    ));
    crate::iterators::pinyin_end_add_phrases(iter);

    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "da1da1da1"), 9);
    assert!(!crate::user_data::pinyin_remember_user_input(
        instance,
        ptr::null(),
        5
    ));
    close(context, instance);
}

/// Invalid bytes in pinyin input: an import reading keeps its parsed
/// prefix, an import text is refused, and the double and chewing parsers
/// stop at the byte.
#[test]
fn invalid_input_bytes_stop_each_pinyin_parser() {
    let user = TempUserDir::new("invalid-bytes");
    let (context, instance) = open_at(&user.path);
    let iter = crate::iterators::pinyin_begin_add_phrases(context, 7);
    let add = |text: &[u8], reading: &[u8]| {
        let text = std::ffi::CString::new(text).expect("no NUL");
        let reading = std::ffi::CString::new(reading).expect("no NUL");
        crate::iterators::pinyin_iterator_add_phrase(iter, text.as_ptr(), reading.as_ptr(), 1)
    };
    assert!(add("你好".as_bytes(), b"ni'ha\xffo"));
    assert!(add("世界".as_bytes(), b"shi'jie\xff"));
    assert!(!add(b"\xe4\xbd\xa0\xe5\xa5\xbd\xff", b"ni'hao"));
    crate::iterators::pinyin_end_add_phrases(iter);
    assert_eq!(
        crate::e2e_tests::export_of(context, 7),
        [
            ("你好".to_owned(), "ni'ha".to_owned(), 1),
            ("世界".to_owned(), "shi'jie".to_owned(), 1),
        ]
    );
    let consumed = |parser: extern "C" fn(*mut PinyinInstance, *const c_char) -> usize,
                    text: &[u8]| {
        let fresh = alloc(context);
        let text = std::ffi::CString::new(text).expect("no NUL");
        let parsed = parser(fresh, text.as_ptr());
        pinyin_free_instance(fresh);
        parsed
    };
    let double = crate::parse::pinyin_parse_more_double_pinyins;
    assert_eq!(consumed(double, b"ni\xffhk"), 2);
    assert_eq!(consumed(double, b"nihk\xff"), 4);
    assert_eq!(consumed(double, b"\xffnihk"), 0);
    let chewings = crate::parse::pinyin_parse_more_chewings;
    assert_eq!(consumed(chewings, b"su\xff3"), 2);
    assert_eq!(consumed(chewings, b"su3\xff"), 3);
    assert_eq!(consumed(chewings, b"\xffsu3"), 0);
    close(context, instance);
}

/// The symbols `in_chewing_keyboard` answers for `key`, freed.
fn keyboard(instance: *mut PinyinInstance, key: u8) -> (bool, Option<Vec<String>>) {
    let mut symbols: *mut *mut c_char = UNTOUCHED as usize as *mut *mut c_char;
    let ret = crate::parse::pinyin_in_chewing_keyboard(
        instance,
        c_char::from_ne_bytes([key]),
        &raw mut symbols,
    );
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

/// The pinyin wrapper of `in_chewing_keyboard`: a key of the
/// scheme answers its symbol, a tone key needs USE_TONE, and other keys
/// answer false with NULL.
#[test]
fn the_pinyin_chewing_keyboard_wrapper_answers_per_key() {
    let user = TempUserDir::new("chewing-keyboard");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, 0));
    assert!(crate::config::pinyin_set_zhuyin_scheme(context, 1));
    assert_eq!(
        keyboard(instance, b's'),
        (true, Some(vec!["ㄋ".to_owned()]))
    );
    assert_eq!(keyboard(instance, b'3'), (false, None));
    assert_eq!(keyboard(instance, b'!'), (false, None));
    assert!(pinyin_set_options(context, 0x60));
    assert_eq!(keyboard(instance, b'3'), (true, Some(vec!["ˇ".to_owned()])));
    close(context, instance);
}
