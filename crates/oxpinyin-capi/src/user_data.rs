//! User data persistence: `pinyin_remember_user_input`.

use std::os::raw::{c_char, c_int};

use oxpinyin_core::Dictionary;
use oxpinyin_engine::{EngineError, MatrixKey, key_pronounced_by, remember_walk};
use oxpinyin_user::{PinyinKey, toned_key};

use crate::iterators::{ImportPlan, ImportRead};
use crate::state::{instance_mut, instance_ref};
use crate::types::PinyinInstance;
use oxpinyin_facade::plan_add;

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
/// `_remember_phrase_recur` (`pinyin.cpp:3578-3666`): after the
/// character-count gate and `_pre_compute_tokens`, the walk runs from
/// column 0 over the parse-time matrix of the active mode, with the pin's
/// fuzzy alternatives, keeps a key only when
/// the next character's item pronounces it, and calls `_add_phrase` for
/// `USER_DICTIONARY` on every path of exactly the phrase's length, ORing
/// the answers. Each path stores its keys with their parsed tones.
/// Index-only: no bigram is trained (§2 — training comes only from the
/// selection entry points). The §3.2 allocation runs once per path
/// (unigram seeded `count × 3`); re-remembering the same phrase merges a
/// reading onto the existing token.
///
/// Returns `false` for a null instance, an empty/oversized or invalid UTF-8
/// phrase, a character with no dictionary token, a walk that completes no
/// path of the phrase's length, an instance without a user store, or a
/// store failure. Where the matrix is empty (`get_column_size(0)`,
/// `phonetic_key_matrix.h:102`; register row 66) or the phrase table holds
/// a state `_add_phrase` asserts on for a completed path (`pinyin.cpp:554`,
/// `:568`, `:571`; register row 93), the process aborts, as the import
/// iterator does.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_remember_user_input(
    instance: *mut PinyinInstance,
    phrase: *const c_char,
    count: c_int,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `phrase` is a C string from the caller (null OK; null and
    // invalid UTF-8 read as empty, which the length gate below rejects).
    let phrase =
        oxpinyin_capi_marshal::input::utf8_to_ucs4(oxpinyin_capi_marshal::borrow_c_string!(phrase))
            .unwrap_or_default();
    // 074a2219 pinyin.cpp:3677-3681 checks the character count before
    // _pre_compute_tokens. A failed g_utf8_to_ucs4 leaves that count zero.
    // Stop counting at the refusal boundary: oversized known-character
    // strings must not reach the per-character dictionary lookups.
    let phrase_length = phrase.phrase_length();
    if phrase_length == 0 || phrase_length >= oxpinyin_user::MAX_PHRASE_LENGTH {
        return false;
    }
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    // 074a2219 pinyin.cpp:3098-3130, 3685-3695: _pre_compute_tokens runs
    // next, before anything reads the matrix. Each character's search, in
    // order, goes through `get_first_token`, whose validating
    // `reduce_tokens` asserts on more than four tokens
    // (`phrase_large_table3.h:95`); a search with no token refuses. Search
    // the loaded system and user tables, as lookup_tokens does.
    let mut tokens = Vec::with_capacity(phrase_length);
    for word in phrase.outward_words() {
        let found = inst.core.dict.tokens_for_ucs4(&[word]);
        match found.first() {
            None => return false,
            Some(_) if found.len() > 4 => std::process::abort(),
            Some(&token) => tokens.push(token),
        }
    }
    // `_remember_phrase_recur` starts with `end = matrix.size() - 1`, which
    // wraps for an empty matrix (a fresh instance, or a parse that placed no
    // key); `get_column_size(0)` then asserts
    // (`phonetic_key_matrix.h:102`, register row 66).
    let columns = match inst.core.matrix_columns() {
        Ok(Some(columns)) => columns,
        Ok(None) => std::process::abort(),
        Err(_) => return false,
    };
    // The walk alternates dictionary reads with the store writes of
    // `_add_phrase`, so each call re-derives its own borrow and ends it
    // before returning; `inst` is not used again.
    let count = (count != -1).then(|| u64::from(count.cast_unsigned()));
    let mut pronounces = |key: MatrixKey, index: usize| -> Result<bool, EngineError> {
        // SAFETY: `instance` is live; no other borrow of it is held while
        // this closure runs, and the C caller serializes the call.
        let read = unsafe { instance_ref(instance) };
        let Some(&token) = tokens.get(index) else {
            return Ok(false);
        };
        key_pronounced_by(&read.core.dict, key, token)
    };
    let mut add = |path: &[MatrixKey]| -> Result<bool, EngineError> {
        Ok(add_remembered_path(instance, &phrase, path, count))
    };
    match remember_walk(&columns, phrase_length, &mut pronounces, &mut add) {
        Ok(added) => added,
        Err(_) => {
            crate::ffi::log_warning("pinyin_remember_user_input: the lookup failed");
            false
        }
    }
}

/// One complete path of `_remember_phrase_recur` reaching `_add_phrase`
/// (`pinyin.cpp:3600-3610`, `:508-600`): the path-length bounds, then the
/// preflight that aborts where `_add_phrase` asserts (`:554`, `:568`,
/// `:571`, `reduce_tokens`; register row 93), then the store write. The
/// import path's preflight serves both.
fn add_remembered_path(
    instance: *mut PinyinInstance,
    phrase: &oxpinyin_core::PhraseText,
    path: &[MatrixKey],
    count: Option<u64>,
) -> bool {
    if path.is_empty() || path.len() > oxpinyin_user::MAX_PHRASE_LENGTH {
        return false;
    }
    let Some(keys) = path
        .iter()
        .map(|key| {
            oxpinyin_core::SyllableKey::from_chewing_key(key.key())
                .and_then(|syllable| toned_key(syllable.index(), key.tone()))
        })
        .collect::<Option<Vec<PinyinKey>>>()
    else {
        return false;
    };
    // SAFETY: `instance` is live; every earlier reader has ended and the C
    // caller grants exclusive access to the runtime for this call.
    let ctx = unsafe { crate::state::instance_context_mut(instance) };
    let phrase = {
        let read = ImportRead {
            index: oxpinyin_user::USER_DICTIONARY,
            user: ctx.core.user_store(),
            dict: Some(ctx.core.runtime.dict()),
        };
        let Some(user) = read.user else {
            return false;
        };
        match plan_add(
            read.index,
            read.dict.as_ref(),
            user,
            phrase.clone(),
            keys.clone(),
        ) {
            ImportPlan::Abort => std::process::abort(),
            ImportPlan::Refuse | ImportPlan::LookupFailed => return false,
            ImportPlan::Add { phrase, .. } => phrase,
        }
    };
    let Some(user) = ctx.core.user_store_mut() else {
        return false;
    };
    // `if (-1 == count) count = default_count;` otherwise the `gint` is
    // used as the `guint32` it becomes (`pinyin.cpp:520-524`, the same
    // `_add_phrase` the import path calls): -2 is 4294967294, so a count
    // below -1 is accepted and exported as it went in.
    user.add_phrase(&phrase, &keys, count).is_ok()
}
