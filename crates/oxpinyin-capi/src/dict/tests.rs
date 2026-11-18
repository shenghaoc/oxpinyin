#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase,
};

use crate::state::instance_ref;

use crate::test_support::{TempUserDir, cstr, open};

use std::ptr;

/// Imports 你好 into libraries `1..=libraries` of the fixture, which loads
/// libraries 1 to 7, so the phrase-table search holds `libraries` tokens.
fn hold_nihao_in_libraries(context: *mut crate::types::PinyinContext, libraries: u8) {
    for library in 1..=libraries {
        let iter = unsafe { pinyin_begin_add_phrases(context, library) };
        assert!(unsafe {
            pinyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("ni3'hao3").as_ptr(), 1)
        });
        unsafe { pinyin_end_add_phrases(iter) };
    }
}

/// `reduce_tokens` clears and fills the caller's array before it validates
/// the count (`phrase_large_table3.h:79-95`). On a NULL array each GLib
/// call logs a critical: one for `g_array_set_size`, one
/// `g_array_append_vals` per loaded library (`phrase_index.h:762-772`).
/// The pin then dies of the assert on five tokens (`pinyin.cpp:2664`), so
/// the criticals come first. GLib's default handler writes them to stderr.
#[test]
fn lookup_tokens_into_a_null_array_logs_the_criticals_before_it_aborts() {
    let user_dir = TempUserDir::new("lookup-null-five");
    let path = user_dir.path.to_str().expect("UTF-8 path").to_owned();
    let stderr = crate::test_support::assert_aborts(
        "dict::tests::lookup_tokens_into_a_null_array_logs_the_criticals_before_it_aborts",
        || {
            // SAFETY: GLib's own default handler, installed for every domain.
            unsafe {
                glib_sys::g_log_set_default_handler(
                    Some(glib_sys::g_log_default_handler),
                    ptr::null_mut(),
                );
            }
            let (context, instance) = open(&path);
            hold_nihao_in_libraries(context, 5);
            unsafe {
                crate::dict::pinyin_lookup_tokens(instance, cstr("你好").as_ptr(), ptr::null_mut())
            };
        },
    );
    let (context, instance) = open(&path);
    // SAFETY: `instance` is live until it is freed below, and the
    // reference is not used after that.
    let inst = unsafe { instance_ref(instance) };
    let loaded = (0..16_u32)
        .filter(|library| inst.core.dict.phrase_table_token_enabled(library << 24))
        .count();
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
    let stderr = String::from_utf8_lossy(&stderr);
    let criticals: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("CRITICAL"))
        .collect();
    assert!(loaded > 0);
    assert_eq!(criticals.len(), 1 + loaded, "{stderr}");
    assert!(criticals[0].contains("g_array_set_size"), "{stderr}");
    assert!(
        criticals[1..]
            .iter()
            .all(|line| line.contains("g_array_append_vals")),
        "{stderr}"
    );
}

/// The pin prepares one array per loaded library before the first critical
/// (074a2219, pinyin.cpp:2652-2667 and phrase_large_table3.h:77-95).
#[test]
fn null_token_array_keeps_prepared_count_across_reentrant_unload() {
    struct Unload {
        context: *mut crate::types::PinyinContext,
        result: Option<bool>,
        appends: usize,
    }

    unsafe extern "C" fn unload_on_clear(
        _domain: *const std::ffi::c_char,
        _level: glib_sys::GLogLevelFlags,
        message: *const std::ffi::c_char,
        data: glib_sys::gpointer,
    ) {
        // SAFETY: GLib supplies the diagnostic string and the child keeps
        // its handler data alive until it removes the handler.
        let (message_text, unload) = unsafe {
            (
                std::ffi::CStr::from_ptr(message),
                &mut *data.cast::<Unload>(),
            )
        };
        if unload.result.is_none() && message_text.to_bytes().starts_with(b"g_array_set_size") {
            unload.result = Some(false);
            unload.result =
                Some(unsafe { crate::config::pinyin_unload_phrase_library(unload.context, 2) });
        } else if message_text.to_bytes().starts_with(b"g_array_append_vals") {
            unload.appends += 1;
        }
    }

    let scratch = tempfile::tempdir().expect("scratch");
    let path = scratch.path();
    let (context, instance) = open(path.to_str().expect("UTF-8 path"));
    hold_nihao_in_libraries(context, 4);
    let before = crate::test_support::lookup(instance, "你好");
    assert_eq!(before.len(), 4);
    assert!(before.iter().any(|token| token >> 24 == 2));
    let prepared = {
        // SAFETY: The live instance is serialized and the view ends here.
        let inst = unsafe { crate::state::instance_ref(instance) };
        (0..16_u32)
            .filter(|library| inst.core.dict.phrase_table_token_enabled(library << 24))
            .count()
    };

    let mut unload = Unload {
        context,
        result: None,
        appends: 0,
    };
    // SAFETY: The handler and its data stay live for the synchronous call.
    let handler = unsafe {
        glib_sys::g_log_set_handler(
            c"GLib".as_ptr(),
            glib_sys::G_LOG_LEVEL_CRITICAL,
            Some(unload_on_clear),
            (&raw mut unload).cast(),
        )
    };
    let answer =
        unsafe { crate::dict::pinyin_lookup_tokens(instance, c"你好".as_ptr(), ptr::null_mut()) };
    // SAFETY: Remove the handler before reading or dropping its data.
    unsafe { glib_sys::g_log_remove_handler(c"GLib".as_ptr(), handler) };
    assert!(answer);
    assert_eq!(unload.result, Some(true));
    assert_eq!(unload.appends, prepared);
    let after = crate::test_support::lookup(instance, "你好");
    assert_eq!(
        after,
        before
            .into_iter()
            .filter(|token| token >> 24 != 2)
            .collect::<Vec<_>>()
    );

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// An in-range unreadable reading returns false and empties the C array
/// (compatibility policy row 130), unlike an ordinary out-of-range read.
#[test]
fn an_unreadable_in_range_pronunciation_returns_false_and_an_empty_array() {
    let scratch = tempfile::tempdir().unwrap();
    let system = crate::test_support::truncated_second_reading_system(scratch.path());
    let user = TempUserDir::new("token-overrun");
    let context = crate::test_support::init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = crate::test_support::alloc(context);
    // SAFETY: the array stores packed two-byte keys and is freed below.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    assert!(!unsafe {
        super::pinyin_token_get_nth_pronunciation(instance, 0x0100_0001, 1, keys.cast())
    });
    // SAFETY: the live array was allocated above and is freed once.
    unsafe {
        assert_eq!((*keys).len, 0);
        glib_sys::g_array_free(keys, 1);
    }
    crate::test_support::close(context, instance);
}

/// The stored surrogate is converted to historical UTF-8, with phrase length 3.
/// The journal bytes are held by oxpinyin's persistent journal witness.
#[test]
fn a_journal_surrogate_is_published_as_historical_utf8() {
    let scratch = tempfile::tempdir().unwrap();
    let system = crate::test_support::items_system(
        &crate::test_support::system_dir(),
        scratch.path(),
        &[
            (&[0x7532, 0x4e01, 0x4e59], &[0x1084; 3], 100),
            (&[0x4e19], &[0x1081], 100),
        ],
        200,
    );
    let user = TempUserDir::new("stored-words-surrogate");
    crate::test_support::write_user_conf(&user.path, &system);
    let hex = "380000006f1285e404000000000000000400c8000000800c000001000000030000001c000301b80b00003275000000d80000594e0000841084108410e8030000";
    let bytes: Vec<_> = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    std::fs::write(user.path.join("gb_char.dbin"), bytes).unwrap();
    let context = crate::test_support::init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = crate::test_support::alloc(context);
    let (mut length, mut text) = (0x00ab_cdef, ptr::null_mut());
    assert!(unsafe {
        super::pinyin_token_get_phrase(instance, 0x0100_0003, &raw mut length, &raw mut text)
    });
    // SAFETY: the successful getter returned an owned NUL-terminated GLib buffer.
    unsafe {
        assert_eq!(length, 3);
        assert_eq!(
            std::ffi::CStr::from_ptr(text).to_bytes(),
            &[0xe7, 0x94, 0xb2, 0xed, 0xa0, 0x80, 0xe4, 0xb9, 0x99]
        );
        glib_sys::g_free(text.cast());
    }
    crate::test_support::close(context, instance);
}
