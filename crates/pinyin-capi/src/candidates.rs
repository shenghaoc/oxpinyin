//! Candidate access, selection, and training.

use std::os::raw::c_int;

use pinyin_engine::CandidateKind;

use crate::ffi::ffi_catch;
use crate::state::{candidate_ptr, candidate_ref, instance_mut, instance_ref};
use crate::types::{GChar, GUint, LookupCandidate, PinyinInstance, lookup_candidate_type_t};

/// Get the number of candidates.
///
/// # C signature
/// ```c
/// bool pinyin_get_n_candidate(pinyin_instance_t * instance, guint * num);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_n_candidate(instance: *mut PinyinInstance, num: *mut GUint) -> bool {
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
                *num = inst.candidates.len() as GUint;
            }
        }
        true
    })
}

/// Get a candidate by index.
///
/// # C signature
/// ```c
/// bool pinyin_get_candidate(pinyin_instance_t * instance,
///                           guint index,
///                           lookup_candidate_t ** candidate);
/// ```
///
/// Out-param `candidate` is instance-borrowed (never freed by caller).
/// Valid until the next `pinyin_guess_candidates` call.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_candidate(
    instance: *mut PinyinInstance,
    index: GUint,
    candidate: *mut *mut LookupCandidate,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let idx = index as usize;
        match inst.candidates.get(idx) {
            Some(cand) => {
                if !candidate.is_null() {
                    // SAFETY: Null-checked above.
                    unsafe {
                        *candidate = candidate_ptr(cand);
                    }
                }
                true
            }
            None => {
                if !candidate.is_null() {
                    // SAFETY: Null-checked above.
                    unsafe {
                        *candidate = std::ptr::null_mut();
                    }
                }
                false
            }
        }
    })
}

/// Get the type of a lookup candidate.
///
/// # C signature
/// ```c
/// bool pinyin_get_candidate_type(pinyin_instance_t * instance,
///                                lookup_candidate_t * candidate,
///                                lookup_candidate_type_t * type);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_candidate_type(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    candidate_type: *mut lookup_candidate_type_t,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `candidate` is non-null and was produced by
        // `pinyin_get_candidate`.
        let cand = unsafe { candidate_ref(candidate) };
        let ctype = match cand.kind {
            CandidateKind::Sentence => lookup_candidate_type_t::NBEST_MATCH_CANDIDATE,
            CandidateKind::Phrase => lookup_candidate_type_t::NORMAL_CANDIDATE,
            CandidateKind::Fallback | _ => lookup_candidate_type_t::NORMAL_CANDIDATE,
        };
        if !candidate_type.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *candidate_type = ctype;
            }
        }
        true
    })
}

/// Get the display string of a candidate.
///
/// # C signature
/// ```c
/// bool pinyin_get_candidate_string(pinyin_instance_t * instance,
///                                  lookup_candidate_t * candidate,
///                                  const gchar ** utf8_str);
/// ```
///
/// Out-param `utf8_str` is instance-borrowed (never freed by caller).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_candidate_string(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    utf8_str: *mut *const GChar,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `candidate` is non-null and was produced by
        // `pinyin_get_candidate`.
        let cand = unsafe { candidate_ref(candidate) };
        if !utf8_str.is_null() {
            // SAFETY: Null-checked above. Pointer borrows into the
            // CapiCandidate's CString, valid until candidates are rebuilt.
            unsafe {
                *utf8_str = cand.text.as_ptr();
            }
        }
        true
    })
}

/// Get the n-best index of a candidate.
///
/// # C signature
/// ```c
/// bool pinyin_get_candidate_nbest_index(pinyin_instance_t * instance,
///                                       lookup_candidate_t * candidate,
///                                       guint8 * index);
/// ```
///
/// Provisional: returns the stored nbest_index (always 0 with StubLm).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_candidate_nbest_index(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    index: *mut u8,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `candidate` is non-null and was produced by
        // `pinyin_get_candidate`.
        let cand = unsafe { candidate_ref(candidate) };
        if !index.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *index = cand.nbest_index;
            }
        }
        true
    })
}

/// Check whether a candidate is a user candidate.
///
/// # C signature
/// ```c
/// bool pinyin_is_user_candidate(pinyin_instance_t * instance,
///                               lookup_candidate_t * candidate);
/// ```
///
/// Provisional: always returns false (no user dictionary yet).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_is_user_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    false
}

/// Remove a user candidate from the dictionary.
///
/// # C signature
/// ```c
/// bool pinyin_remove_user_candidate(pinyin_instance_t * instance,
///                                   lookup_candidate_t * candidate);
/// ```
///
/// Provisional: always returns false (no user dictionary yet).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_remove_user_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    false
}

/// Choose a candidate at an offset, returning the new cursor position.
///
/// # C signature
/// ```c
/// int pinyin_choose_candidate(pinyin_instance_t * instance,
///                             size_t offset,
///                             lookup_candidate_t * candidate);
/// ```
///
/// Returns -1 on failure (consistent with the `int` return type).
/// Provisional: computes candidate index from the pointer offset into the
/// instance's candidate vec and calls `Session::select`.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_choose_candidate(
    instance: *mut PinyinInstance,
    offset: usize,
    candidate: *mut LookupCandidate,
) -> c_int {
    if instance.is_null() || candidate.is_null() {
        return -1;
    }
    ffi_catch(-1, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        if inst.candidates.is_empty() {
            return -1;
        }
        let base = inst.candidates.as_ptr();
        // SAFETY: `candidate` was produced by `pinyin_get_candidate` and
        // points into `inst.candidates`.
        let index = unsafe {
            candidate
                .cast::<crate::state::CapiCandidate>()
                .offset_from(base)
        };
        if index < 0 || index as usize >= inst.candidates.len() {
            return -1;
        }
        let index = index as usize;
        let consumed_bytes = inst
            .session
            .candidates()
            .get(index)
            .map(|c| c.consumed_bytes())
            .unwrap_or(0);
        match inst.session.select(index) {
            Ok(_) => (offset + consumed_bytes) as c_int,
            Err(_) => -1,
        }
    })
}

/// Choose a predicted candidate.
///
/// # C signature
/// ```c
/// bool pinyin_choose_predicted_candidate(pinyin_instance_t * instance,
///                                        lookup_candidate_t * candidate);
/// ```
///
/// Provisional: always returns false (prediction requires a real LM).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_choose_predicted_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    false
}

/// Train the current sentence with the given n-best index.
///
/// # C signature
/// ```c
/// bool pinyin_train(pinyin_instance_t * instance, guint8 index);
/// ```
///
/// Provisional: always returns true (no training with StubLm).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_train(instance: *mut PinyinInstance, _index: u8) -> bool {
    if instance.is_null() {
        return false;
    }
    true
}
