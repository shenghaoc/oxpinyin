//! Import iterator symbols: `zhuyin_begin_add_phrases`,
//! `zhuyin_iterator_add_phrase`, `zhuyin_end_add_phrases`.

use std::os::raw::{c_char, c_int};

use oxpinyin_core::{OptionBits, parse_zhuyin_direct};
use oxpinyin_user::{PinyinKey, toned_key};

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
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_begin_add_phrases(
    context: *mut ZhuyinContext,
    index: u8,
) -> *mut ImportIterator {
    if context.is_null() {
        // The pin allocates the iterator and stores the context unread
        // (`zhuyin.cpp:392-398`). Every later use that reads the context is a
        // NULL dereference (row 41): `add_phrase` answers `false` and
        // `end_add_phrases` frees the handle.
        let handle = ImportHandle { index, owner: None };
        return Box::into_raw(Box::new(handle)).cast();
    }

    // The constructor only builds the iterator (`zhuyin.cpp:392-398`); the compact is
    // `end_add_phrases`'s (`ContextCore::end_import`).
    // SAFETY: the public context is live for this serialized constructor call.
    let owner = unsafe { context_owner(context) };
    let handle = ImportHandle {
        index,
        owner: Some(owner),
    };
    Box::into_raw(Box::new(handle)).cast()
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
///
/// `zhuyin.cpp:500-534` then `_add_phrase` (`:400-498`): the reading is
/// bopomofo, parsed by `ZhuyinDirectParser2` under `USE_TONE |
/// FORCE_TONE` (`:515-523`) — keys separated by spaces or apostrophes,
/// each carrying its tone (tone 1 when unmarked) — so a romanized reading
/// parses no key and is refused. The phrase's character count must equal
/// the key count, `0 < len < 16`. Count, storage and library routing are
/// the pinyin facade's (`pinyin_iterator_add_phrase`): -1 is the default
/// 5, any other count its `guint32` bit pattern; the `USER_FILE`
/// libraries and loaded system libraries take the phrase; every other nibble answers
/// `false` — 16..=255 without reproducing the pin's out-of-bounds read of
/// its 16-slot array (library 16 SIGSEGVs the pin). Where the pin asserts —
/// more than four phrase-table tokens (`phrase_large_table3.h:95`), two of
/// them in the target library (`zhuyin.cpp:440`), or the target library's
/// item with another length (`:454`) or other text (`:457`) — the process
/// aborts (register rows 74, 89, 104 and 90).
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
    let Some(owner) = handle.owner.as_ref() else {
        return false;
    };
    let plan = {
        // SAFETY: the iterator retains the allocation; the C caller excludes concurrent mutation.
        let context = unsafe { owner_ref(owner) };
        plan_import(&context.core, handle.index, phrase, pinyin)
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
    let Some(owner) = handle.owner.as_mut() else {
        return false;
    };
    // SAFETY: the planning readers ended above; this call now owns the context mutation.
    let context = unsafe { owner_mut(owner) };
    context
        .core
        .commit_import(handle.index, &phrase, &keys, original, count)
}

/// What [`zhuyin_iterator_add_phrase`] does with one pair before it writes.
pub(crate) use oxpinyin_facade::ImportPlan;

/// `zhuyin.cpp:500-534` and `_add_phrase` (`:400-458`) up to the add calls:
/// the parse, the length gates and the phrase-table checks, in the pin's
/// order. Reads only.
fn plan_import(
    core: &oxpinyin_facade::ContextCore,
    index: u8,
    phrase: *const c_char,
    pinyin: *const c_char,
) -> ImportPlan {
    let phrase =
        oxpinyin_capi_marshal::input::utf8_to_ucs4(oxpinyin_capi_marshal::borrow_c_string!(phrase))
            .unwrap_or_default();
    let pinyin = oxpinyin_capi_marshal::input::direct_zhuyin_reading(
        oxpinyin_capi_marshal::borrow_c_string!(pinyin),
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

/// End the import iterator, arm `m_modified`, and free it.
///
/// # C signature
/// ```c
/// void zhuyin_end_add_phrases(import_iterator_t * iter);
/// ```
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
