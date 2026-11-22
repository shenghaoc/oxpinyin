//! Iterators for importing phrase pronunciations and exporting phrases and bigrams.
//!
//! [User-store export](https://github.com/shenghaoc/oxpinyin/blob/main/docs/findings/user-store.md#9-value-comparison-surface): `pinyin_begin_get_phrases` and
//! the bigram quartet. Import: `pinyin_begin_add_phrases` /
//! `pinyin_iterator_add_phrase` / `pinyin_end_add_phrases`.

use std::os::raw::{c_char, c_int};
use std::ptr;

use oxpinyin::OptionBits;
use oxpinyin::facade::{BigramExportWalk, BigramProbeFault, BigramStep, ExportStep, GramLoadFault};
use oxpinyin::graph::SegmentGraph;
use oxpinyin::user::{PinyinKey, toned_key};

use crate::ffi::{
    backing_mut, backing_ref, borrow_or_return, malloc_cstr_with, strconcat_pair, strdup,
    strdup_raw, strjoinv, write_if_nonnull,
};
use crate::state::{ContextOwner, context_owner, context_ref, owner_mut, owner_ref};
use crate::types::{
    BigramExportIterator, ExportIterator, GChar, GUint, ImportIterator, PinyinContext,
};
use oxpinyin::marshal::phrase_text_bytes;

/// State behind `export_iterator_t *`: the pin's iterator state, which
/// reads each item when a call needs it and buffers no row.
struct ExportHandle {
    owner: ContextOwner,
    cursor: oxpinyin::facade::PhraseExportCursor,
}

/// State behind `bigram_export_iterator_t *`.
struct BigramHandle {
    owner: ContextOwner,
    walk: BigramExportWalk,
    /// The pin's `m_phrase` (`pinyin.cpp:830-832`): the loaded pair's
    /// conversion, which every row of the pair duplicates; NULL before the
    /// first load and where the predecessor's conversion failed.
    phrase: *mut c_char,
    /// The walk's load count `phrase` was converted at.
    loads: u64,
}

impl BigramHandle {
    /// Converts a pair the walk has just loaded as the pin's `has_next` does
    /// (`pinyin.cpp:821-835`): each phrase through the fallible
    /// `g_ucs4_to_utf8` encoder, `g_strconcat(first, second, NULL)` (NULL
    /// for a NULL first, the first alone for a NULL second; GLib aborts if
    /// its own allocation fails), the old `m_phrase` freed, then both
    /// conversions. A NULL result is the pin's NULL `m_phrase`, which the
    /// walk then drops.
    fn convert_loaded_pair(&mut self) {
        if self.walk.loads() == self.loads {
            return;
        }
        self.loads = self.walk.loads();
        let Some((first, second)) = self.walk.pair() else {
            return;
        };
        let encode = |text: &oxpinyin::vocab::PhraseText| {
            let bytes = phrase_text_bytes(text);
            malloc_cstr_with(bytes.len(), |out| {
                out.copy_from_slice(&bytes);
                bytes.len()
            })
        };
        let (first, second) = (encode(first), encode(second));
        // SAFETY: Both are NULL or this call's NUL-terminated buffers.
        let phrase = unsafe { strconcat_pair(first, second) };
        // SAFETY: `self.phrase` is NULL or the previous pair's GLib string,
        // which this handle alone owns; `second` and `first` are NULL or
        // this call's encoder buffers. `g_free` releases each.
        unsafe {
            glib_sys::g_free(self.phrase.cast());
            glib_sys::g_free(second.cast());
            glib_sys::g_free(first.cast());
        }
        self.phrase = phrase;
        if phrase.is_null() {
            self.walk.drop_phrase();
        }
    }
}

impl Drop for BigramHandle {
    fn drop(&mut self) {
        // SAFETY: `phrase` is NULL or the GLib string this handle owns.
        unsafe { glib_sys::g_free(self.phrase.cast()) };
    }
}

// ── Import iterator ──────────────────────────────────────────────────
//
// Adds commit immediately; there is no rollback if a later add fails.
// `pinyin_end_add_phrases` only arms `m_modified` so the next `pinyin_save`
// writes (`docs/findings/user-store.md` §4).

/// Import cursor state and a shared owner of the context allocation. Each add
/// borrows storage only for the operation, including after public context fini.
struct ImportHandle {
    index: u8,
    /// `None` for the handle `pinyin_begin_add_phrases(NULL, …)` returns: the
    /// pin stores the NULL context and never reads it (`pinyin.cpp:506-512`).
    owner: Option<ContextOwner>,
}

/// Begin adding phrases to an index.
///
/// # C signature
/// ```c
/// import_iterator_t * pinyin_begin_add_phrases(pinyin_context_t * context,
///                                              guint8 index);
/// ```
///
/// Returns a handle for any context, matching the export iterator shape (a
/// NULL context gets a handle that holds none, as at the pin). The caller
/// frees it with `pinyin_end_add_phrases`. Adds target the loaded system
/// libraries 1–4 and USER_FILE libraries 5–7.
///
/// Choose an index below 16 before using this iterator to add phrases.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_begin_add_phrases(
    context: *mut PinyinContext,
    index: u8,
) -> *mut ImportIterator {
    if context.is_null() {
        // The pin allocates the iterator and stores the context unread
        // (`pinyin.cpp:506-512`, `zhuyin.cpp:392-398`). Every later use that
        // reads the context is a NULL dereference (row 41): `add_phrase`
        // answers `false` and `end_add_phrases` frees the handle.
        let handle = ImportHandle { index, owner: None };
        return Box::into_raw(Box::new(handle)).cast();
    }

    // The constructor only builds the iterator (`pinyin.cpp:506-512`); the compact is
    // `end_add_phrases`'s (`ContextCore::end_import`).
    // SAFETY: the public context is live for this serialized constructor call.
    let owner = unsafe { context_owner(context) };
    let handle = ImportHandle {
        index,
        owner: Some(owner),
    };
    Box::into_raw(Box::new(handle)).cast()
}

// pinyin.cpp:614-652, phrase_index.h:630.
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
/// Add a phrase and reading to its loaded library. Use full pinyin with tone and corrections.
/// The parser keeps the longest prefix of the reading that it can parse and ignores the rest,
/// so trailing text that does not parse does not by itself cause a refusal. The add is accepted
/// only when the number of parsed keys equals the phrase's character count, from 1 to 15. A
/// count of -1 selects 5; other counts are interpreted as unsigned. A successful add changes
/// the live library. A null argument, a length mismatch or an absent sub-index returns false.
///
/// # Aborts
///
/// More than four matching phrase tokens abort during lookup (`phrase_large_table3.h:95`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `iter` is a handle and `phrase` and `pinyin`
/// are strings. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_iterator_add_phrase(
    iter: *mut ImportIterator,
    phrase: *const c_char,
    pinyin: *const c_char,
    count: c_int,
) -> bool {
    if iter.is_null() || phrase.is_null() || pinyin.is_null() {
        return false;
    }

    // `if (-1 == count) count = default_count;` (`pinyin.cpp:523-524`);
    // otherwise the `gint` is used as the `guint32` it becomes.
    let count = (count != -1).then(|| u64::from(count.cast_unsigned()));

    // SAFETY: `iter` is non-null and was produced by
    // `pinyin_begin_add_phrases`; the unique borrow lasts for this call.
    let handle = unsafe { &mut *(iter.cast::<ImportHandle>()) };
    // A NULL-context handle: `_add_phrase` would read the NULL context
    // (`pinyin.cpp:526`), and the early returns before it answer `false`.
    let Some(owner) = handle.owner.as_ref() else {
        return false;
    };
    let plan = {
        // SAFETY: the iterator retains the allocation; the C caller excludes concurrent mutation.
        let context = unsafe { owner_ref(owner) };

        // SAFETY: The entry point guarantees Strings: both pointers are NULL or live NUL-terminated
        // strings, unchanged for this call.
        unsafe { plan_import(&context.core, handle.index, phrase, pinyin) }
    };
    let (phrase, keys, original) = match plan {
        ImportPlan::Refuse => return false,
        ImportPlan::LookupFailed => {
            crate::ffi::log_warning("pinyin_iterator_add_phrase: phrase-table lookup failed");
            return false;
        }
        // Register rows 74 and 93: the pin's `reduce_tokens` count assert
        // (`phrase_large_table3.h:95`) and `_add_phrase`'s asserts
        // (`pinyin.cpp:554`, `:568`, `:571`), before either add call.
        ImportPlan::Abort => std::process::abort(),
        ImportPlan::Add {
            phrase,
            keys,
            original,
        } => (phrase, keys, original),
    };
    let Some(owner) = handle.owner.as_mut() else {
        return false;
    };
    // SAFETY: the planning readers ended above; this call now owns the context mutation.
    let context = unsafe { owner_mut(owner) };
    context
        .core
        .commit_import(handle.index, &phrase, &keys, original, count)
}

/// What [`pinyin_iterator_add_phrase`] does with one pair before it writes.
use oxpinyin::facade::ImportPlan;

/// `pinyin.cpp:614-652` and `_add_phrase` (`:514-611`) up to the add calls:
/// the parse, the length gates and the phrase-table checks, in the pin's
/// order. Reads only.
///
/// # Safety
///
/// Under the crate-level rules, Strings: both pointers are NULL or live
/// NUL-terminated strings, unchanged for this call.
unsafe fn plan_import(
    core: &oxpinyin::facade::ContextCore,
    index: u8,
    phrase: *const c_char,
    pinyin: *const c_char,
) -> ImportPlan {
    // Scalar input conversion; raw-word import is tracked by #1279.
    let phrase = oxpinyin::marshal::utf8_to_ucs4(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(phrase) },
    )
    .unwrap_or_default();
    // The reading is parsed from the raw bytes (`pinyin.cpp:638`): the
    // keys before an invalid sequence stand, as they do at the pin.
    let pinyin = oxpinyin::marshal::parser_prefix(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(pinyin) },
    );
    let Some(keys) = parse_import_pinyin(&pinyin) else {
        return ImportPlan::Refuse;
    };
    // The facade checks the phrase/key lengths, then plans the add.
    core.plan_import(index, phrase, keys)
}

/// `FullPinyinParser2::parse` under `PINYIN_CORRECT_ALL | USE_TONE`
/// (`pinyin.cpp:629-637`, `pinyin_parser2.cpp:217-381`): the longest
/// parsable prefix, fewest keys, as toned store keys. `None` when a key
/// does not fit the store's key layout.
///
/// The whole reading is parsed. The pin's `parse_value_t` keeps its steps
/// in `gint16`s (`pinyin_parser2.cpp:67-70`), so its parse stops at byte
/// 32767; that width is a language artefact
/// (`docs/findings/upstream-divergences.md#the-full-pinyin-parses-16-bit-steps`).
fn parse_import_pinyin(pinyin: &str) -> Option<Vec<PinyinKey>> {
    let options = OptionBits::PINYIN_CORRECT_ALL | OptionBits::USE_TONE;
    let graph = SegmentGraph::build_with_options(pinyin.as_bytes(), options)?;
    graph
        .fewest_keys(false)
        .iter()
        .map(|edge| toned_key(edge.key().index(), edge.tone()))
        .collect()
}

// pinyin.cpp:657-658.
/// Finish the import and release the iterator.
///
/// # C signature
/// ```c
/// void pinyin_end_add_phrases(import_iterator_t * iter);
/// ```
///
/// Release the iterator. With a context, this compacts its phrase libraries and arms the next
/// save, including when no add succeeded. Call `pinyin_save` to write the changes. An iterator
/// opened with a null context is simply freed.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `iter` is a handle. The serial-calls rule
/// applies. After the call `iter` is dangling.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_end_add_phrases(iter: *mut ImportIterator) {
    if iter.is_null() {
        return;
    }

    // SAFETY: `iter` was produced by `pinyin_begin_add_phrases` via
    // `Box::into_raw`; the caller transfers ownership back here and
    // only here.
    let mut handle = unsafe { Box::from_raw(iter.cast::<ImportHandle>()) };
    // A NULL-context handle: the pin reads the context here
    // (`pinyin.cpp:657`), so the handle is only freed (row 41).
    let Some(owner) = handle.owner.as_mut() else {
        return;
    };
    // SAFETY: the iterator retains the allocation; serialized C calls grant exclusive access.
    let context = unsafe { owner_mut(owner) };
    context.core.end_import();
}

// ── Export iterator (unigram phrases) ────────────────────────────────

// pinyin.cpp:126, pinyin.cpp:662-768.
/// Begin exporting phrases from an index.
///
/// # C signature
/// ```c
/// export_iterator_t * pinyin_begin_get_phrases(pinyin_context_t * context,
///                                              guint index);
/// ```
///
/// Start exporting one row per pronunciation in token order. The library index is narrowed to
/// eight bits. An absent sub-index gives an empty iterator. A null context returns null. Free
/// the iterator with `pinyin_end_get_phrases`.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_begin_get_phrases(
    context: *mut PinyinContext,
    index: GUint,
) -> *mut ExportIterator {
    let ctx = borrow_or_return!(context, context_ref, ptr::null_mut());
    // The pin's begin cannot fail: its sub-index is in memory, and so are
    // the session's phrases (policy row 62).
    let cursor = ctx.core.phrase_export_cursor(index);
    // SAFETY: the public context is live for this serialized constructor call.
    let owner = unsafe { context_owner(context) };
    Box::into_raw(Box::new(ExportHandle { owner, cursor })).cast()
}

/// Check whether the export iterator has a next phrase.
///
/// # C signature
/// ```c
/// bool pinyin_iterator_has_next_phrase(export_iterator_t * iter);
/// ```
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `iter` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_iterator_has_next_phrase(iter: *mut ExportIterator) -> bool {
    let handle = borrow_or_return!(iter.cast::<ExportHandle>(), backing_ref, false);
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
/// First check `pinyin_iterator_has_next_phrase`. A delivered row returns true, including the
/// last row. Free the returned phrase and reading separately with `g_free`. A failed phrase
/// conversion can yield null text. A zero frequency exports as -1. A null iterator returns false
/// without writing.
///
/// # Aborts
///
/// Reading after exhaustion or after the probed item disappears aborts. On the missing-item
/// assertion, supplied outputs have been set to null, null and -1 (`pinyin.cpp:703-709`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `iter` is a handle and `phrase`, `pinyin` and
/// `count` are out-parameters. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_iterator_get_next_phrase(
    iter: *mut ExportIterator,
    phrase: *mut *mut GChar,
    pinyin: *mut *mut GChar,
    count: *mut c_int,
) -> bool {
    let handle = borrow_or_return!(iter.cast::<ExportHandle>(), backing_mut, false);
    // SAFETY: the iterator retains the allocation; the C caller excludes concurrent mutation.
    let context = unsafe { owner_ref(&handle.owner) };
    let step = handle.cursor.get_next_with(&context.core, |row| {
        // `pinyin.cpp:715-716`: the fallible `g_ucs4_to_utf8` conversion,
        // NULL on failure, unchecked.
        let text = (!phrase.is_null()).then(|| {
            row.with_text_bytes(|bytes| {
                malloc_cstr_with(bytes.len(), |out| {
                    out.copy_from_slice(bytes);
                    bytes.len()
                })
            })
            .unwrap_or(ptr::null_mut())
        });
        // `:726-733`: the key strings joined by `g_strjoinv("'", …)`, which
        // GLib allocates.
        let reading = (!pinyin.is_null()).then(|| {
            let keys = row.pinyin_strings().map(|(spelling, digit)| {
                let mut key = Vec::with_capacity(spelling.len() + 1);
                key.extend_from_slice(spelling.as_bytes());
                key.extend(digit);
                key
            });
            strjoinv(c"'", keys)
        });
        (text, reading, export_count(row.count()))
    });
    let (text, reading, frequency) = match step {
        ExportStep::Row(row) => row,
        // Register row 62: the pin `assert`s at the site (`pinyin.cpp:709`,
        // `:721`, or `chewing_key.cpp:30-32`/`:48` from `:728`), and the
        // process aborts, after its first line wrote
        // `*phrase = NULL; *pinyin = NULL; *count = -1;` (`:703`).
        ExportStep::Assert(_) => {
            write_if_nonnull!(phrase, ptr::null_mut());
            write_if_nonnull!(pinyin, ptr::null_mut());
            write_if_nonnull!(count, -1);
            std::process::abort()
        }
    };
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
    write_if_nonnull!(count, frequency);
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
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `iter` is a handle. The serial-calls rule
/// applies. After the call `iter` is dangling.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_end_get_phrases(iter: *mut ExportIterator) {
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
/// Start exporting user bigrams, skipping sentence-start predecessors. Each row combines
/// predecessor and successor text and joins their readings with an apostrophe. Its count is
/// twice the stored bigram count. A null context returns null. Release the iterator with
/// `pinyin_end_get_bigram_phrases`.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_begin_get_bigram_phrases(
    context: *mut PinyinContext,
) -> *mut BigramExportIterator {
    let ctx = borrow_or_return!(context, context_ref, ptr::null_mut());
    // Register row 86, `ngram_bdb.cpp:199` (kc `:173`, tkrzw `:150`): the
    // user bigram carries a key that is not a phrase_token_t and the pin's
    // `get_all_items` assert dies.
    if ctx.has_non_token_bigram_key() {
        std::process::abort();
    }
    // Rows 85 and 98 are not decided here: the pin's begin reads no gram
    // (`pinyin.cpp:775-787`); `has_next` loads each predecessor in walk
    // order (`:888-889`), where the walk meets a short or partial value.
    let walk = ctx.bigram_export_walk().unwrap_or_default();
    // SAFETY: the public context is live for this serialized constructor call.
    let owner = unsafe { context_owner(context) };
    Box::into_raw(Box::new(BigramHandle {
        owner,
        walk,
        phrase: ptr::null_mut(),
        loads: 0,
    }))
    .cast()
}

/// Check whether the bigram export iterator has a next phrase.
///
/// # C signature
/// ```c
/// bool pinyin_bigram_iterator_has_next_phrase(
///     bigram_export_iterator_t * iter);
/// ```
///
/// Prepare the next readable row. Returns false for a null iterator, exhaustion, or a refused
/// predecessor read.
///
/// # Aborts
///
/// A stored key of the pair this call loads that is outside the table aborts in this call
/// (`pinyin.cpp:846`, `:859`; `chewing_key.cpp:47-50`, `:29-32`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `iter` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_bigram_iterator_has_next_phrase(
    iter: *mut BigramExportIterator,
) -> bool {
    let handle = borrow_or_return!(iter.cast::<BigramHandle>(), backing_mut, false);
    // SAFETY: the iterator retains the allocation; the C caller excludes concurrent mutation.
    let context = unsafe { owner_ref(&handle.owner) };
    let more = handle.walk.has_next(&context.core);
    if more.is_ok() {
        handle.convert_loaded_pair();
    }
    walk_answer(more, "pinyin_bigram_iterator_has_next_phrase")
}

/// The answer of a `has_next` the walk ran:
/// `pinyin.cpp:889` through `ngram.cpp:141` and `memory_chunk.h:390`, a
/// predecessor too short for its total, where `retrieve_all`'s assert dies
/// and so does the process; `ngram.cpp:133-143`,
/// a partial item tail `retrieve_all` reads past, where the walk ends with
/// one warning and `false`; and a stored key of the loaded pair outside the
/// table, where `get_pinyin_string` asserts inside the pin's `has_next`
/// (`pinyin.cpp:846`, `:859`; the tone at `chewing_key.cpp:48`, then the
/// fields at `:30-32`) and the process dies.
fn walk_answer(more: Result<bool, BigramProbeFault>, name: &str) -> bool {
    match more {
        Ok(more) => more,
        Err(
            BigramProbeFault::Key(_) | BigramProbeFault::Gram(GramLoadFault::ShortTotal { .. }),
        ) => std::process::abort(),
        Err(BigramProbeFault::Gram(GramLoadFault::PartialTail { .. })) => {
            crate::ffi::log_warning(&format!(
                "{name}: a user bigram value has a partial item tail \
                 (upstream reads past it, ngram.cpp:133-143)"
            ));
            false
        }
    }
}

// pinyin.cpp:896-911.
/// Get the next phrase from the bigram export iterator.
///
/// # C signature
/// ```c
/// bool pinyin_bigram_iterator_get_next_phrase(
///     bigram_export_iterator_t * iter,
///     gchar ** phrase, gchar ** pinyin, gint * count);
/// ```
///
/// Call only after `pinyin_bigram_iterator_has_next_phrase` returns true. Writes the current
/// row, then checks for another. False on the last row still delivers both strings; free each
/// with `g_free`. A null iterator returns false without writing.
///
/// # Aborts
///
/// Reading without a prepared predecessor can abort (`pinyin.cpp:902`). The trailing check for
/// another row aborts when a stored key of the pair it loads is outside the table
/// (`pinyin.cpp:846`, `:859`; `chewing_key.cpp:47-50`, `:29-32`). That abort comes after this
/// call has written the current row's outputs (`pinyin.cpp:904-906`, `:910`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `iter` is a handle and `phrase`, `pinyin` and
/// `count` are out-parameters. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_bigram_iterator_get_next_phrase(
    iter: *mut BigramExportIterator,
    phrase: *mut *mut GChar,
    pinyin: *mut *mut GChar,
    count: *mut c_int,
) -> bool {
    let handle = borrow_or_return!(iter.cast::<BigramHandle>(), backing_mut, false);
    // SAFETY: the iterator retains the allocation; the C caller excludes concurrent mutation.
    let context = unsafe { owner_ref(&handle.owner) };
    let (row, more) = match handle.walk.get_next(&context.core) {
        BigramStep::Row(row, more) => (row, more),
        // Register row 62, `pinyin.cpp:902`: the pin asserts and aborts.
        BigramStep::Aborts => std::process::abort(),
        // Class (b): the pin reads past its pinyin array.
        BigramStep::Undefined => return false,
    };
    // `pinyin.cpp:904-905`: `g_strdup` of the pair's `m_phrase` (NULL for
    // NULL) and of the row's pinyin, both allocated by GLib, before the
    // `has_next` that may load and convert the next pair (`:910`).
    write_if_nonnull!(phrase, {
        // SAFETY: `handle.phrase` is NULL or the handle's NUL-terminated GLib string.
        unsafe { strdup_raw(handle.phrase) }
    });
    write_if_nonnull!(pinyin, strdup(&row.pinyin));
    write_if_nonnull!(count, c_int::try_from(row.count).unwrap_or(c_int::MAX));
    // The `has_next` the pin's `get_next` returns (`pinyin.cpp:910`), after
    // the row's out-params were written (`:904-906`); it converts the pair
    // it loaded, unless it died loading it.
    if more.is_ok() {
        handle.convert_loaded_pair();
    }
    walk_answer(more, "pinyin_bigram_iterator_get_next_phrase")
}

/// End the bigram export iterator and free it.
///
/// # C signature
/// ```c
/// void pinyin_end_get_bigram_phrases(bigram_export_iterator_t * iter);
/// ```
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `iter` is a handle. The serial-calls rule
/// applies. After the call `iter` is dangling.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_end_get_bigram_phrases(iter: *mut BigramExportIterator) {
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

#[cfg(test)]
mod tests;

/// Whether [`pinyin_iterator_add_phrase`] of this pair reaches one of the
/// pin's asserts ([`oxpinyin::facade::ImportPlan::Abort`]). Reads only. For the in-tree fuzz
/// harness, which skips such a call: not in `pinyin.h`, a Rust function
/// compiled only with `test-hooks` and never into the shipped artifact.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `iter` is a handle and `phrase` and `pinyin`
/// are strings. The serial-calls rule applies.
#[cfg(any(test, feature = "test-hooks"))]
#[must_use]
pub unsafe fn oxpinyin_import_aborts(
    iter: *mut ImportIterator,
    phrase: *const c_char,
    pinyin: *const c_char,
) -> bool {
    let handle = borrow_or_return!(
        iter.cast::<ImportHandle>(),
        backing_ref,
        false,
        phrase,
        pinyin
    );
    let Some(owner) = handle.owner.as_ref() else {
        return false;
    };
    {
        // SAFETY: the iterator retains the allocation; the C caller excludes concurrent mutation.
        let context = unsafe { owner_ref(owner) };
        matches!(
            // SAFETY: This fuzz API guarantees Strings: both pointers name live
            // NUL-terminated strings, unchanged for this call.
            unsafe { plan_import(&context.core, handle.index, phrase, pinyin) },
            ImportPlan::Abort
        )
    }
}
