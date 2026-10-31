//! The dictionary-introspection surface: token lookups, per-token reads,
//! the unigram-frequency write.

use oxpinyin_capi_marshal::phrase_text::phrase_text_bytes;
use std::os::raw::{c_char, c_uint, c_void};

use oxpinyin_core::Dictionary;

use glib_sys::{g_array_append_vals, g_array_set_size};

use crate::state::instance_ref;
use crate::types::{GArray, GChar, GUint, PhraseTokenT, ZhuyinInstance};

/// Look up the phrase tokens stored for an exact phrase string.
///
/// # C signature
/// ```c
/// bool zhuyin_lookup_tokens(zhuyin_instance_t * instance,
///                           const char * phrase, GArray * tokenarray);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_lookup_tokens(
    instance: *mut ZhuyinInstance,
    phrase: *const c_char,
    tokenarray: *mut GArray,
) -> bool {
    if instance.is_null() {
        return false;
    }
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // SAFETY: Null-checked above.
    let text =
        oxpinyin_capi_marshal::input::utf8_to_ucs4(oxpinyin_capi_marshal::borrow_c_string!(phrase))
            .unwrap_or_default();
    let tokens: Vec<u32> = inst
        .core
        .dict
        .tokens_for_ucs4(&text.outward_words().collect::<Vec<_>>())
        .iter()
        .map(|token| token.value())
        .collect();
    // SAFETY: A non-null array is a caller-owned GLib GArray. GLib
    // 2.84.4 garray.c:595,762 checks NULL, logs a critical, and returns
    // before accessing the array or data (including a zero-length append).
    unsafe {
        g_array_set_size(tokenarray, 0);
        if tokenarray.is_null() {
            // prepare_tokens creates one array per loaded library, and
            // reduce_tokens appends each, including empty arrays
            // (074a2219 phrase_index.h:762-772,
            // phrase_large_table3.h:77-97). GLib checks NULL before len.
            for library in 0..16 {
                if inst.core.dict.phrase_table_token_enabled(library << 24) {
                    g_array_append_vals(tokenarray, std::ptr::null(), 0);
                }
            }
            return !tokens.is_empty();
        }
    }
    if tokens.is_empty() {
        return false;
    }
    // SAFETY: Null-checked above.
    unsafe {
        g_array_append_vals(
            tokenarray,
            tokens.as_ptr().cast::<c_void>(),
            c_uint::try_from(tokens.len()).unwrap_or(0),
        );
    }
    true
}

/// Get the phrase text of a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_get_phrase(zhuyin_instance_t * instance,
///                              phrase_token_t token, guint * len,
///                              gchar ** utf8_str);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_token_get_phrase(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    len: *mut GUint,
    utf8_str: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // An unknown token answers `false` with both out-params untouched
    // (`_token_get_phrase` returns before writing, `pinyin.cpp:1627`; the
    // zhuyin twin shares it).
    let Some(text) = inst.core.dict.token_text(token) else {
        return false;
    };
    if !len.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *len = GUint::try_from(text.phrase_length()).unwrap_or(GUint::MAX);
        }
    }
    if !utf8_str.is_null() {
        let text = phrase_text_bytes(&text);
        let rendered = crate::ffi::malloc_cstr_with(text.len(), |out| {
            out.copy_from_slice(&text);
            text.len()
        });
        // SAFETY: Null-checked above.
        unsafe {
            *utf8_str = rendered;
        }
        // The pin ignores the nonfatal g_ucs4_to_utf8 allocation failure
        // (074a2219 pinyin.cpp:1643-1644, zhuyin.cpp:1112-1113).
    }
    true
}

/// Get the number of pronunciations of a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_get_n_pronunciation(zhuyin_instance_t * instance,
///                                       phrase_token_t token, guint * num);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_token_get_n_pronunciation(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    num: *mut GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    if !num.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *num = 0;
        }
    }
    let Some(count) = inst.core.dict.token_pronunciation_count(token) else {
        return false;
    };
    if !num.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *num = count;
        }
    }
    true
}

/// Append one reading's stored packed words to the caller's array, or the
/// pin's zeroed past-the-end words when the item has no such reading.
///
/// Returns the token getter's result: `false` only for a present reading
/// whose stored word list is empty.
///
/// # Safety
/// `keys` must be a non-NULL `GArray` whose element size is two bytes, as
/// the caller of `zhuyin_token_get_nth_pronunciation` creates it.
unsafe fn append_stored_words(keys: *mut GArray, length: usize, words: Option<&[u8]>) -> bool {
    match words {
        None => {
            // SAFETY: the caller guarantees a real GArray; `zeroed` outlives
            // the append call, which copies from it.
            let zeroed = vec![0_u16; length];
            // SAFETY: as above; element size is two bytes.
            unsafe {
                g_array_append_vals(
                    keys,
                    zeroed.as_ptr().cast::<c_void>(),
                    c_uint::try_from(zeroed.len()).unwrap_or(0),
                );
            }
            true
        }
        Some([]) => false,
        Some(words) => {
            // SAFETY: the caller guarantees a real GArray with element size
            // two bytes; `words` holds `2 * phrase_length` stored bytes and
            // outlives the append call, which copies from it.
            unsafe {
                g_array_append_vals(
                    keys,
                    words.as_ptr().cast::<c_void>(),
                    c_uint::try_from(words.len() / 2).unwrap_or(0),
                );
            }
            true
        }
    }
}

/// Get the nth pronunciation of a token as a vector of chewing keys.
///
/// # C signature
/// ```c
/// bool zhuyin_token_get_nth_pronunciation(zhuyin_instance_t * instance,
///                                         phrase_token_t token, guint nth,
///                                         ChewingKeyVector keys);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_token_get_nth_pronunciation(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    nth: GUint,
    keys: *mut GArray,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // The pin clears the caller's array before appending
    // (`zhuyin.cpp:1793` `g_array_set_size(keys, 0)`), so a stale or
    // re-used GArray never shows concatenated results on either path.
    // SAFETY: GLib checks NULL and logs a critical before any access.
    // `g_array_set_size` on a real glib
    // GArray updates `len` and preserves its private metadata.
    unsafe {
        g_array_set_size(keys, 0);
    }
    // A `USER_FILE` reading lives in the session arena, so hand its stored
    // words straight to the caller's array instead of copying them through
    // an intermediate `Vec`.
    if let Some(result) =
        inst.core
            .dict
            .with_user_nth_pronunciation_words(token, nth as usize, |length, words| {
                // SAFETY: `keys` is the caller's non-NULL GArray with element
                // size two bytes; `with_user_nth_pronunciation_words` keeps the
                // borrowed words live for this call.
                unsafe { append_stored_words(keys, length, words) }
            })
    {
        return result;
    }
    let Some(reading) = inst.core.dict.token_nth_pronunciation(token, nth as usize) else {
        return false;
    };
    let Some(packed) = reading.keys.as_deref() else {
        // Past the last reading: `phrase_length` keys of indeterminate
        // content at the pin; zeroed here (class (b) for the content).
        let zeroed = vec![0_u16; reading.phrase_length];
        // SAFETY: GLib checks NULL and logs a critical before any access.
        // the caller creates the keys array
        // with element_size = sizeof(u16), as for the in-range path.
        unsafe {
            g_array_append_vals(
                keys,
                zeroed.as_ptr().cast::<c_void>(),
                c_uint::try_from(zeroed.len()).unwrap_or(0),
            );
        }
        return true;
    };
    // System and USER_FILE items alike hand out their stored words
    // (`get_nth_pronunciation` copies the item's `ChewingKey`s verbatim).
    if packed.is_empty() {
        return false;
    }
    // SAFETY: GLib checks NULL before access; otherwise the caller owns
    // a real GArray and the source slice remains live for the call.
    unsafe {
        g_array_append_vals(
            keys,
            packed.as_ptr().cast::<c_void>(),
            c_uint::try_from(packed.len()).unwrap_or(0),
        );
    }
    true
}

/// Get the unigram frequency of a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_get_unigram_frequency(zhuyin_instance_t * instance,
///                                         phrase_token_t token, guint * freq);
/// ```
///
/// The pin answers the default facade's item field verbatim
/// (`zhuyin.cpp:1813-1826`: `get_phrase_item` on `m_phrase_index`, then
/// `PhraseItem::get_unigram_frequency` — the stored `guint32`,
/// `gen_unigram`'s `+1` already included for system items), system and
/// `USER_FILE` tokens alike. `*freq` is zeroed before the dispatch, so a
/// `false` still delivers 0; the read includes the
/// `zhuyin_token_add_unigram_frequency` overlay.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_token_get_unigram_frequency(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    freq: *mut GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    if !freq.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *freq = 0;
        }
    }
    // The pin reads the item of whichever sub-index the token's nibble
    // names; a library that is missing or owns no such item answers
    // `false` (`get_phrase_item`'s `ERROR_NO_SUB_PHRASE_INDEX` /
    // `ERROR_NO_ITEM`).
    let nibble = token >> 24;
    let base = match nibble {
        1..=4 => {
            if !inst.core.dict.token_exists(token) {
                // Unloaded library or no such item — nothing is
                // reported (matches the visibility filter every other
                // Tier-C read honours).
                None
            } else {
                use oxpinyin_core::LanguageModel;
                inst.core
                    .lm
                    .unigram_freq(&oxpinyin_core::PhraseToken::new(token))
                    .ok()
                    .flatten()
            }
        }
        // The `USER_FILE` sub-indexes (addon.bin / network.bin /
        // user.bin): the item's stored field is the user store's full
        // UNIGRAM accumulation for the token (`count·3` at `_add_phrase`,
        // `seed·7` per training). An absent item answers `false`, as
        // `get_phrase_item`'s `ERROR_NO_ITEM` does upstream.
        5..=7 => inst
            .core
            .user
            .as_ref()
            .filter(|_| inst.core.dict.token_exists(token))
            .and_then(|store| store.unigram_delta(token).ok()),
        _ => None,
    };
    let Some(base) = base else {
        return false;
    };
    // A system item's frequency comes from the language model, which
    // already prices the overlay into it; the other libraries take it here.
    let overlay = if (1..=4).contains(&nibble) {
        0
    } else {
        inst.core.dict.unigram_delta(token).unwrap_or(0)
    };
    let count = base + overlay;
    if !freq.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *freq = GUint::try_from(count).unwrap_or(GUint::MAX);
        }
    }
    true
}

/// Add a unigram-frequency delta to a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_add_unigram_frequency(zhuyin_instance_t * instance,
///                                         phrase_token_t token, guint delta);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_token_add_unigram_frequency(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    delta: GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    inst.core.dict.add_unigram_delta(token, delta as u64)
}
