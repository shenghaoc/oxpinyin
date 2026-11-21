//! The phrase-result surface: `zhuyin_phrase_segment` and the two getters
//! over `m_phrase_result`.

use std::os::raw::c_char;

use crate::ffi::{borrow_or_return, write_if_nonnull};
use crate::state::{instance_mut, instance_ref};
use crate::types::{GUint, PhraseTokenT, ZhuyinInstance};

// zhuyin.cpp:973.
/// Segment an arbitrary sentence string into phrase tokens.
///
/// # C signature
/// ```c
/// bool zhuyin_phrase_segment(zhuyin_instance_t * instance,
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
pub unsafe extern "C" fn zhuyin_phrase_segment(
    instance: *mut ZhuyinInstance,
    sentence: *const c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // The pin's first call, `g_utf8_strlen(sentence, -1)` (074a2219
    // zhuyin.cpp:969), made through GLib: on NULL, GLib's own precondition
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
        // `zhuyin_alloc_instance`.
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
        // 074a2219 zhuyin.cpp:973: `g_return_val_if_fail(num_of_chars ==
        // ucs4_len, FALSE)` answers false and leaves the result as it was. Its
        // critical is not reproduced (docs/findings/upstream-divergences.md#phrase-segmentation-writes-no-invalid-utf-8-critical).
        return false;
    };
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
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
            crate::sentence::refuse_lookup_gram("zhuyin_phrase_segment", &error);
            false
        }
    }
}

/// Get the number of phrase tokens in the phrase result.
///
/// # C signature
/// ```c
/// bool zhuyin_get_n_phrase(zhuyin_instance_t * instance, guint * num);
/// ```
///
/// Returns true for a live instance and writes the result count if requested. The count starts
/// at zero. A null instance returns false without writing.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `num` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_get_n_phrase(
    instance: *mut ZhuyinInstance,
    num: *mut GUint,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    let count = inst.core.phrase_count();
    write_if_nonnull!(num, GUint::try_from(count).unwrap_or(GUint::MAX));
    true
}

/// Get the phrase token at an index of the phrase result.
///
/// # C signature
/// ```c
/// bool zhuyin_get_phrase_token(zhuyin_instance_t * instance,
///                              guint index, phrase_token_t * token);
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
pub unsafe extern "C" fn zhuyin_get_phrase_token(
    instance: *mut ZhuyinInstance,
    index: GUint,
    token: *mut PhraseTokenT,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    write_if_nonnull!(token, oxpinyin::NULL_TOKEN);
    let Some(value) = inst.core.phrase_token(index as usize) else {
        return false;
    };
    write_if_nonnull!(token, value);
    true
}
