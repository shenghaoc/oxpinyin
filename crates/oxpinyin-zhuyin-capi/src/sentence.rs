//! Sentence guessing, retrieval, and the cursor candidate-construction
//! family (`zhuyin_guess_candidates_before_cursor` / `after_cursor`).
//!
//! The candidate-built symbols accumulate into the instance's snapshot; the
//! offset-mapping helpers translate between the original zhuyin input
//! coordinates and the session's `'`-joined full-pinyin buffer.

use std::os::raw::c_char;

use crate::state::{instance_mut, instance_ref};
use crate::types::ZhuyinInstance;

/// Guess a sentence from saved pinyin keys.
///
/// # C signature
/// ```c
/// bool zhuyin_guess_sentence(zhuyin_instance_t * instance);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_guess_sentence(instance: *mut ZhuyinInstance) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    if refuse_empty_matrix(&mut inst) {
        return false;
    }
    match inst.core.session.guess_sentence() {
        Ok(found) => found,
        Err(error) => {
            refuse_lookup_gram("zhuyin_guess_sentence", &error);
            false
        }
    }
}

/// The lookup faults of a predecessor's user bigram. A value too short for
/// its total is the pin's assert in `get_total_freq` (`memory_chunk.h:390`,
/// read by `merge_single_gram`, `ngram.cpp:303-304`, by `SingleGram::search`,
/// `:165`, and by the `DYNAMIC_ADJUST` term, `pinyin.cpp:1848`,
/// `zhuyin.cpp:1250`; register row 85): the process aborts. So does a Kyoto
/// Cabinet read that fails after its size check, the pin's
/// `check_result(vsiz == m_db->get(...))` in `Bigram::load`
/// (`ngram_kyotodb.cpp:136-137`). A read past the value (`ngram.cpp:241`,
/// `:167`, `:318-349`; register row 98, class (b)) logs the one warning, and
/// the call answers `true` when `error` is that read.
pub(crate) fn refuse_lookup_gram(name: &str, error: &oxpinyin_engine::EngineError) -> bool {
    let message = match error {
        // Register row 4, `pinyin_phrase3.h:152`: a table search of the
        // guess met a key path whose first initial-only key carries a tone.
        oxpinyin_engine::EngineError::TonedIncompleteKey => std::process::abort(),
        oxpinyin_engine::EngineError::ShortLookupPredecessor { .. }
        | oxpinyin_engine::EngineError::ReadAfterCheck { .. } => std::process::abort(),
        oxpinyin_engine::EngineError::UnsafeLookupPredecessor { .. } => {
            "a user bigram value would be read past its end (upstream reads past it)"
        }
        oxpinyin_engine::EngineError::CandidateIndexOutOfRange { .. }
        | oxpinyin_engine::EngineError::LookupOffsetPastSeparator { .. }
        | oxpinyin_engine::EngineError::LookupOffsetOutOfRange { .. }
        | oxpinyin_engine::EngineError::LookupOffsetInsideCharacter { .. }
        | oxpinyin_engine::EngineError::SelectionAnchorBeforeComposition { .. }
        | oxpinyin_engine::EngineError::ZeroKeyOffsetCheck { .. }
        | oxpinyin_engine::EngineError::MatrixColumnAssert { .. }
        | oxpinyin_engine::EngineError::StaleTrainingConstraint { .. }
        | oxpinyin_engine::EngineError::UnsafeTrainingPredecessor { .. }
        | oxpinyin_engine::EngineError::ShortTrainingPredecessor { .. }
        | oxpinyin_engine::EngineError::TrainingSetFreqMiss { .. }
        | oxpinyin_engine::EngineError::FailedUserBigramStore { .. }
        | oxpinyin_engine::EngineError::StaleTrainingSpan { .. }
        | oxpinyin_engine::EngineError::OverlongUserIndexKey { .. }
        | oxpinyin_engine::EngineError::ReduceTokensCount { .. }
        | oxpinyin_engine::EngineError::UserModel(_)
        | oxpinyin_engine::EngineError::Graph(_)
        | oxpinyin_engine::EngineError::Scoring(_) => return false,
    };
    crate::ffi::log_warning(&format!("{name}: {message}"));
    true
}

// fill_matrix leaves an empty matrix whenever the parser places no keys,
// even when separators consume bytes (074a2219, phonetic_key_matrix.cpp:34-38).
// Validate against it, but keep the previous sentence rows: get_nbest_match
// returns false before results->clear (074a2219, phonetic_lookup.h:743-748;
// zhuyin.cpp:902-907). The same rule applies to the prefix-seeded lookup.
fn refuse_empty_matrix(inst: &mut crate::state::CapiInstanceMut<'_>) -> bool {
    if matrix_has_keys(&inst.read()) {
        return false;
    }
    inst.core.session.discard_composition();
    true
}

fn matrix_has_keys(inst: &crate::state::CapiInstanceRead<'_>) -> bool {
    if inst.core.parsed_len == 0 {
        return false;
    }
    if let Some(parse) = &inst.core.zhuyin_parse {
        return !parse.keys().is_empty();
    }
    if let Some(parse) = &inst.core.double_parse {
        return !parse.keys().is_empty();
    }
    if let Some(parse) = &inst.core.full_parse {
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
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_guess_sentence_with_prefix(
    instance: *mut ZhuyinInstance,
    prefix: *const c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    // `m_prefixes` becomes `[sentence_start]` and the prefix adds what it
    // converts to (`zhuyin.cpp:949-953`); `None` adds nothing.
    let prefix =
        oxpinyin_capi_marshal::ucs4::prefix_text(oxpinyin_capi_marshal::borrow_c_string!(prefix));
    // `_compute_prefixes` runs first, whatever the matrix holds, and reduces
    // each tail substring's search with the validating `reduce_tokens`
    // (`zhuyin.cpp:933`, `:953`), which asserts on more than four tokens
    // (`phrase_large_table3.h:95`).
    let Ok(prefixes) =
        oxpinyin_facade::compute_prefixes(&inst.core.dict, inst.core.user, prefix.as_ref())
    else {
        std::process::abort();
    };
    if refuse_empty_matrix(&mut inst) {
        return false;
    }
    let prefix_tokens: Vec<oxpinyin_core::PhraseToken> = prefixes
        .iter()
        .map(|&token| oxpinyin_core::PhraseToken::new(token))
        .collect();
    match inst.core.session.guess_sentence_with_prefix(&prefix_tokens) {
        Ok(found) => found,
        Err(error) => {
            refuse_lookup_gram("zhuyin_guess_sentence_with_prefix", &error);
            false
        }
    }
}

/// Get a sentence string from the instance.
///
/// # C signature
/// ```c
/// bool zhuyin_get_sentence(zhuyin_instance_t * instance,
///                          char ** sentence);
/// ```
///
/// Out-param `sentence` is caller-owned (`g_free`). Decoded-or-nothing, as
/// the pin: the 1-best row's text while a sentence lookup is active, `false`
/// with `*sentence` untouched otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_sentence(
    instance: *mut ZhuyinInstance,
    sentence: *mut *mut c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // The pin answers from `m_nbest_results` alone: with no row — no
    // `zhuyin_guess_sentence` since the last reset — it returns `false` and
    // leaves `*sentence` untouched (`zhuyin.cpp:988-989`). The raw preedit
    // is never an answer.
    if !inst.core.session.sentence_lookup_active() {
        return false;
    }
    match inst.core.session.sentence_phrases(0) {
        Some(phrases) => write_owned_sentence(phrases, sentence),
        None => false,
    }
}

// The `char **`-out sentence writer, stamped from the shared marshalling
// macro (byte-identical to the pinyin facade's). `crate::ffi::sentence_text`
// makes the pin's `convert_to_utf8` calls — the per-facade allocator edge.
oxpinyin_capi_marshal::write_owned_sentence!(crate::ffi::sentence_text);

/// Get the character offset within `phrase` for a lookup byte offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_character_offset(zhuyin_instance_t * instance,
///                                  const char * phrase,
///                                  size_t offset, size_t * length);
/// ```
///
/// The pin (`zhuyin.cpp:2148-2196` at the pin) searches every character of `phrase`
/// in the phrase table and walks the matrix from column 0, consuming one
/// character per key the character's item pronounces, until the next
/// key's raw end lies past `offset`; the out-param is the characters
/// consumed. `false` — with the out-param untouched, as upstream leaves
/// it — for an empty matrix, a NULL, empty or non-UTF-8 phrase, a
/// character with no dictionary token (issue #356: the pinyin string
/// passed as the phrase — both pinned oracles answer `false`), or a walk
/// no key path satisfies. Where the pin asserts (`zhuyin.cpp:2110`,
/// `:2158`, and `:1453` through `:2159`) the process aborts (register row
/// 19).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_character_offset(
    instance: *mut ZhuyinInstance,
    phrase: *const c_char,
    offset: usize,
    length: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by the facade's
    // alloc entry point.
    let inst = unsafe { instance_ref(instance) };
    // `assert(offset < matrix.size())` follows the empty-matrix `false`,
    // before the phrase is converted.
    if let Ok(columns) = inst.core.pin_matrix_columns()
        && columns > 0
        && offset >= columns
    {
        std::process::abort();
    }
    // SAFETY: Null-checked inside; invalid UTF-8 refuses like the pin's
    // `g_utf8_to_ucs4` NULL answer.
    let text =
        oxpinyin_capi_marshal::input::utf8_to_ucs4(oxpinyin_capi_marshal::borrow_c_string!(phrase))
            .unwrap_or_default();
    let char_count = match inst.core.character_offset(&text, offset) {
        // Pin behaviour: the zhuyin `_get_char_offset_recur` has no zero-key
        // branch, so at `offset == parsed_len` the walk reaches the last
        // column's zero key (`zhuyin.cpp:2108-2117`), reads `cached_tokens`
        // at index `phrase_length` (the array's zero terminator, token 0,
        // `:2122-2123`), and the empty item fails the pronunciation test
        // (`:2124-2128`): the call answers `false` with `*plength = 0`.
        Ok((Some(_), true)) if offset == inst.core.parsed_len => {
            if !length.is_null() {
                // SAFETY: Null-checked above; caller supplied the output.
                unsafe {
                    *length = 0;
                }
            }
            return false;
        }
        Ok((Some(char_count), _)) => char_count,
        Ok((None, walked)) => {
            // The walk's own `false` still stores the length it reached
            // (`*plength = length`, `zhuyin.cpp:2192`); the earlier returns
            // (no keys, empty phrase, a character without a token) leave it
            // untouched.
            if walked && !length.is_null() {
                // SAFETY: Null-checked above.
                unsafe {
                    *length = 0;
                }
            }
            return false;
        }
        // Register row 19: the recursion's `assert(size > 0)`
        // (`zhuyin.cpp:2110`), the entry's `assert(offset < matrix.size())`
        // (`:2158`) and `_check_offset` (`:1453` via `:2159`); and
        // `_pre_compute_tokens`' `reduce_tokens` count assert (`:2080`,
        // `phrase_large_table3.h:95`).
        Err(
            oxpinyin_engine::EngineError::MatrixColumnAssert { .. }
            | oxpinyin_engine::EngineError::LookupOffsetOutOfRange { .. }
            | oxpinyin_engine::EngineError::ZeroKeyOffsetCheck { .. }
            | oxpinyin_engine::EngineError::ReduceTokensCount { .. },
        ) => std::process::abort(),
        // A graph or dictionary failure the pin has no counterpart for.
        Err(_) => {
            crate::ffi::log_warning("zhuyin_get_character_offset: the lookup failed");
            return false;
        }
    };
    if !length.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *length = char_count;
        }
    }
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
/// The zhuyin equivalent of `pinyin_guess_candidates` at an offset, from the
/// first key past the cursor onward. Uses the zhuyin enum's
/// `NORMAL_CANDIDATE_AFTER_CURSOR` tag.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_guess_candidates_after_cursor(
    instance: *mut ZhuyinInstance,
    offset: usize,
) -> bool {
    guess_candidates(instance, offset, false)
}

/// Guess candidates at the before-cursor offset.
///
/// # C signature
/// ```c
/// bool zhuyin_guess_candidates_before_cursor(zhuyin_instance_t * instance,
///                                             size_t offset);
/// ```
///
/// The span ending at the cursor, tagged `NORMAL_CANDIDATE_BEFORE_CURSOR`.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_guess_candidates_before_cursor(
    instance: *mut ZhuyinInstance,
    offset: usize,
) -> bool {
    guess_candidates(instance, offset, true)
}

/// The normalized lookup offset, or `None` where the lookup is refused.
///
/// The pin's matrix holds `parsed_len + 1` columns, the last a reserved slot
/// holding a lone zero key, and `zhuyin.cpp`'s `_check_offset` asserts
/// `zero_key != key` on the column before the offset (`:1441-1456`, via
/// `:1495` and `:1558`): an offset just past a separator or one past the
/// reserved slot aborts there, and any further out aborts in that read's
/// `get_column_size` (`phonetic_key_matrix.h:103`). So does this facade
/// (register row 66).
fn validated_lookup_offset(
    inst: &crate::state::CapiInstanceRead<'_>,
    offset: usize,
) -> Option<usize> {
    // The matrix has `parsed_len + 1` columns, fewer than the input has bytes
    // when the parse stopped early; `_check_offset` reads column
    // `offset - 1`, and past the last column `get_column_size` asserts.
    if let Ok(columns) = inst.core.pin_matrix_columns()
        && offset > columns
    {
        std::process::abort();
    }
    match inst.core.validate_abi_lookup_offset(offset, true) {
        Ok(normalized) => Some(normalized),
        Err(
            oxpinyin_engine::EngineError::LookupOffsetOutOfRange { .. }
            | oxpinyin_engine::EngineError::ZeroKeyOffsetCheck { .. },
        ) => std::process::abort(),
        // A parse the facade cannot read, which the pin has no counterpart
        // for.
        Err(_) => {
            crate::ffi::log_warning("zhuyin_guess_candidates: the lookup failed");
            None
        }
    }
}

/// Whether the facade's `guint32` unigram total is zero: the loaded
/// libraries' sum, the user store's deltas and the accepted
/// `zhuyin_token_add_unigram_frequency` deltas, wrapped.
fn facade_total_is_zero(inst: &crate::state::CapiInstanceRead<'_>) -> bool {
    inst.core.lm.amplified_total() == 0
}

/// The shared candidate-build shell over the engine's `candidates_at` /
/// `candidates_ending_at` / cached candidate list.
fn guess_candidates(instance: *mut ZhuyinInstance, offset: usize, before_cursor: bool) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    // Both candidate exports free their rows before the empty-matrix
    // refusal, leaving sentence results intact (074a2219,
    // zhuyin.cpp:1468-1471,1548-1551).
    if !matrix_has_keys(&inst.read()) {
        inst.state.candidates.clear();
        return false;
    }
    if inst.core.session.set_options(inst.core.options()).is_err() {
        return false;
    }
    if !inst.core.session.is_composing() {
        return false;
    }
    let Some(normalized) = validated_lookup_offset(&inst.read(), offset) else {
        inst.state.candidates.clear();
        return false;
    };
    inst.state.candidates.clear();
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
    let session_offset = if let Some(parse) = inst.core.zhuyin_parse.as_ref() {
        oxpinyin_facade::zhuyin_lookup_session_offset(
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
            .zhuyin_parse
            .as_ref()
            .is_some_and(|parse| oxpinyin_facade::zhuyin_offset_is_mid_key(parse, normalized));
    // The before-cursor window is re-anchored just like the after-cursor
    // one: `snapshot_candidates` records each row's index into THIS list,
    // so a later `zhuyin_choose_candidate` must resolve it here and not
    // against the composition-anchored cached list — the two differ in
    // general, and the caller would commit a row it never displayed. The
    // anchor is the buffer start rather than the lookup offset because the
    // ending-at window is END-anchored; see
    // `oxpinyin_facade::BEFORE_CURSOR_ANCHOR`.
    let looked_up = if before_cursor {
        inst.core.session.candidates_ending_at(session_offset)
    } else {
        inst.core.session.candidates_at(session_offset)
    };
    let window_owned: oxpinyin_engine::CandidateList = match looked_up {
        Ok(window) => window,
        Err(error) => {
            refuse_lookup_gram("zhuyin_guess_candidates", &error);
            inst.core.anchored_window = None;
            inst.state.candidates.clear();
            return false;
        }
    };
    // The window is retained on the instance after the snapshot below
    // (moved, not cloned): the snapshot borrows it, and the retained list
    // is the one a later `zhuyin_choose_candidate` resolves against.
    let window_anchor = if before_cursor {
        oxpinyin_facade::BEFORE_CURSOR_ANCHOR
    } else {
        session_offset
    };
    // Register rows 61 and 66, `zhuyin.cpp:1261`: `assert (0 < total_freq)`
    // while ranking a searched row, with the facade total wrapped to zero by
    // `zhuyin_token_add_unigram_frequency`. libzhuyin's
    // `_compute_frequency_of_items` skips no row type and runs on the freshly
    // searched rows (`:1526`; per `len` slice before the cursor, `:1607`),
    // before the sentence prepend and the phrase-string dedup (`:1533-1537`,
    // `:1626-1630`): the engine reports whether that list held a row, so a
    // row the dedup removes still counts. A lookup strictly inside a key
    // searches nothing at the pin, and so does a before-cursor lookup at an
    // offset no key ends at (the composition start, inside a key): the pin's
    // spans end on that offset's empty column, while the session window,
    // whose rows all end at `session_offset`, answers the spans ending
    // elsewhere. The process aborts there, before the phrase strings are
    // read (`:1535`, `:1628`).
    let searched_here = !sentence_rows_only
        && (!before_cursor
            || inst.core.zhuyin_parse.as_ref().is_none_or(|parse| {
                oxpinyin_facade::zhuyin_original_offset(parse, session_offset) == normalized
            }));
    if facade_total_is_zero(&inst.read()) && searched_here && window_owned.ranked_phrase_row() {
        std::process::abort();
    }
    // Class (b), `zhuyin.cpp:1096`: a token of a loaded library whose item
    // cannot be read leaves `m_phrase_string` unset, and the pin's duplicate
    // removal compares it (SIGSEGV). Only a `table.conf` that points a
    // library at another file (or at none) gets here. The call fails, once.
    if window_owned.had_unreadable_item() {
        inst.core.anchored_window = None;
        inst.state.candidates.clear();
        crate::ffi::log_warning(
            "zhuyin_guess_candidates: a candidate's phrase item cannot be read",
        );
        return false;
    }
    let before_end = if before_cursor {
        Some(normalized)
    } else {
        None
    };
    crate::candidates::snapshot_candidates(
        &mut inst,
        &window_owned,
        before_cursor,
        before_end,
        window_anchor,
        sentence_rows_only,
    );
    inst.core
        .replace_anchored_window(window_anchor, window_owned);
    // The pin answers `true` for a valid lookup into a non-empty matrix
    // even when no candidate spans the offset (074a2219,
    // zhuyin.cpp:1460-1551). The empty matrix was refused above.
    true
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::{zhuyin_guess_candidates_before_cursor, zhuyin_guess_sentence};
    use crate::candidates::{
        zhuyin_get_candidate, zhuyin_get_candidate_type, zhuyin_get_n_candidate,
    };
    use crate::parse::zhuyin_parse_more_chewings;
    use crate::test_support::{close, cstr, open};
    use crate::types::{LookupCandidate, lookup_candidate_type_t};

    /// Every row's type in the current list.
    fn kinds(instance: *mut crate::types::ZhuyinInstance) -> Vec<lookup_candidate_type_t> {
        let mut count = 0;
        assert!(zhuyin_get_n_candidate(instance, &raw mut count));
        (0..count)
            .map(|index| {
                let mut cand: *mut LookupCandidate = ptr::null_mut();
                assert!(zhuyin_get_candidate(instance, index, &raw mut cand));
                let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
                assert!(zhuyin_get_candidate_type(instance, cand, &raw mut kind));
                kind
            })
            .collect()
    }

    /// S6, zhuyin-diff's noguess protocol: no sentence guess since the parse, so the n-best results
    /// are empty and nothing is prepended (`zhuyin.cpp:1277-1278` at 074a2219). Before offset 0 the
    /// span loop does not run (`:1563`); inside a key the span is skipped (`:1590-1591`;
    /// `phonetic_key_matrix.cpp:422-423`). A toneless `su` places no key under FORCE_TONE
    /// (`zhuyin_parser2.cpp:178-179`), so the empty matrix answers false (`zhuyin.cpp:1549-1552`).
    /// Pin capture of 2026-11-07, runs 1 and 2 identical, observations
    /// `zhuyin-noguess-before/su3cl3@0,1,2,4,5` and `zhuyin-noguess-before/su@0`.
    #[test]
    fn an_unguessed_before_cursor_window_with_no_span_is_empty() {
        for (keys, consumed, offset, answer) in [
            ("su3cl3", 6, 0, true),
            ("su3cl3", 6, 1, true),
            ("su3cl3", 6, 2, true),
            ("su3cl3", 6, 4, true),
            ("su3cl3", 6, 5, true),
            ("su", 0, 0, false),
        ] {
            let (context, instance) = open();
            let text = cstr(keys);
            assert_eq!(
                zhuyin_parse_more_chewings(instance, text.as_ptr()),
                consumed
            );
            assert_eq!(
                zhuyin_guess_candidates_before_cursor(instance, offset),
                answer,
                "{keys}@{offset}"
            );
            assert_eq!(kinds(instance), [], "{keys}@{offset}");
            close(context, instance);
        }
    }

    /// S6: at a key end every unguessed row is a phrase of the spans ending there,
    /// NORMAL_CANDIDATE_BEFORE_CURSOR (`zhuyin.cpp:1594`); after a guess the sentence is
    /// prepended as the one BEST_MATCH row (`:1283`, `:1626`), and before offset 0 it is the only
    /// row. Pin capture of 2026-11-07, runs 1 and 2 identical, observations
    /// `zhuyin-noguess-before/su3cl3@3,6` and `zhuyin-guess-before/su3cl3@0,3,6`; the row counts
    /// are the pinned data's and are not asserted.
    #[test]
    fn only_a_guess_puts_a_sentence_row_in_the_before_cursor_window() {
        for offset in [3, 6] {
            let (context, instance) = open();
            let text = cstr("su3cl3");
            assert_eq!(zhuyin_parse_more_chewings(instance, text.as_ptr()), 6);
            assert!(zhuyin_guess_candidates_before_cursor(instance, offset));
            let unguessed = kinds(instance);
            assert!(!unguessed.is_empty());
            assert!(
                unguessed
                    .iter()
                    .all(|kind| *kind == lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR),
                "{offset}: {unguessed:?}"
            );
            assert!(zhuyin_guess_sentence(instance));
            assert!(zhuyin_guess_candidates_before_cursor(instance, offset));
            let guessed = kinds(instance);
            assert_eq!(
                guessed[0],
                lookup_candidate_type_t::BEST_MATCH_CANDIDATE,
                "{offset}"
            );
            assert!(
                guessed[1..]
                    .iter()
                    .all(|kind| *kind == lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR),
                "{offset}: {guessed:?}"
            );
            close(context, instance);
        }
        let (context, instance) = open();
        let text = cstr("su3cl3");
        assert_eq!(zhuyin_parse_more_chewings(instance, text.as_ptr()), 6);
        assert!(zhuyin_guess_sentence(instance));
        assert!(zhuyin_guess_candidates_before_cursor(instance, 0));
        assert_eq!(
            kinds(instance),
            [lookup_candidate_type_t::BEST_MATCH_CANDIDATE]
        );
        close(context, instance);
    }
}
