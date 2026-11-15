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
/// The pin reads the candidate's type, token and bounds and never checks
/// which instance published it (`zhuyin.cpp:1635-1666`). The context,
/// matrix and constraints are the receiving `instance`'s. So does this:
/// the row's scalars are copied out of the handle and everything else is
/// this instance's.
///
/// - `BEST_MATCH_CANDIDATE` answers this instance's `matrix.size() - 1`
///   and leaves the constraints alone (`:1643-1644`). Unlike
///   `pinyin.cpp:2515-2520`, this facade does not diff the chosen row
///   against result 0; `zhuyin_train` always walks result 0
///   (`:1709-1713`).
/// - `NORMAL_CANDIDATE_AFTER_CURSOR` and `NORMAL_CANDIDATE_BEFORE_CURSOR`
///   validate, add the row's token over its bounds against this
///   instance's parse, validate again, and answer the row's own `m_end`
///   or `m_begin` (`:1646-1666`), whether `add_constraint` wrote the span
///   or refused it. The caller's offset takes no part.
/// - Any other row validates twice and answers the caller's offset.
///
/// Returns -1 for a NULL argument or an engine failure. The handle must be
/// a row of a live instance's published candidate list, valid for the
/// whole call and not used elsewhere during it. Any other pointer is a
/// caller precondition violation, as for the candidate getters.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_choose_candidate(
    instance: *mut ZhuyinInstance,
    offset: usize,
    candidate: *mut LookupCandidate,
) -> c_int {
    if instance.is_null() || candidate.is_null() {
        return -1;
    }

    // The scalars are copied, and the view ends before any view of
    // `instance`.
    let (candidate_type, token, begin, end) = {
        // SAFETY: `candidate` is non-null and is a row of a live published
        // list (the export's precondition).
        let row = unsafe { candidate_ref(candidate) };
        (row.candidate_type, row.token, row.span_begin, row.span_end)
    };
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    if candidate_type == lookup_candidate_type_t::BEST_MATCH_CANDIDATE {
        return c_int::try_from(inst.core.parsed_len).unwrap_or(c_int::MAX);
    }
    let answer = match candidate_type {
        lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR => end,
        lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR => begin,
        _ => {
            return match inst.core.session.refuse_selection() {
                Ok(_) => offset as u32 as c_int,
                Err(_) => -1,
            };
        }
    };
    let outcome = match token {
        Some(token) => inst.core.select_published(
            &oxpinyin_facade::PublishedRow {
                begin,
                end,
                token,
                text: oxpinyin_engine::RowText::Resolve,
            },
            false,
            false,
        ),
        None => inst.core.session.refuse_selection(),
    };
    match outcome {
        Ok(_) => c_int::from(answer),
        Err(_) => -1,
    }
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
    let unreadable = window.sole_unreadable_candidate().filter(|_| {
        !sentence_rows_only
            && window
                .iter()
                .all(|row| row.kind() == oxpinyin_engine::CandidateKind::Fallback)
            && !crate::sentence::facade_total_is_zero(&inst.read())
    });
    for (cand, missing) in window
        .iter()
        .map(|row| (row, false))
        .chain(unreadable.iter().map(|row| (row, true)))
    {
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
        let text = if missing {
            oxpinyin_facade::TextSpan::NULL
        } else {
            let bytes = phrase_text_bytes(cand.text());
            let Some(text) = inst.state.candidates.intern(&bytes) else {
                continue;
            };
            text
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
            // The pin's `guint16` fields (`zhuyin.cpp:83-84`).
            span_begin: span_begin as u16,
            span_end: span_end as u16,
            token: cand.token(),
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
