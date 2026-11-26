//! Phrase import through a context-owned iterator.
//!
//! Start an import with [`zhuyin_begin_add_phrases`], add phrase and reading
//! pairs with [`zhuyin_iterator_add_phrase`], and finish with
//! [`zhuyin_end_add_phrases`].

use std::os::raw::{c_char, c_int};

use oxpinyin::OptionBits;
use oxpinyin::parser::parse_zhuyin_direct;
use oxpinyin::user::{PinyinKey, toned_key};

use crate::state::{ContextOwner, context_owner, owner_mut, owner_ref};
use crate::types::{ImportIterator, ZhuyinContext};

/// Import cursor state and a shared owner of the context allocation. Each add
/// borrows storage only for the operation, including after public context fini.
struct ImportHandle {
    index: u8,
    /// `None` for the handle `zhuyin_begin_add_phrases(NULL, …)` returns: the
    /// pin stores the NULL context and never reads it (`zhuyin.cpp:392-398`).
    owner: Option<ContextOwner>,
}

/// Begin adding phrases to an index.
///
/// # C signature
/// ```c
/// import_iterator_t * zhuyin_begin_add_phrases(zhuyin_context_t * context,
///                                              guint8 index);
/// ```
///
/// Returns a handle for any context (a NULL context gets a handle that holds
/// none, as at the pin); caller must call
/// `zhuyin_end_add_phrases` to free it.
///
/// Choose an index below 16 before using this iterator to add phrases.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_begin_add_phrases(
    context: *mut ZhuyinContext,
    index: u8,
) -> *mut ImportIterator {
    // The constructor only builds the iterator (`zhuyin.cpp:392-398`); the
    // compact is `end_add_phrases`'s (`ContextCore::end_import`). The pin
    // stores a NULL context unread. Every later use that reads it is a NULL
    // dereference (row 41): `add_phrase` answers `false` and
    // `end_add_phrases` frees the handle.
    // SAFETY: a non-NULL public context is live for this serialized
    // constructor call.
    let owner = (!context.is_null()).then(|| unsafe { context_owner(context) });
    Box::into_raw(Box::new(ImportHandle { index, owner })).cast()
}

// zhuyin.cpp:500-534, zhuyin.cpp:440.
/// Add a phrase/pinyin pair to the import iterator.
///
/// # C signature
/// ```c
/// bool zhuyin_iterator_add_phrase(import_iterator_t * iter,
///                                 const char * phrase,
///                                 const char * pinyin,
///                                 gint count);
/// ```
///
/// Add a phrase and reading to its loaded library. Use Zhuyin symbols separated by spaces or
/// apostrophes; an unmarked syllable has first tone. The parser stops at the first segment that
/// is not a Zhuyin syllable and keeps the syllables before it, so trailing text that does not
/// parse does not by itself cause a refusal. The add is accepted only when the number of parsed
/// keys equals the phrase's character count, from 1 to 15. A count of -1 selects 5; other counts
/// are interpreted as unsigned. A successful add changes the live library. A null argument, a
/// length mismatch or an absent sub-index returns false.
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
pub unsafe extern "C" fn zhuyin_iterator_add_phrase(
    iter: *mut ImportIterator,
    phrase: *const c_char,
    pinyin: *const c_char,
    count: c_int,
) -> bool {
    if iter.is_null() || phrase.is_null() || pinyin.is_null() {
        return false;
    }

    // `if (-1 == count) count = default_count;` (`zhuyin.cpp:409-410`).
    let count = (count != -1).then(|| u64::from(count.cast_unsigned()));
    // SAFETY: `iter` is non-null and was produced by
    // `zhuyin_begin_add_phrases`; the unique borrow lasts for this call.
    let handle = unsafe { &mut *(iter.cast::<ImportHandle>()) };
    // A NULL-context handle: `_add_phrase` would read the NULL context
    // (`zhuyin.cpp:412`), and the early returns before it answer `false`.
    let index = handle.index;
    let Some(owner) = handle.owner.as_mut() else {
        return false;
    };
    let plan = {
        // SAFETY: the iterator retains the allocation; the C caller excludes concurrent mutation.
        let context = unsafe { owner_ref(owner) };

        // SAFETY: The entry point guarantees Strings: both pointers are NULL or live NUL-terminated
        // strings, unchanged for this call.
        unsafe { plan_import(&context.core, index, phrase, pinyin) }
    };
    let (phrase, keys, original) = match plan {
        ImportPlan::Refuse => return false,
        ImportPlan::LookupFailed => {
            crate::ffi::log_warning("zhuyin_iterator_add_phrase: phrase-table lookup failed");
            return false;
        }
        // Register rows 74, 89, 90 and 104: the pin's `reduce_tokens` count
        // assert (`phrase_large_table3.h:95`) and `_add_phrase`'s asserts
        // (`zhuyin.cpp:440`, `:454`, `:457`), before either add call.
        ImportPlan::Abort => std::process::abort(),
        ImportPlan::Add {
            phrase,
            keys,
            original,
        } => (phrase, keys, original),
    };
    // SAFETY: the planning readers ended above; this call now owns the context mutation.
    let context = unsafe { owner_mut(owner) };
    context
        .core
        .commit_import(index, &phrase, &keys, original, count)
}

/// What [`zhuyin_iterator_add_phrase`] does with one pair before it writes.
use oxpinyin::facade::ImportPlan;

/// `zhuyin.cpp:500-534` and `_add_phrase` (`:400-458`) up to the add calls:
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
    let phrase = oxpinyin::marshal::utf8_to_ucs4(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(phrase) },
    )
    .unwrap_or_default();
    let pinyin = oxpinyin::marshal::direct_zhuyin_reading(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(pinyin) },
    );
    let Some(keys) = parse_zhuyin_direct(&pinyin, OptionBits::USE_TONE | OptionBits::FORCE_TONE)
        .into_iter()
        .map(|(key, tone)| toned_key(key.index(), tone))
        .collect::<Option<Vec<PinyinKey>>>()
    else {
        return ImportPlan::Refuse;
    };
    // The facade checks the phrase/key lengths, then plans the add.
    core.plan_import(index, phrase, keys)
}

/// Finish the import and release the iterator.
///
/// # C signature
/// ```c
/// void zhuyin_end_add_phrases(import_iterator_t * iter);
/// ```
///
/// Release the iterator. With a context, this compacts its phrase libraries and arms the next
/// save, including when no add succeeded. Call `zhuyin_save` to write the changes. An iterator
/// opened with a null context is simply freed.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `iter` is a handle. The serial-calls rule
/// applies. After the call `iter` is dangling.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_end_add_phrases(iter: *mut ImportIterator) {
    if iter.is_null() {
        return;
    }

    // SAFETY: `iter` was produced by `zhuyin_begin_add_phrases` via
    // `Box::into_raw`; the caller transfers ownership back here.
    let mut handle = unsafe { Box::from_raw(iter.cast::<ImportHandle>()) };
    // A NULL-context handle: the pin reads the context here
    // (`zhuyin.cpp:542`), so the handle is only freed (row 41).
    let Some(owner) = handle.owner.as_mut() else {
        return;
    };
    // SAFETY: the iterator retains the allocation; serialized C calls grant exclusive access.
    let context = unsafe { owner_mut(owner) };
    context.core.end_import();
}

#[cfg(test)]
mod tests;
