//! Text segmentation into phrase tokens and access to the result.
//!
//! The result getters read `m_phrase_result` (`pinyin.cpp:3269-3287`).
//!
//! The segmenter is the engine's span DP (`oxpinyin::engine::Session::
//! phrase_segment`, the `PhraseLookup::get_best_match` port); this
//! module only stores its result on the instance and hands out reads.
//! The result shape is the pin's: a character-length array with each
//! phrase's token at its span's start position and `null_token` between
//! phrases — and, on a failed match, the fully sized all-null array
//! (`PhraseLookup::final_step` sizes and null-fills before its
//! empty-last-step `false`, `phrase_lookup.cpp:382-428`). `pinyin_reset`
//! clears the array (`pinyin.cpp:2699`); nothing else removes from it.

use std::os::raw::c_char;

use crate::ffi::{borrow_or_return, write_if_nonnull};
use crate::state::{instance_mut, instance_ref};
use crate::types::{GUint, PhraseTokenT, PinyinInstance};

// pinyin.cpp:1454.
/// Segment an arbitrary sentence string into phrase tokens.
///
/// # C signature
/// ```c
/// bool pinyin_phrase_segment(pinyin_instance_t * instance,
///                            const char * sentence);
/// ```
///
/// Segmentation that completes replaces the instance's phrase result, even when no path
/// matches. A failed input conversion returns `false` and keeps the previous result. An
/// unmatched sentence can leave a result sized for the sentence, with every token null. A null
/// sentence follows the empty-input path.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `sentence` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_phrase_segment(
    instance: *mut PinyinInstance,
    sentence: *const c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // The pin's first call, `g_utf8_strlen(sentence, -1)` (074a2219
    // pinyin.cpp:1450), made through GLib: on NULL, GLib's own precondition
    // logs its critical (GLib 2.84.4 gutf8.c:240). The pin then dereferences
    // NULL in `g_utf8_to_ucs4`; take the ruled no-input outcome instead
    // (074a2219 phrase_lookup.cpp:119-157,382-428).
    if sentence.is_null() {
        // SAFETY: GLib checks `p != NULL || max == 0` itself, logs and
        // returns 0 without reading `p`.
        unsafe {
            glib_sys::g_utf8_strlen(sentence, -1);
        }
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let mut inst = unsafe { instance_mut(instance) };
        return match inst.core.segment_phrases("") {
            Ok(matched) => matched,
            Err(error) => {
                crate::sentence::refuse_lookup_gram("phrase_segment", &error);
                false
            }
        };
    }
    let Some(text) = oxpinyin::marshal::utf8_to_ucs4(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(sentence) },
    ) else {
        // 074a2219 pinyin.cpp:1454: `g_return_val_if_fail(num_of_chars ==
        // ucs4_len, FALSE)` answers false and leaves the result as it was. Its
        // critical is not reproduced (docs/findings/upstream-divergences.md#phrase-segmentation-writes-no-invalid-utf-8-critical).
        return false;
    };
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    let outcome = match text.as_str() {
        Some(text) => inst.core.segment_phrases(text),
        None => inst
            .core
            .segment_phrases_ucs4(text.stored_words().unwrap_or_default()),
    };
    match outcome {
        Ok(matched) => matched,
        Err(error) => {
            crate::sentence::refuse_lookup_gram("pinyin_phrase_segment", &error);
            false
        }
    }
}

/// Get the number of phrase tokens in the phrase result.
///
/// # C signature
/// ```c
/// bool pinyin_get_n_phrase(pinyin_instance_t * instance, guint * num);
/// ```
///
/// Upstream answers `true` unconditionally over whatever
/// `pinyin_phrase_segment` last stored (`pinyin.cpp:3269-3274`); without
/// a prior segment call the count is 0.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `num` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_n_phrase(
    instance: *mut PinyinInstance,
    num: *mut GUint,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    let count = inst.core.phrase_count();
    write_if_nonnull!(num, GUint::try_from(count).unwrap_or(GUint::MAX));
    true
}

// pinyin.cpp:3276-3287.
/// Get the phrase token at an index of the phrase result.
///
/// # C signature
/// ```c
/// bool pinyin_get_phrase_token(pinyin_instance_t * instance,
///                              guint index,
///                              phrase_token_t * token);
/// ```
///
/// Returns true for an existing result index. On a live instance an absent index writes the null
/// token and returns false. A null instance returns false without writing.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `token` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_phrase_token(
    instance: *mut PinyinInstance,
    index: GUint,
    token: *mut PhraseTokenT,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    write_if_nonnull!(token, oxpinyin::vocab::NULL_TOKEN);
    let Some(value) = inst.core.phrase_token(index as usize) else {
        return false;
    };
    write_if_nonnull!(token, value);
    true
}
