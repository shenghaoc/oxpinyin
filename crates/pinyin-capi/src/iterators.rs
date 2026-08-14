//! Import and export iterator symbols.

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::types::{
    BigramExportIterator, ExportIterator, GChar, GUint, ImportIterator, PinyinContext,
};

// ── Import iterator ──────────────────────────────────────────────────

/// Begin adding phrases to an index.
///
/// # C signature
/// ```c
/// import_iterator_t * pinyin_begin_add_phrases(pinyin_context_t * context,
///                                              guint8 index);
/// ```
///
/// Returns a handle; caller must call `pinyin_end_add_phrases` to free.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_begin_add_phrases(
    context: *mut PinyinContext,
    _index: u8,
) -> *mut ImportIterator {
    if context.is_null() {
        return ptr::null_mut();
    }
    // STUB: T4 will implement.
    ptr::null_mut()
}

/// Add a phrase/pinyin pair to the import iterator.
///
/// # C signature
/// ```c
/// bool pinyin_iterator_add_phrase(import_iterator_t * iter,
///                                 const char * phrase,
///                                 const char * pinyin,
///                                 gint count);
/// ```
///
/// `count` of -1 means use the default value.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_iterator_add_phrase(
    iter: *mut ImportIterator,
    _phrase: *const c_char,
    _pinyin: *const c_char,
    _count: c_int,
) -> bool {
    if iter.is_null() {
        return false;
    }
    // STUB: T4 will implement.
    false
}

/// End the import iterator and free it.
///
/// # C signature
/// ```c
/// void pinyin_end_add_phrases(import_iterator_t * iter);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_end_add_phrases(iter: *mut ImportIterator) {
    if iter.is_null() {
        return;
    }
    // SAFETY: `iter` is non-null (guarded above). `pinyin_begin_add_phrases`
    // currently always returns NULL (T1 stub), so this branch is unreachable
    // until T4 makes the constructor return `Box::into_raw(..)`. At that point
    // the caller transfers ownership back here and only here, so reconstructing
    // and dropping the Box is sound.
    unsafe {
        drop(Box::from_raw(iter));
    }
}

// ── Export iterator (unigram phrases) ────────────────────────────────

/// Begin exporting phrases from an index.
///
/// # C signature
/// ```c
/// export_iterator_t * pinyin_begin_get_phrases(pinyin_context_t * context,
///                                              guint index);
/// ```
///
/// Note: the index parameter is `guint` (not `guint8`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_begin_get_phrases(
    context: *mut PinyinContext,
    _index: GUint,
) -> *mut ExportIterator {
    if context.is_null() {
        return ptr::null_mut();
    }
    // STUB: T4 will implement.
    ptr::null_mut()
}

/// Check whether the export iterator has a next phrase.
///
/// # C signature
/// ```c
/// bool pinyin_iterator_has_next_phrase(export_iterator_t * iter);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_iterator_has_next_phrase(iter: *mut ExportIterator) -> bool {
    if iter.is_null() {
        return false;
    }
    // STUB: T4 will implement.
    false
}

/// Get the next phrase from the export iterator.
///
/// # C signature
/// ```c
/// bool pinyin_iterator_get_next_phrase(export_iterator_t * iter,
///                                     gchar ** phrase,
///                                     gchar ** pinyin,
///                                     gint * count);
/// ```
///
/// Out-params `phrase` and `pinyin` are caller-owned (`g_free` each).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_iterator_get_next_phrase(
    iter: *mut ExportIterator,
    phrase: *mut *mut GChar,
    pinyin: *mut *mut GChar,
    count: *mut c_int,
) -> bool {
    if iter.is_null() {
        return false;
    }
    if !phrase.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *phrase = ptr::null_mut();
        }
    }
    if !pinyin.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *pinyin = ptr::null_mut();
        }
    }
    if !count.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *count = 0;
        }
    }
    // STUB: T4 will implement.
    false
}

/// End the export iterator and free it.
///
/// # C signature
/// ```c
/// void pinyin_end_get_phrases(export_iterator_t * iter);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_end_get_phrases(iter: *mut ExportIterator) {
    if iter.is_null() {
        return;
    }
    // SAFETY: `iter` is non-null (guarded above). `pinyin_begin_get_phrases`
    // currently always returns NULL (T1 stub), so this branch is unreachable
    // until T4 makes the constructor return `Box::into_raw(..)`. At that point
    // the caller transfers ownership back here and only here, so reconstructing
    // and dropping the Box is sound.
    unsafe {
        drop(Box::from_raw(iter));
    }
}

// ── Bigram export iterator ───────────────────────────────────────────

/// Begin exporting bigram phrases.
///
/// # C signature
/// ```c
/// bigram_export_iterator_t * pinyin_begin_get_bigram_phrases(
///     pinyin_context_t * context);
/// ```
///
/// Note: no index parameter (unlike unigram export).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_begin_get_bigram_phrases(
    context: *mut PinyinContext,
) -> *mut BigramExportIterator {
    if context.is_null() {
        return ptr::null_mut();
    }
    // STUB: T4 will implement.
    ptr::null_mut()
}

/// Check whether the bigram export iterator has a next phrase.
///
/// # C signature
/// ```c
/// bool pinyin_bigram_iterator_has_next_phrase(
///     bigram_export_iterator_t * iter);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_bigram_iterator_has_next_phrase(iter: *mut BigramExportIterator) -> bool {
    if iter.is_null() {
        return false;
    }
    // STUB: T4 will implement.
    false
}

/// Get the next phrase from the bigram export iterator.
///
/// # C signature
/// ```c
/// bool pinyin_bigram_iterator_get_next_phrase(
///     bigram_export_iterator_t * iter,
///     gchar ** phrase, gchar ** pinyin, gint * count);
/// ```
///
/// Out-params `phrase` and `pinyin` are caller-owned (`g_free` each).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_bigram_iterator_get_next_phrase(
    iter: *mut BigramExportIterator,
    phrase: *mut *mut GChar,
    pinyin: *mut *mut GChar,
    count: *mut c_int,
) -> bool {
    if iter.is_null() {
        return false;
    }
    if !phrase.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *phrase = ptr::null_mut();
        }
    }
    if !pinyin.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *pinyin = ptr::null_mut();
        }
    }
    if !count.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *count = 0;
        }
    }
    // STUB: T4 will implement.
    false
}

/// End the bigram export iterator and free it.
///
/// # C signature
/// ```c
/// void pinyin_end_get_bigram_phrases(bigram_export_iterator_t * iter);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_end_get_bigram_phrases(iter: *mut BigramExportIterator) {
    if iter.is_null() {
        return;
    }
    // SAFETY: `iter` is non-null (guarded above). `pinyin_begin_get_bigram_phrases`
    // currently always returns NULL (T1 stub), so this branch is unreachable
    // until T4 makes the constructor return `Box::into_raw(..)`. At that point
    // the caller transfers ownership back here and only here, so reconstructing
    // and dropping the Box is sound.
    unsafe {
        drop(Box::from_raw(iter));
    }
}
