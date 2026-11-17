//! Null arguments to every entry point.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// The NULL arguments that the pin dereferences answer the declared value
/// (register row 41, class (b); `docs/findings/upstream-divergences.md`,
/// "NULL pointer arguments: the pin dereferences, oxpinyin null-guards"). The
/// NULL strings after defined prior work keep the earlier state
/// ("NULL string conversion after defined prior work (#1279)").
#[test]
fn null_arguments_answer_the_declared_value() {
    let _slots = crate::test_support::key_slot_guard();
    let user_dir = TempUserDir::new("null-arguments");
    let (context, instance) = open_with_user(&user_dir.path);
    let su3 = cstr("su3");
    let mut key = ChewingKey::ZERO;

    let iter = unsafe { crate::iterators::zhuyin_begin_add_phrases(context, 16) };
    assert!(!iter.is_null());
    let reading = cstr("ㄋㄧˇ ㄏㄠˇ");
    assert!(!unsafe {
        crate::iterators::zhuyin_iterator_add_phrase(
            iter,
            cstr("你好").as_ptr(),
            reading.as_ptr(),
            1,
        )
    });
    unsafe { crate::iterators::zhuyin_end_add_phrases(iter) };
    let iter = unsafe { crate::iterators::zhuyin_begin_add_phrases(context, 7) };
    assert!(!unsafe {
        crate::iterators::zhuyin_iterator_add_phrase(iter, ptr::null(), reading.as_ptr(), 5)
    });
    unsafe { crate::iterators::zhuyin_end_add_phrases(iter) };

    assert!(!unsafe { zhuyin_parse_chewing(ptr::null_mut(), su3.as_ptr(), &raw mut key) });
    let mut seeded = ChewingKey { packed: 0xCDEF };
    assert!(!unsafe { zhuyin_parse_chewing(instance, ptr::null(), &raw mut seeded) });
    assert_eq!(seeded.packed, 0xCDEF);
    assert!(!unsafe { zhuyin_parse_chewing(instance, su3.as_ptr(), ptr::null_mut()) });
    let mut symbols = SEEDED as *mut *mut c_char;
    assert!(!unsafe {
        zhuyin_in_chewing_keyboard(ptr::null_mut(), b's' as c_char, &raw mut symbols)
    });
    assert!(unsafe { zhuyin_in_chewing_keyboard(instance, b's' as c_char, ptr::null_mut()) });

    assert!(unsafe { zhuyin_parse_chewing(instance, su3.as_ptr(), &raw mut key) });
    let mut text = SEEDED as *mut c_char;
    assert!(!unsafe { zhuyin_get_pinyin_string(ptr::null_mut(), &raw mut key, &raw mut text) });
    assert!(!unsafe { zhuyin_get_zhuyin_string(instance, ptr::null_mut(), &raw mut text) });
    assert!(!unsafe { zhuyin_get_zhuyin_string(instance, &raw mut key, ptr::null_mut()) });

    let (mut begin, mut end) = (0_u16, 0_u16);
    assert!(!unsafe {
        zhuyin_get_zhuyin_key_rest_positions(
            instance,
            ptr::null_mut(),
            &raw mut begin,
            &raw mut end,
        )
    });
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()) },
        6
    );
    let mut rest: *mut ChewingKeyRest = ptr::null_mut();
    assert!(unsafe { zhuyin_get_zhuyin_key_rest(instance, 0, &raw mut rest) } && !rest.is_null());
    let mut length = 0_u16;
    assert!(!unsafe {
        zhuyin_get_zhuyin_key_rest_length(instance, ptr::null_mut(), &raw mut length)
    });
    assert!(unsafe { zhuyin_get_zhuyin_key_rest_length(instance, rest, ptr::null_mut()) });

    assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
    let candidate = candidate_at(instance, 0);
    let mut kind = crate::types::lookup_candidate_type_t::ZOMBIE_CANDIDATE;
    assert!(!unsafe { zhuyin_get_candidate_type(instance, ptr::null_mut(), &raw mut kind) });
    assert!(unsafe { zhuyin_get_candidate_type(instance, candidate, ptr::null_mut()) });
    let mut borrowed = SEEDED as *const c_char;
    assert!(!unsafe { zhuyin_get_candidate_string(instance, ptr::null_mut(), &raw mut borrowed) });
    assert!(unsafe { zhuyin_get_candidate_string(instance, candidate, ptr::null_mut()) });

    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, ptr::null()) },
        0
    );
    assert_eq!(unsafe { zhuyin_get_parsed_input_length(instance) }, 0);

    let mut count = 0;
    assert!(!unsafe { zhuyin_get_n_phrase(ptr::null_mut(), &raw mut count) });
    assert!(unsafe { zhuyin_get_n_phrase(instance, ptr::null_mut()) });
    let mut slot = 0;
    assert!(!unsafe { zhuyin_get_phrase_token(ptr::null_mut(), 0, &raw mut slot) });
    assert!(!unsafe { zhuyin_get_phrase_token(instance, 0, ptr::null_mut()) });

    assert!(unsafe { zhuyin_phrase_segment(instance, cstr("你好").as_ptr()) });
    let logs = LogCapture::new();
    assert!(unsafe { zhuyin_phrase_segment(instance, ptr::null()) });
    assert_eq!(logs.take(), [(Some("GLib".to_owned()), CRITICAL)]);
    drop(logs);
    let mut count = UNTOUCHED;
    assert!(unsafe { zhuyin_get_n_phrase(instance, &raw mut count) });
    assert_eq!(count, 0);
    close(context, instance);
}
