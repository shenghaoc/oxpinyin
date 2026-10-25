//! The dictionary-introspection surface: token lookups, per-token reads,
//! the unigram-frequency write.

use oxpinyin_capi_marshal::phrase_text::phrase_text_bytes;
use std::os::raw::{c_char, c_uint, c_void};

use oxpinyin_core::Dictionary;

use glib_sys::{g_array_append_vals, g_array_set_size};

use crate::ffi::cstr_to_string;
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
    if instance.is_null() || phrase.is_null() {
        return false;
    }
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // SAFETY: Null-checked above.
    let text = unsafe { cstr_to_string(phrase) };
    let tokens: Vec<u32> = inst
        .core
        .dict
        .tokens_for_text(&text)
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
    let Some(intro) = inst.core.dict.token_introspection(token) else {
        return false;
    };
    if !len.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *len = GUint::try_from(intro.phrase_length).unwrap_or(GUint::MAX);
        }
    }
    if !utf8_str.is_null() {
        let text = phrase_text_bytes(&intro.text);
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
    let Some(intro) = inst.core.dict.token_introspection(token) else {
        return false;
    };
    let Some((keys_list, _count)) = intro.pronunciations.get(nth as usize) else {
        // Past the last reading: `phrase_length` keys of indeterminate
        // content at the pin; zeroed here (class (b) for the content).
        let zeroed = vec![0_u16; intro.phrase_length];
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
    // A system item hands out the stored words directly; a USER_FILE item
    // carries structured keys and packs them the same way.
    let packed: std::borrow::Cow<'_, [u16]> = match keys_list {
        oxpinyin_runtime::IntroPronunciation::Stored(keys) => {
            std::borrow::Cow::Borrowed(keys.as_slice())
        }
        oxpinyin_runtime::IntroPronunciation::Structured(keys) => {
            let mut packed: Vec<u16> = Vec::with_capacity(keys.len());
            for &key in keys {
                let Some(syllable) =
                    oxpinyin_core::SyllableKey::from_index(oxpinyin_user::key_syllable(key))
                else {
                    return false;
                };
                let Some(chewing) = oxpinyin_core::ChewingKey::from_pinyin(syllable.text()) else {
                    return false;
                };
                // The stored key's tone rides along (`get_nth_pronunciation`
                // copies the item's `ChewingKey`s verbatim).
                packed.push(chewing.with_tone(oxpinyin_user::key_tone(key)).to_packed());
            }
            std::borrow::Cow::Owned(packed)
        }
    };
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
            if !inst.core.dict.library_visible_token(token)
                || inst.core.dict.system_unigram_count(token).is_none()
            {
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
            .and_then(|store| match store.phrase(token) {
                Ok(Some(_)) => store.unigram_delta(token).ok(),
                _ => None,
            }),
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
