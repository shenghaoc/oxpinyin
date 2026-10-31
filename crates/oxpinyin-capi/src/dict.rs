//! The dictionary-introspection surface: token lookups, per-token
//! reads, the unigram-frequency write, and the phrase-library
//! load/unload pair — Tier C of the 79/79 target.
//!
//! Upstream these run against `FacadePhraseIndex`/`PhraseItem`
//! (`pinyin.cpp:2648-2665, 2774-2843`, `phrase_index.h:607-636`). The
//! reads dispatch by library nibble across oxpinyin's seams (system
//! monolith, loaded addons, user store); the unigram write is an
//! in-memory overlay — nothing persists system counts, matching
//! upstream where `pinyin_save` flushes user data exclusively.
//!
//! The `GArray`-taking symbols (`pinyin_lookup_tokens`,
//! `pinyin_token_get_nth_pronunciation`) delegate to glib itself —
//! `g_array_append_vals` grows the array and updates its private
//! `_GRealArray` metadata (`alloc`, `element_size`, `clear_func`) in
//! lockstep with the public `data`/`len`. Hand-`realloc`ing the buffer
//! and bumping `len` (an earlier revision's shortcut) left those private
//! fields stale, so a downstream `g_array_append_val` from the same
//! consumer would `g_realloc` from the wrong `alloc` and either truncate
//! the appended payload or trip `g_array_maybe_expand`'s SIGFPE. Linking
//! glib is free at the target: every box that runs a libpinyin consumer
//! already has `libglib-2.0.so.0` resident (upstream `libpinyin.so.15`
//! records it in `DT_NEEDED`).

use oxpinyin_capi_marshal::phrase_text::phrase_text_bytes;
use std::os::raw::{c_char, c_uint, c_void};

use oxpinyin_core::Dictionary;

use glib_sys::{g_array_append_vals, g_array_set_size};

use crate::state::instance_ref;
use crate::types::{GChar, GUint, PhraseTokenT, PinyinInstance};

/// Look up the phrase tokens stored for an exact phrase string.
///
/// # C signature
/// ```c
/// bool pinyin_lookup_tokens(pinyin_instance_t * instance,
///                           const char * phrase, GArray * tokenarray);
/// ```
///
/// Upstream reduces the per-library search hits into the caller's
/// array in library-index order (`reduce_tokens`,
/// `phrase_large_table3.h:77-96`) and answers `SEARCH_OK & retval` —
/// `true` exactly when the span matched at least one stored phrase,
/// `false` for a no-hit span (the array still cleared). Unloaded
/// libraries contribute nothing.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_lookup_tokens(
    instance: *mut PinyinInstance,
    phrase: *const c_char,
    tokenarray: *mut crate::types::GArray,
) -> bool {
    if instance.is_null() {
        return false;
    }
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
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
    // `reduce_tokens` clears the caller's array before appending
    // (`phrase_large_table3.h:81`) — an empty result clears too.
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
    // The retval is `SEARCH_OK & retval`: exact hits exist.
    if tokens.is_empty() {
        return false;
    }
    // SAFETY: Null-checked above; `g_array_append_vals` copies
    // `tokens.len()` elements of the array's stored `element_size`
    // (guint32, the caller creates the array with element_size=4).
    // The tokens slice is a live Rust allocation and stays valid
    // for the duration of the call.
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
/// bool pinyin_token_get_phrase(pinyin_instance_t * instance,
///                              phrase_token_t token, guint * len,
///                              gchar ** utf8_str);
/// ```
///
/// `false` for an unknown token or an unloaded library, with both
/// out-params untouched; both out-params are optional; the string is
/// caller-owned (`g_free`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_token_get_phrase(
    instance: *mut PinyinInstance,
    token: PhraseTokenT,
    len: *mut GUint,
    utf8_str: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // An unknown token answers `false` with both out-params untouched
    // (`_token_get_phrase` returns before writing, `pinyin.cpp:1627`).
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
/// bool pinyin_token_get_n_pronunciation(pinyin_instance_t * instance,
///                                       phrase_token_t token, guint * num);
/// ```
///
/// `false` for an unknown token or an unloaded library; `*num` is
/// zeroed before the dispatch, so a `false` still delivers 0.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_token_get_n_pronunciation(
    instance: *mut PinyinInstance,
    token: PhraseTokenT,
    num: *mut GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
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
/// the caller of `pinyin_token_get_nth_pronunciation` creates it.
unsafe fn append_stored_words(
    keys: *mut crate::types::GArray,
    length: usize,
    words: Option<&[u8]>,
) -> bool {
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
/// bool pinyin_token_get_nth_pronunciation(pinyin_instance_t * instance,
///                                         phrase_token_t token, guint nth,
///                                         ChewingKeyVector keys);
/// ```
///
/// Appends the pronunciation's keys to the caller's array — packed
/// two-byte chewing-key words, the same layout
/// `pinyin_get_pinyin_key` hands out. The array is emptied first
/// (`pinyin.cpp:2805`), so `false` for an unknown token leaves it empty.
/// An `nth` past the last reading answers `true` and appends
/// `phrase_length` keys, where upstream appends whatever its
/// uninitialised stack buffer held (`pinyin.cpp:2808-2817`, the return
/// value of `get_nth_pronunciation` ignored): zeroed keys, class (b) for
/// the content (register row 51).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_token_get_nth_pronunciation(
    instance: *mut PinyinInstance,
    token: PhraseTokenT,
    nth: GUint,
    keys: *mut crate::types::GArray,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: GLib checks NULL and logs a critical before any access.
    // `g_array_set_size` on a real glib GArray
    // updates `len` and preserves the private metadata.
    unsafe {
        g_array_set_size(keys, 0);
    }
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
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
        // content at the pin; zeroed here.
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
    // SAFETY: GLib checks NULL and logs a critical before any access.
    // the caller creates the keys
    // array with element_size = sizeof(u16) (chewing-key vector,
    // packed two-byte words). `g_array_append_vals` copies
    // `packed.len()` such elements from the buffer.
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
/// bool pinyin_token_get_unigram_frequency(pinyin_instance_t * instance,
///                                          phrase_token_t token,
///                                          guint * freq);
/// ```
///
/// `*freq` is zeroed before the dispatch, so a `false` still delivers
/// 0 (`pinyin.cpp:2821-2831`); the read includes the
/// `pinyin_token_add_unigram_frequency` overlay.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_token_get_unigram_frequency(
    instance: *mut PinyinInstance,
    token: PhraseTokenT,
    freq: *mut GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    if !freq.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *freq = 0;
        }
    }
    // The pin's `PhraseItem::get_unigram_frequency` is the chunk
    // item's stored field (`gen_unigram`'s +1 included), which the
    // language model hands out as is; the user store's trained delta
    // rides on it there, the overlay delta below. A token whose
    // library owns no item answers `false`, as `get_phrase_item`
    // failing does upstream.
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
        // The default facade's `USER_FILE` sub-indexes (addon.bin /
        // network.bin / user.bin): the pin reads the item from
        // `m_phrase_index`, never the addon facade (`pinyin.cpp:2821-2834`
        // via `phrase_index.h:646-652`). The stored field is the user
        // store's UNIGRAM accumulation; an absent item answers `false`, as
        // `get_phrase_item`'s `ERROR_NO_ITEM` does.
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
/// bool pinyin_token_add_unigram_frequency(pinyin_instance_t * instance,
///                                          phrase_token_t token,
///                                          guint delta);
/// ```
///
/// In-memory only — nothing persists it, matching upstream where the
/// facade total and the item count move in RAM and `pinyin_save`
/// flushes user data exclusively. The facade-total bump is
/// unconditional once the token's library is loaded
/// (`phrase_index.h:632`, before the item dispatch), so an absent-token
/// add answers `false` yet still shifts the amplified-law denominator
/// (`pinyin.cpp:1817`) — reproduced exactly.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_token_add_unigram_frequency(
    instance: *mut PinyinInstance,
    token: PhraseTokenT,
    delta: GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    inst.core.dict.add_unigram_delta(token, delta as u64)
}
