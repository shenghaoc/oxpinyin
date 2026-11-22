//! Parsing, the key and offset getters, the chewing schemes and the
//! keyboard.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    ChewingKey, ChewingKeyRest, ZhuyinContext, ZhuyinInstance, assert_aborts, close, cstr, open,
    take_sentence, zhuyin_alloc_instance, zhuyin_fini, zhuyin_free_instance,
    zhuyin_get_character_offset, zhuyin_get_left_zhuyin_offset, zhuyin_get_parsed_input_length,
    zhuyin_get_pinyin_string, zhuyin_get_right_zhuyin_offset, zhuyin_get_zhuyin_key,
    zhuyin_get_zhuyin_key_rest, zhuyin_get_zhuyin_string, zhuyin_guess_sentence,
    zhuyin_in_chewing_keyboard, zhuyin_init, zhuyin_load_phrase_library, zhuyin_parse_chewing,
    zhuyin_parse_more_chewings, zhuyin_parse_more_full_pinyins, zhuyin_set_chewing_scheme,
    zhuyin_set_options, *,
};

use rstest::rstest;

use std::ffi::CString;

use std::os::raw::c_char;

use std::path::Path;

use std::ptr;

/// `'ni'hao` under the separator options, guessed: column 3 holds the zero
/// key that `_check_offset` asserts on (`zhuyin.cpp:1453`).
///
/// # Safety
///
/// `context` is NULL or a live context: `zhuyin_init` returned it and `zhuyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn separated(context: *mut ZhuyinContext) -> *mut ZhuyinInstance {
    // SAFETY: `context` meets this function's `# Safety` contract; only this thread calls into it.
    let instance = unsafe { full_pinyin(context, SEPARATOR_OPTIONS, "'ni'hao") };
    assert_eq!(unsafe { zhuyin_get_parsed_input_length(instance) }, 7);
    unsafe { zhuyin_guess_sentence(instance) };
    instance
}

/// The keyboard symbols of one key: the answer and the symbols, `None` for
/// a NULL vector. The vector is freed.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `zhuyin_alloc_instance` returned it and
/// `zhuyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn keyboard(instance: *mut ZhuyinInstance, key: u8) -> (bool, Option<Vec<String>>) {
    let mut symbols: *mut *mut c_char = ptr::null_mut();
    #[allow(clippy::cast_possible_wrap)]
    let ret = unsafe { zhuyin_in_chewing_keyboard(instance, key as c_char, &raw mut symbols) };
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

/// The two string getters refuse the zero key that a separator column holds
/// and render a real key (`zhuyin.cpp:1732-1760`).
#[test]
fn string_getters_refuse_the_zero_key_and_render_a_real_key() {
    let _slots = crate::support::key_slot_guard();
    let (context, instance) = open();
    assert!(unsafe { zhuyin_set_options(context, 0x1aa) });
    unsafe { zhuyin_parse_more_full_pinyins(instance, cstr("xi1'").as_ptr()) };
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    let mut key: *mut ChewingKey = ptr::null_mut();
    assert!(unsafe { zhuyin_get_zhuyin_key(instance, 3, &raw mut key) });
    // SAFETY: the getter answered a live key slot.
    assert_eq!(unsafe { *key }.packed, 0);
    for getter in [zhuyin_get_zhuyin_string, zhuyin_get_pinyin_string] {
        let mut text = SEEDED as *mut c_char;
        assert!(!unsafe { getter(instance, key, &raw mut text) });
        assert!(text.is_null());
    }
    assert!(unsafe { zhuyin_get_zhuyin_key(instance, 0, &raw mut key) });
    let mut text: *mut c_char = ptr::null_mut();
    assert!(unsafe { zhuyin_get_pinyin_string(instance, key, &raw mut text) });
    // SAFETY: `text` is the string the library just returned; this code owns it and frees it only
    // here.
    assert_eq!(unsafe { take_sentence(text) }, "xi1");

    let mut key = ChewingKey::ZERO;
    assert!(unsafe { zhuyin_parse_chewing(instance, cstr("su3").as_ptr(), &raw mut key) });
    let mut text: *mut c_char = ptr::null_mut();
    assert!(unsafe { zhuyin_get_zhuyin_string(instance, &raw mut key, &raw mut text) });
    // SAFETY: `text` is the string the library just returned; this code owns it and frees it only
    // here.
    assert_eq!(unsafe { take_sentence(text) }, "ㄋㄧˇ");
    let mut text: *mut c_char = ptr::null_mut();
    assert!(unsafe { zhuyin_get_pinyin_string(instance, &raw mut key, &raw mut text) });
    // SAFETY: `text` is the string the library just returned; this code owns it and frees it only
    // here.
    assert_eq!(unsafe { take_sentence(text) }, "ni3");
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The left move from just past a separator asserts (`zhuyin.cpp:1998`).
#[test]
fn left_offset_just_past_a_separator_aborts() {
    assert_aborts(
        "parse_keys::left_offset_just_past_a_separator_aborts",
        || {
            let (context, _) = open();
            let mut out = 0;
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
            let instance = unsafe { separated(context) };
            unsafe { zhuyin_get_left_zhuyin_offset(instance, 4, &raw mut out) };
        },
    );
}

/// The right move from just past a separator asserts (`zhuyin.cpp:2030`).
#[test]
fn right_offset_just_past_a_separator_aborts() {
    assert_aborts(
        "parse_keys::right_offset_just_past_a_separator_aborts",
        || {
            let (context, _) = open();
            let mut out = 0;
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
            let instance = unsafe { separated(context) };
            unsafe { zhuyin_get_right_zhuyin_offset(instance, 4, &raw mut out) };
        },
    );
}

/// The key rest just past a separator asserts (`zhuyin.cpp:1920`).
#[test]
fn key_rest_just_past_a_separator_aborts() {
    assert_aborts("parse_keys::key_rest_just_past_a_separator_aborts", || {
        let (context, _) = open();
        let mut rest: *mut ChewingKeyRest = ptr::null_mut();
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { separated(context) };
        unsafe { zhuyin_get_zhuyin_key_rest(instance, 4, &raw mut rest) };
    });
}

/// The character offset walk on a leading separator asserts
/// (`zhuyin.cpp:2110`).
#[test]
fn character_offset_on_a_leading_separator_aborts() {
    assert_aborts(
        "parse_keys::character_offset_on_a_leading_separator_aborts",
        || {
            let (context, _) = open();
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            let instance = unsafe { full_pinyin(context, SEPARATOR_OPTIONS, "'ni") };
            unsafe { zhuyin_guess_sentence(instance) };
            let mut out = 0;
            unsafe { zhuyin_get_character_offset(instance, cstr("你").as_ptr(), 0, &raw mut out) };
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
                as unsafe extern "C" fn(*mut ZhuyinInstance, *const c_char) -> usize,
            &b"ni3\xffhao3"[..],
            3,
        ),
        (zhuyin_parse_more_full_pinyins, b"ni3hao3\xff", 7),
        (zhuyin_parse_more_full_pinyins, b"\xffni3", 0),
        (zhuyin_parse_more_chewings, b"su\xff3", 0),
        (zhuyin_parse_more_chewings, b"su3\xff", 3),
        (zhuyin_parse_more_chewings, b"\xffsu3", 0),
    ] {
        let instance = unsafe { zhuyin_alloc_instance(context) };
        let input = std::ffi::CString::new(text).expect("no interior NUL");
        assert_eq!(
            unsafe { parse(instance, input.as_ptr()) },
            consumed,
            "{text:?}"
        );
        unsafe { zhuyin_free_instance(instance) };
    }
    unsafe { zhuyin_fini(context) };
}

/// The left move past the matrix asserts in its column read
/// (`phonetic_key_matrix.h:103`).
#[test]
fn left_offset_past_the_matrix_aborts() {
    assert_aborts("parse_keys::left_offset_past_the_matrix_aborts", || {
        let (context, _) = open();
        let mut out = 0;
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { chewing(context, "su3cl3") };
        unsafe { zhuyin_get_left_zhuyin_offset(instance, 99, &raw mut out) };
    });
}

/// The right move past the matrix asserts in its column read.
#[test]
fn right_offset_past_the_matrix_aborts() {
    assert_aborts("parse_keys::right_offset_past_the_matrix_aborts", || {
        let (context, _) = open();
        let mut out = 0;
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { chewing(context, "su3cl3") };
        unsafe { zhuyin_get_right_zhuyin_offset(instance, 99, &raw mut out) };
    });
}

/// The key rest on an empty matrix asserts after `matrix.size() - 1` wraps
/// (`zhuyin.cpp:1914-1917`).
#[test]
fn key_rest_on_an_empty_matrix_aborts() {
    assert_aborts("parse_keys::key_rest_on_an_empty_matrix_aborts", || {
        let (_context, instance) = open();
        let mut rest: *mut ChewingKeyRest = ptr::null_mut();
        unsafe { zhuyin_get_zhuyin_key_rest(instance, 0, &raw mut rest) };
    });
}

/// A character walk that fails stores the length it reached, 0
/// (`zhuyin.cpp:2192`).
#[test]
fn a_failed_character_walk_writes_zero() {
    let (context, _) = open();
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { chewing(context, "su3cl3") };
    let mut length = UNTOUCHED as usize;
    assert!(!unsafe {
        zhuyin_get_character_offset(instance, cstr("啊").as_ptr(), 3, &raw mut length)
    });
    assert_eq!(length, 0);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The key getters hand out the pin's function-local statics
/// (`zhuyin.cpp:1898`, `:1921`): one slot for every instance, which outlives
/// the instance. The slots are process-wide; nextest runs each test in a
/// process of its own.
#[test]
fn key_slots_are_process_wide_statics() {
    let (context, _) = open();
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let (first, second) = (unsafe { chewing(context, "su3") }, unsafe {
        chewing(context, "cl3")
    });
    let mut keys: [*mut ChewingKey; 2] = [ptr::null_mut(); 2];
    let mut rests: [*mut ChewingKeyRest; 2] = [ptr::null_mut(); 2];
    assert!(unsafe { zhuyin_get_zhuyin_key(first, 0, &raw mut keys[0]) });
    assert!(unsafe { zhuyin_get_zhuyin_key(second, 0, &raw mut keys[1]) });
    assert!(unsafe { zhuyin_get_zhuyin_key_rest(first, 0, &raw mut rests[0]) });
    assert!(unsafe { zhuyin_get_zhuyin_key_rest(second, 0, &raw mut rests[1]) });
    assert_eq!(keys[0], keys[1]);
    assert_eq!(rests[0], rests[1]);
    // SAFETY: the slots are statics, readable for the process's life.
    let read = || unsafe { ((*keys[0]).packed, *rests[0].cast::<[u8; 4]>()) };
    assert_eq!(read(), (0x3286, [0, 0, 3, 0]));
    unsafe { zhuyin_free_instance(first) };
    assert_eq!(read(), (0x3286, [0, 0, 3, 0]));
}

/// The chewing scheme setter accepts schemes 6, 8 and 9; the loader refuses
/// indexes 16 and 255.
#[test]
fn chewing_schemes_and_library_bounds_follow_the_pin() {
    let (context, instance) = open();
    for scheme in [6, 8, 9] {
        assert!(
            unsafe { zhuyin_set_chewing_scheme(context, scheme) },
            "scheme {scheme}"
        );
    }
    assert!(!unsafe { zhuyin_load_phrase_library(context, 16) });
    assert!(!unsafe { zhuyin_load_phrase_library(context, 255) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// `zhuyin_in_chewing_keyboard` maps a key by the scheme and the tone option
/// at the pin.
#[test]
fn the_chewing_keyboard_maps_keys_by_scheme_and_options() {
    let (context, instance) = open();
    let one = |symbol: &str| (true, Some(vec![symbol.to_owned()]));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { keyboard(instance, b's') }, one("ㄋ"));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { keyboard(instance, b'3') }, one("ˇ"));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { keyboard(instance, b'1') }, one("ㄅ"));
    for key in [b'!', 0x80, b'~'] {
        assert_eq!(
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { keyboard(instance, key) },
            (false, None),
            "{key:#x}"
        );
    }
    assert!(unsafe { zhuyin_set_options(context, 0) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { keyboard(instance, b'3') }, (false, None));
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    let (context, instance) = open();
    assert!(unsafe { zhuyin_set_chewing_scheme(context, 2) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { keyboard(instance, b's') },
        (true, Some(vec!["ㄙ".to_owned(), "˙".to_owned()]))
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
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
            unsafe { zhuyin_parse_chewing(instance, cstr(text).as_ptr(), &raw mut key) },
            ret,
            "{text}"
        );
        assert_eq!(key.packed, packed, "{text}");
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// At 074a2219, Standard `1,u` spells ㄅㄝㄧ (src/storage/zhuyin_table.h:10,15,41).
/// Its row requires SHUFFLE and resolves to bie (src/storage/pinyin_parser_table.h:1496,3995).
/// src/zhuyin.cpp:1049-1055 forwards the options to the one-key lookup
/// (src/storage/zhuyin_parser2.cpp:162-179,185-208), whose flag check is at :44-63,89-97.
#[test]
fn parse_chewing_accepts_a_shuffled_reading_when_enabled() {
    let (context, instance) = open();
    let mut key = ChewingKey::ZERO;
    let input = c"1,u";
    let shuffle = oxpinyin::OptionBits::ZHUYIN_CORRECT_SHUFFLE.bits();
    let mut text = ptr::null_mut();
    // SAFETY: The fixture handles are live and used serially. The output slots are local,
    // and the successful getter transfers ownership of its string to this test.
    unsafe {
        assert!(zhuyin_set_options(context, 0));
        assert!(!zhuyin_parse_chewing(
            instance,
            input.as_ptr(),
            &raw mut key
        ));
        assert!(zhuyin_set_options(context, shuffle));
        assert!(zhuyin_parse_chewing(instance, input.as_ptr(), &raw mut key));
        assert!(zhuyin_get_pinyin_string(
            instance,
            &raw mut key,
            &raw mut text
        ));
        assert_eq!(take_sentence(text), "bie");
        close(context, instance);
    }
}

/// The `ZHUYIN_STANDARD_DVORAK` slot aborts the process, as the pin's
/// `set_scheme` falls through to `abort()` (`zhuyin_parser2.cpp:295`).
#[test]
fn the_dvorak_chewing_scheme_aborts() {
    crate::support::assert_aborts("parse_keys::the_dvorak_chewing_scheme_aborts", || {
        let (context, _instance) = open();
        unsafe { crate::support::zhuyin_set_chewing_scheme(context, 7) };
    });
}

/// An out-of-enum keyboard (`zhuyin.cpp:736`) and a full-pinyin scheme
/// outside 1-3 (`pinyin_parser2.cpp:398`) abort the process.
#[test]
fn an_out_of_enum_chewing_scheme_aborts() {
    crate::support::assert_aborts("parse_keys::an_out_of_enum_chewing_scheme_aborts", || {
        let (context, _instance) = open();
        unsafe { crate::support::zhuyin_set_chewing_scheme(context, 10) };
    });
}

#[test]
fn an_out_of_range_full_pinyin_scheme_aborts() {
    crate::support::assert_aborts(
        "parse_keys::an_out_of_range_full_pinyin_scheme_aborts",
        || {
            let (context, _instance) = open();
            unsafe { crate::support::zhuyin_set_full_pinyin_scheme(context, 4) };
        },
    );
}

/// An apostrophe in the single-key parser's input is the shared
/// `parse_one_key` assert (`pinyin_parser2.cpp:170`).
#[test]
fn a_single_key_apostrophe_aborts() {
    crate::support::assert_aborts("parse_keys::a_single_key_apostrophe_aborts", || {
        let (_context, instance) = open();
        let text = cstr("n'i");
        let mut key = crate::support::ChewingKey::ZERO;
        unsafe { crate::support::zhuyin_parse_full_pinyin(instance, text.as_ptr(), &raw mut key) };
    });
}

/// on a fresh instance `offset >= matrix.size() - 1` is
/// `offset >= SIZE_MAX` (`zhuyin.cpp:1892-1893`, `:1914-1915`), so
/// `SIZE_MAX` answers `false` with the out-param NULLed.
#[test]
fn the_key_accessors_answer_false_at_size_max_on_an_empty_matrix() {
    let _slots = crate::support::key_slot_guard();
    let (context, instance) = open();
    let mut key = std::ptr::NonNull::<crate::support::ChewingKey>::dangling().as_ptr();
    assert!(!unsafe { crate::support::zhuyin_get_zhuyin_key(instance, usize::MAX, &raw mut key) });
    assert!(key.is_null());
    let mut rest = std::ptr::NonNull::<crate::support::ChewingKeyRest>::dangling().as_ptr();
    assert!(!unsafe {
        crate::support::zhuyin_get_zhuyin_key_rest(instance, usize::MAX, &raw mut rest)
    });
    assert!(rest.is_null());
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// Past byte 65535 the key-rest positions narrow into the pin's `guint16`
/// fields mod 2^16 (`docs/findings/upstream-divergences.md#key-positions-past-byte-65535`). `su3` then `su`
/// repeated puts a key at 65535..65537, which crosses byte 65536, and one
/// at 65537..65539, wholly past it; both lengths stay 2.
#[test]
fn key_rest_positions_past_byte_65535_wrap() {
    let _slots = crate::support::key_slot_guard();
    let (context, instance) = open();
    assert!(unsafe { crate::support::zhuyin_set_options(context, 1 << 5) });
    assert!(unsafe { crate::support::zhuyin_set_chewing_scheme(context, 1) });
    let input = format!("su3{}", "su".repeat(32_769));
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, cstr(&input).as_ptr()) },
        65_541
    );
    for (offset, begin, end) in [(65_535_usize, 65_535_u16, 1_u16), (65_537, 1, 3)] {
        let mut rest: *mut crate::support::ChewingKeyRest = ptr::null_mut();
        assert!(unsafe {
            crate::support::zhuyin_get_zhuyin_key_rest(instance, offset, &raw mut rest)
        });
        let (mut b, mut e) = (0_u16, 0_u16);
        assert!(unsafe {
            crate::support::zhuyin_get_zhuyin_key_rest_positions(
                instance, rest, &raw mut b, &raw mut e,
            )
        });
        assert_eq!((b, e), (begin, end), "offset {offset}");
        let mut length = 0_u16;
        assert!(unsafe {
            crate::support::zhuyin_get_zhuyin_key_rest_length(instance, rest, &raw mut length)
        });
        assert_eq!(length, 2, "offset {offset}");
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// `zhuyin_get_zhuyin_key_rest_positions` reads `key_rest` only to fill a
/// non-NULL `begin` or `end` (`zhuyin.cpp:1930-1940`).
#[test]
fn key_rest_positions_follow_the_pins_reads() {
    let _slots = crate::support::key_slot_guard();
    let (context, instance) = open();
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()) },
        6
    );
    let mut rest = ptr::null_mut();
    assert!(unsafe { crate::support::zhuyin_get_zhuyin_key_rest(instance, 0, &raw mut rest) });
    // NULL `key_rest` with both outs NULL: no read, `true` (`:1933-1938`).
    assert!(unsafe {
        crate::support::zhuyin_get_zhuyin_key_rest_positions(
            instance,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    });
    assert!(unsafe {
        crate::support::zhuyin_get_zhuyin_key_rest_positions(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    });
    // NULL `key_rest` with an out: the pin reads it (row 41), so this stays guarded.
    let mut begin = u16::MAX;
    assert!(!unsafe {
        crate::support::zhuyin_get_zhuyin_key_rest_positions(
            instance,
            ptr::null_mut(),
            &raw mut begin,
            ptr::null_mut(),
        )
    });
    assert_eq!(begin, u16::MAX);
    let mut end = u16::MAX;
    assert!(!unsafe {
        crate::support::zhuyin_get_zhuyin_key_rest_positions(
            instance,
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut end,
        )
    });
    // A real rest with a NULL instance fills both.
    let (mut begin, mut end) = (u16::MAX, u16::MAX);
    assert!(unsafe {
        crate::support::zhuyin_get_zhuyin_key_rest_positions(
            ptr::null_mut(),
            rest,
            &raw mut begin,
            &raw mut end,
        )
    });
    assert_eq!((begin, end), (0, 3));
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

// At 074a2219, zhuyin.cpp:1017-1081,1460-1471,1718-1729,1865-1870: parsing
// leaves candidate storage alone; guessing and reset free it. Keep the
// original C pointers, not just copied text or a row fetched after parse.
#[rstest]
#[case::full(crate::support::zhuyin_parse_more_full_pinyins, "ni3")]
#[case::chewing(crate::support::zhuyin_parse_more_chewings, "su3")]
fn reparse_preserves_candidate_strings_until_guess_or_reset(
    #[case] parse: unsafe extern "C" fn(
        *mut crate::support::ZhuyinInstance,
        *const std::ffi::c_char,
    ) -> usize,
    #[case] input: &str,
) {
    use crate::support::{
        zhuyin_get_candidate, zhuyin_get_candidate_string, zhuyin_get_n_candidate,
    };
    use std::ffi::CStr;
    use std::ptr;

    let (context, instance) = open();
    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(instance, cstr("ni3hao3").as_ptr()) },
        7
    );
    assert!(unsafe { crate::support::zhuyin_guess_candidates_after_cursor(instance, 0) });
    let mut count = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    assert!(count > 0);
    let mut original = ptr::null_mut();
    assert!(unsafe { zhuyin_get_candidate(instance, 0, &raw mut original) });
    let mut string = ptr::null();
    assert!(unsafe { zhuyin_get_candidate_string(instance, original, &raw mut string) });
    assert!(!string.is_null());
    // SAFETY: the freshly guessed candidate owns a NUL-terminated string.
    let expected = unsafe { CStr::from_ptr(string) }.to_owned();

    assert_eq!(
        unsafe { parse(instance, cstr(input).as_ptr()) },
        input.len()
    );
    let mut after = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut after) });
    assert_eq!(after, count, "a reparse preserves the original list");
    let mut current = ptr::null_mut();
    assert!(unsafe { zhuyin_get_candidate(instance, 0, &raw mut current) });
    assert_eq!(current, original);
    let mut current_string = ptr::null();
    assert!(unsafe { zhuyin_get_candidate_string(instance, original, &raw mut current_string) });
    assert_eq!(current_string, string);
    // SAFETY: parsing has preserved the original snapshot, as checked above;
    // no guess or reset has invalidated either saved pointer.
    assert_eq!(unsafe { CStr::from_ptr(string) }, expected.as_c_str());

    // A successful guess replaces the old list with the reparsed input's
    // candidates. Read only newly obtained pointers after this boundary.
    assert!(unsafe { crate::support::zhuyin_guess_candidates_after_cursor(instance, 0) });
    assert!(unsafe { zhuyin_get_candidate(instance, 0, &raw mut current) });
    assert!(unsafe { zhuyin_get_candidate_string(instance, current, &raw mut current_string) });
    assert_ne!(
        // SAFETY: this pointer comes from the new candidate list.
        unsafe { CStr::from_ptr(current_string) },
        expected.as_c_str()
    );

    // Even a refused empty-input guess invalidates the preceding list.
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    assert!(count > 0);
    assert_eq!(unsafe { parse(instance, cstr("").as_ptr()) }, 0);
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut after) });
    assert_eq!(after, count);
    assert!(!unsafe { crate::support::zhuyin_guess_candidates_after_cursor(instance, 0) });
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut after) });
    assert_eq!(after, 0);
    assert!(!unsafe { zhuyin_get_candidate(instance, 0, &raw mut current) });
    assert!(current.is_null());

    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(instance, cstr("ni3hao3").as_ptr()) },
        7
    );
    assert!(unsafe { crate::support::zhuyin_guess_candidates_after_cursor(instance, 0) });
    assert!(unsafe { crate::support::zhuyin_reset(instance) });
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut after) });
    assert_eq!(after, 0);
    assert!(!unsafe { zhuyin_get_candidate(instance, 0, &raw mut current) });
    assert!(current.is_null());
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// An apostrophe-only parse under the default options. libzhuyin's
/// full-pinyin parse runs the same separator branch, which moves the
/// parse one byte per `'` and places no key
/// (`storage/pinyin_parser2.cpp:238-250` at 074a2219);
/// `zhuyin_parse_more_full_pinyins` answers that length and stores it for
/// `zhuyin_get_parsed_input_length` (`zhuyin.cpp:1030`, `:1034`, `:1042`,
/// `:1083-1085`). Each case holds the pin's consumed and parsed lengths
/// from its frozen observation (pin 074a2219, the pinned
/// libpinyin-data). No
/// database is read, so every cell runs this.
#[rstest]
// zero-key-guess/zhuyin/default/'/guess_sentence; pinyin_parser2.cpp:238-250.
#[case::one("'", 1, 1)]
// zero-key-guess/zhuyin/default/''/guess_sentence; pinyin_parser2.cpp:238-250.
#[case::two("''", 2, 2)]
// zero-key-guess/zhuyin/default/'''/guess_sentence; pinyin_parser2.cpp:238-250.
#[case::three("'''", 3, 3)]
fn an_apostrophe_only_parse_matches_the_pin(
    #[case] input: &str,
    #[case] consumed: usize,
    #[case] parsed_length: usize,
) {
    let (context, instance) = open();

    let text = cstr(input);
    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(instance, text.as_ptr()) },
        consumed
    );
    assert_eq!(
        unsafe { zhuyin_get_parsed_input_length(instance) },
        parsed_length
    );

    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

#[test]
fn luoma_left_offset_reads_the_unsplit_matrix() {
    let system = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/w3")
        .join(oxpinyin::__internal::DEFAULT_STORE_EXT);
    let system = CString::new(system.as_os_str().as_encoded_bytes()).expect("fixture path");
    let context = unsafe { zhuyin_init(system.as_ptr(), ptr::null()) };
    assert!(!context.is_null());

    let (left, has_middle_key) = {
        assert!(unsafe { zhuyin_set_options(context.cast(), 0x8 | (1 << 7)) });
        assert!(unsafe { zhuyin_set_full_pinyin_scheme(context.cast(), 2) });
        let instance = unsafe { zhuyin_alloc_instance(context.cast()) };
        assert!(!instance.is_null());
        assert_eq!(
            unsafe { zhuyin_parse_more_full_pinyins(instance, c"sian".as_ptr()) },
            4
        );

        let mut left = usize::MAX;
        assert!(unsafe { zhuyin_get_left_zhuyin_offset(instance, 4, &raw mut left) });
        let mut rest = ptr::null_mut();
        let has_middle_key = unsafe { zhuyin_get_zhuyin_key_rest(instance, 2, &raw mut rest) };
        unsafe { zhuyin_free_instance(instance) };
        (left, has_middle_key)
    };
    unsafe { zhuyin_fini(context) };

    // Pin 074a2219 zhuyin.cpp:1017-1042 fills and fuzzes the parsed xian
    // key over 0..4. Column 2 stays empty, so the backward scan reaches
    // 0 (:1994-2023) and the key-rest getter refuses at 2 (:1908-1918).
    assert_eq!(left, 0);
    assert!(!has_middle_key);
}

/// A choose answers the row's own `m_end` (after the cursor) or
/// `m_begin` (before it), fixed at the guess, and never the caller's
/// offset, which `zhuyin_choose_candidate` overwrites (074a2219
/// `zhuyin.cpp:1649-1666`); here the caller names 0 for a row over
/// `[3, 6)`. A reparse that shortens the first key moves the key spans
/// under the row, and the answer stays the row's own.
#[rstest]
fn a_choose_at_another_offset_answers_the_rows_own_end(#[values(false, true)] before: bool) {
    use crate::support::{zhuyin_choose_candidate, zhuyin_get_candidate};
    use crate::support::{
        zhuyin_guess_candidates_after_cursor, zhuyin_guess_candidates_before_cursor,
    };
    use std::ptr;

    let (context, instance) = open();
    let input = cstr("su3cl3");
    assert_eq!(
        unsafe { crate::support::zhuyin_parse_more_chewings(instance, input.as_ptr()) },
        6
    );
    assert!(if before {
        unsafe { zhuyin_guess_candidates_before_cursor(instance, 6) }
    } else {
        unsafe { zhuyin_guess_candidates_after_cursor(instance, 3) }
    });
    let index = {
        let mut count = 0;
        assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
        (0..count as usize)
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            .position(|index| unsafe { crate::support::candidate_text(instance, index) } == "好")
            .expect("好 over the second key")
    };
    let mut row = ptr::null_mut();
    assert!(unsafe {
        zhuyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut row,
        )
    });
    let expected = if before { 3 } else { 6 };
    assert_eq!(
        unsafe { zhuyin_choose_candidate(instance, 0, row) },
        expected
    );
    let shorter = cstr("83cl3");
    assert_eq!(
        unsafe { crate::support::zhuyin_parse_more_chewings(instance, shorter.as_ptr()) },
        5
    );
    assert_eq!(
        unsafe { zhuyin_choose_candidate(instance, 0, row) },
        expected
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}
