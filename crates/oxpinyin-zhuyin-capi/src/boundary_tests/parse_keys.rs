//! Parsing, the key and offset getters, the chewing schemes and the
//! keyboard.

use super::*;

/// The two string getters refuse the zero key that a separator column holds
/// and render a real key (`zhuyin.cpp:1732-1760`).
#[test]
fn string_getters_refuse_the_zero_key_and_render_a_real_key() {
    let _slots = crate::test_support::key_slot_guard();
    let (context, instance) = open();
    assert!(zhuyin_set_options(context, 0x1aa));
    zhuyin_parse_more_full_pinyins(instance, cstr("xi1'").as_ptr());
    assert!(zhuyin_guess_sentence(instance));
    let mut key: *mut ChewingKey = ptr::null_mut();
    assert!(zhuyin_get_zhuyin_key(instance, 3, &raw mut key));
    // SAFETY: the getter answered a live key slot.
    assert_eq!(unsafe { *key }.packed, 0);
    for getter in [zhuyin_get_zhuyin_string, zhuyin_get_pinyin_string] {
        let mut text = SEEDED as *mut c_char;
        assert!(!getter(instance, key, &raw mut text));
        assert!(text.is_null());
    }
    assert!(zhuyin_get_zhuyin_key(instance, 0, &raw mut key));
    let mut text: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_pinyin_string(instance, key, &raw mut text));
    assert_eq!(take_sentence(text), "xi1");

    let mut key = ChewingKey::ZERO;
    assert!(zhuyin_parse_chewing(
        instance,
        cstr("su3").as_ptr(),
        &raw mut key
    ));
    let mut text: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_zhuyin_string(
        instance,
        &raw mut key,
        &raw mut text
    ));
    assert_eq!(take_sentence(text), "ㄋㄧˇ");
    let mut text: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_pinyin_string(
        instance,
        &raw mut key,
        &raw mut text
    ));
    assert_eq!(take_sentence(text), "ni3");
    close(context, instance);
}

/// `'ni'hao` under the separator options, guessed: column 3 holds the zero
/// key that `_check_offset` asserts on (`zhuyin.cpp:1453`).
fn separated(context: *mut ZhuyinContext) -> *mut ZhuyinInstance {
    let instance = full_pinyin(context, SEPARATOR_OPTIONS, "'ni'hao");
    assert_eq!(zhuyin_get_parsed_input_length(instance), 7);
    zhuyin_guess_sentence(instance);
    instance
}

/// The left move from just past a separator asserts (`zhuyin.cpp:1998`).
#[test]
fn left_offset_just_past_a_separator_aborts() {
    assert_aborts(
        "boundary_tests::parse_keys::left_offset_just_past_a_separator_aborts",
        || {
            let (context, _) = open();
            let mut out = 0;
            zhuyin_get_left_zhuyin_offset(separated(context), 4, &raw mut out);
        },
    );
}

/// The right move from just past a separator asserts (`zhuyin.cpp:2030`).
#[test]
fn right_offset_just_past_a_separator_aborts() {
    assert_aborts(
        "boundary_tests::parse_keys::right_offset_just_past_a_separator_aborts",
        || {
            let (context, _) = open();
            let mut out = 0;
            zhuyin_get_right_zhuyin_offset(separated(context), 4, &raw mut out);
        },
    );
}

/// The key rest just past a separator asserts (`zhuyin.cpp:1920`).
#[test]
fn key_rest_just_past_a_separator_aborts() {
    assert_aborts(
        "boundary_tests::parse_keys::key_rest_just_past_a_separator_aborts",
        || {
            let (context, _) = open();
            let mut rest: *mut ChewingKeyRest = ptr::null_mut();
            zhuyin_get_zhuyin_key_rest(separated(context), 4, &raw mut rest);
        },
    );
}

/// The character offset walk on a leading separator asserts
/// (`zhuyin.cpp:2110`).
#[test]
fn character_offset_on_a_leading_separator_aborts() {
    assert_aborts(
        "boundary_tests::parse_keys::character_offset_on_a_leading_separator_aborts",
        || {
            let (context, _) = open();
            let instance = full_pinyin(context, SEPARATOR_OPTIONS, "'ni");
            zhuyin_guess_sentence(instance);
            let mut out = 0;
            zhuyin_get_character_offset(instance, cstr("你").as_ptr(), 0, &raw mut out);
        },
    );
}

/// The zhuyin facade's own parsers stop at the first invalid byte.
#[test]
fn parses_stop_at_an_invalid_byte() {
    let (context, _) = open();
    for (parse, text, consumed) in [
        (
            zhuyin_parse_more_full_pinyins
                as extern "C" fn(*mut ZhuyinInstance, *const c_char) -> usize,
            &b"ni3\xffhao3"[..],
            3,
        ),
        (zhuyin_parse_more_full_pinyins, b"ni3hao3\xff", 7),
        (zhuyin_parse_more_full_pinyins, b"\xffni3", 0),
        (zhuyin_parse_more_chewings, b"su\xff3", 0),
        (zhuyin_parse_more_chewings, b"su3\xff", 3),
        (zhuyin_parse_more_chewings, b"\xffsu3", 0),
    ] {
        let instance = zhuyin_alloc_instance(context);
        let input = std::ffi::CString::new(text).expect("no interior NUL");
        assert_eq!(parse(instance, input.as_ptr()), consumed, "{text:?}");
        zhuyin_free_instance(instance);
    }
    zhuyin_fini(context);
}

/// The left move past the matrix asserts in its column read
/// (`phonetic_key_matrix.h:103`).
#[test]
fn left_offset_past_the_matrix_aborts() {
    assert_aborts(
        "boundary_tests::parse_keys::left_offset_past_the_matrix_aborts",
        || {
            let (context, _) = open();
            let mut out = 0;
            zhuyin_get_left_zhuyin_offset(chewing(context, "su3cl3"), 99, &raw mut out);
        },
    );
}

/// The right move past the matrix asserts in its column read.
#[test]
fn right_offset_past_the_matrix_aborts() {
    assert_aborts(
        "boundary_tests::parse_keys::right_offset_past_the_matrix_aborts",
        || {
            let (context, _) = open();
            let mut out = 0;
            zhuyin_get_right_zhuyin_offset(chewing(context, "su3cl3"), 99, &raw mut out);
        },
    );
}

/// The key rest on an empty matrix asserts after `matrix.size() - 1` wraps
/// (`zhuyin.cpp:1914-1917`).
#[test]
fn key_rest_on_an_empty_matrix_aborts() {
    assert_aborts(
        "boundary_tests::parse_keys::key_rest_on_an_empty_matrix_aborts",
        || {
            let (_context, instance) = open();
            let mut rest: *mut ChewingKeyRest = ptr::null_mut();
            zhuyin_get_zhuyin_key_rest(instance, 0, &raw mut rest);
        },
    );
}

/// A character walk that fails stores the length it reached, 0
/// (`zhuyin.cpp:2192`).
#[test]
fn a_failed_character_walk_writes_zero() {
    let (context, _) = open();
    let instance = chewing(context, "su3cl3");
    let mut length = UNTOUCHED as usize;
    assert!(!zhuyin_get_character_offset(
        instance,
        cstr("啊").as_ptr(),
        3,
        &raw mut length
    ));
    assert_eq!(length, 0);
    close(context, instance);
}

/// The key getters hand out the pin's function-local statics
/// (`zhuyin.cpp:1898`, `:1921`): one slot for every instance, which outlives
/// the instance. The slots are process-wide, so the test runs alone in a child.
#[test]
fn key_slots_are_process_wide_statics() {
    run_in_child(
        "boundary_tests::parse_keys::key_slots_are_process_wide_statics",
        |_| {
            let (context, _) = open();
            let (first, second) = (chewing(context, "su3"), chewing(context, "cl3"));
            let mut keys: [*mut ChewingKey; 2] = [ptr::null_mut(); 2];
            let mut rests: [*mut ChewingKeyRest; 2] = [ptr::null_mut(); 2];
            assert!(zhuyin_get_zhuyin_key(first, 0, &raw mut keys[0]));
            assert!(zhuyin_get_zhuyin_key(second, 0, &raw mut keys[1]));
            assert!(zhuyin_get_zhuyin_key_rest(first, 0, &raw mut rests[0]));
            assert!(zhuyin_get_zhuyin_key_rest(second, 0, &raw mut rests[1]));
            assert_eq!(keys[0], keys[1]);
            assert_eq!(rests[0], rests[1]);
            // SAFETY: the slots are statics, readable for the process's life.
            let read = || unsafe { ((*keys[0]).packed, *rests[0].cast::<[u8; 4]>()) };
            assert_eq!(read(), (0x3286, [0, 0, 3, 0]));
            zhuyin_free_instance(first);
            assert_eq!(read(), (0x3286, [0, 0, 3, 0]));
        },
    );
}

/// The chewing scheme setter accepts schemes 6, 8 and 9; the loader refuses
/// indexes 16 and 255.
#[test]
fn chewing_schemes_and_library_bounds_follow_the_pin() {
    let (context, instance) = open();
    for scheme in [6, 8, 9] {
        assert!(
            zhuyin_set_chewing_scheme(context, scheme),
            "scheme {scheme}"
        );
    }
    assert!(!zhuyin_load_phrase_library(context, 16));
    assert!(!zhuyin_load_phrase_library(context, 255));
    close(context, instance);
}

/// The keyboard symbols of one key: the answer and the symbols, `None` for
/// a NULL vector. The vector is freed.
fn keyboard(instance: *mut ZhuyinInstance, key: u8) -> (bool, Option<Vec<String>>) {
    let mut symbols: *mut *mut c_char = ptr::null_mut();
    #[allow(clippy::cast_possible_wrap)]
    let ret = zhuyin_in_chewing_keyboard(instance, key as c_char, &raw mut symbols);
    if symbols.is_null() {
        return (ret, None);
    }
    let mut out = Vec::new();
    // SAFETY: a non-NULL vector is NULL-terminated and the caller frees it.
    unsafe {
        let mut at = 0;
        while !(*symbols.add(at)).is_null() {
            out.push(
                std::ffi::CStr::from_ptr(*symbols.add(at))
                    .to_str()
                    .expect("UTF-8")
                    .to_owned(),
            );
            at += 1;
        }
        glib_sys::g_strfreev(symbols);
    }
    (ret, Some(out))
}

/// `zhuyin_in_chewing_keyboard` maps a key by the scheme and the tone option
/// at the pin.
#[test]
fn the_chewing_keyboard_maps_keys_by_scheme_and_options() {
    let (context, instance) = open();
    let one = |symbol: &str| (true, Some(vec![symbol.to_owned()]));
    assert_eq!(keyboard(instance, b's'), one("ㄋ"));
    assert_eq!(keyboard(instance, b'3'), one("ˇ"));
    assert_eq!(keyboard(instance, b'1'), one("ㄅ"));
    for key in [b'!', 0x80, b'~'] {
        assert_eq!(keyboard(instance, key), (false, None), "{key:#x}");
    }
    assert!(zhuyin_set_options(context, 0));
    assert_eq!(keyboard(instance, b'3'), (false, None));
    close(context, instance);
    let (context, instance) = open();
    assert!(zhuyin_set_chewing_scheme(context, 2));
    assert_eq!(
        keyboard(instance, b's'),
        (true, Some(vec!["ㄙ".to_owned(), "˙".to_owned()]))
    );
    close(context, instance);
}

/// `zhuyin_parse_chewing` writes the key only for one whole syllable.
#[test]
fn parse_chewing_writes_the_key_only_on_success() {
    let (context, instance) = open();
    for (text, ret, packed) in [
        ("su3", true, 0x302B),
        ("cl3", true, 0x3286),
        ("su", false, 0xCDEF),
        ("su3cl3", false, 0xCDEF),
        ("", false, 0xCDEF),
    ] {
        let mut key = ChewingKey { packed: 0xCDEF };
        assert_eq!(
            zhuyin_parse_chewing(instance, cstr(text).as_ptr(), &raw mut key),
            ret,
            "{text}"
        );
        assert_eq!(key.packed, packed, "{text}");
    }
    close(context, instance);
}
