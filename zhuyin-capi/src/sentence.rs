//! Sentence guessing, text retrieval, and candidate generation.
//!
//! [`zhuyin_guess_sentence`] and [`zhuyin_guess_sentence_with_prefix`]
//! decode the parsed input. [`zhuyin_get_sentence`] retrieves sentence text,
//! and [`zhuyin_get_character_offset`] maps a cursor to a character offset.
//! [`zhuyin_guess_candidates_before_cursor`] and
//! [`zhuyin_guess_candidates_after_cursor`] build candidate lists.
//! Inspect and select their rows through [`crate::candidates`].

use std::os::raw::c_char;

use oxpinyin::facade::CharacterOffset;

use crate::ffi::{borrow_or_return, write_if_nonnull};
use crate::state::{instance_mut, instance_ref};
use crate::types::ZhuyinInstance;

/// Guess a sentence from saved pinyin keys.
///
/// # C signature
/// ```c
/// bool zhuyin_guess_sentence(zhuyin_instance_t * instance);
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
pub unsafe extern "C" fn zhuyin_guess_sentence(instance: *mut ZhuyinInstance) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    let matrix_has_keys = matrix_has_keys(&inst.read());
    let result = inst.core.guess_sentence_with_matrix(matrix_has_keys);
    oxpinyin::facade::guess_result(result, "zhuyin_guess_sentence", crate::ffi::log_warning)
}

/// Logs one warning for a lookup that would read past a predecessor's system
/// or user bigram value. Returns `true` when it logs the warning.
pub(crate) fn refuse_lookup_gram(name: &str, error: &oxpinyin::facade::EngineError) -> bool {
    oxpinyin::facade::refuse_lookup_gram(name, error, crate::ffi::log_warning)
}

fn matrix_has_keys(inst: &crate::state::CapiInstanceRead<'_>) -> bool {
    if inst.core.parsed_len == 0 {
        return false;
    }
    if let Some(parse) = inst.core.zhuyin_parse() {
        return !parse.keys().is_empty();
    }
    if let Some(parse) = inst.core.double_parse() {
        return !parse.keys().is_empty();
    }
    if let Some(parse) = inst.core.full_parse() {
        return !parse.keys().is_empty();
    }
    // Hanyu's only consumed bytes without keys are apostrophes
    // (074a2219, storage/pinyin_parser2.cpp:238-250,360-378).
    // Inspect the consumed prefix without rebuilding the parse matrix.
    inst.core
        .session
        .raw_input()
        .bytes()
        .take(inst.core.parsed_len)
        .any(|byte| byte != b'\'')
}

/// Guess a sentence seeded with prefix tokens.
///
/// # C signature
/// ```c
/// bool zhuyin_guess_sentence_with_prefix(zhuyin_instance_t * instance,
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
pub unsafe extern "C" fn zhuyin_guess_sentence_with_prefix(
    instance: *mut ZhuyinInstance,
    prefix: *const c_char,
) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    // `m_prefixes` becomes `[sentence_start]` and the prefix adds what it
    // converts to (`zhuyin.cpp:949-953`); `None` adds nothing.
    let prefix = oxpinyin::facade::prefix_text(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(prefix) },
    );
    // `_compute_prefixes` runs first, whatever the matrix holds, and reduces
    // each tail substring's search with the validating `reduce_tokens`
    // (`zhuyin.cpp:933`, `:953`), which asserts on more than four tokens
    // (`phrase_large_table3.h:95`).
    let Ok(prefixes) = oxpinyin::facade::compute_prefixes(&inst.core.dict, prefix.as_ref()) else {
        std::process::abort();
    };
    let matrix_has_keys = matrix_has_keys(&inst.read());
    let result = inst
        .core
        .guess_sentence_with_prefix_and_matrix(&prefixes, matrix_has_keys);
    oxpinyin::facade::guess_result(
        result,
        "zhuyin_guess_sentence_with_prefix",
        crate::ffi::log_warning,
    )
}

/// Get the decoded text of the top-ranked sentence.
///
/// # C signature
/// ```c
/// bool zhuyin_get_sentence(zhuyin_instance_t * instance,
///                          char ** sentence);
/// ```
///
/// A null `instance` or a missing top-ranked row returns `false` without
/// writing. The pin checks for empty results and reads row zero
/// (`zhuyin.cpp:988-992`).
///
/// If the row exists, the function returns `true`. A null `sentence`
/// skips the write. Otherwise, the function writes the decoded string to
/// `*sentence`, or null if no phrase converts. The caller frees a non-null
/// string with `g_free` (`zhuyin.cpp:982-999`, `lookup/lookup.cpp:27-70`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `sentence` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_get_sentence(
    instance: *mut ZhuyinInstance,
    sentence: *mut *mut c_char,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // The pin answers from `m_nbest_results` alone: with no row — no
    // `zhuyin_guess_sentence` since the last reset — it returns `false` and
    // leaves `*sentence` untouched (`zhuyin.cpp:988-989`). The raw preedit
    // is never an answer.
    if !inst.core.sentence_lookup_active() {
        return false;
    }
    match inst.core.sentence_phrases(0) {
        Some(phrases) => {
            // The allocator's result, NULL where the pin's would be; the
            // answer is `true` (`lookup.cpp:69`).
            // SAFETY: The entry point guarantees Out-parameters: a non-NULL sentence destination is aligned
            // and writable without conflicting with live borrows, including the phrase inputs. Its
            // contents need not be initialized.
            write_if_nonnull!(sentence, crate::ffi::sentence_text(phrases));
            true
        }
        None => false,
    }
}

// zhuyin.cpp:2148-2196, zhuyin.cpp:2110.
/// Get the character offset within `phrase` for a lookup byte offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_character_offset(zhuyin_instance_t * instance,
///                                  const char * phrase,
///                                  size_t offset, size_t * length);
/// ```
///
/// Each key must match a reading of the next phrase character. A separator or terminal zero
/// key matches a zero-initial reading. When a matching key ends past `offset`, return true
/// and write the number of characters consumed before that key. Otherwise, consume the
/// character and continue along the path.
///
/// Early failure, including an empty matrix or a phrase character without a token, leaves
/// `length` untouched. A walk that finds no matching path writes zero and returns false.
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
pub unsafe extern "C" fn zhuyin_get_character_offset(
    instance: *mut ZhuyinInstance,
    phrase: *const c_char,
    offset: usize,
    length: *mut usize,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // `assert(offset < matrix.size())` follows the empty-matrix `false`,
    // before the phrase is converted.
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
        .character_offset(&text, offset, oxpinyin::facade::Law::Zhuyin)
    {
        Ok(CharacterOffset::Found(char_count)) => char_count,
        // The walk's own `false` still stores the length it reached
        // (`*plength = length`, `zhuyin.cpp:2192`); the earlier returns (no
        // keys, empty phrase, a character without a token) leave it
        // untouched.
        Ok(CharacterOffset::NoPath) => {
            write_if_nonnull!(length, 0);
            return false;
        }
        Ok(CharacterOffset::Refused) => return false,
        // Register row 19: the recursion's `assert(size > 0)`
        // (`zhuyin.cpp:2110`), the entry's `assert(offset <
        // matrix.size())` (`:2158`) and `_check_offset` (`:1453` via
        // `:2159`); and `_pre_compute_tokens`' `reduce_tokens` count
        // assert (`:2080`, `phrase_large_table3.h:95`).
        Err(_) => std::process::abort(),
    };
    write_if_nonnull!(length, char_count);
    true
}

/// Guess candidates at the after-cursor offset.
///
/// # C signature
/// ```c
/// bool zhuyin_guess_candidates_after_cursor(zhuyin_instance_t * instance,
///                                            size_t offset);
/// ```
///
/// Replace the instance's candidate list, including on a keyless refusal. Returns false when
/// lookup cannot proceed. True can mean an empty list. Inside a chewing key the lookup can
/// publish only sentence rows.
///
/// # Aborts
///
/// An offset that fails the matrix offset assertion aborts (`zhuyin.cpp:1453`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_guess_candidates_after_cursor(
    instance: *mut ZhuyinInstance,
    offset: usize,
) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    guess_candidates(&mut inst, offset, false)
}

/// Guess candidates at the before-cursor offset.
///
/// # C signature
/// ```c
/// bool zhuyin_guess_candidates_before_cursor(zhuyin_instance_t * instance,
///                                             size_t offset);
/// ```
///
/// Replace the instance's candidate list, including on a keyless refusal. Returns false when
/// lookup cannot proceed. True can mean an empty list. Only spans ending at the normalized
/// cursor qualify.
///
/// # Aborts
///
/// An offset that fails the matrix offset assertion aborts (`zhuyin.cpp:1453`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_guess_candidates_before_cursor(
    instance: *mut ZhuyinInstance,
    offset: usize,
) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    guess_candidates(&mut inst, offset, true)
}

#[derive(Clone, Copy)]
enum CandidateGuessRefusal {
    Lookup(oxpinyin::facade::EngineError),
}

/// Emit a refused lookup's diagnostic after its instance view is gone.
fn candidate_guess_answer(result: Result<bool, CandidateGuessRefusal>) -> bool {
    match result {
        Ok(answer) => answer,
        Err(CandidateGuessRefusal::Lookup(error)) => {
            refuse_lookup_gram("zhuyin_guess_candidates", &error);
            false
        }
    }
}

/// The shared candidate-build shell over the engine's `candidates_at` /
/// `candidates_ending_at` / cached candidate list.
fn guess_candidates(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    offset: usize,
    before_cursor: bool,
) -> bool {
    let outcome = guess_candidates_result(inst, offset, before_cursor);
    candidate_guess_answer(outcome)
}

/// The candidate window of one guess, searched on this call: the spans
/// ending at `session_offset` before the cursor, the spans starting at it
/// after the cursor. A refused lookup clears the published rows; an error
/// the pin asserts on aborts.
fn lookup_window(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    session_offset: usize,
    before_cursor: bool,
    abort_on_rank: bool,
) -> Result<oxpinyin::facade::CandidateList, CandidateGuessRefusal> {
    let looked_up = if before_cursor {
        inst.core
            .session
            .candidates_ending_at_for_abi(session_offset, abort_on_rank)
    } else {
        inst.core
            .session
            .candidates_at_for_abi(session_offset, abort_on_rank)
    };
    let window = match looked_up {
        Ok(window) => window,
        // The engine terminates where the pin's toned-initial and
        // short-total asserts fire during the guess; the Kyoto Cabinet
        // read that fails after its size check still arrives here.
        Err(oxpinyin::facade::EngineError::ReadAfterCheck) => std::process::abort(),
        Err(
            error @ (oxpinyin::facade::EngineError::LookupOffsetOutOfRange { .. }
            | oxpinyin::facade::EngineError::Failure
            | oxpinyin::facade::EngineError::ZeroKeyOffsetCheck
            | oxpinyin::facade::EngineError::PinAssert
            | oxpinyin::facade::EngineError::UnsafeTrainingPredecessor
            | oxpinyin::facade::EngineError::TrainingAbort
            | oxpinyin::facade::EngineError::UnsafeLookupPredecessor
            | oxpinyin::facade::EngineError::UserModel),
        ) => {
            inst.state.candidates.clear();
            return Err(CandidateGuessRefusal::Lookup(error));
        }
    };
    Ok(window)
}

fn guess_candidates_result(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    offset: usize,
    before_cursor: bool,
) -> Result<bool, CandidateGuessRefusal> {
    // Both candidate exports free their rows before any check, leaving
    // sentence results intact (074a2219, zhuyin.cpp:1468-1471,1548-1551),
    // so every refusal below leaves an empty list.
    inst.state.candidates.clear();
    if !matrix_has_keys(&inst.read()) {
        return Ok(false);
    }
    if inst.core.session.set_options(inst.core.options()).is_err() {
        return Ok(false);
    }
    if !inst.core.session.is_composing() {
        return Ok(false);
    }
    // `zhuyin.cpp`'s `_check_offset` asserts `zero_key != key` on the column
    // before the offset (`:1441-1456`, via `:1495` and `:1558`), and
    // `get_column_size` asserts past the matrix (`phonetic_key_matrix.h:103`;
    // register row 66).
    let normalized = inst
        .core
        .abi_candidate_offset(offset, oxpinyin::facade::Law::Zhuyin)
        .unwrap_or_else(|_| std::process::abort());
    // The before-cursor entry searches the spans ENDING at the offset
    // (the engine's backward-anchored window builder, the pin's
    // `search_matrix` walk over `(start, offset)`); the after-cursor
    // entry searches the span STARTING at it. Offsets cross the seam in
    // the session's `'`-joined raw-buffer coordinates: the original
    // zhuyin offset maps through `zhuyin_lookup_session_offset`, whose
    // terminal case answers the buffer's one-past-end (the pin's
    // reserved slot). Every after-cursor guess searches again, even at
    // the composition offset: unload may have changed library visibility
    // without a parse (074a2219 zhuyin.cpp:1468-1512).
    let session_offset = if let Some(parse) = inst.core.zhuyin_parse() {
        oxpinyin::facade::zhuyin_lookup_session_offset(
            parse,
            inst.core.session.raw_input().len(),
            normalized,
            before_cursor,
        )
    } else {
        normalized
    };
    // An after-cursor lookup strictly inside a key: the pin's matrix column
    // there is empty (keys sit on their key rests' `m_raw_begin` only), so
    // every `search_matrix` answers nothing and the list is the prepended
    // sentence rows alone (`zhuyin.cpp:1498-1512`, `:1624-1626`). The
    // mapped column is the containing key's, which would answer that key's
    // window instead (issue #577); keep the window anchored there for a
    // later choose, but snapshot only its sentence rows.
    let sentence_rows_only = !before_cursor
        && inst
            .core
            .zhuyin_parse()
            .is_some_and(|parse| oxpinyin::facade::zhuyin_offset_is_mid_key(parse, normalized));
    // The before-cursor window is re-anchored just like the after-cursor
    // one: `snapshot_candidates` records each row's index into THIS list,
    // so a later `zhuyin_choose_candidate` must resolve it here and not
    // against the composition-anchored cached list — the two differ in
    // general, and the caller would commit a row it never displayed. The
    // anchor is the buffer start rather than the lookup offset because the
    // ending-at window is END-anchored; see
    // `oxpinyin::facade::BEFORE_CURSOR_ANCHOR`.
    let searched_here = !sentence_rows_only
        && (!before_cursor
            || inst.core.zhuyin_parse().is_none_or(|parse| {
                oxpinyin::facade::zhuyin_original_offset(parse, session_offset) == normalized
            }));
    // The facade's `guint32` unigram total: the loaded libraries' sum, the
    // user store's deltas and the accepted
    // `zhuyin_token_add_unigram_frequency` deltas, wrapped.
    let abort_on_rank = searched_here && inst.core.lm.amplified_total() == 0;
    let window_owned = lookup_window(inst, session_offset, before_cursor, abort_on_rank)?;
    // Every snapshot row below carries its own span in this window, which a
    // later `zhuyin_choose_candidate` selects; the window's storage then
    // goes back to the session.
    let window_anchor = if before_cursor {
        oxpinyin::facade::BEFORE_CURSOR_ANCHOR
    } else {
        session_offset
    };
    let before_end = if before_cursor {
        Some(normalized)
    } else {
        None
    };
    crate::candidates::snapshot_candidates(
        inst,
        &window_owned,
        before_cursor,
        before_end,
        window_anchor,
        sentence_rows_only,
    );
    inst.core.publish_window(window_owned);
    // The pin answers `true` for a valid lookup into a non-empty matrix
    // even when no candidate spans the offset (074a2219,
    // zhuyin.cpp:1460-1551). The empty matrix was refused above.
    Ok(true)
}

#[cfg(test)]
mod tests;
