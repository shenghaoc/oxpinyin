//! Candidate access, selection, and training.

use std::os::raw::c_int;
use std::ptr;

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
    if !num.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *num = 0;
        }
    }
    // STUB: T3 will implement.
    false
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
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_candidate(
    instance: *mut PinyinInstance,
    _index: GUint,
    candidate: *mut *mut LookupCandidate,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !candidate.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *candidate = ptr::null_mut();
        }
    }
    // STUB: T3 will implement.
    false
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
    if !candidate_type.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *candidate_type = lookup_candidate_type_t::NORMAL_CANDIDATE;
        }
    }
    // STUB: T3 will implement.
    false
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
    if !utf8_str.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *utf8_str = ptr::null();
        }
    }
    // STUB: T3 will implement.
    false
}

/// Get the n-best index of a candidate.
///
/// # C signature
/// ```c
/// bool pinyin_get_candidate_nbest_index(pinyin_instance_t * instance,
///                                       lookup_candidate_t * candidate,
///                                       guint8 * index);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_candidate_nbest_index(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    index: *mut u8,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    if !index.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *index = 0;
        }
    }
    // STUB: T3 will implement.
    false
}

/// Check whether a candidate is a user candidate.
///
/// # C signature
/// ```c
/// bool pinyin_is_user_candidate(pinyin_instance_t * instance,
///                               lookup_candidate_t * candidate);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_is_user_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    // STUB: T4 will implement.
    false
}

/// Remove a user candidate from the dictionary.
///
/// # C signature
/// ```c
/// bool pinyin_remove_user_candidate(pinyin_instance_t * instance,
///                                   lookup_candidate_t * candidate);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_remove_user_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    // STUB: T4 will implement.
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
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_choose_candidate(
    instance: *mut PinyinInstance,
    _offset: usize,
    candidate: *mut LookupCandidate,
) -> c_int {
    if instance.is_null() || candidate.is_null() {
        return -1;
    }
    // STUB: T4 will implement.
    -1
}

/// Choose a predicted candidate.
///
/// # C signature
/// ```c
/// bool pinyin_choose_predicted_candidate(pinyin_instance_t * instance,
///                                        lookup_candidate_t * candidate);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_choose_predicted_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    // STUB: T4 will implement.
    false
}

/// Train the current sentence with the given n-best index.
///
/// # C signature
/// ```c
/// bool pinyin_train(pinyin_instance_t * instance, guint8 index);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_train(instance: *mut PinyinInstance, _index: u8) -> bool {
    if instance.is_null() {
        return false;
    }
    // STUB: T4 will implement.
    false
}
