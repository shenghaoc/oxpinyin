//! Candidate access, selection, and training.

use std::cell::Cell;
use std::os::raw::c_int;

use oxpinyin::PhraseToken;
use oxpinyin::engine::SORT_WITHOUT_SENTENCE_CANDIDATE;
use oxpinyin::marshal::phrase_text::{phrase_text_bytes, phrase_text_len};
use oxpinyin::user::{UnigramAdd, is_user_token};

use crate::ffi::{borrow_or_return, write_if_nonnull};
use crate::state::{CapiCandidate, candidate_ptr, candidate_ref, instance_mut, instance_ref};
use crate::types::{GChar, GUint, LookupCandidate, PinyinInstance, lookup_candidate_type_t};

/// Get the number of candidates.
///
/// # C signature
/// ```c
/// bool pinyin_get_n_candidate(pinyin_instance_t * instance, guint * num);
/// ```
///
/// A null `instance` returns `false` without writing. Otherwise, the
/// function returns `true`. A null `num` skips the write. A non-null `num`
/// receives the candidate count.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `num` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_n_candidate(
    instance: *mut PinyinInstance,
    num: *mut GUint,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    write_if_nonnull!(
        num,
        GUint::try_from(inst.state.candidates.len()).unwrap_or(GUint::MAX)
    );
    true
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
/// A null `instance` returns `false` without writing. Otherwise, the
/// function returns `true` if the row exists and `false` if it is absent.
/// If `candidate` is non-null, the function writes a borrowed pointer to
/// the row, or null if the row is absent (`pinyin.cpp:2851-2863`).
/// The caller does not free the pointer. See the
/// [crate-level rule for borrowed objects](crate#borrowed-objects).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `candidate` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_candidate(
    instance: *mut PinyinInstance,
    index: GUint,
    candidate: *mut *mut LookupCandidate,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    let idx = index as usize;
    if let Some(cand) = inst.state.candidates.get(idx) {
        write_if_nonnull!(candidate, candidate_ptr(cand));
        true
    } else {
        write_if_nonnull!(candidate, std::ptr::null_mut());
        false
    }
}

/// Get the type of a lookup candidate.
///
/// # C signature
/// ```c
/// bool pinyin_get_candidate_type(pinyin_instance_t * instance,
///                                lookup_candidate_t * candidate,
///                                lookup_candidate_type_t * type);
/// ```
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `candidate` is a borrowed object and
/// `candidate_type` is an out-parameter. `_instance` is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_candidate_type(
    _instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    candidate_type: *mut lookup_candidate_type_t,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2866-2871`).
    let cand = borrow_or_return!(candidate, candidate_ref, false);
    let ctype = cand.candidate_type();
    write_if_nonnull!(candidate_type, ctype);
    true
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
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `candidate` is a borrowed object and `utf8_str`
/// is an out-parameter. `_instance` is not read. The string written to `*utf8_str` belongs to
/// the row's candidate list and lives as long as the row; the caller does not free it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_candidate_string(
    _instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    utf8_str: *mut *const GChar,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2873-2878`).
    let cand = borrow_or_return!(candidate, candidate_ref, false);
    write_if_nonnull!(utf8_str, cand.text_ptr);
    true
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
/// Returns `false` for a null `candidate`. For an `NBEST_MATCH_CANDIDATE`
/// row, returns `true` and writes its stored tail rank to a non-null
/// `index` (`m_nbest_index`, `pinyin.cpp:2880-2886`).
///
/// # Aborts
///
/// The process aborts if `candidate` points to a row of any other type,
/// even if `index` is null (`pinyin.cpp:2883`, register row 64).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `candidate` is a borrowed object and `index` is
/// an out-parameter. `_instance` is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_candidate_nbest_index(
    _instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    index: *mut u8,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2880-2886`).
    let cand = borrow_or_return!(candidate, candidate_ref, false);
    // Register row 64, `pinyin.cpp:2883`:
    // `assert(NBEST_MATCH_CANDIDATE == type)`, before `*index` is written.
    if cand.candidate_type() != lookup_candidate_type_t::NBEST_MATCH_CANDIDATE {
        std::process::abort();
    }
    write_if_nonnull!(index, cand.nbest_index);
    true
}

/// Check whether a candidate is a user candidate.
///
/// # C signature
/// ```c
/// bool pinyin_is_user_candidate(pinyin_instance_t * instance,
///                               lookup_candidate_t * candidate);
/// ```
///
/// The §3.2 type and nibble test: only a normal or longer row can be a
/// user candidate (`NORMAL_CANDIDATE != type && LONGER_CANDIDATE != type`
/// returns false, `pinyin.cpp:3712-3715`), and its token must live in the
/// [`USER_DICTIONARY`](crate::USER_DICTIONARY) sub-index. Network (nibble 6) is not "user" —
/// `if (USER_DICTIONARY != index) return false` (`:3720-3721`). Predicted rows
/// and sentence-level/fallback rows carry no user status even when a
/// predicted prefix token names the user library.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `candidate` is a borrowed object. `_instance`
/// is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_is_user_candidate(
    _instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:3712-3724`).
    let cand = borrow_or_return!(candidate, candidate_ref, false);
    if !matches!(
        cand.candidate_type(),
        lookup_candidate_type_t::NORMAL_CANDIDATE | lookup_candidate_type_t::LONGER_CANDIDATE
    ) {
        return false;
    }
    cand.token.get().is_some_and(is_user_token)
}

// pinyin.cpp:3726-3768.
/// Remove a user candidate from the dictionary.
///
/// # C signature
/// ```c
/// bool pinyin_remove_user_candidate(pinyin_instance_t * instance,
///                                   lookup_candidate_t * candidate);
/// ```
///
/// Remove the row’s item, pronunciations and associated frequency data from the receiving
/// context. The change requires a separately armed save. A null instance or candidate returns
/// false. A true result from `pinyin_is_user_candidate` can also describe a longer row.
///
/// # Aborts
///
/// Removal requires a normal user-dictionary row. A longer row, another row type, or a token
/// outside the user dictionary aborts. Trying to remove the item after it was already removed
/// can also abort.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `candidate` is a
/// borrowed object. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_remove_user_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }

    // The row's scalars, copied before any view of `instance`.
    let (candidate_type, token) = {
        // SAFETY: `candidate` is non-null and is a row of a live published
        // list (the export's precondition).
        let row = unsafe { candidate_ref(candidate) };
        (row.candidate_type(), row.token.get())
    };
    // Register row 64: `assert(NORMAL_CANDIDATE == type)` (`pinyin.cpp:3734`)
    // and `assert(USER_DICTIONARY == index)` (`:3738`).
    if candidate_type != lookup_candidate_type_t::NORMAL_CANDIDATE {
        std::process::abort();
    }
    let Some(token) = token else {
        return false;
    };
    if !is_user_token(token) {
        std::process::abort();
    }
    // SAFETY: the candidate reads are complete and the retained owner is now
    // borrowed exclusively for the store mutation.
    let ctx = unsafe { crate::state::instance_context_mut(instance) };
    let user = ctx.core.user_store_mut();
    match user.remove_user_phrase(token) {
        Ok(true) => true,
        // Class (b), register row 98: the call's `user_bigram->mask_out`
        // (`pinyin.cpp:3766`) would read past a partial item tail
        // (`ngram.cpp:76-86`). One warning, as before.
        Err(oxpinyin::user::UserStoreError::PartialUserBigramTail) => {
            crate::ffi::log_warning(
                "pinyin_remove_user_candidate: malformed user bigram value \
                 (upstream asserts on a missing total or reads past a partial item; \
                 memory_chunk.h:390, ngram.cpp:76-86)",
            );
            false
        }
        // Register row 81: the phrase item, its phrase-table row
        // (`user_phrase_index.bin`) or one of its indexed readings
        // (`user_pinyin_index.bin`) is missing, and the pin aborts on the
        // matching `assert(ERROR_OK == retval)` (`pinyin.cpp:3743`,
        // `:3750`, `:3759`).
        Ok(false) => std::process::abort(),
        // Register rows 85, 86 and 87: the call reached
        // `user_bigram->mask_out` (`pinyin.cpp:3766`) over a key that is not
        // a token (`ngram_bdb.cpp:199`), a gram too short for its total
        // (`memory_chunk.h:390`) or a gram the mask leaves with a residual
        // total (`ngram.cpp:70`), and the pin's assert dies.
        Err(
            oxpinyin::user::UserStoreError::NonTokenUserBigramKey
            | oxpinyin::user::UserStoreError::ShortUserBigramValue
            | oxpinyin::user::UserStoreError::ResidualUserBigramGram
            | oxpinyin::user::UserStoreError::UserReadingRemoval,
        ) => std::process::abort(),
        // `copy_tkrzwdb` / `copy_bdb` assert on a failed destination write
        // or source walk (`tkrzwdb_utils.h:65`, `:72`; `bdb_utils.h:61`,
        // `:67`). This call does not copy, so it never reaches here.
        Err(oxpinyin::user::UserStoreError::FatalCopy) => std::process::abort(),
        // A store failure the pin has no counterpart for.
        Err(
            oxpinyin::user::UserStoreError::Io(_)
            | oxpinyin::user::UserStoreError::Store(_)
            | oxpinyin::user::UserStoreError::Decode
            | oxpinyin::user::UserStoreError::InvalidPhrase
            | oxpinyin::user::UserStoreError::Persistence(_)
            | oxpinyin::user::UserStoreError::UnknownDatabaseFormat
            | oxpinyin::user::UserStoreError::ChunkHeaderWrite(_)
            | oxpinyin::user::UserStoreError::LibraryChunkHeaderRead
            | oxpinyin::user::UserStoreError::SaveRemapFailed
            | oxpinyin::user::UserStoreError::SaveTooLarge
            | oxpinyin::user::UserStoreError::UnigramTotalOverflow
            | oxpinyin::user::UserStoreError::OverlongIndexKey
            | oxpinyin::user::UserStoreError::Journal(_)
            | oxpinyin::user::UserStoreError::LibraryLoop(_)
            | oxpinyin::user::UserStoreError::TrainingSetFreqMiss
            | oxpinyin::user::UserStoreError::SessionSeed(_)
            | oxpinyin::user::UserStoreError::FailedUserBigramStore,
        ) => {
            crate::ffi::log_warning("pinyin_remove_user_candidate: the user store failed");
            false
        }
    }
}

/// The published type of a window row; `is_longer` names the LONGER row.
fn row_type(cand: &oxpinyin::engine::Candidate, is_longer: bool) -> lookup_candidate_type_t {
    match cand.kind() {
        oxpinyin::engine::CandidateKind::Sentence => lookup_candidate_type_t::NBEST_MATCH_CANDIDATE,
        oxpinyin::engine::CandidateKind::Addon => lookup_candidate_type_t::ADDON_CANDIDATE,
        oxpinyin::engine::CandidateKind::Phrase | oxpinyin::engine::CandidateKind::Fallback
            if is_longer =>
        {
            lookup_candidate_type_t::LONGER_CANDIDATE
        }
        oxpinyin::engine::CandidateKind::Phrase | oxpinyin::engine::CandidateKind::Fallback => {
            lookup_candidate_type_t::NORMAL_CANDIDATE
        }
    }
}

/// Fill the instance's candidate snapshot from the window of one guess.
///
/// Each row carries its own span in the caller's original coordinates and
/// its text in the snapshot's buffer, so no row borrows from `window`: the
/// caller returns the window to the session afterwards.
pub(crate) fn snapshot_candidates(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    window: &oxpinyin::engine::CandidateList,
    frame: &crate::sentence::GuessFrame,
) {
    // One row vector and one text buffer, each sized once for the window
    // (rows the loop skips leave a little slack, never a second growth).
    // The size is the texts' encoded length, so each text is encoded once,
    // by the loop.
    inst.state.candidates.reserve(
        window.len(),
        window
            .iter()
            .map(|cand| phrase_text_len(cand.text()) + 1)
            .sum(),
    );
    let published_bound = inst.core.publication_bound_mapper(true);
    for cand in window {
        // The engine's remaining-raw-input `Fallback` row is the
        // session-API affordance (`session-api.md`: it keeps `select`
        // meaningful when no phrase is found) —
        // the pin has no raw-input fallback: an empty matrix answers
        // false (`pinyin.cpp:2193`), an empty result answers true
        // with no rows. The C ABI translates the engine shape, it
        // does not surface it.
        if cand.kind() == oxpinyin::engine::CandidateKind::Fallback {
            continue;
        }
        // §9: a LONGER row is the engine's token-carrying row with the
        // zero span the pin's prepend leaves (`m_begin == m_end == 0`,
        // never set for the type) — a normal phrase row always covers at
        // least one key, so `token && consumed_bytes == 0` names the type
        // uniquely on this surface. The RAW span decides the type, under a
        // transform (double pinyin, the zhuyin keyboards) too.
        // `pinyin_get_candidate_type` answers `LONGER_CANDIDATE` (7) for
        // the row and ibus maps it (`CANDIDATE_LONGER`/
        // `CANDIDATE_LONGER_USER`, `PYPLibPinyinCandidates.cc:56-62`).
        let is_longer = cand.token().is_some() && cand.consumed_bytes() == 0;
        // A lookup strictly inside a key: the pin's column there is empty,
        // so the span loop finds no phrase row, but its two prepends still
        // run (`pinyin.cpp:2292-2296`) — the list is the sentence rows with
        // the LONGER row beneath them (row 65). Keep exactly those kinds.
        if frame.sentence_rows_only
            && cand.kind() != oxpinyin::engine::CandidateKind::Sentence
            && !is_longer
        {
            continue;
        }
        let bytes = phrase_text_bytes(cand.text());
        let Some(text) = inst.state.candidates.intern(&bytes) else {
            continue;
        };
        // The pin's `m_begin` and `m_end`: `m_begin` is the caller's guess
        // offset itself (`start = offset`, `pinyin.cpp:2228`, `:2246`), and
        // `m_end` the span end in the same coordinates, both `guint16`
        // (`:103-104`). A sentence or LONGER row keeps the default zero
        // span (`:114`; `_prepend_sentence_candidates`, `:1936-1950`).
        let (begin, end) = if is_longer || cand.kind() == oxpinyin::engine::CandidateKind::Sentence
        {
            (0, 0)
        } else {
            let selection =
                oxpinyin::facade::SelectionSpan::of_candidate(cand, frame.session_offset);
            (frame.offset as u16, published_bound(selection.end()) as u16)
        };
        if cand.text().outward_bytes().is_none() {
            inst.state.candidates.keep_stored_text(cand.text().clone());
        }
        inst.state.candidates.push(CapiCandidate {
            text,
            text_ptr: std::ptr::null(),
            type_tag: Cell::new(row_type(cand, is_longer).tag()),
            nbest_index: cand.nbest_index(),
            token: Cell::new(cand.token()),
            begin,
            end,
        });
    }
}

/// The normal leg of `pinyin_choose_candidate`: write the row's constraint
/// and answer the caller's offset plus the length written. `token` and
/// `text` are the row's, after any addon promotion.
fn choose_phrase_row(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    offset: usize,
    row: &RowArgs,
    token: Option<PhraseToken>,
    text: oxpinyin::engine::RowText,
) -> c_int {
    let Some(token) = token else {
        return match inst.core.choose_tokenless() {
            Ok(()) => choose_answer(offset, 0),
            Err(_) => -1,
        };
    };
    let published = oxpinyin::facade::PublishedRow {
        begin: row.begin,
        end: row.end,
        token,
        text,
    };
    match inst.core.select_published(&published, true) {
        // `add_constraint` refused the span: `len = 0` (`phonetic_lookup.cpp:64-65`).
        Ok(oxpinyin::engine::Selection::Refused) => choose_answer(offset, 0),
        // The pin answers the caller's choose offset plus the length
        // `add_constraint` wrote, `m_end - m_begin` of the row's own span
        // (`pinyin.cpp:2582-2590`, `phonetic_lookup.cpp:61-86`), whatever
        // offset the caller passes.
        Ok(oxpinyin::engine::Selection::Continued | oxpinyin::engine::Selection::Completed) => {
            choose_answer(
                offset,
                usize::from(row.end).wrapping_sub(usize::from(row.begin)),
            )
        }
        Err(_) => -1,
    }
}

// pinyin.cpp:2501-2588, phonetic_key_matrix.cpp:34-38, pinyin.cpp:2523-2531.
/// Choose a candidate at an offset, returning the new cursor position.
///
/// # C signature
/// ```c
/// int pinyin_choose_candidate(pinyin_instance_t * instance,
///                             size_t offset,
///                             lookup_candidate_t * candidate);
/// ```
///
/// Apply the supplied row to the receiving instance. A null argument or engine failure returns
/// -1. For a normal row, the result is the supplied offset plus the accepted constraint length,
/// or the original offset on refusal. Pass the same offset used to guess the row. An addon row
/// promotes its item and becomes a normal row. The receiving context must contain the addon item
/// and its destination library. An n-best row applies the differences from row zero and returns
/// the last matrix offset. A longer row, or a selection with sentence candidates disabled,
/// trains only the unigram and returns 1 on success or 0 on frequency overflow. These frequency
/// changes require a separately armed save. Normal selection defers bigram training to
/// `pinyin_train`.
///
/// # Aborts
///
/// A predicted bigram or prefix row aborts. Choosing an n-best row aborts if its required result is absent or a
/// differing constraint span cannot be applied. A nonzero offset aborts when the selection
/// reaches the unigram-only path with sentence candidates disabled.
///
/// A phrase write aborts if clearing its span reaches a continuation whose head is gone.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `candidate` is a
/// borrowed object. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_choose_candidate(
    instance: *mut PinyinInstance,
    offset: usize,
    candidate: *mut LookupCandidate,
) -> c_int {
    if instance.is_null() || candidate.is_null() {
        return -1;
    }

    // SAFETY: `candidate` is non-null and is a row of a live published
    // list (the export's precondition). The shared view ends here, before
    // any view of `instance`.
    let row = RowArgs::read(unsafe { candidate_ref(candidate) });
    // Register row 64, `pinyin.cpp:2507`: the pin asserts the row is not a
    // predicted bigram or prefix row (those go to
    // `pinyin_choose_predicted_candidate`).
    if matches!(
        row.candidate_type,
        lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE
            | lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE
    ) {
        std::process::abort();
    }
    if row.candidate_type == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let mut inst = unsafe { instance_mut(instance) };
        // `check_result(results.get_result(0, best))` and
        // `check_result(results.get_result(m_nbest_index, other))`
        // (`pinyin.cpp:2517-2518`): both are `assert`s.
        if !inst.core.has_nbest_rank(row.nbest_index) {
            std::process::abort();
        }
        if inst.core.select_nbest_rank(row.nbest_index).is_err() {
            return -1;
        }
        return c_int::try_from(inst.core.pin_matrix_columns())
            .map_or(c_int::MAX, |columns| columns - 1);
    }
    if row.candidate_type == lookup_candidate_type_t::LONGER_CANDIDATE {
        let Some(token) = row.token else {
            return -1;
        };
        // SAFETY: no instance view is live; this serialized C operation
        // exclusively borrows the retained owner for the unigram write.
        let ctx = unsafe { crate::state::instance_context_mut(instance) };
        return unigram_leg_answer(ctx.core.runtime.add_unigram_outcome(token, 483));
    }
    let mut token = row.token;
    let mut text = oxpinyin::engine::RowText::Resolve;
    if row.candidate_type == lookup_candidate_type_t::ADDON_CANDIDATE {
        // SAFETY: no instance or candidate view is live; the handle meets
        // the export's precondition.
        match unsafe { try_promote_addon(instance, candidate) } {
            Some(promoted) => token = Some(promoted),
            None => text = oxpinyin::engine::RowText::Empty,
        }
    }
    // SAFETY: promotion has released its context borrow; the live instance
    // is borrowed for the remainder of this serialized operation.
    let mut inst = unsafe { instance_mut(instance) };
    // The `SORT_WITHOUT_SENTENCE_CANDIDATE` leg (`pinyin.cpp:2565-2576`),
    // reached after the addon promotion exactly as the pin's dispatch
    // falls through to it.
    if inst.core.session.sort_options() & SORT_WITHOUT_SENTENCE_CANDIDATE != 0 {
        if offset != 0 {
            // Register row 45: the pin's `assert(0 == offset)` (`:2566`),
            // after the addon promotion above, which is in memory.
            std::process::abort();
        }
        // The row's `m_token`, the null token for a predicted punctuation
        // row: library 0 is reserved and never loaded, so the add meets no
        // sub-index and the pin answers `true` (`phrase_index.h:628-635`).
        let token = token.unwrap_or(0);
        // SAFETY: the instance view is no longer used; this serialized C
        // operation exclusively borrows its retained owner for the unigram
        // write.
        let ctx = unsafe { crate::state::instance_context_mut(instance) };
        return unigram_leg_answer(ctx.core.runtime.add_unigram_outcome(token, 483));
    }
    choose_phrase_row(&mut inst, offset, &row, token, text)
}

/// Whether [`pinyin_choose_candidate`] with the same arguments reaches one
/// of the pin's asserts, in the pin's order: a predicted row
/// (`pinyin.cpp:2507`); for an `NBEST_MATCH_CANDIDATE` row, a result this
/// instance does not hold (`:2517-2518`) or a differing phrase whose
/// constraint write answers zero (`phonetic_lookup.cpp:203`), the facade's
/// [`oxpinyin::facade::InstanceView::nbest_choose_aborts`]; a nonzero
/// `offset` under `SORT_WITHOUT_SENTENCE_CANDIDATE` (`pinyin.cpp:2566`), which a
/// `LONGER_CANDIDATE` row does not reach. Reads only. For the in-tree fuzz
/// harness, which skips such a call: not in `pinyin.h`, a Rust function
/// compiled only with `fuzz-api` and never into the shipped artifact.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `candidate` is a
/// borrowed object. The serial-calls rule applies.
#[cfg(feature = "fuzz-api")]
#[must_use]
pub unsafe fn oxpinyin_choose_aborts(
    instance: *mut PinyinInstance,
    offset: usize,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }
    // SAFETY: `candidate` is non-null and is a row of a live published
    // list (the export's precondition). The shared view ends here, before
    // any view of `instance`.
    let row = RowArgs::read(unsafe { candidate_ref(candidate) });
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    match row.candidate_type {
        lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE
        | lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE => true,
        lookup_candidate_type_t::NBEST_MATCH_CANDIDATE => {
            inst.core.nbest_choose_aborts(row.nbest_index)
        }
        lookup_candidate_type_t::LONGER_CANDIDATE => false,
        lookup_candidate_type_t::NORMAL_CANDIDATE
        | lookup_candidate_type_t::ZOMBIE_CANDIDATE
        | lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE
        | lookup_candidate_type_t::ADDON_CANDIDATE => {
            offset != 0 && inst.core.session.sort_options() & SORT_WITHOUT_SENTENCE_CANDIDATE != 0
        }
    }
}

/// The scalars the pin's choose reads off a candidate (`pinyin.cpp:97-105`),
/// copied so that no view of the row outlives the read.
struct RowArgs {
    candidate_type: lookup_candidate_type_t,
    token: Option<PhraseToken>,
    nbest_index: u8,
    begin: u16,
    end: u16,
}

impl RowArgs {
    fn read(row: &CapiCandidate) -> Self {
        Self {
            candidate_type: row.candidate_type(),
            token: row.token.get(),
            nbest_index: row.nbest_index,
            begin: row.begin,
            end: row.end,
        }
    }
}

/// `return offset + len;` (`pinyin.cpp:2588`): `len` is the `guint8` the
/// span length is stored in (`:2582`), so it wraps at 256, and the
/// `size_t` sum converts to `int`, which GCC reduces modulo 2^32 (its
/// documented implementation-defined conversion to a narrower signed
/// type).
fn choose_answer(offset: usize, span_len: usize) -> c_int {
    let len = usize::from(span_len as u8);
    offset.wrapping_add(len) as u32 as c_int
}

/// The answer of a choose leg that only trains a unigram
/// (`pinyin.cpp:2523-2531`, `:2565-2575`). The pin returns `false`, the `int`
/// `0`, for `ERROR_INTEGER_OVERFLOW` alone and `true`, the `int` `1`, for every
/// other answer: a library with no sub-index (`ERROR_NO_SUB_PHRASE_INDEX`) and
/// a missing item (`ERROR_NO_ITEM`) among them.
fn unigram_leg_answer(outcome: UnigramAdd) -> c_int {
    if matches!(outcome, UnigramAdd::Refused) {
        0
    } else {
        1
    }
}

// pinyin.cpp:2641-2647, pinyin.h:585-593.
/// Clear the constraint a prior choose pinned, by offset.
///
/// # C signature
/// ```c
/// bool pinyin_clear_constraint(pinyin_instance_t * instance,
///                              size_t offset);
/// ```
///
/// Remove the whole constraint containing the supplied offset. Returns true if removed and false
/// for a null instance, an unconstrained position or an out-of-range position.
///
/// # Aborts
///
/// The offset reaches a continuation whose head is gone.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_clear_constraint(
    instance: *mut PinyinInstance,
    offset: usize,
) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    inst.core.clear_abi_constraint(offset, true)
}

/// Promotes the chosen candidate when it is an `ADDON_CANDIDATE`
/// (`pinyin.cpp:2532-2561`): copies this context's addon phrase item into
/// its default facade under the token the range's end holds, rewrites the
/// supplied candidate to a `NORMAL_CANDIDATE` at that token, and returns
/// the token so the constraint records it in place of the addon-facade
/// token. The candidate may belong to another instance: the pin rewrites
/// the row it was given.
///
/// Two paths promote nothing and rewrite nothing:
/// - If sub-index 5 is missing, `get_range` leaves its output unwritten
///   (074a2219 `phrase_index.cpp:610-613`). The pin reads it anyway
///   (`pinyin.cpp:2541-2545`), so its token is undefined.
/// - If this context's addon dictionary has no item for the token (a row
///   from a context with other addon data, or an addon library unloaded
///   since the guess), the failed `get_phrase_item` leaves the default
///   item (`phrase_index.h:81-84`, `:646-652`; `phrase_index.cpp:180-205`)
///   and the pin calls `add_index(0, phrase, token)`
///   (`pinyin.cpp:2555-2557`). Its prefix walk starts at `size_t len =
///   phrase_length - 1`, which wraps to `SIZE_MAX`, and reads past the key
///   buffer on every backend (`phrase_large_table3_tkrzwdb.cpp:215-218`,
///   `phrase_large_table3_kyotodb.cpp:280-283`,
///   `phrase_large_table3_bdb.cpp:298-305`). Class (b).
///
/// # Safety
///
/// `instance` is live, `candidate` meets [`pinyin_choose_candidate`]'s
/// precondition, and neither has a live view.
unsafe fn try_promote_addon(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> Option<PhraseToken> {
    let item = {
        // SAFETY: the caller supplies a live candidate row; the view ends
        // with this block.
        let addon_token = unsafe { candidate_ref(candidate) }.token.get()?;
        // SAFETY: the caller supplies a live instance with no other view.
        let inst = unsafe { instance_ref(instance) };
        inst.core.dict.addon_phrase_item(addon_token)?
    };
    let promoted = {
        // SAFETY: the read views above have ended; the caller serializes
        // this context mutation with every other operation on its
        // instances.
        let ctx = unsafe { crate::state::instance_context_mut(instance) };
        let store = ctx.core.user_store_mut();
        let range_end = store.addon_promotion_range_end()?;
        // `pinyin.cpp:2547-2558`: both index adds and `add_phrase_item`
        // take this one token.
        store.promote_addon_phrase(range_end, &item).ok()?;
        range_end
    };
    // SAFETY: the context write has ended. The row is reached only through
    // this shared view, and its tag and token are cells, so the rewrite
    // is sound even when the row belongs to another instance or to this
    // one (`pinyin.cpp:2561-2562`).
    let row = unsafe { candidate_ref(candidate) };
    row.set_candidate_type(lookup_candidate_type_t::NORMAL_CANDIDATE);
    row.token.set(Some(promoted));
    Some(promoted)
}

// pinyin.cpp:1711-1740, pinyin.cpp:2591-2640.
/// Choose a predicted candidate.
///
/// # C signature
/// ```c
/// bool pinyin_choose_predicted_candidate(pinyin_instance_t * instance,
///                                        lookup_candidate_t * candidate);
/// ```
///
/// A punctuation row succeeds without training. A prefix row updates the receiving context’s
/// unigram. A bigram row also updates the transition from the receiving instance’s prefix
/// predecessor. The unigram increment is 483 and the transition increment is 69. A missing
/// prefix item can still yield true, and a loaded sub-index can receive the aggregate increment.
/// Null arguments, unigram overflow or a store failure can return false. These changes require a
/// separately armed save.
///
/// # Aborts
///
/// Passing a row other than a predicted prefix, bigram or punctuation row aborts
/// (`pinyin.cpp:2593`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `candidate` is a
/// borrowed object. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_choose_predicted_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    if instance.is_null() || candidate.is_null() {
        return false;
    }

    // The row's scalars, copied before any view of `instance`.
    let (candidate_type, token) = {
        // SAFETY: `candidate` is non-null and is a row of a live published
        // list (the export's precondition).
        let row = unsafe { candidate_ref(candidate) };
        (row.candidate_type(), row.token.get())
    };
    // Register row 64, `pinyin.cpp:2593`: only the three predicted row
    // types are chosen here; the pin asserts on any other.
    if !matches!(
        candidate_type,
        lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE
            | lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE
            | lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE
    ) {
        std::process::abort();
    }
    // "The punctuation candidate does not have the frequency."
    if candidate_type == lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE {
        return true;
    }
    let Some(token) = token else {
        return false;
    };
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`; the candidate view has ended, and this shared
    // view ends before either exclusive context borrow below.
    let inst = unsafe { instance_ref(instance) };
    // A prefix row whose item the receiver lacks is still selectable. The
    // pin advances the loaded facade's total by 483 and ignores
    // the failing sub-index write; the prefix branch then answers true
    // without a bigram (`pinyin.cpp:2608-2616`, `phrase_index.h:628-634`;
    // #1231 ruling). The receiver decides, not the row's display string.
    if candidate_type == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE
        && !inst.core.dict.token_exists(token)
    {
        // An unloaded library has no sub-index: the pin returns
        // ERROR_NO_SUB_PHRASE_INDEX before touching the facade total,
        // and the prefix branch still answers true.
        if inst.core.dict.phrase_table_token_enabled(token) {
            // SAFETY: the read view is no longer used; the retained owner is
            // exclusively borrowed for this serialized unigram mutation.
            let ctx = unsafe { crate::state::instance_context_mut(instance) };
            let _ = ctx.core.runtime.add_unigram_delta(token, 483);
        }
        return true;
    }
    let last = inst.core.session.prefix_predecessor();
    let prefix = candidate_type == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE;
    // SAFETY: all candidate and prefix reads are complete; the retained owner
    // is exclusively borrowed for the prediction training write.
    let ctx = unsafe { crate::state::instance_context_mut(instance) };
    let user = ctx.core.user_store_mut();
    if prefix {
        user.observe_predicted_prefix(token).is_ok()
    } else {
        match user.observe_predicted(last, token) {
            Ok(_) => true,
            // Register row 85, `pinyin.cpp:2630` through `memory_chunk.h:390`:
            // after the in-memory unigram add (`pinyin.cpp:2609-2612`) the pin loads
            // the predecessor's gram, one too short for its total, and
            // `get_total_freq`'s assert dies.
            Err(oxpinyin::user::UserStoreError::ShortUserBigramValue) => std::process::abort(),
            // `copy_tkrzwdb` / `copy_bdb` assert on a failed destination write
            // or source walk (`tkrzwdb_utils.h:65`, `:72`; `bdb_utils.h:61`,
            // `:67`). This call does not copy, so it never reaches here.
            Err(oxpinyin::user::UserStoreError::FatalCopy) => std::process::abort(),
            // Class (b), register row 100: a partial item tail is read past
            // by `get_freq`/`insert_freq` (`ngram.cpp:241`, `:190`). Nothing
            // changed; one warning, as before.
            Err(oxpinyin::user::UserStoreError::PartialUserBigramTail) => {
                crate::ffi::log_warning(
                    "pinyin_choose_predicted_candidate: a user bigram value has a partial \
                     item tail (upstream reads past it, ngram.cpp:190)",
                );
                false
            }
            Err(
                oxpinyin::user::UserStoreError::Io(_)
                | oxpinyin::user::UserStoreError::Store(_)
                | oxpinyin::user::UserStoreError::Decode
                | oxpinyin::user::UserStoreError::InvalidPhrase
                | oxpinyin::user::UserStoreError::Persistence(_)
                | oxpinyin::user::UserStoreError::UnknownDatabaseFormat
                | oxpinyin::user::UserStoreError::ChunkHeaderWrite(_)
                | oxpinyin::user::UserStoreError::LibraryChunkHeaderRead
                | oxpinyin::user::UserStoreError::SaveRemapFailed
                | oxpinyin::user::UserStoreError::SaveTooLarge
                | oxpinyin::user::UserStoreError::UnigramTotalOverflow
                | oxpinyin::user::UserStoreError::OverlongIndexKey
                | oxpinyin::user::UserStoreError::NonTokenUserBigramKey
                | oxpinyin::user::UserStoreError::ResidualUserBigramGram
                | oxpinyin::user::UserStoreError::Journal(_)
                | oxpinyin::user::UserStoreError::LibraryLoop(_)
                | oxpinyin::user::UserStoreError::TrainingSetFreqMiss
                | oxpinyin::user::UserStoreError::UserReadingRemoval
                | oxpinyin::user::UserStoreError::SessionSeed(_)
                | oxpinyin::user::UserStoreError::FailedUserBigramStore,
            ) => false,
        }
    }
}

// pinyin.cpp:2684, phonetic_lookup.h:868, storage/phonetic_key_matrix.cpp:661, phonetic_lookup.h:884.
/// Train the current sentence with the given n-best index.
///
/// # C signature
/// ```c
/// bool pinyin_train(pinyin_instance_t * instance, guint8 index);
/// ```
///
/// Train decoded row `index` using the current constraints. With no user directory or no decoded
/// rows, training returns false. Observations start only at a forced `OneStep` constraint. After a
/// forced span, the pin's `train_next` rule also trains the next decoded span. A successful call
/// can update no frequencies, for example when no phrase is forced, and it still marks the profile
/// modified, so a later save writes it. Observations already applied can survive a failed training
/// call.
///
/// # Aborts
///
/// Training with a forced phrase or span inconsistent with the current decode or matrix can
/// abort. An index outside a nonempty result set aborts after the user-directory check.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_train(instance: *mut PinyinInstance, index: u8) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    train_answer(unsafe { crate::state::train_instance(instance, index) })
}

/// `pinyin_train`'s answer for its training result: the process aborts where the
/// pin dies, and a class (b) refusal is `false` with one warning.
fn train_answer(result: Result<bool, oxpinyin::engine::EngineError>) -> bool {
    use oxpinyin::facade::TrainingRefusal;
    // The Pinyin caller supplies the n-best index (074a2219 pinyin.cpp:2684).
    if matches!(
        result,
        Err(oxpinyin::engine::EngineError::CandidateIndexOutOfRange { .. })
    ) {
        std::process::abort();
    }
    match oxpinyin::facade::training_answer(result) {
        Ok(trained) => trained,
        Err(TrainingRefusal::Abort) => std::process::abort(),
        Err(TrainingRefusal::PartialBigram) => {
            crate::ffi::log_warning(
                "pinyin_train: a user bigram value has a partial item tail \
                 (upstream reads past it, ngram.cpp:190)",
            );
            false
        }
    }
}

/// Whether [`pinyin_train`] of row `index` reaches one of the pin's own
/// training asserts (register rows 6, 38 and 82): the facade's
/// [`oxpinyin::facade::InstanceView::train_aborts`], the same row lookup and
/// span walk `pinyin_train` runs. Reads only. For the in-tree fuzz harness,
/// which skips such a call: not in `pinyin.h`, a Rust function compiled
/// only with `fuzz-api` and never into the shipped artifact.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[cfg(feature = "fuzz-api")]
#[must_use]
pub unsafe fn oxpinyin_train_aborts(instance: *mut PinyinInstance, index: u8) -> bool {
    let inst = borrow_or_return!(instance, crate::state::instance_ref, false);
    inst.core.train_aborts(index)
}

#[cfg(test)]
mod tests;
