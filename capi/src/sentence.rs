//! Sentence decoding, result text, and candidates for parsed input.
//!
//! [`pinyin_guess_candidates`] builds the list. [`crate::candidates`] reads,
//! selects, and trains its rows. [`crate::predict`] builds prediction lists;
//! [`pinyin_guess_predicted_candidates_with_punctuations`] adds punctuation.

use std::os::raw::c_char;

use oxpinyin::facade::CharacterOffset;

use crate::ffi::{borrow_or_return, write_if_nonnull};
use crate::state::{instance_mut, instance_ref};
use crate::types::{GUint, PinyinInstance};

// pinyin.cpp:1372-1387, pinyin.cpp:2295-2296, pinyin.cpp:2700.
/// Guess a sentence from saved pinyin keys.
///
/// # C signature
/// ```c
/// bool pinyin_guess_sentence(pinyin_instance_t * instance);
/// ```
///
/// Decode the current key matrix. Returns whether the lookup found a sentence. A keyless guess
/// returns false and can leave earlier sentence rows available.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_guess_sentence(instance: *mut PinyinInstance) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    // `get_nbest_match` answers `false` before it clears anything when the
    // matrix has no step (`0 == nstep`, `phonetic_lookup.h:743-745`), and
    // `fill_matrix` leaves the matrix empty for a parse without keys
    // (`phonetic_key_matrix.cpp:34-38`): `"'"` and `"!"` consume a byte or
    // none and place nothing.
    let matrix_has_keys = inst.core.mode_has_keys();
    let result = inst.core.guess_sentence_with_matrix(matrix_has_keys);
    oxpinyin::facade::guess_result(result, "pinyin_guess_sentence", crate::ffi::log_warning)
}

/// Logs one warning for a lookup that would read past a predecessor's system
/// or user bigram value. Returns `true` when it logs the warning.
pub(crate) fn refuse_lookup_gram(name: &str, error: &oxpinyin::facade::EngineError) -> bool {
    oxpinyin::facade::refuse_lookup_gram(name, error, crate::ffi::log_warning)
}

/// Whether the facade's `guint32` unigram total is zero: the loaded
/// libraries' sum, the user store's deltas and the accepted
/// `pinyin_token_add_unigram_frequency` deltas, wrapped.
fn facade_total_is_zero(inst: &crate::state::CapiInstanceRead<'_>) -> bool {
    inst.core.lm().amplified_total() == 0
}

// pinyin.cpp:1389-1424, pinyin.cpp:1426-1441.
/// Guess a sentence seeded with prefix tokens.
///
/// # C signature
/// ```c
/// bool pinyin_guess_sentence_with_prefix(pinyin_instance_t * instance,
///                                        const char * prefix);
/// ```
///
/// Decode the current key matrix using the matching suffixes of the supplied prefix as seeds.
/// Returns whether the lookup found a sentence. A keyless guess returns false and can leave
/// earlier sentence rows available.
///
/// # Aborts
///
/// A suffix of the prefix that resolves to more than four tokens aborts (`phrase_large_table3.h:95`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `prefix` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_guess_sentence_with_prefix(
    instance: *mut PinyinInstance,
    prefix: *const c_char,
) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    // `m_prefixes` becomes `[sentence_start]` and the prefix adds what it
    // converts to (`pinyin.cpp:1431-1434`); `None` adds nothing.
    let prefix = oxpinyin::facade::prefix_text(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(prefix) },
    );
    // `_compute_prefixes` reduces each tail substring's search with the
    // validating `reduce_tokens` (`pinyin.cpp:1415`), which asserts on more
    // than four tokens (`phrase_large_table3.h:95`) before the guess.
    let Ok(prefixes) = oxpinyin::facade::compute_prefixes(inst.core.dict(), prefix.as_ref()) else {
        std::process::abort();
    };
    let matrix_has_keys = inst.core.mode_has_keys();
    let result = inst
        .core
        .guess_sentence_with_prefix_and_matrix(&prefixes, matrix_has_keys);
    oxpinyin::facade::guess_result(
        result,
        "pinyin_guess_sentence_with_prefix",
        crate::ffi::log_warning,
    )
}

// pinyin.cpp:2454-2498.
/// Guess predicted candidates with punctuations after a prefix.
///
/// # C signature
/// ```c
/// bool pinyin_guess_predicted_candidates_with_punctuations(
///     pinyin_instance_t * instance, const char * prefix);
/// ```
///
/// Replace the candidate list using phrase-table suffixes of the prefix. Prepend matching
/// punctuation. A prefix with no matching suffix still returns true. A null instance or refused
/// user-bigram lookup returns false. Supply a prefix that remains valid when the old candidate
/// list is cleared (`pinyin.cpp:2420-2426,2459`).
///
/// # Aborts
///
/// A suffix of the prefix that resolves to more than four tokens aborts (`phrase_large_table3.h:95`).
/// Scoring a predicted bigram candidate aborts if the live phrase-index total frequency is zero
/// (`pinyin.cpp:1855-1859,2456-2459`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `prefix` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_guess_predicted_candidates_with_punctuations(
    instance: *mut PinyinInstance,
    prefix: *const c_char,
) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    // The nested prediction's aliased-prefix outcome leaves no prefix, so
    // no punctuation is prepended, and the pin answers `true`
    // (`pinyin.cpp:2459-2498`).
    if crate::predict::clear_borrowed_prefix(&mut inst, prefix) {
        return true;
    }
    let prefix = oxpinyin::facade::prefix_text(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(prefix) },
    );
    let outcome = crate::predict::guess_predicted_with_punctuations(&mut inst, prefix.as_ref());
    crate::predict::prediction_answer(outcome)
}

/// Get the decoded text of n-best row `index`.
///
/// # C signature
/// ```c
/// bool pinyin_get_sentence(pinyin_instance_t * instance,
///                          guint8 index,
///                          char ** sentence);
/// ```
///
/// A null `instance` or an empty n-best result returns `false` without
/// writing. The pin checks for empty results at `pinyin.cpp:1470-1471`.
///
/// If the row exists, the function returns `true`. A null `sentence`
/// skips the write. Otherwise, the function writes the decoded string to
/// `*sentence`, or null if no phrase converts. The caller frees a non-null
/// string with `g_free` (`pinyin.cpp:1463-1482`, `lookup/lookup.cpp:27-70`).
///
/// # Aborts
///
/// The process aborts when `index > 0`, at least one n-best row exists, and
/// no row exists at `index` (`pinyin.cpp:1474`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `sentence` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_sentence(
    instance: *mut PinyinInstance,
    index: u8,
    sentence: *mut *mut c_char,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // The pin answers from `m_nbest_results` alone: with no row — no
    // `pinyin_guess_sentence` since the last reset — it returns `false`
    // and leaves `*sentence` untouched (`pinyin.cpp:1470-1471`); past the
    // rows it asserts (`:1474`). The raw preedit is never an answer.
    if !inst.core.sentence_lookup_active() {
        return false;
    }
    match inst.core.sentence_phrases(index) {
        Some(phrases) => {
            // The allocator's result, NULL where the pin's would be; the
            // answer is `true` (`lookup.cpp:69`).
            // SAFETY: The entry point guarantees Out-parameters: a non-NULL sentence destination is aligned
            // and writable without conflicting with live borrows, including the phrase inputs. Its
            // contents need not be initialized.
            write_if_nonnull!(sentence, crate::ffi::sentence_text(phrases));
            true
        }
        None => {
            // Register row 10, `pinyin.cpp:1474`: with rows present, an
            // index past them is `assert(index < results.size())`, and the
            // process aborts. No rows at all is the pin's own graceful
            // `false` (`:1470-1471`).
            if index > 0 && inst.core.session().has_nbest_rank(0) {
                std::process::abort();
            }
            false
        }
    }
}

// pinyin.cpp:3193-3241, pinyin.cpp:3147.
/// Get the character offset within `phrase` for a lookup byte offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_character_offset(pinyin_instance_t * instance,
///                                  const char * phrase,
///                                  size_t offset,
///                                  size_t * length);
/// ```
///
/// Write the number of characters consumed along a key path through the supplied phrase. Early
/// failure, including an empty matrix or a phrase character without a token, leaves the output
/// untouched. A walk that runs but finds no matching path writes zero and returns false.
///
/// # Aborts
///
/// An offset outside a nonempty matrix, a failed offset or recursion assertion, or a character
/// resolving to more than four tokens aborts.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle, `phrase` is a string
/// and `length` is an out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_character_offset(
    instance: *mut PinyinInstance,
    phrase: *const c_char,
    offset: usize,
    length: *mut usize,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // `assert(offset < matrix.size())` (`pinyin.cpp:3203`) follows the
    // empty-matrix `false`, before the phrase is converted.
    let columns = inst.core.pin_matrix_columns();
    if columns > 0 && offset >= columns {
        std::process::abort();
    }
    let text = oxpinyin::facade::utf8_to_ucs4(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(phrase) },
    )
    .unwrap_or_default();
    let char_count = match inst
        .core
        .character_offset(&text, offset, oxpinyin::facade::Law::Pinyin)
    {
        Ok(CharacterOffset::Found(char_count)) => char_count,
        // The walk's own `false` still stores the length it reached
        // (`*plength = length`, `pinyin.cpp:3238`); the earlier returns (no
        // keys, empty phrase, a character without a token) leave it
        // untouched.
        Ok(CharacterOffset::NoPath) => {
            write_if_nonnull!(length, 0);
            return false;
        }
        Ok(CharacterOffset::Refused) => return false,
        // Register row 19: the pin's recursion and entry asserts
        // (`pinyin.cpp:3147`, `:3161`, `:3203`, `:3204`).
        // `_pre_compute_tokens`' `reduce_tokens` count assert
        // (`pinyin.cpp:3117`, `phrase_large_table3.h:95`).
        Err(_) => std::process::abort(),
    };
    write_if_nonnull!(length, char_count);
    true
}

/// The coordinates of one `pinyin_guess_candidates` call.
#[derive(Clone, Copy)]
pub(crate) struct GuessFrame {
    /// The caller's offset, in the active parse mode's original input
    /// coordinates: a normal row's `m_begin`.
    pub(crate) offset: usize,
    /// The matrix column the lookup searches, in the session's coordinates.
    pub(crate) session_offset: usize,
    /// The lookup is strictly inside a key: only the prepended rows are kept.
    pub(crate) sentence_rows_only: bool,
    /// The facade total is zero and the lookup searches: ranking a searched
    /// row aborts.
    abort_on_rank: bool,
}

impl GuessFrame {
    /// The frame of a lookup at `offset`, which `validated_lookup_offset`
    /// normalized to `normalized`.
    fn of(inst: &crate::state::CapiInstanceMut<'_>, offset: usize, normalized: usize) -> Self {
        // Search on every guess, including the composition offset: library
        // visibility can change without a parse (074a2219 pinyin.cpp:2193,
        // :2224-2262).
        //
        // The caller's offset lives in the active parse mode's ORIGINAL input
        // coordinates — the pin's matrix keeps each key at its key rest's
        // `m_raw_begin` (`phonetic_key_matrix.cpp:52-56`) for every scheme —
        // while the double-pinyin and chewing sessions are driven with the
        // `'`-joined full-pinyin spelling. Those seams map the offset across to
        // the matrix column of the key it starts (`*_session_offset`: 0, or the
        // apostrophe in front of the key, the column a forcing there spells
        // under); plain and index-parsed full pinyin are the identity.
        let session_offset = if let Some(parse) = inst.core.double_parse() {
            oxpinyin::facade::double_session_offset(parse, normalized)
        } else if let Some(parse) = inst.core.zhuyin_parse() {
            oxpinyin::facade::zhuyin_session_offset(parse, normalized)
        } else {
            normalized
        };
        // A lookup strictly inside a key of the active transformed parse: the
        // pin's matrix column there is empty (keys sit on their key rests'
        // `m_raw_begin` only), so the search finds nothing and the list is the
        // prepended sentence rows alone (`pinyin.cpp:2224-2262`, `:2295-2296`).
        // The rows keep their spans at the containing key for a later choose,
        // as in the zhuyin facade (issue #577); only its sentence rows are
        // snapshot.
        let sentence_rows_only = if let Some(parse) = inst.core.double_parse() {
            oxpinyin::facade::double_offset_is_mid_key(parse, normalized)
        } else if let Some(parse) = inst.core.zhuyin_parse() {
            oxpinyin::facade::zhuyin_offset_is_mid_key(parse, normalized)
        } else {
            false
        };
        let abort_on_rank = !sentence_rows_only && facade_total_is_zero(&inst.read());
        Self {
            offset,
            session_offset,
            sentence_rows_only,
            abort_on_rank,
        }
    }
}

/// The candidate window of one guess: the engine's lookup at the frame's
/// column, searched on this call. `None` is a refused lookup, with the
/// published rows cleared; an error the pin asserts on aborts.
fn lookup_window(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    frame: &GuessFrame,
) -> Option<oxpinyin::facade::CandidateList> {
    let window = match inst
        .core
        .session_mut()
        .candidates_at_for_abi(frame.session_offset, frame.abort_on_rank)
    {
        Ok(window) => window,
        // The engine terminates where the pin's own asserts fire during the
        // guess: the over-long suggestion index key (register rows 84 and
        // 95, `chewing_large_table2_bdb.cpp:282`; kc `:269`, tkrzw `:252`),
        // the toned first initial-only key on a searched path
        // (pinyin_phrase3.h:152) and a short system or user gram total
        // (ngram.cpp:303-304, pinyin.cpp:1848).
        // A Kyoto Cabinet body read after its size check asserts at the load
        // (pinyin.cpp:2212, ngram_kyotodb.cpp:138-139).
        Err(oxpinyin::facade::EngineError::ReadAfterCheck) => std::process::abort(),
        // Register row 98: the DYNAMIC_ADJUST merge or read of the previous
        // token's grams reads past a partial value (class (b), one warning;
        // `pinyin.cpp:2214`, `:1848-1849`).
        Err(error @ oxpinyin::facade::EngineError::UnsafeLookupPredecessor) => {
            inst.state.candidates.clear();
            refuse_lookup_gram("pinyin_guess_candidates", &error);
            return None;
        }
        Err(
            oxpinyin::facade::EngineError::LookupOffsetOutOfRange { .. }
            | oxpinyin::facade::EngineError::Failure
            | oxpinyin::facade::EngineError::ZeroKeyOffsetCheck
            | oxpinyin::facade::EngineError::PinAssert
            | oxpinyin::facade::EngineError::UnsafeTrainingPredecessor
            | oxpinyin::facade::EngineError::TrainingAbort
            | oxpinyin::facade::EngineError::UserModel,
        ) => {
            // Unreachable for a well-formed lookup: the offset-shaped
            // contracts are refused by `validate_lookup_offset` and
            // `candidates_at`'s own range/char-boundary checks, and a
            // mid-syllable byte is not an error — the window answers the
            // pin's empty-column law. The arm remains for genuine backend
            // failures during the re-anchored scan.
            inst.state.candidates.clear();
            return None;
        }
    };
    Some(window)
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
/// The caller `offset` may sit one position past the zero-`ChewingKey` `'`
/// separator run (ibus-libpinyin ≥ 1.16.1 passes the raw begin of the next
/// key rest, issue #570). libpinyin@dbff264 normalizes it back to the first
/// byte of that run and validates the normalized offset —
/// [`oxpinyin::facade::InstanceView::validate_lookup_offset`] runs that law in
/// the active parse mode's own coordinates: the full walk where `'` is a
/// zero-key separator (plain full pinyin, Luoma/secondary-zhuyin), the
/// range refusal alone where a composition cannot hold one (double pinyin,
/// the zhuyin keyboards — there `'` is out of scheme or a content symbol).
/// The leading-run shape empties the snapshot and returns `false`.
///
/// The normalized caller offset selects the lookup window for this call.
/// The composition offset stays fixed during the lookup. Pass the original
/// `offset` to [`crate::candidates::pinyin_choose_candidate`] to select a row from this
/// lookup.
///
/// When [`SORT_WITHOUT_SENTENCE_CANDIDATE`](oxpinyin::facade::SORT_WITHOUT_SENTENCE_CANDIDATE)
/// is clear, rows from [`pinyin_guess_sentence`] appear at the head as
/// `NBEST_MATCH_CANDIDATE` entries with their tail rank. Setting the bit
/// skips that prepend (`pinyin.cpp:2295-2296`). A phrase row with the same
/// text as a skipped sentence can then survive deduplication
/// (`pinyin.cpp:2298-2300`, issue #582). Clearing
/// `SORT_WITHOUT_LONGER_CANDIDATE` allows the lookup to prepend a
/// `LONGER_CANDIDATE` row when it finds a longer suggestion
/// (`pinyin.cpp:1870-1932`, `:2292-2293`). The `SORT_BY_*` bits determine
/// list order (`pinyin.cpp:1678-1709`).
///
/// # Aborts
///
/// An offset two or more past the reserved slot aborts at the matrix offset check
/// (`pinyin.cpp:2163–2171`, `phonetic_key_matrix.h:103`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_guess_candidates(
    instance: *mut PinyinInstance,
    offset: usize,
    sort_option: GUint,
) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    // The pin frees the candidates before any check (`pinyin.cpp:2193`),
    // so every `false` below leaves an empty list.
    inst.state.candidates.clear();
    // The keyless check reads the keys of the matrix the last parse
    // filled; the borrow ends before the sort-word store below. The
    // session reads the context's option word (`pinyin.cpp:2189`).
    //
    // The pin answers `false` for a matrix without a step before it
    // stores the sort word (`pinyin.cpp:2195-2198`); a keyless parse is
    // such a matrix.
    if !inst.core.mode_has_keys() {
        return false;
    }
    // §9: the whole sort word, re-stored per call exactly as the pin's
    // `instance->m_sort_option = sort_option` (`pinyin.cpp:2203`) — the
    // engine's sort keys, the LONGER gate and the snapshot's LONGER row
    // are all functions of it. The lookup below searches the live
    // dictionary on every call and applies the over-long-index gate, which
    // the pin reaches only inside `_prepend_longer_candidates`
    // (`pinyin.cpp:2292-2293`).
    inst.core.session_mut().set_sort_options(sort_option);
    if !inst.core.session().is_composing() {
        return false;
    }
    // `_check_offset` reads the column before the offset, unasserted
    // (`pinyin.cpp:2226`), and `get_column_size` asserts past the matrix
    // (`phonetic_key_matrix.h:103`; register row 66). The leading-separator
    // refusal keeps its silent `false`.
    let normalized = match inst
        .core
        .abi_candidate_offset(offset, oxpinyin::facade::Law::Pinyin)
    {
        Ok(normalized) => normalized,
        Err(oxpinyin::facade::CursorFault::PinAssert) => std::process::abort(),
        Err(oxpinyin::facade::CursorFault::Unreadable) => return false,
    };
    let frame = GuessFrame::of(&inst, offset, normalized);
    // Publish each row of the re-anchored window with its own span. A later
    // `pinyin_choose_candidate` selects the row's span, as the pin's reads
    // `m_begin` and `m_end` off the row.
    let Some(window) = lookup_window(&mut inst, &frame) else {
        return false;
    };
    crate::candidates::snapshot_candidates(&mut inst, &window, &frame);
    // Every row now carries its span; the window's storage goes back to
    // the session for the next guess.
    inst.core.publish_window(window);
    // The pin's empty-matrix early return (`pinyin.cpp:2193`): a
    // parse that produced no keys answers false, not an empty list.
    // A non-empty parse with no candidates (apostrophe-only runs,
    // unmatchable tails) answers true with zero rows.
    !(inst.state.candidates.is_empty() && inst.core.parsed_len() == 0)
}

#[cfg(test)]
mod tests;
