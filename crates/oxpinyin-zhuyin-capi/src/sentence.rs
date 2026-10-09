//! Sentence guessing, retrieval, and the cursor candidate-construction
//! family (`zhuyin_guess_candidates_before_cursor` / `after_cursor`).
//!
//! The candidate-built symbols accumulate into the instance's snapshot; the
//! offset-mapping helpers translate between the original zhuyin input
//! coordinates and the session's `'`-joined full-pinyin buffer.

use std::os::raw::c_char;

use crate::ffi::cstr_to_strict;
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
    inst.core.session.guess_sentence().unwrap_or(false)
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
    let Some(prefix) = cstr_to_strict(prefix) else {
        return false;
    };
    let prefixes =
        oxpinyin_facade::compute_prefixes(&inst.core.dict, inst.core.user.as_ref(), &prefix);
    let prefix_tokens: Vec<oxpinyin_core::PhraseToken> = prefixes
        .iter()
        .map(|&token| oxpinyin_core::PhraseToken::new(token))
        .collect();
    inst.core
        .session
        .guess_sentence_with_prefix(&prefix_tokens)
        .unwrap_or(false)
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
        Some(decoded) => write_owned_sentence(decoded, sentence),
        None => false,
    }
}

// The `char **`-out sentence writer, stamped from the shared marshalling
// macro (byte-identical to the pinyin facade's). `crate::ffi::owned_cstr` is
// this facade's libc-`malloc` duplicator — the per-facade allocator edge.
oxpinyin_capi_marshal::write_owned_sentence!(crate::ffi::owned_cstr);

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
    let Some(text) = cstr_to_strict(phrase) else {
        return false;
    };
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
fn character_walk_ran(inst: &crate::state::CapiInstance, text: &str) -> bool {
    use oxpinyin_core::Dictionary as _;

    let Ok((keys, input, _)) = inst.core.mode_keys() else {
        return false;
    };
    if input.is_empty() || keys.is_empty() || text.is_empty() {
        return false;
    }
    text.chars().all(|character| {
        let mut buffer = [0_u8; 4];
        !inst
            .core
            .dict
            .tokens_for_text(character.encode_utf8(&mut buffer))
            .is_empty()
    })
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

    inst.core.lm.has_real_unigrams()
        && inst
            .core
            .lm
            .amplified_total()
            .wrapping_add(u64::from(inst.core.dict.unigram_total_delta()))
            & u64::from(u32::MAX)
            == 0
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
    let window_owned: oxpinyin_engine::CandidateList = if before_cursor {
        let Ok(window) = inst.core.session.candidates_ending_at(session_offset) else {
            inst.core.anchored_window = None;
            inst.candidates.clear();
            return false;
        };
        // The before-cursor window is re-anchored just like the
        // after-cursor one: `snapshot_candidates` records each row's index
        // into THIS list, so a later `zhuyin_choose_candidate` must resolve
        // it here and not against the composition-anchored cached list —
        // the two differ in general, and the caller would commit a row it
        // never displayed. The anchor is the buffer start rather than the
        // lookup offset because the ending-at window is END-anchored; see
        // `oxpinyin_facade::BEFORE_CURSOR_ANCHOR`.
        inst.core.anchored_window = Some((oxpinyin_facade::BEFORE_CURSOR_ANCHOR, window.clone()));
        window
    } else {
        let Ok(window) = inst.core.session.candidates_at(session_offset) else {
            inst.core.anchored_window = None;
            inst.candidates.clear();
            return false;
        };
        inst.core.anchored_window = Some((session_offset, window.clone()));
        window
    };
    let before_end = if before_cursor {
        Some(normalized)
    } else {
        None
    };
    let anchor = match inst.core.anchored_window.as_ref() {
        Some((anchor, _)) => *anchor,
        None => inst.core.session.composition_offset(),
    };
    crate::candidates::snapshot_candidates(
        &mut *inst,
        &window_owned,
        before_cursor,
        before_end,
        anchor,
        sentence_rows_only,
    );
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
    // even when no candidate spans the offset (the empty-col-window
    // shape, `zhuyin.cpp:1474,1549`); only an empty matrix (nothing
    // parsed) answers `false` (`0 == matrix.size()`, `:1475`).
    if inst.candidates.is_empty() && inst.core.parsed_len == 0 {
        return false;
    }
    true
}
