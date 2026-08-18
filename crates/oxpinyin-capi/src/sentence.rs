//! Sentence guessing and retrieval.

use std::ffi::CString;
use std::os::raw::c_char;

use oxpinyin_core::{DoublePinyinParse, ZhuyinParse};

use crate::ffi::{cstr_to_string, ffi_catch, owned_cstr};
use crate::state::{CapiCandidate, instance_mut, instance_ref};
use crate::types::{GUint, PinyinInstance, lookup_candidate_type_t, sort_option_t};

/// Guess a sentence from saved pinyin keys.
///
/// # C signature
/// ```c
/// bool pinyin_guess_sentence(pinyin_instance_t * instance);
/// ```
///
/// W14: runs the n-best sentence lookup (`Session::guess_sentence`, the
/// port of `PhoneticLookup<2, 3>`). While the rows live, the candidate
/// list carries them at its head typed `NBEST_MATCH_CANDIDATE`, and
/// [`pinyin_get_sentence`] returns their decoded text — upstream's
/// `m_nbest_results` gate (`pinyin.cpp:1373-1385`, `2292-2293`). Rows are
/// cleared by [`pinyin_reset`], nothing else.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_sentence(instance: *mut PinyinInstance) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        inst.session.guess_sentence().unwrap_or(false)
    })
}

/// Guess predicted candidates with punctuations after a prefix.
///
/// # C signature
/// ```c
/// bool pinyin_guess_predicted_candidates_with_punctuations(
///     pinyin_instance_t * instance, const char * prefix);
/// ```
///
/// Phrase prediction, then punctuation candidates prepended from
/// `punct.redb` (`pinyin.cpp:2454-2498`). Always returns `true`, matching
/// upstream, even when the prefix matched no phrase-table suffix.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_predicted_candidates_with_punctuations(
    instance: *mut PinyinInstance,
    prefix: *const c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        // SAFETY: `prefix` is a C string from the caller (null OK).
        let prefix = unsafe { cstr_to_string(prefix) };
        crate::predict::guess_predicted_with_punctuations(inst, &prefix)
    })
}

/// Get a sentence string from the instance (n-best variant).
///
/// # C signature
/// ```c
/// bool pinyin_get_sentence(pinyin_instance_t * instance,
///                          guint8 index,
///                          char ** sentence);
/// ```
///
/// Out-param `sentence` is caller-owned (`g_free`). The returned buffer is
/// allocated with libc `malloc`, which `g_free` releases on every platform.
///
/// W14: with sentence rows live this is the decoded text of n-best `index`
/// (`convert_to_utf8` through the phrase index, `pinyin.cpp:1463-1482`);
/// past the row count, or before any [`pinyin_guess_sentence`] call, the
/// pre-W14 provisional ground stands — scheme parses return the original
/// keystroke buffer, full pinyin the session preedit.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_sentence(
    instance: *mut PinyinInstance,
    index: u8,
    sentence: *mut *mut c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let text = if let Some(decoded) = inst.session.sentence_text(index) {
            decoded.to_owned()
        } else if inst
            .zhuyin_parse
            .as_ref()
            .is_some_and(|parse| !parse.keys().is_empty())
        {
            inst.zhuyin_input.clone()
        } else if inst
            .double_parse
            .as_ref()
            .is_some_and(|parse| !parse.keys().is_empty())
        {
            inst.double_input.clone()
        } else {
            inst.session.preedit().text().to_owned()
        };
        if text.is_empty() {
            if !sentence.is_null() {
                // SAFETY: Null-checked above.
                unsafe {
                    *sentence = std::ptr::null_mut();
                }
            }
            return false;
        }
        if !sentence.is_null() {
            // SAFETY: Null-checked above. `owned_cstr` returns null on an
            // interior NUL or allocation failure; otherwise ownership
            // transfers to the caller, which frees it with `g_free`.
            let owned = owned_cstr(&text);
            // SAFETY: Null-checked above.
            unsafe {
                *sentence = owned;
            }
            if owned.is_null() {
                return false;
            }
        }
        true
    })
}

/// Maps a byte offset in the transformed `'`-joined full-pinyin string back
/// to the original double-pinyin input offset.
///
/// Candidate consumption always lands on a key boundary, so the mapping is
/// exact there; an offset inside a transformed key is clamped to that key's
/// original end (the same place a candidate would consume it).
fn double_original_offset(parse: &DoublePinyinParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        let key_len = item.key().text().len();
        let boundary = transformed + key_len;
        if offset <= boundary {
            return item.end();
        }
        transformed = boundary + 1; // apostrophe between keys
    }
    parse.consumed()
}

fn zhuyin_original_offset(parse: &ZhuyinParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        let key_len = item.key().text().len();
        let boundary = transformed + key_len;
        if offset <= boundary {
            return item.end();
        }
        transformed = boundary + 1; // apostrophe between keys
    }
    parse.consumed()
}

/// Get character offset from a lookup byte offset within a sentence.
///
/// # C signature
/// ```c
/// bool pinyin_get_character_offset(pinyin_instance_t * instance,
///                                  const char * phrase,
///                                  size_t offset,
///                                  size_t * length);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_character_offset(
    instance: *mut PinyinInstance,
    phrase: *const c_char,
    offset: usize,
    length: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `phrase` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(phrase) };
        let mut clamped = offset.min(text.len());
        // Floor to a UTF-8 char boundary so the slice never panics.
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

/// Guess candidates at the given offset with sort option.
///
/// # C signature
/// ```c
/// bool pinyin_guess_candidates(pinyin_instance_t * instance,
///                              size_t offset,
///                              guint sort_option);
/// ```
///
/// `offset` remains ignored — the engine has no positional backend yet.
/// W14 honours [`sort_option_t::SORT_WITHOUT_SENTENCE_CANDIDATE`]: with
/// the bit clear, sentence rows guessed by [`pinyin_guess_sentence`]
/// appear at the head typed `NBEST_MATCH_CANDIDATE` with their tail rank;
/// with the bit set they are excluded, exactly upstream's gate
/// (`pinyin.cpp:2292-2293`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_candidates(
    instance: *mut PinyinInstance,
    _offset: usize,
    sort_option: GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        if inst.session.set_options(inst.options()).is_err() {
            return false;
        }
        if !inst.session.is_composing() {
            return false;
        }
        let without_sentence =
            sort_option & sort_option_t::SORT_WITHOUT_SENTENCE_CANDIDATE as GUint != 0;
        inst.candidates.clear();
        let double_parse = inst.double_parse.clone();
        let zhuyin_parse = inst.zhuyin_parse.clone();
        for cand in inst.session.candidates().iter() {
            if without_sentence && cand.kind() == oxpinyin_engine::CandidateKind::Sentence {
                continue;
            }
            let text = match CString::new(cand.text().to_owned()) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let consumed_bytes = if let Some(parse) = zhuyin_parse.as_ref() {
                zhuyin_original_offset(parse, cand.consumed_bytes())
            } else if let Some(parse) = double_parse.as_ref() {
                double_original_offset(parse, cand.consumed_bytes())
            } else {
                cand.consumed_bytes()
            };
            inst.candidates.push(CapiCandidate {
                text,
                kind: cand.kind(),
                candidate_type: match cand.kind() {
                    oxpinyin_engine::CandidateKind::Sentence => {
                        lookup_candidate_type_t::NBEST_MATCH_CANDIDATE
                    }
                    oxpinyin_engine::CandidateKind::Addon => {
                        lookup_candidate_type_t::ADDON_CANDIDATE
                    }
                    oxpinyin_engine::CandidateKind::Phrase
                    | oxpinyin_engine::CandidateKind::Fallback
                    | _ => lookup_candidate_type_t::NORMAL_CANDIDATE,
                },
                nbest_index: cand.nbest_index(),
                consumed_bytes,
                token: cand.token(),
            });
        }
        true
    })
}
