//! Sentence guessing and retrieval.

use std::ffi::CString;
use std::os::raw::c_char;

use oxpinyin_core::{DoublePinyinParse, FullPinyinIndexParse, ZhuyinParse};

use crate::ffi::{cstr_to_strict, cstr_to_string, ffi_catch, owned_cstr};
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

/// Guess a sentence seeded with prefix tokens.
///
/// # C signature
/// ```c
/// bool pinyin_guess_sentence_with_prefix(pinyin_instance_t * instance,
///                                        const char * prefix);
/// ```
///
/// Upstream resets `m_prefixes`, appends the virtual start, appends the
/// tail-substring tokens `_compute_prefixes` finds for the prefix text
/// (`pinyin.cpp:1389-1424`), validates the constraint store, and drives
/// the ordinary full-matrix decode with those seeds
/// (`pinyin.cpp:1426-1441`). Operates on the existing parse state; the
/// retval is the decode's.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_sentence_with_prefix(
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
        // Reject invalid UTF-8 before the prefix-token lookup — same
        // upstream FALSE gate the sibling prediction seam honours
        // (`ffi::cstr_to_strict`).
        let Some(prefix) = cstr_to_strict(prefix) else {
            return false;
        };
        let prefixes = crate::predict::compute_prefixes(&inst.dict, inst.user.as_ref(), &prefix);
        let prefix_tokens: Vec<oxpinyin_core::PhraseToken> = prefixes
            .iter()
            .map(|&token| oxpinyin_core::PhraseToken::new(token))
            .collect();
        inst.session
            .guess_sentence_with_prefix(&prefix_tokens)
            .unwrap_or(false)
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
/// W14: once a sentence lookup is active ([`pinyin_guess_sentence`] ran
/// since the last reset), this answers decoded-or-nothing — the text of
/// n-best `index` through the phrase index (`pinyin.cpp:1463-1482`), and
/// `false` with an empty out-param past the row count or after a lookup
/// that produced none, exactly upstream's `0 == results.size()` false.
/// The pre-W14 raw form (scheme keystroke buffer / session preedit)
/// survives only before any lookup has occurred.
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
        if inst.session.sentence_lookup_active() {
            // An active lookup answers decoded-or-nothing — the row text
            // when held, `false` past the row count or after a lookup that
            // produced none (upstream's `0 == results.size()` false),
            // never the raw form.
            return if let Some(decoded) = inst.session.sentence_text(index) {
                write_owned_sentence(decoded, sentence)
            } else {
                if !sentence.is_null() {
                    // SAFETY: Null-checked above.
                    unsafe {
                        *sentence = std::ptr::null_mut();
                    }
                }
                false
            };
        }
        let text = if inst
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
        write_owned_sentence(&text, sentence)
    })
}

/// Writes `text` through the caller-owned out-param: `false` on an empty
/// text, an interior NUL, or allocation failure, with the out-param nulled
/// on every failure path.
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
        // SAFETY: Null-checked above. `owned_cstr` returns null on an
        // interior NUL or allocation failure; otherwise ownership
        // transfers to the caller, which frees it with `g_free`.
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

/// Maps a byte offset in the transformed `'`-joined full-pinyin string back
/// to the original double-pinyin input offset.
///
/// Candidate consumption — and the session's post-select composition
/// offset — always lands on a key boundary, so the mapping is exact there;
/// an offset inside a transformed key is clamped to that key's original
/// end (the same place a candidate would consume it).
pub fn double_original_offset(parse: &DoublePinyinParse, offset: usize) -> usize {
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

pub fn zhuyin_original_offset(parse: &ZhuyinParse, offset: usize) -> usize {
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

/// The Luoma/secondary-zhuyin sibling of [`double_original_offset`]: the
/// transformed string is the `'`-joined canonical spellings, and each key
/// remembers its original byte span (tone digit included).
pub fn full_original_offset(parse: &FullPinyinIndexParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        let key_len = item.canonical().len();
        let boundary = transformed + key_len;
        if offset <= boundary {
            return item.end();
        }
        transformed = boundary + 1; // apostrophe between keys
    }
    parse.consumed()
}

/// Maps an original-input offset to the transformed session offset — the
/// inverse of [`double_original_offset`]: the transformed start of the
/// first key whose original span ends past `offset`. A key-boundary offset
/// therefore maps to the next key's start, the position a forced run at
/// that key would sit at.
pub fn double_session_offset(parse: &DoublePinyinParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        if offset < item.end() {
            return transformed;
        }
        transformed += item.key().text().len() + 1; // key + apostrophe
    }
    transformed
}

/// [`double_session_offset`]'s zhuyin sibling.
pub fn zhuyin_session_offset(parse: &ZhuyinParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        if offset < item.end() {
            return transformed;
        }
        transformed += item.key().text().len() + 1; // key + apostrophe
    }
    transformed
}

/// [`double_session_offset`]'s Luoma/secondary-zhuyin sibling.
pub fn full_session_offset(parse: &FullPinyinIndexParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        if offset < item.end() {
            return transformed;
        }
        transformed += item.canonical().len() + 1; // key + apostrophe
    }
    transformed
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
/// The caller `offset` may sit one position past the zero-`ChewingKey` `'`
/// separator run (ibus-libpinyin ≥ 1.16.1 passes the raw begin of the next
/// key rest, issue #570). libpinyin@dbff264 normalizes it back to the first
/// byte of that run and validates the normalized offset —
/// [`crate::state::CapiInstance::validate_lookup_offset`] runs that law in
/// the active parse mode's own coordinates: the full walk where `'` is a
/// zero-key separator (plain full pinyin, Luoma/secondary-zhuyin), the
/// range refusal alone where a composition cannot hold one (double pinyin,
/// the zhuyin keyboards — there `'` is out of scheme or a content symbol).
/// A refusal — the leading-run shape, or an offset beyond one-past-end —
/// empties the snapshot and answers `false` where upstream's
/// `_check_offset` aborts (or reads its matrix out of bounds). The lookup
/// itself stays anchored at the session's composition offset, so
/// `pinyin_choose_candidate(offset, cand)` keeps round-tripping for such
/// candidates; the scan anchor is otherwise still positionless — the
/// engine has no positional backend yet.
///
/// W14 honours [`sort_option_t::SORT_WITHOUT_SENTENCE_CANDIDATE`]: with
/// the bit clear, sentence rows guessed by [`pinyin_guess_sentence`]
/// appear at the head typed `NBEST_MATCH_CANDIDATE` with their tail rank;
/// with the bit set they are excluded, exactly upstream's gate
/// (`pinyin.cpp:2292-2293`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_candidates(
    instance: *mut PinyinInstance,
    offset: usize,
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
        let Ok(normalized) = inst.validate_lookup_offset(offset) else {
            inst.candidates.clear();
            return false;
        };
        let without_sentence =
            sort_option & sort_option_t::SORT_WITHOUT_SENTENCE_CANDIDATE as GUint != 0;
        inst.candidates.clear();
        let double_parse = inst.double_parse.clone();
        let zhuyin_parse = inst.zhuyin_parse.clone();
        // Mirror the pin's per-offset span search. `pinyin_guess_candidates`
        // anchors its window at `start = offset` (`pinyin.cpp:2224-2262`); the
        // session's cached list is anchored at the composition offset it owns.
        // When the caller's normalized lookup offset differs — a
        // mid-composition cursor with no prior choose — rebuild the window at
        // that offset. When it matches (offset 0 unconstrained, and every
        // post-choose lookup, where the frontend's offset equals the
        // composition offset), the cached list already answers, so those paths
        // stay bit-identical.
        //
        // Re-anchoring is valid only for plain full pinyin, where the caller's
        // offset is a direct byte index into the session's raw buffer — the
        // same coordinate space as `composition_offset`. Under a transform
        // (double pinyin, zhuyin, or the LUOMA / secondary-zhuyin full-pinyin
        // index) the offset lives in the original input's coordinates, which
        // `self.raw` does not share; the normalized offset is range-checked
        // there but is not a raw index, so the cached list stands (the C2
        // differential never drives a transformed scheme).
        let transformed =
            inst.double_parse.is_some() || inst.zhuyin_parse.is_some() || inst.full_parse.is_some();
        // Retain the re-anchored window on the instance: a session's
        // selection records against the cached (composition-anchored) list,
        // but the row the caller saw came from the offset-anchored window —
        // an index into the cached list would select a different row
        // whenever the two differ. `anchored_window` is set here and a later
        // `pinyin_choose_candidate` resolves its index against it.
        // Re-anchor only at a normalized offset strictly PAST the
        // composition offset. A normalized offset equal to it is the
        // composition-anchored cached list; one BELOW it is a stale cursor
        // behind the selection, whose anchored span would regress the
        // composition (rejected — the `select_anchored` guard refuses it)
        // and is served the cached list instead. Under a transform the
        // cached list stands (the offset is in the original input's
        // coordinates `self.raw` does not share).
        inst.anchored_window = if transformed || normalized <= inst.session.composition_offset() {
            None
        } else if let Ok(window) = inst.session.candidates_at(normalized) {
            Some((normalized, window))
        } else {
            // Unreachable for a well-formed plain-pinyin lookup:
            // the offset-shaped contracts are refused by
            // `validate_lookup_offset` and `candidates_at`'s own
            // range/char-boundary checks, and a mid-syllable byte
            // is not an error — the window answers the pin's
            // empty-column law. The arm remains for genuine
            // backend failures during the re-anchored scan.
            inst.candidates.clear();
            return false;
        };
        let candidates: &oxpinyin_engine::CandidateList = match inst.anchored_window.as_ref() {
            Some((_, window)) => window,
            None => inst.session.candidates(),
        };
        for (window_index, cand) in candidates.iter().enumerate() {
            if without_sentence && cand.kind() == oxpinyin_engine::CandidateKind::Sentence {
                continue;
            }
            // The engine's remaining-raw-input `Fallback` row is the
            // session-API affordance (`session-api.md`: it keeps `Space`
            // and `select` meaningful before a decoder result exists) —
            // the pin has no raw-input fallback: an empty matrix answers
            // false (`pinyin.cpp:2193`), an empty result answers true
            // with no rows. The C ABI translates the engine shape, it
            // does not surface it.
            if cand.kind() == oxpinyin_engine::CandidateKind::Fallback {
                continue;
            }
            let Ok(text) = CString::new(cand.text().as_bytes()) else {
                continue;
            };
            let consumed_bytes = zhuyin_parse.as_ref().map_or_else(
                || {
                    double_parse.as_ref().map_or_else(
                        || cand.consumed_bytes(),
                        |parse| double_original_offset(parse, cand.consumed_bytes()),
                    )
                },
                |parse| zhuyin_original_offset(parse, cand.consumed_bytes()),
            );
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
                source_index: window_index,
            });
        }
        // The pin's empty-matrix early return (`pinyin.cpp:2193`): a
        // parse that produced no keys answers false, not an empty list.
        // A non-empty parse with no candidates (apostrophe-only runs,
        // unmatchable tails) answers true with zero rows.
        if inst.candidates.is_empty() && inst.parsed_len == 0 {
            return false;
        }
        true
    })
}
