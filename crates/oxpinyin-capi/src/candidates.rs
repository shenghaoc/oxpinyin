//! Candidate access, selection, and training.

use std::os::raw::c_int;

use oxpinyin_core::PhraseToken;
use oxpinyin_engine::CandidateKind;
use oxpinyin_user::{SENTENCE_START, is_user_token};

use crate::ffi::ffi_catch;
use crate::state::{
    CapiCandidate, CapiInstance, candidate_ptr, candidate_ref, instance_mut, instance_ref,
};
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
                *num = GUint::try_from(inst.candidates.len()).unwrap_or(GUint::MAX);
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
/// W14: the stored tail rank of an `NBEST_MATCH_CANDIDATE` row (upstream
/// asserts the type and returns `m_nbest_index`, `pinyin.cpp:2878-2884`);
/// `0` for every non-row candidate.
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
/// The §3.2 nibble test: the candidate's token lives in the
/// [`USER_DICTIONARY`] sub-index. Network (nibble 6) is not "user" —
/// `if (USER_DICTIONARY != index) return false` (`pinyin.cpp:3718`).
/// Sentence-level and fallback candidates carry no token and are not
/// user candidates.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_is_user_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `candidate` is non-null and was produced by
        // `pinyin_get_candidate`.
        let cand = unsafe { candidate_ref(candidate) };
        cand.token.is_some_and(|token| is_user_token(token.value()))
    })
}

/// Remove a user candidate from the dictionary.
///
/// # C signature
/// ```c
/// bool pinyin_remove_user_candidate(pinyin_instance_t * instance,
///                                   lookup_candidate_t * candidate);
/// ```
///
/// The §3.4 removal: the candidate's token must live in the user
/// dictionary (upstream asserts it; oxpinyin reports `false` instead of
/// panicking), then the phrase, its pronunciations, its bigram rows and
/// its unigram delta are deleted. Does **not** arm `m_modified`, matching
/// upstream's set-sites. Note: on the current ABI no user token ever
/// surfaces in a candidate list (candidate collection reads the system
/// dictionary only), so this resolves `false` through the ABI until user
/// phrases join the candidate surface.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_remove_user_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        // Identify the candidate by pointer equality over the snapshot.
        let Some(index) = inst
            .candidates
            .iter()
            .position(|c| std::ptr::eq(c, candidate.cast::<CapiCandidate>()))
        else {
            return false;
        };
        let Some(token) = inst.candidates[index].token else {
            return false;
        };
        if !is_user_token(token.value()) {
            return false;
        }
        let Some(user) = inst.user.as_mut() else {
            return false;
        };
        user.remove_user_phrase(token.value()).unwrap_or(false)
    })
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
/// Returns -1 on failure (consistent with the `int` return type). The
/// returned cursor is the chosen candidate's absolute end position in the
/// active parse mode's own coordinates — never past the parsed input, even
/// when the caller offset sits one position past a separator run the
/// candidate's span also covers (the ibus idiom commits exactly at
/// `cursor == length`).
///
/// An `NBEST_MATCH_CANDIDATE` row answers the whole parse end instead
/// (`pinyin.cpp:2513-2519` returns `matrix.size() - 1` unconditionally):
/// the reserved tail slot one past the last real matrix column —
/// `fill_matrix` sizes the matrix to `parsed_len + 1` and no split/fuzzy
/// step resizes it, so that is `m_parsed_len`, carried here as
/// [`CapiInstance::parsed_len`] in the active parse mode's own
/// coordinates — whatever span the row's own path covered. The lookup at
/// that cursor starts no span, so the next `pinyin_guess_candidates`
/// there offers no word candidates and the frontend re-runs
/// `pinyin_guess_sentence` under the forcings `diff_result` wrote
/// (`docs/findings/upstream-divergences.md`, the closed row-choose-cursor
/// entry).
///
/// Resolves the candidate by pointer identity over the instance's snapshot
/// and calls `Session::select`, which records the constraint — the selected
/// token joins the session's sentence record. Per §2.2 the *bigram* training
/// of a normal selection is deferred to [`pinyin_train`]; a `NORMAL_CANDIDATE`
/// writes nothing to the user store. A chosen `ADDON_CANDIDATE` is promoted
/// into the default facade's nibble-5 (`ADDON_DICTIONARY`, `addon.bin`)
/// sub-index first — #105 (`pinyin.cpp:2532-2561`,
/// `docs/findings/addon-choose-promotion.md`): the addon phrase item is copied
/// across facades, the snapshot candidate becomes a `NORMAL_CANDIDATE` at the
/// freshly allocated nibble-5 token, and it is that token the constraint
/// records. The §2.2 special-candidate unigram training (`LONGER_CANDIDATE`,
/// `SORT_WITHOUT_SENTENCE_CANDIDATE`) has no reachable call site.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_choose_candidate(
    instance: *mut PinyinInstance,
    _offset: usize,
    candidate: *mut LookupCandidate,
) -> c_int {
    if instance.is_null() || candidate.is_null() {
        return -1;
    }
    ffi_catch(-1, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        // Identify the candidate by pointer equality over the current
        // snapshot. `offset_from` would be UB unless `candidate` points
        // into `inst.candidates`, which cannot be assumed across C calls.
        let Some(index) = inst
            .candidates
            .iter()
            .position(|c| std::ptr::eq(c, candidate.cast::<CapiCandidate>()))
        else {
            return -1;
        };
        // `index` is the candidate's position in the SNAPSHOT, which
        // `try_promote_addon` reads (it indexes `inst.candidates`); the
        // snapshot may omit entries (sentence rows under
        // `SORT_WITHOUT_SENTENCE_CANDIDATE`, a `CString` conversion
        // failure), so that position is NOT the row's position in the
        // window the `select*` calls index. Select by the candidate's
        // recorded source index.
        let addon_token = try_promote_addon(inst, index);
        let source_index = inst.candidates[index].source_index;
        // Resolve the selection against the window the caller actually saw:
        // when the last `pinyin_guess_candidates` re-anchored at an offset
        // other than the composition's own, that window is held in
        // `anchored_window`, and an index into the composition-anchored
        // cached list would select a different row.
        let selection = match addon_token {
            Some(promoted) => match inst.anchored_window.as_ref() {
                Some((anchor, window)) => {
                    inst.session
                        .select_anchored_promoted(source_index, window, *anchor, promoted)
                }
                None => inst.session.select_promoted(source_index, promoted),
            },
            None => match inst.anchored_window.as_ref() {
                Some((anchor, window)) => {
                    inst.session.select_anchored(source_index, window, *anchor)
                }
                None => inst.session.select(source_index),
            },
        };
        if selection.is_err() {
            return -1;
        }
        // The selection refreshed the cached list at the new composition
        // offset, so a subsequent index is against that refreshed list.
        inst.anchored_window = None;
        // The candidate's absolute end. The snapshot span is anchored at
        // the session's composition offset and includes any separator run
        // it crossed, while the caller offset may already sit past that
        // run (the begin of the next key rest) — adding them would count
        // the run twice and answer parsed length + 1, derailing the ibus
        // commit branch. Upstream never overshoots because its candidates
        // are anchored at the caller offset (`m_begin = start`,
        // libpinyin@412f88e3); the post-select composition offset is that
        // same end, mapped back to the transformed seams' original
        // coordinates through the parse's key spans. The sentence-row
        // branch above answers the parse end before this mapping — a
        // whole-composition hypothesis consumes the composition.
        let end = if inst.candidates[index].candidate_type
            == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE
        {
            inst.parsed_len
        } else if let Some(parse) = inst.zhuyin_parse.as_ref() {
            crate::sentence::zhuyin_original_offset(parse, inst.session.composition_offset())
        } else if let Some(parse) = inst.double_parse.as_ref() {
            crate::sentence::double_original_offset(parse, inst.session.composition_offset())
        } else if let Some(parse) = inst.full_parse.as_ref() {
            crate::sentence::full_original_offset(parse, inst.session.composition_offset())
        } else {
            inst.session.composition_offset()
        };
        c_int::try_from(end).unwrap_or(c_int::MAX)
    })
}

/// Clear the constraint a prior choose pinned, by offset.
///
/// # C signature
/// ```c
/// bool pinyin_clear_constraint(pinyin_instance_t * instance,
///                              size_t offset);
/// ```
///
/// `pinyin_clear_constraint` (`pinyin.cpp:2641-2647`, decl
/// `pinyin.h:585-593`): un-forces the chosen run `offset` lands in — a hit
/// anywhere inside a run (its `NoSearch` interior included) clears the
/// whole run — and the selection record follows the surviving forcings.
/// The caller offset lives in the active parse mode's original input
/// coordinates (the same space `pinyin_guess_candidates` takes); the
/// store lives in the session's raw buffer, so the transformed seams map
/// it across first. Plain full pinyin is the identity.
///
/// Returns `false` for a free cell or an out-of-range offset — upstream's
/// own defined return, never an abort — and for a null instance.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_clear_constraint(instance: *mut PinyinInstance, offset: usize) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        let session_offset = if let Some(parse) = inst.zhuyin_parse.as_ref() {
            crate::sentence::zhuyin_session_offset(parse, offset)
        } else if let Some(parse) = inst.double_parse.as_ref() {
            crate::sentence::double_session_offset(parse, offset)
        } else if let Some(parse) = inst.full_parse.as_ref() {
            crate::sentence::full_session_offset(parse, offset)
        } else {
            offset
        };
        inst.session.clear_constraint(session_offset)
    })
}

/// Promotes the chosen candidate when it is an `ADDON_CANDIDATE`
/// (`pinyin.cpp:2532-2561`): copies the addon phrase item into the default
/// facade's nibble-5 sub-index, rewrites the snapshot candidate to a
/// `NORMAL_CANDIDATE` at the promoted token, and returns that token so the
/// constraint records it in place of the addon-facade token.
///
/// `None` — leaving a plain selection — when the candidate is not an addon
/// candidate, the addon item cannot be resolved, there is no user store to
/// promote into, or the store write fails.
fn try_promote_addon(inst: &mut CapiInstance, index: usize) -> Option<PhraseToken> {
    if inst.candidates[index].kind != CandidateKind::Addon {
        return None;
    }
    let addon_token = inst.candidates[index].token?;
    let item = inst.dict.addon_phrase_item(addon_token.value())?;
    let promoted = inst
        .user
        .as_mut()?
        .promote_addon_phrase(&item.text, &item.readings, item.unigram)
        .ok()?;
    let promoted = PhraseToken::new(promoted);
    let snapshot = &mut inst.candidates[index];
    snapshot.candidate_type = lookup_candidate_type_t::NORMAL_CANDIDATE;
    snapshot.kind = CandidateKind::Phrase;
    snapshot.token = Some(promoted);
    Some(promoted)
}

/// Choose a predicted candidate.
///
/// # C signature
/// ```c
/// bool pinyin_choose_predicted_candidate(pinyin_instance_t * instance,
///                                        lookup_candidate_t * candidate);
/// ```
///
/// The §2.3 flat path: raises the candidate token's unigram by
/// `69 * 7 = 483` and the user bigram `(last → token)` — and `last`'s total —
/// by a flat `69`, never the reselection doubling of [`pinyin_train`]. `last`
/// is the most recent selected token, or `sentence_start` when nothing was
/// selected yet (upstream's `_get_previous_token` default).
///
/// Returns `false` for a candidate the snapshot does not hold, a candidate
/// without a token, an instance without a user store, or a store failure.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_choose_predicted_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        // Identify the candidate by pointer equality over the snapshot.
        let Some(index) = inst
            .candidates
            .iter()
            .position(|c| std::ptr::eq(c, candidate.cast::<CapiCandidate>()))
        else {
            return false;
        };
        let Some(token) = inst.candidates[index].token else {
            return false;
        };
        let Some(user) = inst.user.as_mut() else {
            return false;
        };
        let last = inst
            .session
            .selected_tokens()
            .last()
            .map_or(SENTENCE_START, |token| token.value());
        user.observe_predicted(last, token.value()).is_ok()
    })
}

/// Train the current sentence with the given n-best index.
///
/// # C signature
/// ```c
/// bool pinyin_train(pinyin_instance_t * instance, guint8 index);
/// ```
///
/// The §2.1 path: walks the sentence recorded by [`pinyin_choose_candidate`]
/// (the phrases the user pinned) and applies the seed arithmetic to the user
/// bigram — first selection `69`, reselections
/// `min(max(prev_freq, 69) × 2, 22080)` — plus `seed × 7` to each token's
/// unigram. The `index` n-best parameter is accepted but unused: the C ABI
/// has no n-best sentence results yet.
///
/// Returns `false` when there is no user store (upstream refuses without a
/// user dir, `pinyin.cpp:2669`), when no candidate has been chosen (upstream
/// refuses without a sentence result, `pinyin.cpp:2674`), or on a store
/// failure.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_train(instance: *mut PinyinInstance, _index: u8) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        let Some(user) = inst.user.as_mut() else {
            return false;
        };
        if inst.session.selected_tokens().is_empty() {
            return false;
        }
        inst.session.train(user).is_ok()
    })
}
