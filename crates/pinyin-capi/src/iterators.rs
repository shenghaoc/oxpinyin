//! Import and export iterator symbols.
//!
//! T7 implements the §9 export surface (`docs/findings/user-store.md`):
//! `pinyin_begin_get_phrases` / `pinyin_iterator_has_next_phrase` /
//! `pinyin_iterator_get_next_phrase` / `pinyin_end_get_phrases`, and the
//! bigram quartet. The import trio stays a stub: nothing in the ABI subset's
//! differential drives it, and its `m_modified` set-site (`:658`) arrives
//! with import proper.

use std::os::raw::{c_char, c_int};
use std::ptr;

use pinyin_user::ExportedPhrase;

use crate::ffi::{ffi_catch, owned_cstr};
use crate::state::{ExportedBigramRow, context_ref};
use crate::types::{
    BigramExportIterator, ExportIterator, GChar, GUint, ImportIterator, PinyinContext,
};

/// State behind `export_iterator_t *`: the materialized §9 rows and the
/// cursor into them. Materializing at begin (rather than streaming) keeps
/// the store transaction scoped to the constructor and matches the
/// snapshot the differential compares.
struct ExportHandle {
    rows: Vec<ExportedPhrase>,
    index: usize,
}

/// State behind `bigram_export_iterator_t *`.
struct BigramHandle {
    rows: Vec<ExportedBigramRow>,
    index: usize,
}

// ── Import iterator ──────────────────────────────────────────────────
//
// Out of scope for T7: the differential drives remember_user_input, not the
// import trio. These stay stubbed (begin returns NULL, add returns false)
// until import lands; upstream's pinyin_end_add_phrases is the other
// m_modified set-site (pinyin.cpp:658), noted for that task.

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
    // STUB: import lands with its m_modified set-site (upstream :658).
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
    // STUB: import lands with its m_modified set-site (upstream :658).
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
    // currently always returns NULL, so this branch is unreachable until
    // import lands; at that point the caller transfers ownership back here
    // and only here, so reconstructing and dropping the Box is sound.
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
///
/// [`USER_DICTIONARY`] exports every user phrase — one row per stored
/// pronunciation, `(phrase, `'`-joined pinyin, pronunciation count)`.
/// Any other index exports nothing: the system sub-indexes are the system
/// dictionary's data, not this store's.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_begin_get_phrases(
    context: *mut PinyinContext,
    index: GUint,
) -> *mut ExportIterator {
    if context.is_null() {
        return ptr::null_mut();
    }
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let ctx = unsafe { context_ref(context) };
        let rows = ctx.export_phrases(index).unwrap_or_default();
        Box::into_raw(Box::new(ExportHandle { rows, index: 0 })).cast()
    })
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
    ffi_catch(false, || {
        // SAFETY: `iter` is non-null and was produced by
        // `pinyin_begin_get_phrases`.
        let handle = unsafe { &*(iter.cast::<ExportHandle>()) };
        handle.index < handle.rows.len()
    })
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
/// Returns `false` once the iterator is exhausted.
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
    ffi_catch(false, || {
        // SAFETY: `iter` is non-null and was produced by
        // `pinyin_begin_get_phrases`; the unique borrow lasts for this call.
        let handle = unsafe { &mut *(iter.cast::<ExportHandle>()) };
        let Some(row) = handle.rows.get(handle.index) else {
            return false;
        };
        if !phrase.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *phrase = owned_cstr(&row.text);
            }
        }
        if !pinyin.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *pinyin = owned_cstr(&row.pinyin);
            }
        }
        if !count.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *count = c_int::try_from(row.count).unwrap_or(c_int::MAX);
            }
        }
        handle.index += 1;
        true
    })
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
    // SAFETY: `iter` was produced by `pinyin_begin_get_phrases` via
    // `Box::into_raw`; the caller transfers ownership back here and only
    // here.
    unsafe {
        drop(Box::from_raw(iter.cast::<ExportHandle>()));
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
///
/// Rows follow upstream's rendering (`pinyin.cpp`): `sentence_start`
/// predecessors are skipped; the phrase is the predecessor's text followed
/// by the successor's text; the pinyin joins the pair's pronunciations with
/// `'`; the count is the stored bigram count × 2.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_begin_get_bigram_phrases(
    context: *mut PinyinContext,
) -> *mut BigramExportIterator {
    if context.is_null() {
        return ptr::null_mut();
    }
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let ctx = unsafe { context_ref(context) };
        let rows = ctx.export_bigram_rows().unwrap_or_default();
        Box::into_raw(Box::new(BigramHandle { rows, index: 0 })).cast()
    })
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
    ffi_catch(false, || {
        // SAFETY: `iter` is non-null and was produced by
        // `pinyin_begin_get_bigram_phrases`.
        let handle = unsafe { &*(iter.cast::<BigramHandle>()) };
        handle.index < handle.rows.len()
    })
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
/// Returns `false` once the iterator is exhausted.
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
    ffi_catch(false, || {
        // SAFETY: `iter` is non-null and was produced by
        // `pinyin_begin_get_bigram_phrases`; the unique borrow lasts for
        // this call.
        let handle = unsafe { &mut *(iter.cast::<BigramHandle>()) };
        let Some(row) = handle.rows.get(handle.index) else {
            return false;
        };
        if !phrase.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *phrase = owned_cstr(&row.phrase);
            }
        }
        if !pinyin.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *pinyin = owned_cstr(&row.pinyin);
            }
        }
        if !count.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *count = c_int::try_from(row.count).unwrap_or(c_int::MAX);
            }
        }
        handle.index += 1;
        true
    })
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
    // SAFETY: `iter` was produced by `pinyin_begin_get_bigram_phrases` via
    // `Box::into_raw`; the caller transfers ownership back here and only
    // here.
    unsafe {
        drop(Box::from_raw(iter.cast::<BigramHandle>()));
    }
}
