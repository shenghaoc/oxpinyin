//! Sentence guessing, retrieval, and the cursor candidate-construction
//! family (`zhuyin_guess_candidates_before_cursor` / `after_cursor`).
//!
//! The candidate-built symbols accumulate into the instance's snapshot; the
//! offset-mapping helpers translate between the original zhuyin input
//! coordinates and the session's `'`-joined full-pinyin buffer.

use std::os::raw::c_char;

use crate::ffi::{cstr_to_owned_lossy, cstr_to_strict, ffi_catch, owned_cstr};
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
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        inst.core.session.guess_sentence().unwrap_or(false)
    })
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
    ffi_catch(false, || {
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
    })
}

/// Get a sentence string from the instance.
///
/// # C signature
/// ```c
/// bool zhuyin_get_sentence(zhuyin_instance_t * instance,
///                          char ** sentence);
/// ```
///
/// Out-param `sentence` is caller-owned (`g_free`).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_sentence(
    instance: *mut ZhuyinInstance,
    sentence: *mut *mut c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        if inst.core.session.sentence_lookup_active() {
            const INDEX: u8 = 0;
            return match inst.core.session.sentence_text(INDEX) {
                Some(decoded) => write_owned_sentence(decoded, sentence),
                None => {
                    if !sentence.is_null() {
                        // SAFETY: Null-checked above.
                        unsafe {
                            *sentence = std::ptr::null_mut();
                        }
                    }
                    false
                }
            };
        }
        let text = if inst
            .core
            .zhuyin_parse
            .as_ref()
            .is_some_and(|parse| !parse.keys().is_empty())
        {
            inst.core.zhuyin_input.clone()
        } else {
            inst.core.session.preedit().text().to_owned()
        };
        write_owned_sentence(&text, sentence)
    })
}

fn write_owned_sentence(text: &str, sentence: *mut *mut c_char) -> bool {
    if text.is_empty() {
        if !sentence.is_null() {
            // SAFETY: Caller null-checks the out-param.
            unsafe {
                *sentence = std::ptr::null_mut();
            }
        }
        return false;
    }
    if !sentence.is_null() {
        // SAFETY: Null-checked above.
        let owned = owned_cstr(text);
        // SAFETY: Null-checked above.
        unsafe {
            *sentence = owned;
        }
        if owned.is_null() {
            return false;
        }
    }
    true
}

/// Get character offset from a lookup byte offset within a sentence.
///
/// # C signature
/// ```c
/// bool zhuyin_get_character_offset(zhuyin_instance_t * instance,
///                                  const char * phrase,
///                                  size_t offset, size_t * length);
/// ```
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
    ffi_catch(false, || {
        // SAFETY: `phrase` is a C string from the caller (null OK).
        let text = cstr_to_owned_lossy(phrase);
        let mut clamped = offset.min(text.len());
        while !text.is_char_boundary(clamped) {
            clamped -= 1;
        }
        let char_count = text[..clamped].chars().count();
        if !length.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *length = char_count;
            }
        }
        true
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

/// The shared candidate-build shell over the engine's `candidates_at` /
/// `candidates_ending_at` / cached candidate list.
fn guess_candidates(instance: *mut ZhuyinInstance, offset: usize, before_cursor: bool) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        if inst.core.session.set_options(inst.core.options()).is_err() {
            return false;
        }
        if !inst.core.session.is_composing() {
            return false;
        }
        let normalized = match inst.core.validate_lookup_offset(offset) {
            Ok(normalized) => normalized,
            Err(_) => {
                inst.candidates.clear();
                return false;
            }
        };
        inst.candidates.clear();
        // The before-cursor entry searches the spans ENDING at the offset
        // (the engine's backward-anchored window builder, the pin's
        // `search_matrix` walk over `(start, offset)`); the after-cursor
        // entry searches the span STARTING at it. Offsets cross the seam in
        // the session's `'`-joined raw-buffer coordinates: the original
        // zhuyin offset maps through `zhuyin_lookup_session_offset`, whose
        // terminal case answers the buffer's one-past-end (the pin's
        // reserved slot). The after-cursor path keeps the composition-anchored
        // cached list when the mapped offset is at/before the composition
        // offset.
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
        let window_owned: oxpinyin_engine::CandidateList = if before_cursor {
            inst.core.anchored_window = None;
            match inst.core.session.candidates_ending_at(session_offset) {
                Ok(window) => window,
                Err(_) => {
                    inst.candidates.clear();
                    return false;
                }
            }
        } else {
            inst.core.anchored_window = if session_offset <= inst.core.session.composition_offset()
            {
                None
            } else {
                match inst.core.session.candidates_at(session_offset) {
                    Ok(window) => Some((session_offset, window)),
                    Err(_) => {
                        inst.candidates.clear();
                        return false;
                    }
                }
            };
            match inst.core.anchored_window.as_ref() {
                Some((_, window)) => window.clone(),
                None => inst.core.session.candidates().clone(),
            }
        };
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
        );
        // The pin answers `true` for a valid lookup into a non-empty matrix
        // even when no candidate spans the offset (the empty-col-window
        // shape, `zhuyin.cpp:1474,1549`); only an empty matrix (nothing
        // parsed) answers `false` (`0 == matrix.size()`, `:1475`).
        if inst.candidates.is_empty() && inst.core.parsed_len == 0 {
            return false;
        }
        true
    })
}
