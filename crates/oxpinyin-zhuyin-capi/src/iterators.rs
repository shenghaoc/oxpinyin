//! Import iterator symbols: `zhuyin_begin_add_phrases`,
//! `zhuyin_iterator_add_phrase`, `zhuyin_end_add_phrases`.

use std::os::raw::{c_char, c_int};
use std::ptr;

use oxpinyin_core::{FORCE_TONE, USE_TONE, parse_zhuyin_direct};
use oxpinyin_user::{MAX_PHRASE_LENGTH, PinyinKey, UserStore, is_user_file_library, toned_key};

use crate::state::context_ref;
use crate::types::{ImportIterator, ZhuyinContext};

/// State behind `import_iterator_t *`: the target index and the shared user
/// store clone the adds write through.
struct ImportHandle {
    index: u8,
    user: Option<UserStore>,
    dict: Option<oxpinyin_runtime::RuntimeDict>,
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

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
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
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_iterator_add_phrase(
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
    let (phrase, keys, original) = match plan_import(handle, phrase, pinyin) {
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
    let Some(user) = handle.user.as_mut() else {
        return false;
    };
    if (1..=4).contains(&handle.index) {
        let Some(dict) = handle.dict.as_ref() else {
            return false;
        };
        if !dict.library_visible(u32::from(handle.index)) {
            return false;
        }
        // The selected token comes from the combined phrase-table search,
        // including prior imported system-library items.
        user.add_system_phrase_in(handle.index, original, &phrase, &keys, count)
            .is_ok()
    } else {
        user.add_phrase_in(handle.index, &phrase, &keys, count)
            .is_ok()
    }
}

/// What [`zhuyin_iterator_add_phrase`] does with one pair before it writes.
enum ImportPlan {
    /// The pin's ordinary `false`.
    Refuse,
    /// A phrase-table lookup failed outright: no pin counterpart, one
    /// warning and `false`.
    LookupFailed,
    /// One of the pin's asserts: `reduce_tokens`' count
    /// (`phrase_large_table3.h:95`, register row 74) or `_add_phrase`'s
    /// same-library (`zhuyin.cpp:440`, row 89), length (`:454`, row 104)
    /// and text (`:457`, row 90) checks.
    Abort,
    /// The add goes ahead, with the same-library token the phrase table
    /// already holds for the phrase, if any.
    Add {
        phrase: oxpinyin_core::PhraseText,
        keys: Vec<PinyinKey>,
        original: Option<u32>,
    },
}

/// `zhuyin.cpp:500-534` and `_add_phrase` (`:400-458`) up to the add calls:
/// the parse, the length gates and the phrase-table checks, in the pin's
/// order. Reads only.
fn plan_import(handle: &ImportHandle, phrase: *const c_char, pinyin: *const c_char) -> ImportPlan {
    let phrase =
        oxpinyin_capi_marshal::input::utf8_to_ucs4(oxpinyin_capi_marshal::borrow_c_string!(phrase))
            .unwrap_or_default();
    let pinyin = oxpinyin_capi_marshal::input::direct_zhuyin_reading(
        oxpinyin_capi_marshal::borrow_c_string!(pinyin),
    );
    let Some(user) = handle.user.as_ref() else {
        return ImportPlan::Refuse;
    };
    let Some(keys) = parse_zhuyin_direct(&pinyin, USE_TONE | FORCE_TONE)
        .into_iter()
        .map(|(key, tone)| toned_key(key.index(), tone))
        .collect::<Option<Vec<PinyinKey>>>()
    else {
        return ImportPlan::Refuse;
    };
    // The iterator validates the phrase/key lengths before _add_phrase:
    // 074a2219 zhuyin.cpp:525-529.
    let length = phrase.phrase_length();
    if length == 0 || length >= MAX_PHRASE_LENGTH || length != keys.len() {
        return ImportPlan::Refuse;
    }
    // Search once and preserve both tables' token multiplicity before
    // reduce_tokens validates the combined count (074a2219
    // phrase_large_table3.h:77-95).
    let Ok(tokens) = phrase_table_tokens(user, handle.dict.as_ref(), &phrase) else {
        return ImportPlan::LookupFailed;
    };
    if tokens.len() > 4 {
        return ImportPlan::Abort;
    }
    // The pin reduces tokens before get_range rejects an unused library.
    if !is_user_file_library(handle.index) && !(1..=4).contains(&handle.index) {
        return ImportPlan::Refuse;
    }
    // 074a2219 zhuyin.cpp:431-458: the pin then walks the phrase table's
    // tokens for the phrase. Two tokens in the same sub-index library trip
    // `assert(PHRASE_INDEX_LIBRARY_INDEX(token) != index)` (`:440`); a lone
    // one whose phrase-index item has another length trips
    // `assert(phrase_length == item.get_phrase_length())` (`:454`), and one
    // of the same length with other text the `memcmp` assert (`:457`). The
    // phrase table is the `user_phrase_index` membership plus the system
    // phrase index; the item text is read without its pronunciation view.
    let same_library: Vec<u32> = tokens
        .into_iter()
        .filter(|token| token >> 24 == u32::from(handle.index))
        .collect();
    if same_library.len() > 1 {
        return ImportPlan::Abort;
    }
    if let Some(&token) = same_library.first() {
        let item_text = if let Some(dict) = handle.dict.as_ref() {
            dict.token_text(token)
        } else {
            user.phrase_text(token).ok().flatten().and_then(|words| {
                oxpinyin_core::PhraseText::from_stored_words(words.into_iter().map(Some))
            })
        };
        // A missing item (a token a library reload left in the phrase
        // table) leaves the pin's `PhraseItem` at length zero: `:454`
        // asserts before any import mutation. The stored length first, then
        // every stored word (`zhuyin.cpp:454-457`).
        let same_length = item_text
            .as_ref()
            .is_some_and(|text| text.phrase_length() == length);
        let same_words = item_text.as_ref().is_some_and(|text| {
            text.stored_words().map_or_else(
                || text.outward_words().eq(phrase.outward_words()),
                |words| words.iter().copied().eq(phrase.outward_words()),
            )
        });
        if !(same_length && same_words) {
            return ImportPlan::Abort;
        }
    }
    ImportPlan::Add {
        phrase,
        keys,
        original: same_library.first().copied(),
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
    phrase: &oxpinyin_core::PhraseText,
) -> Result<Vec<u32>, ()> {
    let words: Vec<u32> = phrase.outward_words().collect();
    let mut tokens = if let Some(dict) = dict {
        dict.system().tokens_for_ucs4(&words).map_err(|_| ())?
    } else {
        Vec::new()
    };
    tokens.extend(user.phrase_table_tokens_for_ucs4(&words).map_err(|_| ())?);
    if let Some(dict) = dict {
        tokens.retain(|token| dict.phrase_table_token_enabled(*token));
    }
    tokens.sort_by_key(|token| token >> 24);
    Ok(tokens)
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

    // SAFETY: `iter` was produced by `zhuyin_begin_add_phrases` via
    // `Box::into_raw`; the caller transfers ownership back here.
    let mut handle = unsafe { Box::from_raw(iter.cast::<ImportHandle>()) };
    if let Some(user) = handle.user.as_mut() {
        // The compact (`zhuyin.cpp:542`) rebuilds each library's total from
        // its items.
        user.compact_libraries();
        user.mark_modified();
    };
}
