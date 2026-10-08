//! Import iterator symbols: `zhuyin_begin_add_phrases`,
//! `zhuyin_iterator_add_phrase`, `zhuyin_end_add_phrases`.

use std::os::raw::{c_char, c_int};
use std::ptr;

use oxpinyin_core::{FORCE_TONE, USE_TONE, parse_zhuyin_direct};
use oxpinyin_user::{MAX_PHRASE_LENGTH, PinyinKey, UserStore, is_user_file_library, toned_key};

use crate::ffi::cstr_to_owned_lossy;
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
/// its 16-slot array (library 16 SIGSEGVs the pin).
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

    let phrase = cstr_to_owned_lossy(phrase);
    let pinyin = cstr_to_owned_lossy(pinyin);
    // `if (-1 == count) count = default_count;` (`zhuyin.cpp:409-410`).
    let count = (count != -1).then(|| u64::from(count.cast_unsigned()));
    // SAFETY: `iter` is non-null and was produced by
    // `zhuyin_begin_add_phrases`.
    let handle = unsafe { &mut *(iter.cast::<ImportHandle>()) };
    let Some(user) = handle.user.as_mut() else {
        return false;
    };
    let Some(keys) = parse_zhuyin_direct(&pinyin, USE_TONE | FORCE_TONE)
        .into_iter()
        .map(|(key, tone)| toned_key(key.index(), tone))
        .collect::<Option<Vec<PinyinKey>>>()
    else {
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
            "zhuyin_iterator_add_phrase: assertion '0 <= num && num <= 4' failed",
        );
        return false;
    }
    // The pin reduces tokens before get_range rejects an unused library.
    if !is_user_file_library(handle.index) && !(1..=4).contains(&handle.index) {
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
        user.mark_modified();
    };
}
