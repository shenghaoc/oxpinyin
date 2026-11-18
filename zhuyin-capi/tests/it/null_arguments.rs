//! Null arguments to every entry point.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    ChewingKey, ChewingKeyRest, LogCapture, TempUserDir, candidate, close, cstr, open,
    open_with_user, take_sentence, zhuyin_alloc_instance, zhuyin_begin_add_phrases,
    zhuyin_choose_candidate, zhuyin_end_add_phrases, zhuyin_fini, zhuyin_free_instance,
    zhuyin_get_candidate_string, zhuyin_get_candidate_type, zhuyin_get_n_candidate,
    zhuyin_get_n_phrase, zhuyin_get_parsed_input_length, zhuyin_get_phrase_token,
    zhuyin_get_pinyin_string, zhuyin_get_sentence, zhuyin_get_zhuyin_key_rest,
    zhuyin_get_zhuyin_key_rest_length, zhuyin_get_zhuyin_key_rest_positions,
    zhuyin_get_zhuyin_string, zhuyin_guess_candidates_after_cursor, zhuyin_guess_sentence,
    zhuyin_in_chewing_keyboard, zhuyin_init, zhuyin_iterator_add_phrase, zhuyin_parse_chewing,
    zhuyin_parse_more_chewings, zhuyin_phrase_segment, zhuyin_reset, zhuyin_save, zhuyin_train, *,
};

use std::os::raw::c_char;

use std::ptr;

/// The NULL arguments that the pin dereferences answer the declared value
/// (register row 41, class (b); `docs/findings/upstream-divergences.md`,
/// "NULL pointer arguments: the pin dereferences, oxpinyin null-guards"). The
/// NULL strings after defined prior work keep the earlier state
/// ("NULL string conversion after defined prior work (#1279)").
#[test]
fn null_arguments_answer_the_declared_value() {
    let _slots = crate::support::key_slot_guard();
    let user_dir = TempUserDir::new("null-arguments");
    let (context, instance) = open_with_user(&user_dir.path);
    let su3 = cstr("su3");
    let mut key = ChewingKey::ZERO;

    let iter = unsafe { crate::support::zhuyin_begin_add_phrases(context, 16) };
    assert!(!iter.is_null());
    let reading = cstr("ㄋㄧˇ ㄏㄠˇ");
    assert!(!unsafe {
        crate::support::zhuyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), reading.as_ptr(), 1)
    });
    unsafe { crate::support::zhuyin_end_add_phrases(iter) };
    let iter = unsafe { crate::support::zhuyin_begin_add_phrases(context, 7) };
    assert!(!unsafe {
        crate::support::zhuyin_iterator_add_phrase(iter, ptr::null(), reading.as_ptr(), 5)
    });
    unsafe { crate::support::zhuyin_end_add_phrases(iter) };

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
    let mut kind = crate::support::lookup_candidate_type_t::ZOMBIE_CANDIDATE;
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

/// Null and empty inputs are refused, never dereferenced: the
/// availability-class law every entry point carries.
#[test]
fn null_and_empty_inputs_are_refused() {
    // Empty system dir: init's NULL.
    let empty = cstr("");
    assert!(unsafe { zhuyin_init(empty.as_ptr(), empty.as_ptr()) }.is_null());

    // Null handles: every entry point answers its false shape.
    assert!(unsafe { zhuyin_alloc_instance(ptr::null_mut()) }.is_null());
    assert!(!unsafe { zhuyin_train(ptr::null_mut()) });
    assert!(!unsafe { zhuyin_guess_sentence(ptr::null_mut()) });
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(!unsafe { zhuyin_get_sentence(ptr::null_mut(), &raw mut sentence) });
    let mut count = 0;
    assert!(!unsafe { zhuyin_get_n_candidate(ptr::null_mut(), &raw mut count) });
    assert_eq!(
        unsafe { zhuyin_choose_candidate(ptr::null_mut(), 0, ptr::null_mut()) },
        -1
    );
    assert!(!unsafe { zhuyin_save(ptr::null_mut()) });
    assert!(!unsafe { zhuyin_reset(ptr::null_mut()) });
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(ptr::null_mut(), empty.as_ptr()) },
        0
    );
    // The free paths are no-ops on null.
    unsafe { zhuyin_free_instance(ptr::null_mut()) };
    unsafe { zhuyin_fini(ptr::null_mut()) };
}

// ---------------------------------------------------------------------
// NULL arguments the pin never reads (row 41's non-crashing probes). Each
// test hands valid objects and the one NULL argument the pin ignores; the
// expectations are the pin's answer at 074a2219 (cited beside each), not a
// pin run.
// ---------------------------------------------------------------------

/// `zhuyin_get_candidate_type` never reads `instance` (`zhuyin.cpp:1858-1863`).
#[test]
fn null_instance_candidate_type_answers() {
    let (context, instance) = open();
    let cand = candidate(instance, "su3cl3", 0);
    let mut kind = crate::support::lookup_candidate_type_t::ZOMBIE_CANDIDATE;
    assert!(unsafe {
        crate::support::zhuyin_get_candidate_type(ptr::null_mut(), cand, &raw mut kind)
    });
    assert_eq!(
        kind,
        crate::support::lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR
    );
    close(context, instance);
}

/// `zhuyin_get_candidate_string` never reads `instance` (`zhuyin.cpp:1865-1870`).
#[test]
fn null_instance_candidate_string_answers() {
    let (context, instance) = open();
    let cand = candidate(instance, "su3cl3", 0);
    let mut text = ptr::null();
    assert!(unsafe {
        crate::support::zhuyin_get_candidate_string(ptr::null_mut(), cand, &raw mut text)
    });
    assert!(!text.is_null());
    // SAFETY: the candidate text is NUL-terminated and borrowed from the instance.
    let text = unsafe { std::ffi::CStr::from_ptr(text) };
    assert_eq!(text.to_str().expect("UTF-8"), "你好");
    close(context, instance);
}

/// `zhuyin_get_zhuyin_string` never reads `instance` (`zhuyin.cpp:1732-1741`).
#[test]
fn null_instance_zhuyin_string_answers() {
    let (context, instance) = open();
    let mut key = crate::support::ChewingKey::ZERO;
    assert!(unsafe {
        crate::support::zhuyin_parse_chewing(instance, cstr("su3").as_ptr(), &raw mut key)
    });
    let mut out = ptr::null_mut();
    assert!(unsafe {
        crate::support::zhuyin_get_zhuyin_string(ptr::null_mut(), &raw mut key, &raw mut out)
    });
    assert_eq!(take_sentence(out), "ㄋㄧˇ");
    close(context, instance);
}

/// `zhuyin_begin_add_phrases` stores the context unread and returns the
/// iterator (`zhuyin.cpp:392-398`). `zhuyin_iterator_add_phrase` answers
/// `false` before it reads the context for a NULL phrase (`:509-510`), and
/// where it would read it (`_add_phrase`, `:412`) the pin crashes (row 41);
/// `zhuyin_end_add_phrases` reads it at `:542` (row 41), so the handle is freed.
#[test]
fn null_context_import_handle_is_returned_and_freed() {
    let iter = unsafe { zhuyin_begin_add_phrases(ptr::null_mut(), 1) };
    assert!(!iter.is_null());
    assert!(!unsafe {
        zhuyin_iterator_add_phrase(iter, ptr::null(), cstr("ㄋㄧˇ ㄏㄠˇ").as_ptr(), 1)
    });
    assert!(!unsafe {
        zhuyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("ㄋㄧˇ ㄏㄠˇ").as_ptr(), 1)
    });
    unsafe { zhuyin_end_add_phrases(iter) };
}

/// The index assert (`zhuyin.cpp:381`) precedes any read of the context, so a
/// NULL context aborts for index 16; the `tsi.bin` refusal answers `false`
/// (`:385-386`) without reading it.
#[test]
fn null_context_unload_phrase_library_orders_assert_first() {
    assert!(!unsafe {
        crate::support::zhuyin_unload_phrase_library(
            ptr::null_mut(),
            1, // TSI_DICTIONARY
        )
    });
    crate::support::assert_aborts(
        "null_arguments::null_context_unload_phrase_library_orders_assert_first",
        || {
            unsafe { crate::support::zhuyin_unload_phrase_library(ptr::null_mut(), 16) };
        },
    );
}
