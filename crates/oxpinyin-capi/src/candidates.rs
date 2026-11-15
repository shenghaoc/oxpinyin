//! Candidate access, selection, and training.

use std::os::raw::c_int;

use oxpinyin_core::PhraseToken;
use oxpinyin_engine::SORT_WITHOUT_SENTENCE_CANDIDATE;
use oxpinyin_user::{UnigramAdd, is_user_token};

use crate::state::{CapiCandidate, candidate_ptr, candidate_ref, instance_mut, instance_ref};
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

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
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

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
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
/// bool pinyin_get_candidate_type(pinyin_instance_t * instance,
///                                lookup_candidate_t * candidate,
///                                lookup_candidate_type_t * type);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_candidate_type(
    _instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    candidate_type: *mut lookup_candidate_type_t,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2866-2871`).
    if candidate.is_null() {
        return false;
    }

    // SAFETY: `candidate` is non-null and was produced by
    // `pinyin_get_candidate`.
    let cand = unsafe { candidate_ref(candidate) };
    let ctype = cand.candidate_type();
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
/// bool pinyin_get_candidate_string(pinyin_instance_t * instance,
///                                  lookup_candidate_t * candidate,
///                                  const gchar ** utf8_str);
/// ```
///
/// Out-param `utf8_str` is instance-borrowed (never freed by caller).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_candidate_string(
    _instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    utf8_str: *mut *const GChar,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2873-2878`).
    if candidate.is_null() {
        return false;
    }

    // SAFETY: `candidate` is non-null and was produced by
    // `pinyin_get_candidate`.
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

/// Get the n-best index of a candidate.
///
/// # C signature
/// ```c
/// bool pinyin_get_candidate_nbest_index(pinyin_instance_t * instance,
///                                       lookup_candidate_t * candidate,
///                                       guint8 * index);
/// ```
///
/// W14: the stored tail rank of an `NBEST_MATCH_CANDIDATE` row
/// (`m_nbest_index`, `pinyin.cpp:2878-2884`). Any other row type is the
/// pin's `assert`, and the process aborts (register row 64).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_candidate_nbest_index(
    _instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    index: *mut u8,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2880-2886`).
    if candidate.is_null() {
        return false;
    }

    // SAFETY: `candidate` is non-null and was produced by
    // `pinyin_get_candidate`.
    let cand = unsafe { candidate_ref(candidate) };
    // Register row 64, `pinyin.cpp:2883`:
    // `assert(NBEST_MATCH_CANDIDATE == type)`, before `*index` is written.
    if cand.candidate_type() != lookup_candidate_type_t::NBEST_MATCH_CANDIDATE {
        std::process::abort();
    }
    if !index.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *index = cand.nbest_index;
        }
    }
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
/// [`USER_DICTIONARY`] sub-index. Network (nibble 6) is not "user" —
/// `if (USER_DICTIONARY != index) return false` (`:3718`). Predicted rows
/// and sentence-level/fallback rows carry no user status even when a
/// predicted prefix token names the user library.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_is_user_candidate(
    _instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:3712-3724`).
    if candidate.is_null() {
        return false;
    }

    // SAFETY: `candidate` is non-null and was produced by
    // `pinyin_get_candidate`.
    let cand = unsafe { candidate_ref(candidate) };
    if !matches!(
        cand.candidate_type(),
        lookup_candidate_type_t::NORMAL_CANDIDATE | lookup_candidate_type_t::LONGER_CANDIDATE
    ) {
        return false;
    }
    cand.token
        .get()
        .is_some_and(|token| is_user_token(token.value()))
}

/// Remove a user candidate from the dictionary.
///
/// # C signature
/// ```c
/// bool pinyin_remove_user_candidate(pinyin_instance_t * instance,
///                                   lookup_candidate_t * candidate);
/// ```
///
/// The §3.4 removal: the row must be a `NORMAL_CANDIDATE` whose token
/// lives in the user dictionary (upstream asserts both, and so the process
/// aborts here, register row 64), then the phrase, its pronunciations, its bigram rows and
/// its unigram delta are deleted. Does **not** arm `m_modified`, matching
/// upstream's set-sites.
///
/// The pin reads the candidate's type and token and removes from the
/// receiving `instance`'s context, whichever instance published the row
/// (`pinyin.cpp:3726-3768`); so does this. The handle must be a row of a
/// live instance's published candidate list, valid for the whole call and
/// not used elsewhere during it, as for [`pinyin_choose_candidate`].
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_remove_user_candidate(
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
    if !is_user_token(token.value()) {
        std::process::abort();
    }
    // SAFETY: the candidate reads are complete and the retained owner is now
    // borrowed exclusively for the store mutation.
    let ctx = unsafe { crate::state::instance_context_mut(instance) };
    let user = ctx.core.user_store_mut();
    match user.remove_user_phrase(token.value()) {
        Ok(true) => true,
        // Class (b), register row 98: the call's `user_bigram->mask_out`
        // (`pinyin.cpp:3766`) would read past a partial item tail
        // (`ngram.cpp:76-86`). One warning, as before.
        Err(oxpinyin_user::UserStoreError::PartialUserBigramTail) => {
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
            oxpinyin_user::UserStoreError::NonTokenUserBigramKey
            | oxpinyin_user::UserStoreError::ShortUserBigramValue
            | oxpinyin_user::UserStoreError::ResidualUserBigramGram
            | oxpinyin_user::UserStoreError::UserReadingRemoval,
        ) => std::process::abort(),
        // `copy_tkrzwdb` / `copy_bdb` assert on a failed destination write
        // or source walk (`tkrzwdb_utils.h:65`, `:72`; `bdb_utils.h:61`,
        // `:67`). This call does not copy, so it never reaches here.
        Err(oxpinyin_user::UserStoreError::FatalCopy) => std::process::abort(),
        // A store failure the pin has no counterpart for.
        Err(
            oxpinyin_user::UserStoreError::Io(_)
            | oxpinyin_user::UserStoreError::Store(_)
            | oxpinyin_user::UserStoreError::Decode
            | oxpinyin_user::UserStoreError::InvalidPhrase
            | oxpinyin_user::UserStoreError::Persistence(_)
            | oxpinyin_user::UserStoreError::UnknownDatabaseFormat
            | oxpinyin_user::UserStoreError::ChunkHeaderWrite(_)
            | oxpinyin_user::UserStoreError::LibraryChunkHeaderRead
            | oxpinyin_user::UserStoreError::SaveRemapFailed
            | oxpinyin_user::UserStoreError::SaveTooLarge
            | oxpinyin_user::UserStoreError::UnigramTotalOverflow
            | oxpinyin_user::UserStoreError::OverlongIndexKey
            | oxpinyin_user::UserStoreError::Journal(_)
            | oxpinyin_user::UserStoreError::LibraryLoop(_)
            | oxpinyin_user::UserStoreError::TrainingSetFreqMiss
            | oxpinyin_user::UserStoreError::SessionSeed(_)
            | oxpinyin_user::UserStoreError::FailedUserBigramStore,
        ) => {
            crate::ffi::log_warning("pinyin_remove_user_candidate: the user store failed");
            false
        }
    }
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
/// The pin reads the candidate's type, token, n-best index and bounds and
/// never checks which instance published it (`pinyin.cpp:2501-2588`). The
/// context, matrix, constraints, n-best results and sort option are the
/// receiving `instance`'s. So does this: the row's scalars are copied out
/// of the handle, and everything else is read from and written to
/// `instance` and its context.
///
/// Returns -1 for a NULL argument or an engine failure. The handle must be
/// a row of a live instance's published candidate list, valid for the
/// whole call and not used elsewhere during it. Any other pointer is a
/// caller precondition violation, as for the candidate getters.
///
/// The dispatch, in the pin's order:
/// - A predicted row is the pin's `assert` at `:2507`, and the process
///   aborts (register row 64).
/// - `NBEST_MATCH_CANDIDATE` (`:2513-2519`): `diff_result` against this
///   instance's results 0 and `m_nbest_index`. If either is absent, the
///   pin's `check_result` asserts and the process aborts. The answer is
///   this instance's `matrix.size() - 1`, the reserved tail slot one past
///   the last real matrix column, in the active parse mode's coordinates
///   (`CapiInstance::parsed_len`; row 9).
/// - `LONGER_CANDIDATE` (`:2521-2530`): only the token's unigram trains,
///   by `initial_seed * unigram_factor = 69 * 7 = 483`. The answer is 0
///   when the library total would overflow and 1 otherwise.
/// - `ADDON_CANDIDATE` (`:2532-2562`): promoted into this context's
///   nibble-5 (`ADDON_DICTIONARY`, `addon.bin`) sub-index
///   (`docs/findings/addon-choose-promotion.md`). The supplied row becomes
///   a `NORMAL_CANDIDATE` at the new token, and its string is unchanged.
///   If this context's addon dictionary has no item for the token, the pin
///   reaches undefined behaviour (class (b); register entry "An addon row
///   chosen on a context whose addon dictionary has no item for its
///   token"):
///   nothing is promoted or rewritten, and the choose continues with the
///   addon token and no recorded text.
/// - `SORT_WITHOUT_SENTENCE_CANDIDATE` set by the last
///   `pinyin_guess_candidates` (`:2565-2576`): the pin asserts `0 ==
///   offset` (register row 45). Then only the unigram trains, as LONGER.
/// - Otherwise (`:2577-2590`): validate, `add_constraint(m_begin, m_end,
///   token)` against this instance's parse, validate again, and answer
///   `offset + len`. A refused span answers `offset + 0`.
///
/// Per §2.2, the bigram training of a normal selection is deferred to
/// [`pinyin_train`].
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_choose_candidate(
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
        // (`pinyin.cpp:2516-2517`): both are `assert`s.
        if !inst.core.has_nbest_rank(row.nbest_index) {
            std::process::abort();
        }
        if inst.core.select_nbest_rank(row.nbest_index).is_err() {
            return -1;
        }
        return c_int::try_from(inst.core.parsed_len).unwrap_or(c_int::MAX);
    }
    if row.candidate_type == lookup_candidate_type_t::LONGER_CANDIDATE {
        let Some(token) = row.token else {
            return -1;
        };
        // SAFETY: no instance view is live; this serialized C operation
        // exclusively borrows the retained owner for the unigram write.
        let ctx = unsafe { crate::state::instance_context_mut(instance) };
        return unigram_leg_answer(ctx.core.runtime.add_unigram_outcome(token.value(), 483));
    }
    let mut token = row.token;
    let mut text = oxpinyin_engine::RowText::Resolve;
    if row.candidate_type == lookup_candidate_type_t::ADDON_CANDIDATE {
        // SAFETY: no instance or candidate view is live; the handle meets
        // the export's precondition.
        match unsafe { try_promote_addon(instance, candidate) } {
            Some(promoted) => token = Some(promoted),
            None => text = oxpinyin_engine::RowText::Empty,
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
        let token = token.map_or(0, PhraseToken::value);
        // SAFETY: the instance view is no longer used; this serialized C
        // operation exclusively borrows its retained owner for the unigram
        // write.
        let ctx = unsafe { crate::state::instance_context_mut(instance) };
        return unigram_leg_answer(ctx.core.runtime.add_unigram_outcome(token, 483));
    }
    // A row with no token reaching the normal branch is a predicted
    // punctuation row, at `m_begin = m_end = 0` (`pinyin.cpp:114`). The pin
    // writes `add_constraint(0, 0, null_token)` between its two validations
    // and answers `offset + 0` (`:2577-2590`; `phonetic_lookup.cpp:61-86`):
    // a zero-length null-token cell at 0 that blocks every later sentence
    // decode until a reset clears it or a later non-empty forcing at 0
    // overwrites it. After a reset the write lands in the empty array's
    // terminator slot and changes nothing observable (GLib 2.84.4
    // `garray.c:349-354`, `:1051-1079`).
    let Some(token) = token else {
        return match inst.core.session.choose_null_step() {
            Ok(()) => choose_answer(offset, 0),
            Err(_) => -1,
        };
    };
    let published = oxpinyin_facade::PublishedRow {
        begin: row.begin,
        end: row.end,
        token,
        text,
    };
    match inst.core.select_published(&published, true, true) {
        // `add_constraint` refused the span: `len = 0` (`phonetic_lookup.cpp:64-65`).
        Ok(oxpinyin_engine::Selection::Refused) => choose_answer(offset, 0),
        // The pin answers the caller's choose offset plus the length
        // `add_constraint` wrote, `m_end - m_begin` of the row's own span
        // (`pinyin.cpp:2582-2590`, `phonetic_lookup.cpp:61-86`), whatever
        // offset the caller passes.
        Ok(oxpinyin_engine::Selection::Continued | oxpinyin_engine::Selection::Completed) => {
            choose_answer(
                offset,
                usize::from(row.end).wrapping_sub(usize::from(row.begin)),
            )
        }
        Err(_) => -1,
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

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    inst.core.clear_abi_constraint(offset, true, true)
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
///   (`pinyin.cpp:2551-2553`). Its prefix walk starts at `size_t len =
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
        inst.core.dict.addon_phrase_item(addon_token.value())?
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
    let promoted = PhraseToken::new(promoted);
    // SAFETY: the context write has ended. The row is reached only through
    // this shared view, and its tag and token are cells, so the rewrite
    // is sound even when the row belongs to another instance or to this
    // one (`pinyin.cpp:2561-2562`).
    let row = unsafe { candidate_ref(candidate) };
    row.set_candidate_type(lookup_candidate_type_t::NORMAL_CANDIDATE);
    row.token.set(Some(promoted));
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
/// `69 * 7 = 483`. A prefix row returns after that add. A bigram row also
/// raises `(last → token)` and `last`'s total by a flat `69`, never the
/// reselection doubling of [`pinyin_train`]. `last`
/// is the predecessor `_get_previous_token(instance, 0)` finds
/// (`pinyin.cpp:1711-1740`): the longest token among the prefixes the last
/// `pinyin_guess_predicted_candidates` or `pinyin_guess_sentence_with_prefix`
/// computed, `sentence_start` when there are none.
///
/// The pin's return table (`pinyin.cpp:2591-2640`): a punctuation row has no
/// frequency and answers `true` without training anything (`:2604-2605`); a
/// unigram add that overflows its library's total answers `false` before the
/// bigram (`:2609-2612`, [`UserStoreError::UnigramTotalOverflow`]); otherwise
/// `true`.
///
/// The pin reads the candidate's type and token and trains the receiving
/// `instance`'s context with the receiver's own prefixes, whichever
/// instance published the row (`pinyin.cpp:2591-2640`); so does this. The
/// handle must be a row of a live instance's published candidate list,
/// valid for the whole call and not used elsewhere during it, as for
/// [`pinyin_choose_candidate`].
///
/// Returns `false` for a candidate without a token or a store failure.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_choose_predicted_candidate(
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
    // `pinyin_alloc_instance`; the candidate view has ended.
    let inst = unsafe { instance_mut(instance) };
    // A prefix row whose item the receiver lacks is still selectable. The
    // pin advances the loaded facade's total by 483 (wrapping) and ignores
    // the failing sub-index write; the prefix branch then answers true
    // without a bigram (`pinyin.cpp:2608-2616`, `phrase_index.h:628-634`;
    // #1231 ruling). The receiver decides, not the row's display string.
    if candidate_type == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE
        && !inst.core.dict.token_exists(token.value())
    {
        // An unloaded library has no sub-index: the pin returns
        // ERROR_NO_SUB_PHRASE_INDEX before touching the facade total,
        // and the prefix branch still answers true.
        if inst.core.dict.phrase_table_token_enabled(token.value()) {
            // SAFETY: the read view is no longer used; the retained owner is
            // exclusively borrowed for this serialized unigram mutation.
            let ctx = unsafe { crate::state::instance_context_mut(instance) };
            let _ = ctx.core.runtime.add_unigram_delta(token.value(), 483);
        }
        return true;
    }
    let last = inst.core.session.prefix_predecessor().value();
    let prefix = candidate_type == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE;
    // SAFETY: all candidate and prefix reads are complete; the retained owner
    // is exclusively borrowed for the prediction training write.
    let ctx = unsafe { crate::state::instance_context_mut(instance) };
    let user = ctx.core.user_store_mut();
    if prefix {
        user.observe_predicted_prefix(token.value()).is_ok()
    } else {
        match user.observe_predicted(last, token.value()) {
            Ok(_) => true,
            // Register row 85, `pinyin.cpp:2630` through `memory_chunk.h:390`:
            // after the in-memory unigram add (`:2609-2612`) the pin loads
            // the predecessor's gram, one too short for its total, and
            // `get_total_freq`'s assert dies.
            Err(oxpinyin_user::UserStoreError::ShortUserBigramValue) => std::process::abort(),
            // `copy_tkrzwdb` / `copy_bdb` assert on a failed destination write
            // or source walk (`tkrzwdb_utils.h:65`, `:72`; `bdb_utils.h:61`,
            // `:67`). This call does not copy, so it never reaches here.
            Err(oxpinyin_user::UserStoreError::FatalCopy) => std::process::abort(),
            // Class (b), register row 100: a partial item tail is read past
            // by `get_freq`/`insert_freq` (`ngram.cpp:241`, `:190`). Nothing
            // changed; one warning, as before.
            Err(oxpinyin_user::UserStoreError::PartialUserBigramTail) => {
                crate::ffi::log_warning(
                    "pinyin_choose_predicted_candidate: a user bigram value has a partial \
                     item tail (upstream reads past it, ngram.cpp:190)",
                );
                false
            }
            Err(
                oxpinyin_user::UserStoreError::Io(_)
                | oxpinyin_user::UserStoreError::Store(_)
                | oxpinyin_user::UserStoreError::Decode
                | oxpinyin_user::UserStoreError::InvalidPhrase
                | oxpinyin_user::UserStoreError::Persistence(_)
                | oxpinyin_user::UserStoreError::UnknownDatabaseFormat
                | oxpinyin_user::UserStoreError::ChunkHeaderWrite(_)
                | oxpinyin_user::UserStoreError::LibraryChunkHeaderRead
                | oxpinyin_user::UserStoreError::SaveRemapFailed
                | oxpinyin_user::UserStoreError::SaveTooLarge
                | oxpinyin_user::UserStoreError::UnigramTotalOverflow
                | oxpinyin_user::UserStoreError::OverlongIndexKey
                | oxpinyin_user::UserStoreError::NonTokenUserBigramKey
                | oxpinyin_user::UserStoreError::ResidualUserBigramGram
                | oxpinyin_user::UserStoreError::Journal(_)
                | oxpinyin_user::UserStoreError::LibraryLoop(_)
                | oxpinyin_user::UserStoreError::TrainingSetFreqMiss
                | oxpinyin_user::UserStoreError::UserReadingRemoval
                | oxpinyin_user::UserStoreError::SessionSeed(_)
                | oxpinyin_user::UserStoreError::FailedUserBigramStore,
            ) => false,
        }
    }
}

/// Train the current sentence with the given n-best index.
///
/// # C signature
/// ```c
/// bool pinyin_train(pinyin_instance_t * instance, guint8 index);
/// ```
///
/// Train the requested decoded n-best row, with the pin's constraint gate.
/// Empty results refuse quietly. An index beyond nonempty results
/// (`pinyin.cpp:2684`), a forced phrase that no longer matches the decoded
/// result (`phonetic_lookup.h:868`) and a span outside the current matrix
/// (`storage/phonetic_key_matrix.cpp:661`, `:663`, `:664`) are the pin's
/// asserts, and so are a predecessor's gram too short for its total
/// (`phonetic_lookup.h:884`) and a `set_freq` that misses the item its
/// `insert_freq` added (`:903`): the process aborts there before anything is
/// observed (register rows 6, 38, 82 and 85).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_train(instance: *mut PinyinInstance, index: u8) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    train_answer(unsafe { crate::state::train_instance(instance, index) })
}

/// `pinyin_train`'s answer for its training result: the process aborts where the
/// pin dies, and a class (b) refusal is `false` with one warning.
fn train_answer(result: Result<bool, oxpinyin_engine::EngineError>) -> bool {
    use oxpinyin_facade::TrainingRefusal;
    // The Pinyin caller supplies the n-best index (074a2219 pinyin.cpp:2684).
    if matches!(
        result,
        Err(oxpinyin_engine::EngineError::CandidateIndexOutOfRange { .. })
    ) {
        std::process::abort();
    }
    match oxpinyin_facade::training_answer(result) {
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
/// [`oxpinyin_facade::InstanceView::train_aborts`], the same row lookup and
/// span walk `pinyin_train` runs. Reads only. For the in-tree fuzz harness,
/// which skips such a call: not in `pinyin.h`, a Rust function compiled
/// only with `fuzz-api` and never into the shipped artifact.
#[cfg(feature = "fuzz-api")]
#[must_use]
pub fn oxpinyin_train_aborts(instance: *mut PinyinInstance, index: u8) -> bool {
    if instance.is_null() {
        return false;
    }
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { crate::state::instance_ref(instance) };
    inst.core.train_aborts(index)
}

#[cfg(test)]
mod previous_token_tests {
    use crate::config::{pinyin_load_phrase_library, pinyin_unload_phrase_library};
    use crate::parse::pinyin_parse_more_full_pinyins;
    use crate::sentence::pinyin_guess_candidates;
    use crate::state::{instance_mut, instance_ref};
    use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open};

    /// `_get_previous_token(instance, 0)` skips a prefix token whose library
    /// is unloaded (`get_phrase_item` fails, `pinyin.cpp:1722-1730`) and falls
    /// back to `sentence_start`. The pin cannot show it: only a predicted
    /// bigram row reads the predecessor there and the harness never gets
    /// one, so this is a unit test.
    #[test]
    fn an_unloaded_library_prefix_is_not_the_predecessor() {
        let user_dir = TempUserDir::new("prev-token-unloaded");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, cstr("ni").as_ptr()),
            2
        );
        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        // SAFETY: live instance immediately after the guess.
        let gbk = unsafe { instance_ref(instance) }
            .candidates
            .iter()
            .filter_map(|c| c.token.get().map(oxpinyin_core::PhraseToken::value))
            .find(|token| token >> 24 == 2)
            .expect("the fixture carries a GBK candidate under ni");

        // SAFETY: as above.
        unsafe { instance_mut(instance) }
            .core
            .session
            .set_candidate_prefixes([oxpinyin_core::PhraseToken::new(gbk)]);
        assert_eq!(previous(instance), gbk);

        assert!(pinyin_unload_phrase_library(context, 2));
        assert_eq!(previous(instance), oxpinyin_user::SENTENCE_START);

        assert!(pinyin_load_phrase_library(context, 2));
        assert_eq!(previous(instance), gbk);

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    fn previous(instance: *mut crate::types::PinyinInstance) -> u32 {
        // SAFETY: a live `pinyin_alloc_instance` handle of the caller.
        unsafe { instance_ref(instance) }
            .core
            .session
            .prefix_predecessor()
            .value()
    }
}

#[cfg(test)]
mod unloaded_library_tests {
    //! A library `pinyin_unload_phrase_library` let go has no sub-index in the
    //! pin (`phrase_index.cpp:261-269`): a write that names it answers an
    //! error before it moves anything (`phrase_index.h:628-635`), `compact`
    //! skips it (`phrase_index.cpp:651-652`), and the caller goes on.
    //!
    //! Each test follows `previous_token_tests`: it opens the fixture, parses
    //! `ni`, guesses its candidates, takes the one in the GBK library (2) and
    //! unloads that library.

    use std::os::raw::c_uint;
    use std::ptr;

    use oxpinyin_core::{Dictionary, PhraseToken};
    use oxpinyin_user::{SENTENCE_START, UserStore};

    use super::{
        pinyin_choose_candidate, pinyin_choose_predicted_candidate, pinyin_get_candidate,
        pinyin_train,
    };
    use crate::config::{pinyin_load_phrase_library, pinyin_unload_phrase_library};
    use crate::dict::pinyin_token_add_unigram_frequency;
    use crate::iterators::{
        pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase,
    };
    use crate::parse::pinyin_parse_more_full_pinyins;
    use crate::predict::pinyin_guess_predicted_candidates;
    use crate::sentence::{pinyin_guess_candidates, pinyin_guess_sentence};
    use crate::state::{context_mut, instance_ref};
    use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open};
    use crate::types::{LookupCandidate, PinyinContext, PinyinInstance, lookup_candidate_type_t};

    /// `SORT_WITHOUT_SENTENCE_CANDIDATE`: the choose leg that only trains.
    const WITHOUT_SENTENCE: c_uint = 0x1;
    /// The ibus preset that leaves `SORT_WITHOUT_LONGER_CANDIDATE` clear.
    const IBUS_DEFAULT_SORT: c_uint = 0x1c;

    /// The open fixture of `previous_token_tests`, with `ni` parsed.
    struct Open {
        /// The directories the context reads, removed after it is freed.
        _dirs: Box<dyn std::any::Any>,
        context: *mut PinyinContext,
        instance: *mut PinyinInstance,
    }

    impl Open {
        fn new(tag: &str) -> Self {
            let user = TempUserDir::new(tag);
            let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
            Self::parsed(Box::new(user), context, instance)
        }

        fn parsed(
            dirs: Box<dyn std::any::Any>,
            context: *mut PinyinContext,
            instance: *mut PinyinInstance,
        ) -> Self {
            assert_eq!(
                pinyin_parse_more_full_pinyins(instance, cstr("ni").as_ptr()),
                2
            );
            Self {
                _dirs: dirs,
                context,
                instance,
            }
        }

        /// Guesses `ni`'s candidates under `sort` and answers the first row
        /// `wanted` accepts, with its token.
        fn row_of(
            &self,
            sort: c_uint,
            wanted: impl Fn(lookup_candidate_type_t, u32) -> bool,
        ) -> (*mut LookupCandidate, u32) {
            assert!(pinyin_guess_candidates(self.instance, 0, sort));
            self.find_row(wanted)
        }

        fn find_row(
            &self,
            wanted: impl Fn(lookup_candidate_type_t, u32) -> bool,
        ) -> (*mut LookupCandidate, u32) {
            let (index, token) = {
                // SAFETY: live instance immediately after the guess.
                let inst = unsafe { instance_ref(self.instance) };
                inst.candidates
                    .iter()
                    .enumerate()
                    .find_map(|(index, row)| {
                        let token = row.token.get().map(PhraseToken::value)?;
                        wanted(row.candidate_type(), token).then_some((index, token))
                    })
                    .expect("the fixture holds a row of this kind")
            };
            let mut row = ptr::null_mut();
            assert!(pinyin_get_candidate(
                self.instance,
                u32::try_from(index).expect("small index"),
                &raw mut row
            ));
            (row, token)
        }

        /// The row of the GBK library (2) under `ni`, and its token.
        fn gbk_row(&self, sort: c_uint) -> (*mut LookupCandidate, u32) {
            self.row_of(sort, |_, token| token >> 24 == 2)
        }

        fn store<R>(&self, read: impl FnOnce(&UserStore) -> R) -> R {
            // SAFETY: a live instance between serialized C operations; the
            // closure returns no borrow into the store.
            let inst = unsafe { instance_ref(self.instance) };
            read(inst.core.user)
        }

        /// The total a system library's sub-index holds once it is loaded
        /// and before any write: `SubPhraseIndex::load` reads it from the
        /// chunk (`phrase_index.cpp:345` at the pin `074a2219`).
        fn loaded_total(&self, library: u8) -> u32 {
            // SAFETY: as in `store`.
            unsafe { instance_ref(self.instance) }
                .core
                .dict
                .system()
                .libraries()
                .library(u32::from(library) << 24)
                .map(oxpinyin_data::phrase_library::PhraseLibrary::total_freq)
                .expect("the library is mapped")
        }

        /// The facade total the model divides by.
        fn amplified_total(&self) -> u64 {
            // SAFETY: as in `store`.
            unsafe { instance_ref(self.instance) }
                .core
                .lm
                .amplified_total()
        }

        fn token_of(&self, text: &str) -> u32 {
            // SAFETY: as in `store`.
            unsafe { instance_ref(self.instance) }
                .core
                .dict
                .tokens_for_text(text)[0]
                .value()
        }
    }

    impl Drop for Open {
        fn drop(&mut self) {
            crate::instance::pinyin_free_instance(self.instance);
            crate::context::pinyin_fini(self.context);
        }
    }

    // Both unigram-only choose branches must use the instance's owner, not
    // its finalized public context. Keep the ordinary lifetime as a control.
    #[rstest::rstest]
    #[case::longer(IBUS_DEFAULT_SORT, lookup_candidate_type_t::LONGER_CANDIDATE)]
    #[case::sort_bit(DEFAULT_SORT | WITHOUT_SENTENCE, lookup_candidate_type_t::NORMAL_CANDIDATE)]
    fn choosing_a_unigram_row_keeps_the_instance_owner(
        #[case] sort: c_uint,
        #[case] kind: lookup_candidate_type_t,
        #[values(false, true)] finalized: bool,
    ) {
        let mut open = Open::new("choose-retained-owner");
        assert!(pinyin_guess_sentence(open.instance));
        let (row, _) = open.row_of(sort, |candidate_kind, _| candidate_kind == kind);
        if finalized {
            crate::test_support::finalize_public_context(open.context, open.instance);
            open.context = ptr::null_mut();
        }
        let before = open.amplified_total();
        assert_eq!(pinyin_choose_candidate(open.instance, 0, row), 1);
        assert_eq!(open.amplified_total() - before, 483);
    }

    /// `pinyin_train` after the unload. `train_result3` trains the chosen
    /// word's bigram, ignores the unigram add the missing sub-index refuses,
    /// and raises the readings of a stale cached item the failed
    /// `get_phrase_item` left (`phonetic_lookup.h:868-935`): freed memory or
    /// another item, which oxpinyin does not copy (register row 118). It
    /// trains nothing of the word's library and answers `true`.
    #[test]
    fn training_a_chosen_word_of_an_unloaded_library_keeps_its_bigram() {
        let open = Open::new("train-unloaded");
        assert!(pinyin_guess_sentence(open.instance));
        let (row, gbk) = open.gbk_row(DEFAULT_SORT);
        assert!(pinyin_choose_candidate(open.instance, 0, row) > 0);
        assert!(pinyin_guess_sentence(open.instance));
        assert!(pinyin_unload_phrase_library(open.context, 2));
        let total = open.amplified_total();

        assert!(pinyin_train(open.instance, 0));

        assert!(
            open.store(|store| store.bigram_count(SENTENCE_START, gbk))
                .expect("count")
                > 0,
            "the bigram of the chosen word is trained"
        );
        assert_eq!(
            open.amplified_total(),
            total,
            "no sub-index takes the unigram add, so no total moves"
        );
    }

    /// `pinyin_choose_predicted_candidate` after the unload: the unigram add
    /// the missing sub-index refuses is ignored and the bigram trains
    /// (`pinyin.cpp:2609-2639`).
    #[test]
    fn choosing_a_predicted_word_of_an_unloaded_library_keeps_its_bigram() {
        let open = Open::new("predicted-unloaded");
        let (_, gbk) = open.gbk_row(DEFAULT_SORT);
        let ni = open.token_of("你");
        // The bigram at the prediction's filter edge, as
        // `a_predicted_bigram_trains_from_the_prediction_prefix` seeds it.
        // SAFETY: the live context has no reader retained across this write.
        unsafe { context_mut(open.context) }
            .core
            .user_store_mut()
            .set_bigram_count(ni, gbk, 10)
            .expect("seed");
        assert!(pinyin_guess_predicted_candidates(
            open.instance,
            cstr("你").as_ptr()
        ));
        let (row, token) =
            open.find_row(|kind, _| kind == lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE);
        assert_eq!(token, gbk, "the seeded word is the predicted bigram row");
        assert!(pinyin_unload_phrase_library(open.context, 2));
        let before = open
            .store(|store| store.bigram_count(ni, gbk))
            .expect("count");
        let total = open.amplified_total();

        assert!(pinyin_choose_predicted_candidate(open.instance, row));

        assert_eq!(
            open.store(|store| store.bigram_count(ni, gbk))
                .expect("count"),
            before + 69
        );
        assert_eq!(open.amplified_total(), total);
    }

    /// `pinyin_load_phrase_library` over a system file the pin cannot map:
    /// the pin dies (register row 76). The call keeps its answer, and the
    /// library stays unloaded, so no reader meets a library whose bytes the
    /// session no longer holds. The next load maps the repaired file.
    #[test]
    fn a_failed_reload_keeps_the_library_unloaded_until_the_next_load() {
        let scratch = tempfile::tempdir().expect("scratch");
        let system = crate::test_support::private_system(scratch.path());
        let user = scratch.path().join("user");
        std::fs::create_dir_all(&user).expect("user dir");
        let system_c = cstr(system.to_str().expect("UTF-8 path"));
        let user_c = cstr(user.to_str().expect("UTF-8 path"));
        let context = crate::context::pinyin_init(system_c.as_ptr(), user_c.as_ptr());
        assert!(!context.is_null());
        let instance = crate::instance::pinyin_alloc_instance(context);
        assert!(!instance.is_null());
        let open = Open::parsed(Box::new(scratch), context, instance);
        let (_, gbk) = open.gbk_row(DEFAULT_SORT);
        assert!(pinyin_unload_phrase_library(context, 2));

        let library = system.join("gbk_char.bin");
        std::fs::remove_file(&library).expect("remove the system file");
        assert!(pinyin_load_phrase_library(context, 2), "row 76's answer");
        assert!(
            !pinyin_token_add_unigram_frequency(instance, gbk, 1),
            "the library is still unloaded"
        );

        crate::test_support::link_file(
            &crate::test_support::system_dir().join("gbk_char.bin"),
            &library,
        );
        assert!(
            pinyin_load_phrase_library(context, 2),
            "the next load maps it"
        );
        assert!(
            pinyin_token_add_unigram_frequency(instance, gbk, 1),
            "the library is loaded again"
        );
    }

    /// `pinyin_end_add_phrases` compacts the phrase index
    /// (`pinyin.cpp:655-660`), and `compact` skips a missing sub-index
    /// (`phrase_index.cpp:651-652`): the unloaded library's total does not move
    /// into the facade's.
    #[test]
    fn closing_an_import_leaves_the_unloaded_librarys_total_alone() {
        let open = Open::new("compact-unloaded");
        assert!(pinyin_unload_phrase_library(open.context, 2));
        let total = open.amplified_total();

        pinyin_end_add_phrases(pinyin_begin_add_phrases(open.context, 2));

        assert_eq!(open.amplified_total(), total);
    }

    /// The guards that already answer for an unloaded library: the increment
    /// and the import are refused (`ERROR_NO_SUB_PHRASE_INDEX`), and the same
    /// calls take once the library is loaded again.
    #[test]
    fn an_increment_and_an_import_into_an_unloaded_library_answer_false() {
        let open = Open::new("guards-unloaded");
        let (_, gbk) = open.gbk_row(DEFAULT_SORT);
        assert!(pinyin_unload_phrase_library(open.context, 2));
        let total = open.amplified_total();

        assert!(!pinyin_token_add_unigram_frequency(open.instance, gbk, 1));
        let import = pinyin_begin_add_phrases(open.context, 2);
        assert!(!pinyin_iterator_add_phrase(
            import,
            cstr("单").as_ptr(),
            cstr("dan").as_ptr(),
            -1
        ));
        assert_eq!(open.amplified_total(), total);
        pinyin_end_add_phrases(import);

        assert!(pinyin_load_phrase_library(open.context, 2));
        assert!(pinyin_token_add_unigram_frequency(open.instance, gbk, 1));
    }

    /// The `SORT_WITHOUT_SENTENCE_CANDIDATE` leg trains the row's unigram and
    /// answers `true` unless the add overflows (`pinyin.cpp:2565-2576`). A
    /// library with no sub-index answers `ERROR_NO_SUB_PHRASE_INDEX`, not the
    /// overflow.
    #[test]
    fn choosing_a_row_of_an_unloaded_library_under_the_sort_bit_answers_one() {
        let open = Open::new("sort-bit-unloaded");
        let (row, _) = open.gbk_row(DEFAULT_SORT | WITHOUT_SENTENCE);
        assert!(pinyin_unload_phrase_library(open.context, 2));

        assert_eq!(pinyin_choose_candidate(open.instance, 0, row), 1);
    }

    /// `ERROR_INTEGER_OVERFLOW` is the one error the leg answers `false` for.
    #[test]
    fn choosing_a_row_whose_unigram_add_overflows_under_the_sort_bit_answers_zero() {
        let open = Open::new("sort-bit-overflow");
        let (row, gbk) = open.gbk_row(DEFAULT_SORT | WITHOUT_SENTENCE);
        let total = open.loaded_total(2);
        assert!(pinyin_token_add_unigram_frequency(
            open.instance,
            gbk,
            u32::MAX - total
        ));

        assert_eq!(pinyin_choose_candidate(open.instance, 0, row), 0);
    }

    /// The LONGER leg (`pinyin.cpp:2523-2531`) answers as the sort-bit leg.
    #[test]
    fn choosing_a_longer_row_whose_unigram_add_overflows_answers_zero() {
        let open = Open::new("longer-overflow");
        assert!(pinyin_guess_sentence(open.instance));
        let (row, token) = open.row_of(IBUS_DEFAULT_SORT, |kind, _| {
            kind == lookup_candidate_type_t::LONGER_CANDIDATE
        });
        let library = u8::try_from(token >> 24).expect("a library nibble");
        let total = open.loaded_total(library);
        assert!(pinyin_token_add_unigram_frequency(
            open.instance,
            token,
            u32::MAX - total
        ));

        assert_eq!(pinyin_choose_candidate(open.instance, 0, row), 0);
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
