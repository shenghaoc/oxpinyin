//! Candidate access, selection, and training.
//!
//! The four symbols that read or write `lookup_candidate_type_t` use the
//! zhuyin-local 4-value enum — `zhuyin_guess_candidates_after_cursor`,
//! `zhuyin_guess_candidates_before_cursor`, `zhuyin_choose_candidate`,
//! `zhuyin_get_candidate_type`. The zhuyin enum's discriminants collide with
//! the pinyin eight at 3 and 4 (see [`crate::types::lookup_candidate_type_t`]),
//! so tagging must use the zhuyin enum, never the pinyin one.

use std::os::raw::c_int;

use oxpinyin_engine::CandidateKind;

use crate::ffi::ffi_catch;
use crate::state::{
    CapiCandidate, CapiInstance, candidate_ptr, candidate_ref, instance_mut, instance_ref,
};
use crate::types::{GChar, GUint, LookupCandidate, ZhuyinInstance, lookup_candidate_type_t};

/// Get the number of candidates.
///
/// # C signature
/// ```c
/// bool zhuyin_get_n_candidate(zhuyin_instance_t * instance, guint * num);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_n_candidate(instance: *mut ZhuyinInstance, num: *mut GUint) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
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
/// bool zhuyin_get_candidate(zhuyin_instance_t * instance,
///                           guint index, lookup_candidate_t ** candidate);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_candidate(
    instance: *mut ZhuyinInstance,
    index: GUint,
    candidate: *mut *mut LookupCandidate,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
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
/// bool zhuyin_get_candidate_type(zhuyin_instance_t * instance,
///                                lookup_candidate_t * candidate,
///                                lookup_candidate_type_t * type);
/// ```
///
/// Reads the zhuyin 4-value enum. This is one of the four symbols that must
/// NOT reuse the pinyin 8-value enum: the zhuyin discriminants differ at 3
/// and 4.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_candidate_type(
    instance: *mut ZhuyinInstance,
    candidate: *mut LookupCandidate,
    candidate_type: *mut lookup_candidate_type_t,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `candidate` is non-null and was produced by
        // `zhuyin_get_candidate`.
        let cand = unsafe { candidate_ref(candidate) };
        let ctype = cand.candidate_type;
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
/// bool zhuyin_get_candidate_string(zhuyin_instance_t * instance,
///                                  lookup_candidate_t * candidate,
///                                  const gchar ** utf8_str);
/// ```
///
/// Out-param `utf8_str` is instance-borrowed (never freed by caller).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_candidate_string(
    instance: *mut ZhuyinInstance,
    candidate: *mut LookupCandidate,
    utf8_str: *mut *const GChar,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `candidate` is non-null and was produced by
        // `zhuyin_get_candidate`.
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

/// Choose a candidate at an offset, returning the new cursor position.
///
/// # C signature
/// ```c
/// int zhuyin_choose_candidate(zhuyin_instance_t * instance,
///                             size_t offset, lookup_candidate_t * candidate);
/// ```
///
/// Uses the zhuyin 4-value enum: `BEST_MATCH_CANDIDATE` answers the whole
/// parse end; the normal rows record their span.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_choose_candidate(
    instance: *mut ZhuyinInstance,
    _offset: usize,
    candidate: *mut LookupCandidate,
) -> c_int {
    if instance.is_null() || candidate.is_null() {
        return -1;
    }
    ffi_catch(-1, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        let Some(index) = inst
            .candidates
            .iter()
            .position(|c| std::ptr::eq(c, candidate.cast::<CapiCandidate>()))
        else {
            return -1;
        };
        let source_index = inst.candidates[index].source_index;
        let selection = match inst.core.anchored_window.as_ref() {
            Some((anchor, window)) => {
                inst.core
                    .session
                    .select_anchored(source_index, window, *anchor)
            }
            None => inst.core.session.select(source_index),
        };
        if selection.is_err() {
            return -1;
        }
        inst.core.anchored_window = None;
        // The BEST_MATCH row answers the parse end; the normal rows answer
        // their own span's end mapped back to original coordinates.
        let end = if inst.candidates[index].candidate_type
            == lookup_candidate_type_t::BEST_MATCH_CANDIDATE
        {
            inst.core.parsed_len
        } else if let Some(parse) = inst.core.zhuyin_parse.as_ref() {
            oxpinyin_facade::zhuyin_original_offset(parse, inst.core.session.composition_offset())
        } else {
            inst.core.session.composition_offset()
        };
        end as c_int
    })
}

/// Clear the constraint a prior choose pinned, by offset.
///
/// # C signature
/// ```c
/// bool zhuyin_clear_constraint(zhuyin_instance_t * instance,
///                              size_t offset);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_clear_constraint(instance: *mut ZhuyinInstance, offset: usize) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        let session_offset = if let Some(parse) = inst.core.zhuyin_parse.as_ref() {
            oxpinyin_facade::zhuyin_session_offset(parse, offset)
        } else {
            offset
        };
        inst.core.session.clear_constraint(session_offset)
    })
}

/// Train the current sentence.
///
/// # C signature
/// ```c
/// bool zhuyin_train(zhuyin_instance_t * instance);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_train(instance: *mut ZhuyinInstance) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        inst.core.train()
    })
}

/// Fill the instance's candidate snapshot from a `CandidateList`.
///
/// `before_cursor` selects the zhuyin enum tag for the normal rows. When
/// `before_end` is `Some(end)`, only candidates whose consumed span ENDS at
/// `end` (in original input coordinates) are kept — the before-cursor search
/// law. `None` keeps every candidate (the after-cursor window).
pub(crate) fn snapshot_candidates(
    inst: &mut CapiInstance,
    window: &oxpinyin_engine::CandidateList,
    before_cursor: bool,
    before_end: Option<usize>,
) {
    let normal_type = if before_cursor {
        lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR
    } else {
        lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR
    };
    let zhuyin_parse = inst.core.zhuyin_parse.clone();
    for (window_index, cand) in window.iter().enumerate() {
        if cand.kind() == CandidateKind::Sentence && !before_cursor {
            // BEST_MATCH row stays at the head.
        }
        if cand.kind() == CandidateKind::Fallback {
            continue;
        }
        let text = match std::ffi::CString::new(cand.text().as_bytes()) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let consumed_bytes = if let Some(parse) = zhuyin_parse.as_ref() {
            oxpinyin_facade::zhuyin_original_offset(parse, cand.consumed_bytes())
        } else {
            cand.consumed_bytes()
        };
        // Before-cursor law: only candidates whose span ENDS at the requested
        // original-offset. At offset 0 no span ends there (nothing precedes
        // the first key), so the before-cursor window is empty — not the whole
        // composition — matching the pin's `guess_candidates_before_cursor`.
        // The sentence rows are exempt: upstream prepends them regardless of
        // the offset (`_prepend_sentence_candidates` has no offset condition,
        // `zhuyin.cpp:1624-1626` at the pin), and a sentence's consumed span
        // is the whole composition, not a lookup span.
        if let Some(end) = before_end
            && cand.kind() != CandidateKind::Sentence
            && consumed_bytes != end
        {
            continue;
        }
        inst.candidates.push(CapiCandidate {
            text,
            kind: cand.kind(),
            candidate_type: match cand.kind() {
                CandidateKind::Sentence => lookup_candidate_type_t::BEST_MATCH_CANDIDATE,
                _ => normal_type,
            },
            nbest_index: cand.nbest_index(),
            consumed_bytes,
            token: cand.token(),
            source_index: window_index,
        });
    }
}
