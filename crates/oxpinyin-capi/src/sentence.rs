//! Sentence guessing and retrieval.

use std::ffi::CString;
use std::os::raw::c_char;

use crate::ffi::{cstr_to_string, ffi_catch, owned_cstr};
use crate::state::{CapiCandidate, instance_mut, instance_ref};
use crate::types::{GUint, PinyinInstance};

/// Guess a sentence from saved pinyin keys.
///
/// # C signature
/// ```c
/// bool pinyin_guess_sentence(pinyin_instance_t * instance);
/// ```
///
/// With StubDict the sentence is the raw input itself; real sentence
/// decoding arrives with T4 backends.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_sentence(instance: *mut PinyinInstance) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        inst.session.is_composing()
    })
}

/// Guess predicted candidates with punctuations after a prefix.
///
/// # C signature
/// ```c
/// bool pinyin_guess_predicted_candidates_with_punctuations(
///     pinyin_instance_t * instance, const char * prefix);
/// ```
///
/// Provisional: always returns false (prediction requires a real LM).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_predicted_candidates_with_punctuations(
    instance: *mut PinyinInstance,
    _prefix: *const c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }
    false
}

/// Get a sentence string from the instance (n-best variant).
///
/// # C signature
/// ```c
/// bool pinyin_get_sentence(pinyin_instance_t * instance,
///                          guint8 index,
///                          char ** sentence);
/// ```
///
/// Out-param `sentence` is caller-owned (`g_free`). The returned buffer is
/// allocated with libc `malloc`, which `g_free` releases on every platform.
///
/// Provisional: ignores the n-best `index` and returns the preedit text.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_sentence(
    instance: *mut PinyinInstance,
    _index: u8,
    sentence: *mut *mut c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        if !inst.session.is_composing() {
            if !sentence.is_null() {
                // SAFETY: Null-checked above.
                unsafe {
                    *sentence = std::ptr::null_mut();
                }
            }
            return false;
        }
        let preedit = inst.session.preedit();
        if !sentence.is_null() {
            // SAFETY: Null-checked above. `owned_cstr` returns null on an
            // interior NUL or allocation failure; otherwise ownership
            // transfers to the caller, which frees it with `g_free`.
            let owned = owned_cstr(preedit.text());
            // SAFETY: Null-checked above.
            unsafe {
                *sentence = owned;
            }
            if owned.is_null() {
                return false;
            }
        }
        true
    })
}

/// Get character offset from a lookup byte offset within a sentence.
///
/// # C signature
/// ```c
/// bool pinyin_get_character_offset(pinyin_instance_t * instance,
///                                  const char * phrase,
///                                  size_t offset,
///                                  size_t * length);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_character_offset(
    instance: *mut PinyinInstance,
    phrase: *const c_char,
    offset: usize,
    length: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `phrase` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(phrase) };
        let mut clamped = offset.min(text.len());
        // Floor to a UTF-8 char boundary so the slice never panics.
        while !text.is_char_boundary(clamped) {
            clamped -= 1;
        }
        let char_count = text[..clamped].chars().count();
        if !length.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *length = char_count;
            }
        }
        true
    })
}

/// Guess candidates at the given offset with sort option.
///
/// # C signature
/// ```c
/// bool pinyin_guess_candidates(pinyin_instance_t * instance,
///                              size_t offset,
///                              guint sort_option);
/// ```
///
/// Provisional: `offset` and `sort_option` are ignored — the engine has no
/// positional or sort backends yet. Snapshots the session's current
/// candidates into the instance's `CapiCandidate` vec. With StubDict the
/// candidate list is empty.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_candidates(
    instance: *mut PinyinInstance,
    _offset: usize,
    _sort_option: GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        inst.candidates.clear();
        for cand in inst.session.candidates().iter() {
            let text = match CString::new(cand.text().to_owned()) {
                Ok(s) => s,
                Err(_) => continue,
            };
            inst.candidates.push(CapiCandidate {
                text,
                kind: cand.kind(),
                nbest_index: 0,
                consumed_bytes: cand.consumed_bytes(),
                token: cand.token(),
            });
        }
        true
    })
}
