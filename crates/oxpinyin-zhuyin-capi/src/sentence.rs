//! Sentence guessing, retrieval, and the cursor candidate-construction
//! family (`zhuyin_guess_candidates_before_cursor` / `after_cursor`).
//!
//! The candidate-built symbols accumulate into the instance's snapshot; the
//! offset-mapping helpers translate between the original zhuyin input
//! coordinates and the session's `'`-joined full-pinyin buffer.

use oxpinyin_capi_marshal::phrase_text::phrase_text_bytes;
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
    let inst = unsafe { instance_mut(instance) };
    if refuse_empty_matrix(inst) {
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

/// The lookup refusals of register rows 85 and 98: a predecessor's user
/// bigram that the pin's merge or read asserts on (`memory_chunk.h:390`)
/// or reads past (`ngram.cpp:241`, `:167`, `:318-349`). Logs the one
/// warning and answers `true` when `error` is one of them.
pub(crate) fn refuse_lookup_gram(name: &str, error: &oxpinyin_engine::EngineError) -> bool {
    let message = match error {
        oxpinyin_engine::EngineError::ShortLookupPredecessor { .. } => {
            "a user bigram value is shorter than a guint32 total_freq \
             (upstream aborts, memory_chunk.h:390)"
        }
        oxpinyin_engine::EngineError::UnsafeLookupPredecessor { .. } => {
            "a user bigram value would be read past its end (upstream reads past it)"
        }
        oxpinyin_engine::EngineError::ReadAfterCheck { .. } => {
            "a user bigram check found a value, then its read failed \
             (upstream aborts, ngram_kyotodb.cpp:138-139)"
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
        | oxpinyin_engine::EngineError::StaleTrainingSpan { .. }
        | oxpinyin_engine::EngineError::OverlongUserIndexKey { .. }
        | oxpinyin_engine::EngineError::UserModel(_)
        | oxpinyin_engine::EngineError::Graph(_)
        | oxpinyin_engine::EngineError::Decode(_)
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
fn refuse_empty_matrix(inst: &mut crate::state::CapiInstance) -> bool {
    if matrix_has_keys(inst) {
        return false;
    }
    inst.core.session.discard_composition();
    true
}

fn matrix_has_keys(inst: &crate::state::CapiInstance) -> bool {
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
    let inst = unsafe { instance_mut(instance) };
    // `m_prefixes` becomes `[sentence_start]` and the prefix adds what it
    // converts to (`zhuyin.cpp:949-953`); `None` adds nothing.
    let prefix =
        oxpinyin_capi_marshal::ucs4::prefix_text(oxpinyin_capi_marshal::borrow_c_string!(prefix));
    if refuse_empty_matrix(inst) {
        return false;
    }
    let prefixes = oxpinyin_facade::compute_prefixes(
        &inst.core.dict,
        inst.core.user.as_ref(),
        prefix.as_ref(),
    );
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
    match inst.core.session.sentence_text(0) {
        Some(decoded) => write_owned_sentence_bytes(&phrase_text_bytes(decoded), sentence),
        None => false,
    }
}

// The `char **`-out sentence writer, stamped from the shared marshalling
// macro (byte-identical to the pinyin facade's). `crate::ffi::owned_cstr` is
// this facade's libc-`malloc` duplicator — the per-facade allocator edge.
oxpinyin_capi_marshal::write_owned_sentence_bytes!(crate::ffi::malloc_cstr_with_warning);

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
/// no key path satisfies; also `false` where the pin asserts (the
/// range and `_check_offset` shapes — the no-abort policy).
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
    // SAFETY: Null-checked inside; invalid UTF-8 refuses like the pin's
    // `g_utf8_to_ucs4` NULL answer.
    let text =
        oxpinyin_capi_marshal::input::utf8_to_ucs4(oxpinyin_capi_marshal::borrow_c_string!(phrase))
            .unwrap_or_default();
    let char_count = match inst.core.character_offset(&text, offset) {
        Ok(Some(_)) if offset == inst.core.parsed_len && character_walk_ran(inst, &text) => {
            if !length.is_null() {
                // SAFETY: Null-checked above; caller supplied the output.
                unsafe {
                    *length = 0;
                }
            }
            return false;
        }
        Ok(Some(char_count)) => char_count,
        Ok(None) => {
            // The walk's own `false` still stores the length it reached
            // (`*plength = length`, `zhuyin.cpp:2192`); the earlier returns
            // (no keys, empty phrase, a character without a token) leave it
            // untouched.
            if character_walk_ran(inst, &text) && !length.is_null() {
                // SAFETY: Null-checked above.
                unsafe {
                    *length = 0;
                }
            }
            return false;
        }
        Err(_) => {
            // Class (c): `zhuyin.cpp:2110` and `:2158`.
            crate::ffi::log_warning("zhuyin_get_character_offset: assertion failed");
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

/// Whether `zhuyin_get_character_offset` got past its early `false` returns
/// (`zhuyin.cpp:2152-2182`): a non-empty matrix, a non-empty phrase and a
/// dictionary token for every character. The engine answers all of them, and
/// the walk's own failure, with `Ok(None)`.
fn character_walk_ran(inst: &crate::state::CapiInstance, text: &oxpinyin_core::PhraseText) -> bool {
    use oxpinyin_core::Dictionary as _;

    let Ok((keys, input, _)) = inst.core.mode_keys() else {
        return false;
    };
    if input.is_empty() || keys.is_empty() || text.is_empty() {
        return false;
    }
    text.outward_words()
        .all(|word| !inst.core.dict.tokens_for_ucs4(&[word]).is_empty())
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
/// `zero_key != key` on the column before the offset (`:1441-1456`): an
/// offset past the reserved slot aborts. Class (c), `false` and one warning.
fn validated_lookup_offset(inst: &crate::state::CapiInstance, offset: usize) -> Option<usize> {
    match inst.core.validate_abi_lookup_offset(offset, true) {
        Ok(normalized) => Some(normalized),
        Err(oxpinyin_engine::EngineError::LookupOffsetOutOfRange { .. }) => {
            crate::ffi::log_warning("zhuyin_guess_candidates: offset lies past the matrix");
            None
        }
        Err(_) => {
            crate::ffi::log_warning("zhuyin_guess_candidates: zero-column assertion");
            None
        }
    }
}

/// Whether the facade's `guint32` unigram total is zero: the loaded
/// libraries' sum, the user store's deltas and the accepted
/// `zhuyin_token_add_unigram_frequency` deltas, wrapped. A model without real
/// unigrams has no total to divide by.
fn facade_total_is_zero(inst: &crate::state::CapiInstance) -> bool {
    use oxpinyin_core::LanguageModel as _;

    inst.core.lm.has_real_unigrams() && inst.core.lm.amplified_total() == 0
}

/// The shared candidate-build shell over the engine's `candidates_at` /
/// `candidates_ending_at` / cached candidate list.
fn guess_candidates(instance: *mut ZhuyinInstance, offset: usize, before_cursor: bool) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    // Both candidate exports free their rows before the empty-matrix
    // refusal, leaving sentence results intact (074a2219,
    // zhuyin.cpp:1468-1471,1548-1551).
    if !matrix_has_keys(inst) {
        inst.candidates.clear();
        return false;
    }
    if inst.core.session.set_options(inst.core.options()).is_err() {
        return false;
    }
    if !inst.core.session.is_composing() {
        return false;
    }
    let Some(normalized) = validated_lookup_offset(inst, offset) else {
        inst.candidates.clear();
        return false;
    };
    inst.candidates.clear();
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
            inst.candidates.clear();
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
    // Class (b), `zhuyin.cpp:1096`: a token of a loaded library whose item
    // cannot be read leaves `m_phrase_string` unset, and the pin's duplicate
    // removal compares it (SIGSEGV). Only a `table.conf` that points a
    // library at another file (or at none) gets here. The call fails, once.
    if window_owned.had_unreadable_item() {
        inst.core.anchored_window = None;
        inst.candidates.clear();
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
        &mut *inst,
        &window_owned,
        before_cursor,
        before_end,
        window_anchor,
        sentence_rows_only,
    );
    inst.core.anchored_window = Some((window_anchor, window_owned));
    // Class (c), `zhuyin.cpp:1261`: `assert (0 < total_freq)` while ranking a
    // candidate, with the facade total wrapped to zero by
    // `zhuyin_token_add_unigram_frequency`. The assertion sits in the loop
    // over the searched rows (before the sentence rows are prepended), so a
    // window with no phrase row never reaches it and the pin answers true.
    if facade_total_is_zero(inst)
        && inst.candidates.iter().any(|c| {
            c.candidate_type != crate::types::lookup_candidate_type_t::BEST_MATCH_CANDIDATE
        })
    {
        crate::ffi::log_warning("zhuyin_guess_candidates: assertion '0 < total_freq' failed");
        inst.candidates.clear();
        return false;
    }
    // The pin answers `true` for a valid lookup into a non-empty matrix
    // even when no candidate spans the offset (074a2219,
    // zhuyin.cpp:1460-1551). The empty matrix was refused above.
    true
}
