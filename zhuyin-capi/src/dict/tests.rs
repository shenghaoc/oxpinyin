#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::iterators::{
    zhuyin_begin_add_phrases, zhuyin_end_add_phrases, zhuyin_iterator_add_phrase,
};

use crate::test_support::{TempUserDir, close, cstr, open_with_user, tokens_of, with_store};

use oxpinyin::vocab::USER_DICTIONARY;

use std::ptr;

/// Imports 你好 into libraries `1..=libraries` of the fixture, which loads
/// libraries 1 to 7, so the phrase-table search holds `libraries` tokens.
///
/// # Safety
///
/// `context` is NULL or a live context: `zhuyin_init` returned it and `zhuyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn hold_nihao_in_libraries(context: *mut crate::types::ZhuyinContext, libraries: u8) {
    for library in 1..=libraries {
        let iter = unsafe { zhuyin_begin_add_phrases(context, library) };
        assert!(unsafe {
            zhuyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("ㄋㄧˇ ㄏㄠˇ").as_ptr(), 1)
        });
        unsafe { zhuyin_end_add_phrases(iter) };
    }
}

/// `reduce_tokens` clears and fills the caller's array before it validates
/// the count (`phrase_large_table3.h:79-95`). On a NULL array each GLib
/// call logs a critical: one for `g_array_set_size`, one
/// `g_array_append_vals` per loaded library (`phrase_index.h:762-772`).
/// The pin then dies of the assert on five tokens (`zhuyin.cpp:1690`), so
/// the criticals come first. GLib's default handler writes them to stderr.
#[test]
fn lookup_tokens_into_a_null_array_logs_the_criticals_before_it_aborts() {
    let user_dir = TempUserDir::new("lookup-null-five");
    let path = user_dir.path.clone();
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
            let (context, instance) = open_with_user(&path);
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { hold_nihao_in_libraries(context, 5) };
            unsafe {
                crate::dict::zhuyin_lookup_tokens(instance, cstr("你好").as_ptr(), ptr::null_mut())
            };
        },
    );
    let (context, instance) = open_with_user(&path);
    // SAFETY: `instance` is live until `close` below, and the reference is
    // not used after that.
    let inst = unsafe { crate::state::instance_ref(instance) };
    let loaded = (0..oxpinyin::vocab::PHRASE_INDEX_LIBRARY_COUNT)
        .filter(|&library| {
            inst.core
                .dict
                .phrase_table_token_enabled(oxpinyin::vocab::phrase_index_make_token(library, 0))
        })
        .count();
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
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
/// (074a2219, zhuyin.cpp:1678-1693 and phrase_large_table3.h:77-95).
#[test]
fn null_token_array_keeps_prepared_count_across_reentrant_unload() {
    struct Unload {
        context: *mut crate::types::ZhuyinContext,
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
                Some(unsafe { crate::config::zhuyin_unload_phrase_library(unload.context, 2) });
        } else if message_text.to_bytes().starts_with(b"g_array_append_vals") {
            unload.appends += 1;
        }
    }

    let scratch = tempfile::tempdir().expect("scratch");
    let path = scratch.path();
    let (context, instance) = open_with_user(path);
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    unsafe { hold_nihao_in_libraries(context, 4) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let before = unsafe { tokens_of(instance, "你好") };
    assert_eq!(before.len(), 4);
    assert!(before.iter().any(|token| token >> 24 == 2));
    let prepared =
        {
            // SAFETY: The live instance is serialized and the view ends here.
            let inst = unsafe { crate::state::instance_ref(instance) };
            (0..oxpinyin::vocab::PHRASE_INDEX_LIBRARY_COUNT)
                .filter(|&library| {
                    inst.core.dict.phrase_table_token_enabled(
                        oxpinyin::vocab::phrase_index_make_token(library, 0),
                    )
                })
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
        unsafe { crate::dict::zhuyin_lookup_tokens(instance, c"你好".as_ptr(), ptr::null_mut()) };
    // SAFETY: Remove the handler before reading or dropping its data.
    unsafe { glib_sys::g_log_remove_handler(c"GLib".as_ptr(), handler) };
    assert!(answer);
    assert_eq!(unload.result, Some(true));
    assert_eq!(unload.appends, prepared);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let after = unsafe { tokens_of(instance, "你好") };
    assert_eq!(
        after,
        before
            .into_iter()
            .filter(|token| token >> 24 != 2)
            .collect::<Vec<_>>()
    );

    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

#[test]
fn token_unigram_distinguishes_zero_user_item_from_absent() {
    use crate::dict::{zhuyin_token_add_unigram_frequency, zhuyin_token_get_unigram_frequency};

    let user_dir = TempUserDir::new("dict-unigram-zero");
    let (context, instance) = open_with_user(&user_dir.path);
    let iter = unsafe { zhuyin_begin_add_phrases(context, USER_DICTIONARY) };
    assert!(!iter.is_null());
    assert!(unsafe {
        zhuyin_iterator_add_phrase(iter, cstr("网词").as_ptr(), cstr("ㄨㄤˇ ㄘˊ").as_ptr(), 0)
    });
    unsafe { zhuyin_end_add_phrases(iter) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { with_store(instance, |store| store.phrases()[0].token()) };
    assert_eq!(token >> 24, u32::from(USER_DICTIONARY));
    let mut freq = u32::MAX;
    assert!(unsafe { zhuyin_token_get_unigram_frequency(instance, token, &raw mut freq) });
    assert_eq!(freq, 0);
    freq = u32::MAX;
    assert!(!unsafe { zhuyin_token_get_unigram_frequency(instance, token + 1, &raw mut freq) });
    assert_eq!(freq, 0);
    assert!(unsafe { zhuyin_token_add_unigram_frequency(instance, token, 7) });
    assert!(unsafe { zhuyin_token_get_unigram_frequency(instance, token, &raw mut freq) });
    assert_eq!(freq, 7);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}
