//! Import and export iterator symbols.
//!
//! Export (`docs/findings/user-store.md` §9): `pinyin_begin_get_phrases` and
//! the bigram quartet. Import: `pinyin_begin_add_phrases` /
//! `pinyin_iterator_add_phrase` / `pinyin_end_add_phrases`.

use std::os::raw::{c_char, c_int};
use std::ptr;

use oxpinyin_core::graph::SegmentGraph;
use oxpinyin_core::{OptionBits, PINYIN_CORRECT_ALL, USE_TONE};
use oxpinyin_facade::{BigramExportWalk, BigramStep, ExportStep};
use oxpinyin_user::{MAX_PHRASE_LENGTH, PinyinKey, UserStore, is_user_file_library, toned_key};

use crate::ffi::{
    cstr_to_owned_lossy, cstr_to_parsed_prefix, malloc_cstr_with, malloc_cstr_with_warning,
    owned_cstr,
};
use crate::state::context_ref;
use crate::types::{
    BigramExportIterator, ExportIterator, GChar, GUint, ImportIterator, PinyinContext,
};
use oxpinyin_capi_marshal::phrase_text::phrase_text_bytes;

/// State behind `export_iterator_t *`: the pin's iterator state, which
/// reads each item when a call needs it and buffers no row.
struct ExportHandle {
    cursor: oxpinyin_facade::PhraseExportCursor,
}

/// State behind `bigram_export_iterator_t *`.
struct BigramHandle {
    walk: BigramExportWalk,
}

// ── Import iterator ──────────────────────────────────────────────────
//
// Adds commit immediately; there is no rollback if a later add fails.
// `pinyin_end_add_phrases` only arms `m_modified` so the next `pinyin_save`
// writes (`docs/findings/user-store.md` §4).

/// State behind `import_iterator_t *`: the target index and the shared user
/// store clone the adds write through. The clone also carries the shared
/// §4 dirty flag, so `pinyin_end_add_phrases` can arm `m_modified` without
/// keeping a raw context pointer alive behind the C caller's back.
struct ImportHandle {
    index: u8,
    user: Option<UserStore>,
    dict: Option<oxpinyin_runtime::RuntimeDict>,
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
/// the loaded system libraries 1–4 and USER_FILE libraries 5–7.
pub fn begin_add_phrases_impl(context: *mut PinyinContext, index: u8) -> *mut ImportIterator {
    if context.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init` or
    // `CapiContext::new_user_only`; the borrow lasts only for this
    // constructor.
    let ctx = unsafe { context_ref(context) };
    // `begin_add_phrases` starts with a compact (`pinyin.cpp:657`,
    // `zhuyin.cpp:542`); a layout-failed empty sub-index's local total
    // becomes zero there.
    let _compacted = ctx.core.compact_layout_failed();
    let handle = ImportHandle {
        index,
        user: ctx.user_store(),
        dict: ctx.core.runtime.as_ref().map(|runtime| runtime.dict()),
    };
    Box::into_raw(Box::new(handle)).cast()
}

/// Begin adding phrases to an index.
///
/// # C signature
/// ```c
/// import_iterator_t * pinyin_begin_add_phrases(pinyin_context_t * context,
///                                              guint8 index);
/// ```
///
/// Body and handle contract: `begin_add_phrases_impl`.
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
/// `pinyin.cpp:614-652` then `_add_phrase` (`:514-611`): the reading is
/// parsed with `FullPinyinParser2` under `PINYIN_CORRECT_ALL | USE_TONE`
/// (`:630`) — tone digits are kept on the keys, corrections apply
/// (`jv` → `ju`), trailing unparsed bytes are ignored — and the phrase's
/// character count must equal the key count, `0 < len < 16`. `count` of
/// -1 is the default 5; any other value reaches the item as its `guint32`
/// bit pattern (`-2` stores 4294967294 and exports as -2).
///
/// Libraries: loaded system sub-indexes (1..=4) and the `USER_FILE`
/// sub-indexes (5, 6, 7) take the phrase. Every other nibble has no sub-index
/// (`get_range` → `ERROR_NO_SUB_PHRASE_INDEX`) and answers `false`, and
/// 16..=255 — an out-of-bounds read of the pin's 16-slot array
/// (`phrase_index.h:630`) — answer `false` without reproducing it.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_iterator_add_phrase(
    iter: *mut ImportIterator,
    phrase: *const c_char,
    pinyin: *const c_char,
    count: c_int,
) -> bool {
    if iter.is_null() || phrase.is_null() || pinyin.is_null() {
        return false;
    }

    // `cstr_to_owned_lossy` is the C ABI entry point's string
    // marshaller.
    let phrase = cstr_to_owned_lossy(phrase);
    // The reading is parsed from the raw bytes (`pinyin.cpp:638`): the
    // keys before an invalid sequence stand, as they do at the pin.
    let pinyin = cstr_to_parsed_prefix(pinyin);
    // `if (-1 == count) count = default_count;` (`pinyin.cpp:523-524`);
    // otherwise the `gint` is used as the `guint32` it becomes.
    let count = (count != -1).then(|| u64::from(count.cast_unsigned()));

    // SAFETY: `iter` is non-null and was produced by
    // `pinyin_begin_add_phrases`; the unique borrow lasts for this call.
    let handle = unsafe { &mut *(iter.cast::<ImportHandle>()) };
    let Some(user) = handle.user.as_mut() else {
        return false;
    };
    let Some(keys) = parse_import_pinyin(&pinyin) else {
        return false;
    };
    // The iterator validates the phrase/key lengths before _add_phrase:
    // 074a2219 pinyin.cpp:639-646 / zhuyin.cpp:525-532.
    let length = phrase.chars().count();
    if length == 0 || length >= MAX_PHRASE_LENGTH || length != keys.len() {
        return false;
    }
    // Search once and preserve both tables' token multiplicity before
    // reduce_tokens validates the combined count (074a2219
    // phrase_large_table3.h:77-95).
    let tokens = match phrase_table_tokens(user, handle.dict.as_ref(), &phrase) {
        Ok(tokens) => tokens,
        Err(()) => {
            crate::ffi::log_warning("pinyin_iterator_add_phrase: phrase-table lookup failed");
            return false;
        }
    };
    if tokens.len() > 4 {
        crate::ffi::log_warning(
            "pinyin_iterator_add_phrase: assertion '0 <= num && num <= 4' failed",
        );
        return false;
    }
    // The pin reduces tokens before get_range rejects an unused library.
    if !is_user_file_library(handle.index) && !(1..=4).contains(&handle.index) {
        return false;
    }
    // 074a2219 pinyin.cpp:533-571: the pin then walks the phrase table's
    // tokens for the phrase. Two tokens in the same sub-index library trip
    // `assert(PHRASE_INDEX_LIBRARY_INDEX(token) != index)` (`:554`); a lone
    // one whose phrase-index item has another length trips
    // `assert(phrase_length == item.get_phrase_length())` (`:568`), and one
    // of the same length with other text the `memcmp` assert (`:571`). The
    // phrase table is the `user_phrase_index` membership plus the system
    // phrase index; the item text is read without its pronunciation view. Each refusal
    // is one `libpinyin` warning; a lookup that fails outright refuses before
    // either add call rather than reading an empty token list.
    let same_library: Vec<u32> = tokens
        .into_iter()
        .filter(|token| token >> 24 == u32::from(handle.index))
        .collect();
    if same_library.len() > 1 {
        crate::ffi::log_warning(
            "pinyin_iterator_add_phrase: assertion \
             'PHRASE_INDEX_LIBRARY_INDEX(token) != index' failed",
        );
        return false;
    }
    if let Some(&token) = same_library.first() {
        let item_text = if let Some(dict) = handle.dict.as_ref() {
            dict.token_text(token)
        } else {
            user.phrase_text(token).ok().flatten().and_then(|words| {
                oxpinyin_core::PhraseText::from_stored_words(words.into_iter().map(Some))
            })
        };
        // A missing item leaves the pin's PhraseItem at length zero;
        // :568 then asserts before any import mutation.
        // The stored length first, then every stored word (`pinyin.cpp:568-571`).
        let same_length = item_text
            .as_ref()
            .is_some_and(|text| text.phrase_length() == length);
        let same_words = item_text.as_ref().is_some_and(|text| {
            oxpinyin_capi_marshal::phrase_text::stored_words_match(text, &phrase)
        });
        if !(same_length && same_words) {
            crate::ffi::log_warning(if same_length {
                "pinyin_iterator_add_phrase: assertion \
             '0 == memcmp(phrase, tmp_phrase, sizeof(ucs4_t) * phrase_length)' failed"
            } else {
                "pinyin_iterator_add_phrase: assertion \
             'phrase_length == item.get_phrase_length()' failed"
            });
            return false;
        }
    }
    if (1..=4).contains(&handle.index) {
        let Some(dict) = handle.dict.as_ref() else {
            return false;
        };
        if !dict.library_visible(u32::from(handle.index)) {
            return false;
        }
        // The selected token comes from the combined phrase-table search,
        // including prior imported system-library items.
        let original = same_library.first().copied();
        user.add_system_phrase_in(handle.index, original, &phrase, &keys, count)
            .is_ok()
    } else {
        user.add_phrase_in(handle.index, &phrase, &keys, count)
            .is_ok()
    }
}

/// Search the system table followed by the user table, then concatenate
/// the per-library arrays as reduce_tokens does at 074a2219
/// phrase_large_table3.h:77-95. Duplicates remain observable.
///
/// # Errors
///
/// Returns `Err(())` if either table lookup fails.
fn phrase_table_tokens(
    user: &UserStore,
    dict: Option<&oxpinyin_runtime::RuntimeDict>,
    phrase: &str,
) -> Result<Vec<u32>, ()> {
    let mut tokens = if let Some(dict) = dict {
        dict.system().tokens_for_text(phrase).map_err(|_| ())?
    } else {
        Vec::new()
    };
    tokens.extend(user.phrase_table_tokens_for_text(phrase).map_err(|_| ())?);
    if let Some(dict) = dict {
        tokens.retain(|token| dict.phrase_table_token_enabled(*token));
    }
    tokens.sort_by_key(|token| token >> 24);
    Ok(tokens)
}

/// `FullPinyinParser2::parse` under `PINYIN_CORRECT_ALL | USE_TONE`
/// (`pinyin.cpp:629-637`, `pinyin_parser2.cpp:217-381`): the longest
/// parsable prefix, fewest keys, as toned store keys. `None` when the
/// input is past the graph limit or a key does not fit the store's key
/// layout.
fn parse_import_pinyin(pinyin: &str) -> Option<Vec<PinyinKey>> {
    let options = OptionBits::from_bits(PINYIN_CORRECT_ALL | USE_TONE);
    let graph = SegmentGraph::build_with_options(pinyin.as_bytes(), options).ok()?;
    graph
        .fewest_keys(false)
        .iter()
        .map(|edge| toned_key(edge.key().index(), edge.tone()))
        .collect()
}

/// End the import iterator, arm `m_modified`, and free it.
///
/// # C signature
/// ```c
/// void pinyin_end_add_phrases(import_iterator_t * iter);
/// ```
///
/// Upstream compacts the phrase index here and sets `m_modified = true`
/// (`pinyin.cpp:657-658`) whether or not any add succeeded. oxpinyin has no
/// in-memory phrase chunk to compact; the adds are already durable. What the
/// compact changes is each library's total, rebuilt from its items, which
/// the store's `compact_libraries` applies. The dirty-flag arm is the rest
/// of the persistence-side effect, so the next `pinyin_save` compacts and
/// clears (`docs/findings/user-store.md` §4).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_end_add_phrases(iter: *mut ImportIterator) {
    if iter.is_null() {
        return;
    }

    // SAFETY: `iter` was produced by `pinyin_begin_add_phrases` via
    // `Box::into_raw`; the caller transfers ownership back here and
    // only here.
    let mut handle = unsafe { Box::from_raw(iter.cast::<ImportHandle>()) };
    if let Some(user) = handle.user.as_mut() {
        // The compact (`pinyin.cpp:657`) rebuilds each library's total from
        // its items.
        user.compact_libraries();
        user.mark_modified();
    };
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
/// Note: the index parameter is `guint` (not `guint8`), narrowed to the
/// pin's `guint8` iterator field (`pinyin.cpp:126`).
///
/// Every sub-index the pin's default facade holds exports its items — one
/// row per pronunciation, `(phrase, `'`-joined pinyin, pronunciation
/// count)`, token order (`pinyin.cpp:662-768`): the system libraries 1..=4
/// from their chunk files, the `USER_FILE` libraries from the user store.
/// A nibble without a sub-index exports nothing
/// ([`oxpinyin_facade::ContextCore::export_phrases`]).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_begin_get_phrases(
    context: *mut PinyinContext,
    index: GUint,
) -> *mut ExportIterator {
    if context.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_ref(context) };
    // The pin's begin cannot fail: its sub-index is in memory. A store the
    // probe cannot read answers the export's own failure shape, one
    // `libpinyin` warning and a NULL iterator, rather than an empty walk a
    // consumer would write out as its snapshot.
    let Ok(cursor) = ctx.core.phrase_export_cursor(index) else {
        crate::ffi::log_warning("pinyin_begin_get_phrases: user store read failed");
        return ptr::null_mut();
    };
    Box::into_raw(Box::new(ExportHandle { cursor })).cast()
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

    // SAFETY: `iter` is non-null and was produced by
    // `pinyin_begin_get_phrases`.
    let handle = unsafe { &*(iter.cast::<ExportHandle>()) };
    handle.cursor.has_next()
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
/// The pin's `:698-768`: the probed item is read, its current pronunciation
/// rendered, and the next token probed through a fresh `get_range` before
/// the call returns, so an import that reached the next item in between is
/// exported. Out-params `phrase` and `pinyin` are caller-owned (`g_free`
/// each), one allocation apiece as the pin's `g_ucs4_to_utf8` and
/// `g_strjoinv` results are; `phrase` is NULL where the pin's
/// `g_ucs4_to_utf8` hands one out (`:715-716`, a stored code point that is
/// no scalar value). Returns `false` once the iterator is exhausted, at
/// one of the pin's assert sites (one warning naming the expression), or
/// when the store cannot be read (one warning, no pin counterpart).
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

    // SAFETY: `iter` is non-null and was produced by
    // `pinyin_begin_get_phrases`; the unique borrow lasts for this call.
    let handle = unsafe { &mut *(iter.cast::<ExportHandle>()) };
    let step = handle.cursor.get_next_with(|row| {
        let text = (!phrase.is_null()).then(|| {
            row.text_len().map_or(ptr::null_mut(), |len| {
                malloc_cstr_with(len, |out| row.write_text(out))
            })
        });
        let reading = (!pinyin.is_null())
            .then(|| malloc_cstr_with_warning(row.pinyin_len(), |out| row.write_pinyin(out)));
        (text, reading, export_count(row.count()))
    });
    let step = match step {
        Ok(step) => step,
        // No pin counterpart (its sub-index is in memory): one warning,
        // `false`, the out-params untouched — the shape of the add
        // iterator's lookup that fails outright.
        Err(_) => {
            crate::ffi::log_warning("pinyin_iterator_get_next_phrase: user store read failed");
            return false;
        }
    };
    let (text, reading, frequency) = match step {
        ExportStep::Row(row) => row,
        // Class (c), compatibility-policy row 62: the pin **`assert`**s at
        // the site (`pinyin.cpp:709`, `:714`, `:721`, `:724`, or
        // `chewing_key.cpp:48`/`:50` from `:728`). One warning naming the
        // asserted expression, `false`, the out-params untouched; the
        // cursor is unmoved, so a repeated call repeats the line.
        ExportStep::Assert(site) => {
            crate::ffi::log_warning(&format!(
                "pinyin_iterator_get_next_phrase: assertion '{}' failed",
                site.expression()
            ));
            return false;
        }
    };
    if reading.is_some_and(|value| value.is_null()) {
        if let Some(text) = text {
            // SAFETY: This call owns the malloc buffer (or null); no
            // out-param has received it. libc free matches its allocator.
            unsafe { crate::ffi::free(text.cast()) };
        }
        return false;
    }
    if let Some(text) = text {
        // SAFETY: `phrase` was null-checked when `text` was produced.
        unsafe {
            *phrase = text;
        }
    }
    if let Some(reading) = reading {
        // SAFETY: `pinyin` was null-checked when `reading` was produced.
        unsafe {
            *pinyin = reading;
        }
    }
    if !count.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *count = frequency;
        }
    }
    true
}

/// The `gint` a stored pronunciation count exports as: `-1` ("default")
/// for a zero frequency, else the `guint32` frequency's bit pattern
/// (`*count = -1; … if (freq > 0) *count = freq;`, `pinyin.cpp:715`,
/// `:738-739`).
fn export_count(count: u64) -> c_int {
    let freq = u32::try_from(count).unwrap_or(u32::MAX);
    if freq == 0 { -1 } else { freq.cast_signed() }
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
    // SAFETY: `iter` was produced by its begin call via
    // `Box::into_raw`; the caller transfers ownership back here and
    // only here.
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

    // SAFETY: `context` is non-null and was produced by `pinyin_init`
    // or `CapiContext::new_user_only`.
    let ctx = unsafe { context_ref(context) };
    if !ctx.can_render_export_bigrams() {
        return ptr::null_mut();
    }
    // Class (c), `ngram_bdb.cpp:199`: the user bigram carries a key that is
    // not a phrase_token_t and the pin's `get_all_items` assert dies. One
    // warning and a NULL iterator, the export's own failure shape.
    if ctx.has_non_token_bigram_key() {
        crate::ffi::log_warning(
            "pinyin_begin_get_bigram_phrases: a user bigram key is not a phrase_token_t \
             (upstream aborts, ngram_bdb.cpp:199)",
        );
        return ptr::null_mut();
    }
    // Class (c), `memory_chunk.h:390`: `retrieve_all` reads the four-byte
    // total before its item walk. A short row under any predecessor makes
    // the pin assert when the iterator advances; refuse the export here.
    if ctx.core.user.as_ref().is_some_and(|store| {
        store.bigram_predecessors().is_ok_and(|predecessors| {
            predecessors
                .iter()
                .any(|&token| store.has_short_bigram_value_for(token))
        })
    }) {
        crate::ffi::log_warning(
            "pinyin_begin_get_bigram_phrases: a user bigram value is shorter than a guint32 \
             total_freq (upstream aborts, memory_chunk.h:390)",
        );
        return ptr::null_mut();
    }
    // Class (b), `ngram.cpp:133-143`: `retrieve_all` walks with
    // `cur != end`, so a partial item tail is read past its end. The
    // export iterator loads every predecessor; refuse it with one warning.
    if ctx.core.user.as_ref().is_some_and(|store| {
        store.bigram_predecessors().is_ok_and(|predecessors| {
            predecessors
                .iter()
                .any(|&token| store.has_oob_bigram_value_for(token))
        })
    }) {
        crate::ffi::log_warning(
            "pinyin_begin_get_bigram_phrases: a user bigram value has a partial item tail \
             (upstream reads past it, ngram.cpp:133-143)",
        );
        return ptr::null_mut();
    }
    let walk = ctx.bigram_export_walk().unwrap_or_default();
    Box::into_raw(Box::new(BigramHandle { walk })).cast()
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

    // SAFETY: `iter` is non-null and was produced by
    // `pinyin_begin_get_bigram_phrases`; the unique borrow lasts for this
    // call (the pin's `has_next` advances the iterator).
    let handle = unsafe { &mut *(iter.cast::<BigramHandle>()) };
    handle.walk.has_next()
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
/// Returns what `pinyin_bigram_iterator_has_next_phrase` answers after
/// the row is taken — `false` on the last row — as the pin's
/// `return pinyin_bigram_iterator_has_next_phrase(iter);` does
/// (`pinyin.cpp:896-911`, register row 36); `false` once exhausted.
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

    // SAFETY: `iter` is non-null and was produced by
    // `pinyin_begin_get_bigram_phrases`; the unique borrow lasts for
    // this call.
    let handle = unsafe { &mut *(iter.cast::<BigramHandle>()) };
    let (row, more) = match handle.walk.get_next() {
        BigramStep::Row(row, more) => (row, more),
        BigramStep::Aborts => {
            // Class (c): the pin asserts and aborts here.
            crate::ffi::log_warning(
                "pinyin_bigram_iterator_get_next_phrase: assertion \
                 'iter->m_index_token != null_token && \
                 iter->m_index_token != sentence_start' failed",
            );
            return false;
        }
        // Class (b): the pin reads past its pinyin array.
        BigramStep::Undefined => return false,
    };
    let text = (!phrase.is_null()).then(|| {
        let bytes = phrase_text_bytes(&row.phrase);
        malloc_cstr_with_warning(bytes.len(), |out| {
            out[..bytes.len()].copy_from_slice(&bytes);
            bytes.len()
        })
    });
    if text.is_some_and(|value| value.is_null()) {
        return false;
    }
    let reading = (!pinyin.is_null()).then(|| owned_cstr(&row.pinyin));
    if reading.is_some_and(|value| value.is_null()) {
        if let Some(text) = text {
            // SAFETY: This call still owns text, allocated with libc malloc.
            unsafe { crate::ffi::free(text.cast()) };
        }
        return false;
    }
    if let Some(text) = text {
        // SAFETY: `phrase` was null-checked before allocation.
        unsafe { *phrase = text };
    }
    if let Some(reading) = reading {
        // SAFETY: `pinyin` was null-checked before allocation.
        unsafe { *pinyin = reading };
    }
    if !count.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *count = c_int::try_from(row.count).unwrap_or(c_int::MAX);
        }
    }
    more
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
    // SAFETY: `iter` was produced by its begin call via
    // `Box::into_raw`; the caller transfers ownership back here and
    // only here.
    unsafe {
        drop(Box::from_raw(iter.cast::<BigramHandle>()));
    }
}
