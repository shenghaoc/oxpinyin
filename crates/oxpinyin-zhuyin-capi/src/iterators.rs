//! Import iterator symbols: `zhuyin_begin_add_phrases`,
//! `zhuyin_iterator_add_phrase`, `zhuyin_end_add_phrases`.

use std::os::raw::{c_char, c_int};
use std::ptr;

use oxpinyin_core::graph::FewestKeys;
use oxpinyin_user::{PinyinKey, UserStore, is_user_file_library};

use crate::ffi::{cstr_to_owned_lossy, ffi_catch};
use crate::state::context_ref;
use crate::types::{ImportIterator, ZhuyinContext};

/// State behind `import_iterator_t *`: the target index and the shared user
/// store clone the adds write through.
struct ImportHandle {
    index: u8,
    user: Option<UserStore>,
}

/// Begin adding phrases to an index.
///
/// # C signature
/// ```c
/// import_iterator_t * zhuyin_begin_add_phrases(zhuyin_context_t * context,
///                                              guint8 index);
/// ```
///
/// Returns a handle for any non-null context; caller must call
/// `zhuyin_end_add_phrases` to free it.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_begin_add_phrases(
    context: *mut ZhuyinContext,
    index: u8,
) -> *mut ImportIterator {
    if context.is_null() {
        return ptr::null_mut();
    }
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
        let ctx = unsafe { context_ref(context) };
        let handle = ImportHandle {
            index,
            user: ctx.user_store(),
        };
        Box::into_raw(Box::new(handle)).cast()
    })
}

/// Add a phrase/pinyin pair to the import iterator.
///
/// # C signature
/// ```c
/// bool zhuyin_iterator_add_phrase(import_iterator_t * iter,
///                                 const char * phrase,
///                                 const char * pinyin,
///                                 gint count);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_iterator_add_phrase(
    iter: *mut ImportIterator,
    phrase: *const c_char,
    pinyin: *const c_char,
    count: c_int,
) -> bool {
    if iter.is_null() {
        return false;
    }
    ffi_catch(false, || {
        let phrase = cstr_to_owned_lossy(phrase);
        let pinyin = cstr_to_owned_lossy(pinyin);
        let count = if count == -1 {
            None
        } else if count >= 0 {
            Some(count as u64)
        } else {
            return false;
        };
        // SAFETY: `iter` is non-null and was produced by
        // `zhuyin_begin_add_phrases`.
        let handle = unsafe { &mut *(iter.cast::<ImportHandle>()) };
        if !is_user_file_library(handle.index) {
            return false;
        }
        let Some(user) = handle.user.as_mut() else {
            return false;
        };
        let Some(parsed) = FewestKeys::parse(&pinyin) else {
            return false;
        };
        let keys: Vec<PinyinKey> = parsed
            .keys()
            .iter()
            .map(|key| key.index() as PinyinKey)
            .collect();
        user.add_phrase_in(handle.index, &phrase, &keys, count)
            .is_ok()
    })
}

/// End the import iterator, arm `m_modified`, and free it.
///
/// # C signature
/// ```c
/// void zhuyin_end_add_phrases(import_iterator_t * iter);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_end_add_phrases(iter: *mut ImportIterator) {
    if iter.is_null() {
        return;
    }
    ffi_catch((), || {
        // SAFETY: `iter` was produced by `zhuyin_begin_add_phrases` via
        // `Box::into_raw`; the caller transfers ownership back here.
        let mut handle = unsafe { Box::from_raw(iter.cast::<ImportHandle>()) };
        if let Some(user) = handle.user.as_mut() {
            user.mark_modified();
        }
    });
}
