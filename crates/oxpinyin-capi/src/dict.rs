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

use std::os::raw::{c_char, c_uint, c_void};
use std::ptr;

use oxpinyin_core::Dictionary;

use glib_sys::{g_array_append_vals, g_array_set_size};

use crate::ffi::{cstr_to_string, ffi_catch, owned_cstr};
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
    if instance.is_null() || phrase.is_null() {
        return false;
    }
    if tokenarray.is_null() {
        // The pin dereferences the caller's array unguarded; the
        // no-abort policy refuses instead.
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        // SAFETY: Null-checked above.
        let text = unsafe { cstr_to_string(phrase) };
        let tokens: Vec<u32> = inst
            .dict
            .tokens_for_text(&text)
            .iter()
            .map(|token| token.value())
            .collect();
        // `reduce_tokens` clears the caller's array before appending
        // (`phrase_large_table3.h:81`) — an empty result clears too.
        // SAFETY: Null-checked above; `g_array_set_size` on a real
        // glib GArray updates `len` and preserves the private
        // metadata (`alloc`, `element_size`, `clear_func`).
        unsafe {
            g_array_set_size(tokenarray, 0);
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
                tokens.len() as c_uint,
            );
        }
        true
    })
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
/// `false` for an unknown token or an unloaded library; both
/// out-params are optional; the string is caller-owned (`g_free`).
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
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Some(intro) = inst.dict.token_introspection(token) else {
            if !utf8_str.is_null() {
                // SAFETY: Null-checked above.
                unsafe {
                    *utf8_str = ptr::null_mut();
                }
            }
            return false;
        };
        if !len.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *len = intro.text.chars().count() as GUint;
            }
        }
        if !utf8_str.is_null() {
            let rendered = owned_cstr(&intro.text);
            // SAFETY: Null-checked above.
            unsafe {
                *utf8_str = rendered;
            }
            if rendered.is_null() {
                return false;
            }
        }
        true
    })
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
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        if !num.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *num = 0;
            }
        }
        let Some(intro) = inst.dict.token_introspection(token) else {
            return false;
        };
        if !num.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *num = intro.pronunciations.len() as GUint;
            }
        }
        true
    })
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
/// `pinyin_get_pinyin_key` hands out. `false` for an unknown token;
/// an out-of-range `nth` answers `false` where upstream appends
/// uninitialized stack bytes (the no-abort policy refuses instead).
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
    if keys.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Some(intro) = inst.dict.token_introspection(token) else {
            return false;
        };
        let Some((keys_list, _count)) = intro.pronunciations.get(nth as usize) else {
            return false;
        };
        // Pack each syllable key into its two-byte chewing-key word.
        let mut packed: Vec<u16> = Vec::with_capacity(keys_list.len());
        for &key in keys_list {
            let Some(syllable) = oxpinyin_core::SyllableKey::from_index(key as usize) else {
                return false;
            };
            let Some(chewing) = oxpinyin_core::ChewingKey::from_pinyin(syllable.text()) else {
                return false;
            };
            packed.push(chewing.to_packed());
        }
        if packed.is_empty() {
            return false;
        }
        // SAFETY: Null-checked above; the caller creates the keys
        // array with element_size = sizeof(u16) (chewing-key vector,
        // packed two-byte words). `g_array_append_vals` copies
        // `packed.len()` such elements from the buffer.
        unsafe {
            g_array_append_vals(
                keys,
                packed.as_ptr().cast::<c_void>(),
                packed.len() as c_uint,
            );
        }
        true
    })
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
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        if !freq.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *freq = 0;
            }
        }
        // The pin's `PhraseItem::get_unigram_frequency` is the phrase
        // index's trained count — the LM's interpolation2 table carries
        // that number in oxpinyin (the flat pinyin-index aggregation is
        // the suggestion store's, a different surface). User tokens read
        // their stored count; the overlay delta rides on top.
        let nibble = token >> 24;
        let base = match nibble {
            1..=4 => {
                if !inst.dict.library_visible_token(token) {
                    // Unloaded library — the item is hidden, so no
                    // frequency is reported (matches the visibility
                    // filter every other Tier-C read honours).
                    None
                } else {
                    use oxpinyin_core::LanguageModel;
                    let trained = inst
                        .lm
                        .unigram_freq(&oxpinyin_core::PhraseToken::new(token))
                        .ok()
                        .flatten()
                        // The fixture LM carries no real-unigram flag; its
                        // table was seeded from this same map, so the
                        // fallback is the same number.
                        .or_else(|| inst.dict.system_unigram_count(token));
                    // The trainer's avoid-zero constant: gen_unigram adds
                    // `guint32 freq = 1` to every SYSTEM_FILE/DICTIONARY
                    // item ("To avoid zero value when computing unigram
                    // frequency in float format", gen_unigram.cpp:34-49),
                    // so the stored item count is the trained count + 1.
                    // The exported tables carry the pre-constant values.
                    trained.map(|count| count + 1)
                }
            }
            5..=6 => inst.dict.addon_unigram_frequency(token).or_else(|| {
                use oxpinyin_core::LanguageModel;
                inst.lm
                    .addon_unigram_freq(&oxpinyin_core::PhraseToken::new(token))
                    .ok()
                    .flatten()
            }),
            7 => inst
                .user
                .as_ref()
                .and_then(|store| store.unigram_delta(token).ok()),
            _ => None,
        };
        let Some(base) = base else {
            return false;
        };
        let count = base + inst.dict.unigram_delta(token).unwrap_or(0);
        if !freq.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *freq = GUint::try_from(count).unwrap_or(GUint::MAX);
            }
        }
        true
    })
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
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        inst.dict.add_unigram_delta(token, delta as u64)
    })
}
