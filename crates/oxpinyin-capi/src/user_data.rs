//! User data persistence: `pinyin_remember_user_input`.

use std::os::raw::{c_char, c_int};

use oxpinyin_core::SyllableKey;
use oxpinyin_engine::MatrixKey;
use oxpinyin_user::{PinyinKey, toned_key};

use crate::ffi::cstr_to_string;
use crate::state::instance_mut;
use crate::types::PinyinInstance;

/// Remember a user-provided phrase with its current pinyin context.
///
/// # C signature
/// ```c
/// bool pinyin_remember_user_input(pinyin_instance_t * instance,
///                                 const char * phrase,
///                                 gint count);
/// ```
///
/// `count` of -1 means use the default value; any other `gint` is used as
/// the `guint32` it becomes, so a negative count is accepted (register row 57).
///
/// The §3.1 path: stores `phrase` in the [`USER_DICTIONARY`] sub-index with
/// the instance's current composition keys as its pronunciation — the
/// session's selected-parse syllable keys, mapped to their 16-bit ids, which
/// `_remember_phrase_recur` would have walked upstream. Index-only: no
/// bigram is trained (§2 — training comes only from the selection entry
/// points). The §3.2 allocation runs once (unigram seeded `count × 3`);
/// re-remembering the same phrase merges a reading onto the existing token.
///
/// Returns `false` for a null instance, an empty/oversized phrase, a phrase
/// whose character count does not match the current composition's key count,
/// an instance without a user store,
/// or a store failure ([`UserStoreError::InvalidPhrase`] included).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_remember_user_input(
    instance: *mut PinyinInstance,
    phrase: *const c_char,
    count: c_int,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `phrase` is a C string from the caller (null OK; a null
    // pointer reads as empty, which validation rejects).
    let phrase = unsafe { cstr_to_string(phrase) };
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    let Ok(syllables) = inst.core.session.composition_keys() else {
        return false;
    };
    if syllables.is_empty() || syllables.len() >= oxpinyin_user::MAX_PHRASE_LENGTH {
        return false;
    }
    // Keep the existing selected syllable sequence, adding its parsed tones.
    // 074a2219 pinyin.cpp:3578-3668 carries complete ChewingKeys through
    // _remember_phrase_recur into _add_phrase, rather than syllable IDs.
    let tones: Vec<u8> = if let Some(parse) = inst.core.zhuyin_parse.as_ref() {
        parse.keys().iter().map(|key| key.tone()).collect()
    } else if let Some(parse) = inst.core.double_parse.as_ref() {
        parse.keys().iter().map(|key| key.tone()).collect()
    } else if let Some(parse) = inst.core.full_parse.as_ref() {
        parse.keys().iter().map(|key| key.tone()).collect()
    } else {
        // set_options changes the context, not the parsed matrix
        // (074a2219 pinyin.cpp:1299-1306, 3585). Read the session's
        // parse-time matrix instead of parsing again with live options.
        let Ok((matrix, end)) = inst.core.session.matrix_keys() else {
            return false;
        };
        let Some(tones) = selected_tones(
            &matrix,
            inst.core.session.raw_input().as_bytes(),
            &syllables,
            0,
            end,
        ) else {
            return false;
        };
        tones
    };
    if syllables.len() != tones.len() {
        return false;
    }
    let Some(keys) = syllables
        .into_iter()
        .zip(tones)
        .map(|(key, tone)| toned_key(key.index(), tone))
        .collect::<Option<Vec<PinyinKey>>>()
    else {
        return false;
    };
    let Some(user) = inst.core.user.as_mut() else {
        return false;
    };
    // `if (-1 == count) count = default_count;` otherwise the `gint` is
    // used as the `guint32` it becomes (`pinyin.cpp:520-524`, the same
    // `_add_phrase` the import path calls): -2 is 4294967294, so a count
    // below -1 is accepted and exported as it went in.
    let count = (count != -1).then(|| u64::from(count.cast_unsigned()));
    user.add_phrase(&phrase, &keys, count).is_ok()
}

// Match the selected syllable path through the retained scan matrix. Matrix
// alternatives need not have the same spans, so require a complete path;
// apostrophe separator hops do not consume a syllable. Depth is bounded by
// the validated phrase length above.
fn selected_tones(
    matrix: &[MatrixKey],
    input: &[u8],
    syllables: &[SyllableKey],
    mut start: usize,
    end: usize,
) -> Option<Vec<u8>> {
    while start < end && input.get(start) == Some(&b'\'') {
        start += 1;
    }
    let Some((syllable, rest)) = syllables.split_first() else {
        return (start == end).then(Vec::new);
    };
    for edge in matrix {
        if edge.syllable_start() == start
            && edge.end() > start
            && edge.key() == *syllable
            && let Some(mut tones) = selected_tones(matrix, input, rest, edge.end(), end)
        {
            tones.insert(0, edge.tone());
            return Some(tones);
        }
    }
    None
}
