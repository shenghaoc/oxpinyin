//! Import and export iterator symbols.
//!
//! W6-T7 implemented the §9 export surface (`docs/findings/user-store.md`):
//! `pinyin_begin_get_phrases` / `pinyin_iterator_has_next_phrase` /
//! `pinyin_iterator_get_next_phrase` / `pinyin_end_get_phrases`, and the
//! bigram quartet. W7-T1 replaces the import trio stubs with the same
//! Box-handle discipline: `pinyin_begin_add_phrases` /
//! `pinyin_iterator_add_phrase` / `pinyin_end_add_phrases`.

use std::os::raw::{c_char, c_int};
use std::ptr;

use pinyin_core::SyllableKey;
use pinyin_core::graph::{Edge, EdgeKind, SegmentGraph};
use pinyin_user::{ExportedPhrase, PinyinKey, USER_DICTIONARY, UserStore};

use crate::ffi::{cstr_to_owned_lossy, ffi_catch, owned_cstr};
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
// Upstream batch semantics (`pinyin.cpp:506-659`, read for W7-T1): the
// "batch" is **per-phrase**, not atomic. `pinyin_iterator_add_phrase` calls
// `_add_phrase` immediately, which mutates the in-memory phrase/pinyin
// tables and the unigram count; `pinyin_end_add_phrases` only compacts the
// phrase index, sets `m_modified = true`, and deletes the iterator. There is
// no rollback if a later add fails. redb models that exactly: each
// successful add is its own committed write transaction; end has no data
// write, only the dirty-flag set-site.

/// State behind `import_iterator_t *`: the target index and the shared user
/// store clone the adds write through. The clone also carries the shared
/// §4 dirty flag, so `pinyin_end_add_phrases` can arm `m_modified` without
/// keeping a raw context pointer alive behind the C caller's back.
struct ImportHandle {
    index: u8,
    user: Option<UserStore>,
}

/// Begin adding phrases to an index.
///
/// # C signature
/// ```c
/// import_iterator_t * pinyin_begin_add_phrases(pinyin_context_t * context,
///                                              guint8 index);
/// ```
///
/// Returns a handle for any non-null context, matching the export iterator
/// shape; caller must call `pinyin_end_add_phrases` to free it. Adds target
/// [`USER_DICTIONARY`] only — pinyin-rs's system phrase indexes are
/// read-only redb tables — so any other index yields a handle whose adds
/// report `false`.
pub(crate) fn begin_add_phrases_impl(
    context: *mut PinyinContext,
    index: u8,
) -> *mut ImportIterator {
    if context.is_null() {
        return ptr::null_mut();
    }
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init` or
        // `CapiContext::new_user_only`; the borrow lasts only for this
        // constructor.
        let ctx = unsafe { context_ref(context) };
        let handle = ImportHandle {
            index,
            user: ctx.user_store(),
        };
        Box::into_raw(Box::new(handle)).cast()
    })
}

/// Begin adding phrases to an index.
///
/// # C signature
/// ```c
/// import_iterator_t * pinyin_begin_add_phrases(pinyin_context_t * context,
///                                              guint8 index);
/// ```
///
/// Body and handle contract: [`begin_add_phrases_impl`].
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_begin_add_phrases(
    context: *mut PinyinContext,
    index: u8,
) -> *mut ImportIterator {
    begin_add_phrases_impl(context, index)
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
/// `count` of -1 means use the default value. The pinyin is parsed with the
/// frozen untuned full-pinyin inventory under upstream's longest-parsed-prefix
/// then fewest-keys rule (`pinyin_parser2.cpp` selection, represented here by
/// [`SegmentGraph`] with incomplete edges filtered out). A phrase whose
/// character count does not equal the key count reports `false`; trailing
/// unparsed pinyin bytes are ignored exactly as upstream's parser does.
/// Negative counts other than -1 are rejected rather than reproduced as the
/// upstream `guint32` wrap (the pin segfaults on them).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_iterator_add_phrase(
    iter: *mut ImportIterator,
    phrase: *const c_char,
    pinyin: *const c_char,
    count: c_int,
) -> bool {
    if iter.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // `cstr_to_owned_lossy` is the C ABI entry point's string
        // marshaller: null reads as empty, which validation rejects.
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
        // `pinyin_begin_add_phrases`; the unique borrow lasts for this call.
        let handle = unsafe { &mut *(iter.cast::<ImportHandle>()) };
        if handle.index != USER_DICTIONARY {
            return false;
        }
        let Some(user) = handle.user.as_mut() else {
            return false;
        };
        let Some(keys) = parse_import_pinyin(&pinyin) else {
            return false;
        };
        user.add_phrase(&phrase, &keys, count).is_ok()
    })
}

/// Parse `pinyin` the way the import path consumes it: longest parsed prefix
/// from byte zero, fewest complete keys to reach it, first edge winning key
/// ties. Incomplete initial-only keys are not admitted (`pinyin_parser2.cpp`
/// only reaches keys in the full-pinyin index; the oracle rejects `n` here).
pub(crate) fn parse_import_pinyin(pinyin: &str) -> Option<Vec<PinyinKey>> {
    let graph = SegmentGraph::build(pinyin.as_bytes()).ok()?;
    let bound = graph.consumed();
    // steps[node] = (key count, incoming edge, previous node); node 0 is the
    // root with count zero.
    let mut steps: Vec<Option<(usize, Edge, usize)>> = vec![None; bound + 1];
    for node in 0..bound {
        let count = if node == 0 {
            0
        } else {
            match steps[node] {
                Some((count, _, _)) => count,
                None => continue,
            }
        };
        for edge in graph
            .outgoing(node)
            .iter()
            .copied()
            .filter(|edge| edge.kind() != EdgeKind::Incomplete)
        {
            let to = edge.to();
            if to > bound {
                continue;
            }
            let candidate = count + 1;
            if steps[to]
                .as_ref()
                .is_none_or(|(seen, _, _)| candidate < *seen)
            {
                steps[to] = Some((candidate, edge, node));
            }
        }
    }

    let mut keys = Vec::new();
    let mut node = bound;
    while node > 0 {
        let (_, edge, previous) = steps[node]?;
        keys.push(SyllableKey::from_index(edge.key().index())?.index() as PinyinKey);
        node = previous;
    }
    keys.reverse();
    Some(keys)
}

/// End the import iterator, arm `m_modified`, and free it.
///
/// # C signature
/// ```c
/// void pinyin_end_add_phrases(import_iterator_t * iter);
/// ```
///
/// Upstream compacts the phrase index here and sets `m_modified = true`
/// (`pinyin.cpp:657-658`) whether or not any add succeeded. pinyin-rs has no
/// in-memory phrase chunk to compact; the redb adds are already durable. The
/// dirty-flag arm is the whole persistence-side effect, so the next
/// `pinyin_save` compacts and clears (`docs/findings/user-store.md` §4).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_end_add_phrases(iter: *mut ImportIterator) {
    if iter.is_null() {
        return;
    }
    // SAFETY: `iter` was produced by `pinyin_begin_add_phrases` via
    // `Box::into_raw`; the caller transfers ownership back here and only
    // here.
    let mut handle = unsafe { Box::from_raw(iter.cast::<ImportHandle>()) };
    if let Some(user) = handle.user.as_mut() {
        user.mark_modified();
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
