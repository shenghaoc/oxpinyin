//! Import and export iterator symbols.
//!
//! Export (`docs/findings/user-store.md` §9): `pinyin_begin_get_phrases` and
//! the bigram quartet. Import: `pinyin_begin_add_phrases` /
//! `pinyin_iterator_add_phrase` / `pinyin_end_add_phrases`.

use std::os::raw::{c_char, c_int};
use std::ptr;

use oxpinyin_core::graph::SegmentGraph;
use oxpinyin_core::{OptionBits, PINYIN_CORRECT_ALL, USE_TONE};
use oxpinyin_facade::{BigramExportWalk, BigramStep};
use oxpinyin_user::{MAX_PHRASE_LENGTH, PinyinKey, UserStore, is_user_file_library, toned_key};

use crate::ffi::{cstr_to_owned_lossy, cstr_to_parsed_prefix, owned_cstr};
use crate::state::context_ref;
use crate::types::{
    BigramExportIterator, ExportIterator, GChar, GUint, ImportIterator, PinyinContext,
};

/// The phrase cursor retains one system item, rather than its whole library.
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
    // 074a2219 phrase_large_table3.h:95, called by _add_phrase before
    // mutation: reduce_tokens asserts when more than four tokens already
    // spell this phrase. A fifth token is allowed; the next add refuses.
    if handle.dict.as_ref().is_some_and(|dict| {
        // Direct keyed probes avoid rebuilding the complete UserLookup
        // after every import. Each library has at most one token per text.
        let mut tokens = dict.system().tokens_for_text(&phrase).unwrap_or_default();
        for library in 1..=7 {
            if let Ok(Some(token)) = user.token_for_phrase_in(library, &phrase) {
                tokens.push(token);
            }
        }
        tokens.retain(|token| dict.library_visible_token(*token));
        tokens.sort_unstable();
        tokens.dedup();
        tokens.len() > 4
    }) {
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
    // phrase index; the item text is the token introspection. Each refusal
    // is one `libpinyin` warning; a lookup that fails outright refuses before
    // either add call rather than reading an empty token list.
    let same_library =
        match same_library_phrase_table_tokens(user, handle.dict.as_ref(), &phrase, handle.index) {
            Ok(tokens) => tokens,
            Err(()) => {
                crate::ffi::log_warning("pinyin_iterator_add_phrase: phrase-table lookup failed");
                return false;
            }
        };
    if same_library.len() > 1 {
        crate::ffi::log_warning(
            "pinyin_iterator_add_phrase: assertion \
             'PHRASE_INDEX_LIBRARY_INDEX(token) != index' failed",
        );
        return false;
    }
    if let (Some(&token), Some(dict)) = (same_library.first(), handle.dict.as_ref())
        && let Some(introspection) = dict.token_introspection(token)
        && introspection.text != phrase
    {
        crate::ffi::log_warning(if introspection.text.chars().count() == length {
            "pinyin_iterator_add_phrase: assertion \
             '0 == memcmp(phrase, tmp_phrase, sizeof(ucs4_t) * phrase_length)' failed"
        } else {
            "pinyin_iterator_add_phrase: assertion \
             'phrase_length == item.get_phrase_length()' failed"
        });
        return false;
    }
    if (1..=4).contains(&handle.index) {
        let Some(dict) = handle.dict.as_ref() else {
            return false;
        };
        if !dict.library_visible(u32::from(handle.index)) {
            return false;
        }
        // 074a2219 pinyin.cpp:533-571 / zhuyin.cpp:419-457: choose the
        // same-library token. The store also searches prior imported items.
        let original = dict
            .system()
            .tokens_for_text(&phrase)
            .unwrap_or_default()
            .into_iter()
            .find(|token| token >> 24 == u32::from(handle.index));
        user.add_system_phrase_in(handle.index, original, &phrase, &keys, count)
            .is_ok()
    } else {
        user.add_phrase_in(handle.index, &phrase, &keys, count)
            .is_ok()
    }
}

/// The phrase-table tokens for `phrase` in `index`, in the pin's
/// `phrase_table->search` shape (`pinyin.cpp:533`): the `user_phrase_index`
/// membership ([`UserStore::phrase_table_tokens_for_text`]) plus the system
/// phrase index, filtered to the visible libraries. Sorted but **not**
/// deduped: the pin's `reduce_tokens` (`phrase_large_table3.h:77-95`)
/// concatenates the per-library arrays unchanged, so a token that is both in
/// the membership and in the system index appears twice and the caller's
/// `:554` check forbids the second one. The zhuyin facade keeps its own
/// copy (`zhuyin_iterator_add_phrase`), as the facades duplicate by design.
///
/// # Errors
///
/// `Err(())` when either lookup fails, so the caller answers `false` without
/// letting an empty token list pass the same-library refusal.
fn same_library_phrase_table_tokens(
    user: &UserStore,
    dict: Option<&oxpinyin_runtime::RuntimeDict>,
    phrase: &str,
    index: u8,
) -> Result<Vec<u32>, ()> {
    let mut tokens = user.phrase_table_tokens_for_text(phrase).map_err(|_| ())?;
    if let Some(dict) = dict {
        tokens.extend(dict.system().tokens_for_text(phrase).map_err(|_| ())?);
        tokens.retain(|token| dict.library_visible_token(*token));
    }
    tokens.sort_unstable();
    Ok(tokens
        .into_iter()
        .filter(|token| token >> 24 == u32::from(index))
        .collect())
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
/// in-memory phrase chunk to compact; the adds are already durable. The
/// dirty-flag arm is the whole persistence-side effect, so the next
/// `pinyin_save` compacts and clears (`docs/findings/user-store.md` §4).
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
    let cursor = ctx.core.phrase_export_cursor(index).unwrap_or_default();
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

    // SAFETY: `iter` is non-null and was produced by
    // `pinyin_begin_get_phrases`; the unique borrow lasts for this call.
    let handle = unsafe { &mut *(iter.cast::<ExportHandle>()) };
    let Some(row) = handle.cursor.next() else {
        // Class (c), `pinyin.cpp:709`: the pin **`assert`**s that the token
        // a previous call (or the begin) probed is valid; an exhausted or
        // empty iterator has none.
        crate::ffi::log_warning(
            "pinyin_iterator_get_next_phrase: assertion 'ERROR_OK == retval' failed",
        );
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
            *count = export_count(row.count);
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
