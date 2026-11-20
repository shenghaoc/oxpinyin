//! The dictionary-introspection surface: token lookups, per-token reads,
//! the unigram-frequency write.

use oxpinyin::marshal::phrase_text::phrase_text_bytes;
use std::os::raw::{c_char, c_uint, c_void};

use glib_sys::{g_array_append_vals, g_array_set_size};

use crate::ffi::{borrow_or_return, write_if_nonnull};
use crate::state::instance_ref;
use crate::types::{GArray, GChar, GUint, PhraseTokenT, ZhuyinInstance};

/// Look up the phrase tokens stored for an exact phrase string.
///
/// # C signature
/// ```c
/// bool zhuyin_lookup_tokens(zhuyin_instance_t * instance,
///                           const char * phrase, GArray * tokenarray);
/// ```
///
/// Look up an exact phrase in the loaded libraries. The caller’s array is cleared, then filled
/// in library order. Returns true when there was a match. A miss leaves the array empty. A null
/// instance returns false before clearing it.
///
/// # Aborts
///
/// More than four matching tokens abort after the hits have been appended
/// (`phrase_large_table3.h:79-95`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `phrase` is a
/// string. The serial-calls rule applies. `tokenarray` is NULL or a live GLib `GArray` whose
/// element size is four bytes (`phrase_token_t`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_lookup_tokens(
    instance: *mut ZhuyinInstance,
    phrase: *const c_char,
    tokenarray: *mut GArray,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    let text = oxpinyin::marshal::input::utf8_to_ucs4(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(phrase) },
    )
    .unwrap_or_default();
    let tokens = inst.core.phrase_tokens(&text);
    // `reduce_tokens` clears the caller's array and appends every library's
    // hits, and only then validates the combined count
    // (`zhuyin.cpp:1690`, `phrase_large_table3.h:79-95`): five or more
    // tokens reach `assert(0 <= num && num <= 4)` and the pin dies before
    // answering. The abort below follows the clear and the appends, so a
    // NULL array's GLib criticals come first, as at the pin.
    let reduce_asserts = tokens.len() > 4;
    let null_append_count = tokenarray.is_null().then(|| {
        // Count the pin's prepared arrays before GLib can call a log handler.
        (0..oxpinyin::PHRASE_INDEX_LIBRARY_COUNT)
            .filter(|&library| {
                inst.core
                    .dict
                    .phrase_table_token_enabled(oxpinyin::phrase_index_make_token(library, 0))
            })
            .count()
    });
    // SAFETY: A non-null array is a caller-owned GLib GArray. GLib
    // 2.84.4 garray.c:595,762 checks NULL, logs a critical, and returns
    // before accessing the array or data (including a zero-length append).
    unsafe {
        g_array_set_size(tokenarray, 0);
        if let Some(count) = null_append_count {
            for _ in 0..count {
                g_array_append_vals(tokenarray, std::ptr::null(), 0);
            }
            if reduce_asserts {
                std::process::abort();
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
    if reduce_asserts {
        std::process::abort();
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
///
/// A missing token or null instance returns false without touching either optional output. On
/// success the length counts characters. Conversion failure can give null text with a true
/// return. Free a returned string with `g_free`.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `len` and `utf8_str`
/// are out-parameters. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_token_get_phrase(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    len: *mut GUint,
    utf8_str: *mut *mut GChar,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // An unknown token answers `false` with both out-params untouched
    // (`_token_get_phrase` returns before writing, `pinyin.cpp:1627`; the
    // zhuyin twin shares it).
    let Some(phrase) = inst.core.token_phrase(token) else {
        return false;
    };
    write_if_nonnull!(
        len,
        GUint::try_from(phrase.phrase_length).unwrap_or(GUint::MAX)
    );
    // The pin stores NULL and answers true when g_ucs4_to_utf8 cannot
    // convert a word or allocate the string (pinyin.cpp:1641-1645,
    // zhuyin.cpp:1110-1113).
    write_if_nonnull!(
        utf8_str,
        phrase.text.map_or(std::ptr::null_mut(), |text| {
            let text = phrase_text_bytes(&text);
            crate::ffi::malloc_cstr_with(text.len(), |out| {
                out.copy_from_slice(&text);
                text.len()
            })
        })
    );
    true
}

/// Get the number of pronunciations of a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_get_n_pronunciation(zhuyin_instance_t * instance,
///                                       phrase_token_t token, guint * num);
/// ```
///
/// Write the pronunciation count and return true for a known token. On a live instance a token
/// miss writes zero and returns false. A null instance returns false without writing.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `num` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_token_get_n_pronunciation(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    num: *mut GUint,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    write_if_nonnull!(num, 0);
    let Some(count) = inst.core.token_pronunciation_count(token) else {
        return false;
    };
    write_if_nonnull!(num, count);
    true
}

/// Append one reading's stored packed words to the caller's array, or the
/// pin's zeroed past-the-end words when the item has no such reading.
///
/// # Safety
/// `keys` must be a non-NULL `GArray` whose element size is two bytes, as
/// the caller of `zhuyin_token_get_nth_pronunciation` creates it.
unsafe fn append_stored_words(keys: *mut GArray, length: usize, words: Option<&[u8]>) {
    match words {
        None => {
            let zeroed = vec![0_u16; length];
            // SAFETY: `keys` has two-byte elements. The append copies from the live zeroed buffer.
            unsafe {
                g_array_append_vals(
                    keys,
                    zeroed.as_ptr().cast::<c_void>(),
                    c_uint::try_from(zeroed.len()).unwrap_or(0),
                );
            }
        }
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
///
/// For an existing token, `nth` must be less than its pronunciation count. The function clears
/// the caller’s array, appends the selected reading’s packed two-byte keys and returns true. A
/// token miss returns false with an empty array. A null instance returns false without clearing
/// it.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies. `keys` is NULL or a live GLib `GArray` whose element size is two bytes
/// (`ChewingKey`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_token_get_nth_pronunciation(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    nth: GUint,
    keys: *mut GArray,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // The pin clears the caller's array before appending
    // (`zhuyin.cpp:1793` `g_array_set_size(keys, 0)`), so a stale or
    // re-used GArray never shows concatenated results on either path.
    // SAFETY: GLib checks NULL and logs a critical before any access.
    // `g_array_set_size` on a real glib
    // GArray updates `len` and preserves its private metadata.
    unsafe {
        g_array_set_size(keys, 0);
    }
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // A `USER_FILE` reading lives in the session arena, so hand its stored
    // words straight to the caller's array instead of copying them through
    // an intermediate `Vec`.
    if inst
        .core
        .dict
        .with_user_nth_pronunciation_words(token, nth as usize, |length, words| {
            if !keys.is_null() {
                // SAFETY: The caller supplies a GArray with two-byte
                // elements and the store lends the words for this copy.
                unsafe { append_stored_words(keys, length, words) };
            }
        })
        .is_some()
    {
        if keys.is_null() {
            // SAFETY: GLib diagnoses a NULL array before accessing any data.
            // The item was found, and the store has released its reading.
            unsafe { g_array_append_vals(keys, std::ptr::null(), 0) };
        }
        return true;
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
    // An empty persisted phrase still owns its empty-key pronunciation;
    // the pin appends zero words and answers true (074a2219 zhuyin.cpp:1807-1810).
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

// zhuyin.cpp:1813-1826.
/// Get the unigram frequency of a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_get_unigram_frequency(zhuyin_instance_t * instance,
///                                         phrase_token_t token, guint * freq);
/// ```
///
/// Write the live unigram frequency and return true for a known token. On a live instance a
/// token miss writes zero and returns false. A null instance returns false without writing.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `freq` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_token_get_unigram_frequency(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    freq: *mut GUint,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    write_if_nonnull!(freq, 0);
    // The pin reads the item of whichever sub-index the token's nibble
    // names; a library that is missing or owns no such item answers
    // `false` (`get_phrase_item`'s `ERROR_NO_SUB_PHRASE_INDEX` /
    // `ERROR_NO_ITEM`).
    let Some(count) = inst.core.token_unigram_frequency(token) else {
        return false;
    };
    write_if_nonnull!(freq, GUint::try_from(count).unwrap_or(GUint::MAX));
    true
}

/// Add a unigram-frequency delta to a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_add_unigram_frequency(zhuyin_instance_t * instance,
///                                         phrase_token_t token, guint delta);
/// ```
///
/// Add the delta to the live item. Returns false on a failed update. If the sub-index is loaded,
/// the aggregate can change even when the item update fails (`phrase_index.h:628-635`). A later
/// armed save persists live item changes; this call alone does not arm it.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_token_add_unigram_frequency(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    delta: GUint,
) -> bool {
    let ctx = borrow_or_return!(instance, crate::state::instance_context_mut, false);
    ctx.core.runtime.add_unigram_delta(token, u64::from(delta))
}

#[cfg(test)]
mod tests;
