//! Candidate access, selection, and training.
//!
//! The four symbols that read or write `lookup_candidate_type_t` use the
//! zhuyin-local 4-value enum — `zhuyin_guess_candidates_after_cursor`,
//! `zhuyin_guess_candidates_before_cursor`, `zhuyin_choose_candidate`,
//! `zhuyin_get_candidate_type`. The zhuyin enum's discriminants collide with
//! the pinyin eight at 3 and 4 (see [`crate::types::lookup_candidate_type_t`]),
//! so tagging must use the zhuyin enum, never the pinyin one.

use oxpinyin_capi_marshal::phrase_text::{phrase_text_bytes, phrase_text_len};
use std::os::raw::c_int;

use oxpinyin_engine::CandidateKind;

use crate::state::{CapiCandidate, candidate_ptr, candidate_ref, instance_mut, instance_ref};
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

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    if !num.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *num = GUint::try_from(inst.state.candidates.len()).unwrap_or(GUint::MAX);
        }
    }
    true
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

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    let idx = index as usize;
    if let Some(cand) = inst.state.candidates.get(idx) {
        if !candidate.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *candidate = candidate_ptr(cand);
            }
        }
        true
    } else {
        if !candidate.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *candidate = std::ptr::null_mut();
            }
        }
        false
    }
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
///
/// The pin's body never reads the instance (`zhuyin.cpp:1858-1863`): a NULL
/// instance is a defined call there, so it is unused here. A NULL `candidate`
/// is read unconditionally by the pin, so it is register row 41's class (b)
/// site and the guard answers `false`.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_candidate_type(
    _instance: *mut ZhuyinInstance,
    candidate: *mut LookupCandidate,
    candidate_type: *mut lookup_candidate_type_t,
) -> bool {
    if candidate.is_null() {
        return false;
    }

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
///
/// The pin's body never reads the instance (`zhuyin.cpp:1865-1870`): a NULL
/// instance is a defined call there, so it is unused here. A NULL `candidate`
/// is read unconditionally by the pin, so it is register row 41's class (b)
/// site and the guard answers `false`.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_candidate_string(
    _instance: *mut ZhuyinInstance,
    candidate: *mut LookupCandidate,
    utf8_str: *mut *const GChar,
) -> bool {
    if candidate.is_null() {
        return false;
    }

    // SAFETY: `candidate` is non-null and was produced by
    // `zhuyin_get_candidate`.
    let cand = unsafe { candidate_ref(candidate) };
    if !utf8_str.is_null() {
        // SAFETY: Null-checked above. Pointer borrows into the instance's
        // packed candidate text, valid until candidates are rebuilt.
        unsafe {
            *utf8_str = cand.text_ptr;
        }
    }
    true
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

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    let Some(index) = inst
        .candidates
        .iter()
        .position(|c| std::ptr::eq(c, candidate.cast::<CapiCandidate>()))
    else {
        return -1;
    };
    // 074a2219 zhuyin.cpp:1643-1644: every BEST_MATCH candidate returns
    // the matrix tail without touching constraints or the selection
    // record. Unlike pinyin.cpp:2515-2520, this facade does not diff the
    // chosen row against result 0; zhuyin_train always walks result 0
    // (zhuyin.cpp:1709-1713).
    if inst.state.candidates[index].candidate_type == lookup_candidate_type_t::BEST_MATCH_CANDIDATE
    {
        return c_int::try_from(inst.core.parsed_len).unwrap_or(c_int::MAX);
    }
    let row = &inst.state.candidates[index];
    let text = inst.state.candidates.phrase_text(index, row.text);
    // Only a BEST_MATCH row is a sentence row, and it answered above.
    let published = oxpinyin_facade::PublishedRow {
        span: row.selection,
        token: row.token,
        nbest_row: None,
        text: &text,
        source_index: row.source_index as usize,
    };
    let window_end = match inst.core.select_published(&published, None) {
        Ok((oxpinyin_engine::Selection::Refused, _)) => {
            let chosen = &inst.state.candidates[index];
            let offset = if chosen.candidate_type
                == lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR
            {
                chosen.span_begin
            } else {
                chosen.span_end
            };
            return c_int::try_from(offset).unwrap_or(c_int::MAX);
        }
        Ok((
            oxpinyin_engine::Selection::Continued | oxpinyin_engine::Selection::Completed,
            window_end,
        )) => window_end,
        Err(_) => return -1,
    };
    // `zhuyin_choose_candidate`'s return (`zhuyin.cpp:1644-1663` at the
    // pin): the BEST_MATCH row answers the parse end (`matrix.size() - 1`);
    // an after-cursor row answers its span's end (`offset = m_end`); a
    // before-cursor row answers its span's START (`offset = m_begin`) —
    // the constraint is written on `[m_begin, m_end)` either way.
    let chosen = &inst.state.candidates[index];
    let end = match chosen.candidate_type {
        lookup_candidate_type_t::BEST_MATCH_CANDIDATE => inst.core.parsed_len,
        lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR => chosen.span_begin as usize,
        lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR
        | lookup_candidate_type_t::ZOMBIE_CANDIDATE => {
            let end = window_end.unwrap_or_else(|| inst.core.session.composition_offset());
            if let Some(parse) = inst.core.zhuyin_parse.as_ref() {
                oxpinyin_facade::zhuyin_original_offset(parse, end)
            } else {
                end
            }
        }
    };
    c_int::try_from(end).unwrap_or(c_int::MAX)
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

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    inst.core.clear_abi_constraint(offset, false, false)
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

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    train_answer(unsafe { crate::state::train_instance(instance, 0) })
}

/// `zhuyin_train`'s answer for its training result: the process aborts where the
/// pin dies, and a class (b) refusal is `false` with one warning.
fn train_answer(result: Result<bool, oxpinyin_engine::EngineError>) -> bool {
    use oxpinyin_facade::TrainingRefusal;
    match oxpinyin_facade::training_answer(result) {
        Ok(trained) => trained,
        Err(TrainingRefusal::Abort) => std::process::abort(),
        Err(TrainingRefusal::PartialBigram) => {
            crate::ffi::log_warning(
                "zhuyin_train: a user bigram value has a partial item tail \
                 (upstream reads past it, ngram.cpp:190)",
            );
            false
        }
    }
}

/// Fill the instance's candidate snapshot from a `CandidateList`.
///
/// `before_cursor` selects the zhuyin enum tag for the normal rows. When
/// `before_end` is `Some(end)`, only candidates whose consumed span ENDS at
/// `end` (in original input coordinates) are kept — the before-cursor search
/// law. `None` keeps every candidate (the after-cursor window). `anchor` is
/// the session-coordinate origin the window's rows are measured from (the
/// lookup offset, the composition offset, or
/// [`oxpinyin_facade::BEFORE_CURSOR_ANCHOR`]), so each row's absolute span
/// start is `anchor + span_start()`. `sentence_rows_only` keeps the
/// sentence rows and nothing else — the after-cursor window at a mid-key
/// offset, where the pin's matrix column is empty (issue #577).
pub fn snapshot_candidates(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    window: &oxpinyin_engine::CandidateList,
    before_cursor: bool,
    before_end: Option<usize>,
    anchor: usize,
    sentence_rows_only: bool,
) {
    let normal_type = if before_cursor {
        lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR
    } else {
        lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR
    };
    let offsets = inst.core.zhuyin_parse.as_ref().map(|parse| {
        let mut transformed = 0;
        let ends: Vec<_> = parse
            .keys()
            .iter()
            .map(|key| {
                let boundary = transformed + key.key().text().len();
                transformed = boundary + 1;
                (boundary, key.end())
            })
            .collect();
        (ends, parse.consumed())
    });
    let original_end = |offset| {
        offsets.as_ref().map_or(offset, |(ends, consumed)| {
            let index = ends.partition_point(|&(boundary, _)| boundary < offset);
            ends.get(index).map_or(*consumed, |&(_, end)| end)
        })
    };
    // One row vector and one text buffer, each sized once for the window.
    // The size is the texts' encoded length, so each text is encoded once,
    // by the loop.
    inst.state.candidates.reserve(
        window.len(),
        window
            .iter()
            .map(|cand| phrase_text_len(cand.text()) + 1)
            .sum(),
    );
    for (window_index, cand) in window.iter().enumerate() {
        if cand.kind() == CandidateKind::Sentence && !before_cursor {
            // BEST_MATCH row stays at the head.
        }
        if cand.kind() == CandidateKind::Fallback {
            continue;
        }
        if sentence_rows_only && cand.kind() != CandidateKind::Sentence {
            continue;
        }
        let consumed_bytes = original_end(cand.consumed_bytes());
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
        let span_begin_session = anchor.saturating_add(cand.span_start());
        let span_begin = if offsets.is_some() {
            span_begin_session.checked_sub(1).map_or(0, original_end)
        } else {
            span_begin_session
        };
        let span_end = original_end(anchor.saturating_add(cand.consumed_bytes()));
        let bytes = phrase_text_bytes(cand.text());
        let Some(text) = inst.state.candidates.intern(&bytes) else {
            continue;
        };
        if cand.text().outward_bytes().is_none() {
            inst.state.candidates.keep_stored_text(cand.text().clone());
        }
        inst.state.candidates.push(CapiCandidate {
            text,
            text_ptr: std::ptr::null(),
            candidate_type: match cand.kind() {
                CandidateKind::Sentence => lookup_candidate_type_t::BEST_MATCH_CANDIDATE,
                CandidateKind::Phrase | CandidateKind::Addon | CandidateKind::Fallback => {
                    normal_type
                }
            },
            source_index: u32::try_from(window_index).unwrap_or(u32::MAX),
            span_begin: u32::try_from(span_begin).unwrap_or(u32::MAX),
            span_end: u32::try_from(span_end).unwrap_or(u32::MAX),
            token: cand.token(),
            selection: oxpinyin_facade::SelectionSpan::of_candidate(cand, anchor),
        });
    }
}

#[cfg(test)]
mod tests {
    /// `phonetic_lookup.h:904`: training stores into the failed Berkeley DB
    /// user bigram container and the pin's `check_result` dies. Only
    /// running out of memory makes that container, so the training answer
    /// is handed to the refusal directly.
    #[test]
    fn a_store_into_the_failed_user_bigram_aborts_training() {
        let stderr = crate::test_support::assert_aborts(
            "candidates::tests::a_store_into_the_failed_user_bigram_aborts_training",
            || {
                super::train_answer(Err(oxpinyin_engine::EngineError::FailedUserBigramStore {
                    token: 0x0100_0001,
                }));
            },
        );
        // No Rust panic text or other line: the pin writes none here but
        // glibc's assert line, the register's policy-accepted missing line.
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
    }
}
