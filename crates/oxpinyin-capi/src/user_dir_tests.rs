//! Several contexts on one user directory, observed through the C ABI only.
//!
//! Upstream holds no state per directory: each `pinyin_init` is a context of
//! its own, and each `pinyin_save` writes that context's whole state, every
//! file written and renamed into place (`pinyin.cpp:1132-1147` at 074a2219).

use std::ffi::{CStr, c_char, c_int};
use std::ptr;

use crate::context::{pinyin_fini, pinyin_save};
use crate::instance::pinyin_free_instance;
use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
    pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_get_next_phrase,
    pinyin_iterator_has_next_phrase,
};
use crate::test_support::{TempUserDir, cstr, open};
use crate::types::{PinyinContext, PinyinInstance};

// The deallocator for buffers the export iterators hand out (libc `malloc`).
unsafe extern "C" {
    fn free(ptr: *mut std::ffi::c_void);
}

/// Reads a caller-owned, NUL-terminated export string and frees it.
fn take_exported(text: *mut c_char) -> String {
    // SAFETY: the iterator hands out a NUL-terminated, valid UTF-8 buffer.
    let owned = unsafe { CStr::from_ptr(text) }
        .to_str()
        .expect("UTF-8 export string")
        .to_owned();
    // SAFETY: the buffer comes from libc `malloc` and is freed once.
    unsafe { free(text.cast()) };
    owned
}

/// The user dictionary's phrases and counts, as the export iterator lists
/// them.
fn exported_phrases(context: *mut PinyinContext) -> Vec<(String, c_int)> {
    let iter = pinyin_begin_get_phrases(context, oxpinyin_user::USER_DICTIONARY.into());
    assert!(!iter.is_null());
    let mut rows = Vec::new();
    while pinyin_iterator_has_next_phrase(iter) {
        let mut phrase: *mut c_char = ptr::null_mut();
        let mut pinyin: *mut c_char = ptr::null_mut();
        let mut count: c_int = 0;
        assert!(pinyin_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count,
        ));
        take_exported(pinyin);
        rows.push((take_exported(phrase), count));
    }
    pinyin_end_get_phrases(iter);
    rows
}

/// Imports `phrase` under `pinyin` into the user dictionary.
fn import(context: *mut PinyinContext, phrase: &str, pinyin: &str, count: c_int) {
    let iter = pinyin_begin_add_phrases(context, oxpinyin_user::USER_DICTIONARY);
    assert!(!iter.is_null());
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr(phrase).as_ptr(),
        cstr(pinyin).as_ptr(),
        count,
    ));
    pinyin_end_add_phrases(iter);
}

fn close(context: *mut PinyinContext, instance: *mut PinyinInstance) {
    pinyin_free_instance(instance);
    pinyin_fini(context);
}

#[test]
fn when_two_sessions_save_the_later_save_is_the_profile() {
    // Both contexts learn and both save: each writes its own whole state,
    // so the profile is the later save's and the earlier context's learning
    // is gone from it, whichever context saves last.
    for first_saves_last in [false, true] {
        let user_dir = TempUserDir::new("two-saves");
        let dir = user_dir.path.to_str().expect("UTF-8 path");
        let (first, first_instance) = open(dir);
        let (second, second_instance) = open(dir);
        import(first, "你好", "nihao", 5);
        import(second, "世界", "shijie", 3);
        assert_eq!(exported_phrases(first), [("你好".to_owned(), 5)]);
        assert_eq!(exported_phrases(second), [("世界".to_owned(), 3)]);

        let (earlier, later) = if first_saves_last {
            (second, first)
        } else {
            (first, second)
        };
        assert!(pinyin_save(earlier));
        assert!(pinyin_save(later));
        let expected = exported_phrases(later);
        close(first, first_instance);
        close(second, second_instance);

        let (reopened, reopened_instance) = open(dir);
        assert_eq!(
            exported_phrases(reopened),
            expected,
            "first saves last: {first_saves_last}"
        );
        close(reopened, reopened_instance);
    }
}
