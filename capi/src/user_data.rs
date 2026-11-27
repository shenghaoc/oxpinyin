//! Storage of user phrases with pronunciations from the current input.

use std::os::raw::{c_char, c_int};

use oxpinyin::facade::{
    Dictionary, EngineError, ImportPlan, MatrixKey, PinyinKey, key_pronounced_by, remember_walk,
    toned_key,
};

use crate::state::instance_ref;
use crate::types::PinyinInstance;

// pinyin.cpp:3578-3666, phonetic_key_matrix.h:102, pinyin.cpp:554.
/// Remember a user-provided phrase with its current pinyin context.
///
/// # C signature
/// ```c
/// bool pinyin_remember_user_input(pinyin_instance_t * instance,
///                                 const char * phrase,
///                                 gint count);
/// ```
///
/// Remember the phrase for each matching path through the current parse. A count of -1 selects
/// the default of 5; other counts are interpreted as unsigned. Returns true if at least one path
/// was added or merged. Invalid phrase length, missing tokens, no matching path or a store
/// refusal returns false. A later failure can return false after earlier paths were already
/// stored. The live change requires a separately armed save.
///
/// # Aborts
///
/// An empty key matrix aborts after all phrase characters resolve. A character resolving to
/// more than four tokens aborts. After a complete matching path, a whole phrase resolving to
/// more than four tokens aborts.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `phrase` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_remember_user_input(
    instance: *mut PinyinInstance,
    phrase: *const c_char,
    count: c_int,
) -> bool {
    if instance.is_null() {
        return false;
    }

    let phrase = oxpinyin::facade::utf8_to_ucs4(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(phrase) },
    )
    .unwrap_or_default();
    // 074a2219 pinyin.cpp:3677-3681 checks the character count before
    // _pre_compute_tokens. A failed g_utf8_to_ucs4 leaves that count zero.
    // Stop counting at the refusal boundary: oversized known-character
    // strings must not reach the per-character dictionary lookups.
    let phrase_length = phrase.phrase_length();
    if phrase_length == 0 || phrase_length >= oxpinyin::facade::MAX_PHRASE_LENGTH {
        return false;
    }
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`; this shared view ends before the walk's
    // per-step `instance_ref` and the exclusive context borrow in
    // `add_remembered_path`.
    let inst = unsafe { instance_ref(instance) };
    // 074a2219 pinyin.cpp:3098-3130, 3685-3695: _pre_compute_tokens runs
    // next, before anything reads the matrix. Each character's search, in
    // order, goes through `get_first_token`, whose validating
    // `reduce_tokens` asserts on more than four tokens
    // (`phrase_large_table3.h:95`); a search with no token refuses. Search
    // the loaded system and user tables, as lookup_tokens does.
    let mut tokens = Vec::with_capacity(phrase_length);
    for word in phrase.outward_words() {
        let found = inst.core.dict().tokens_for_ucs4(&[word]);
        match found.first() {
            None => return false,
            Some(_) if found.len() > 4 => std::process::abort(),
            Some(&token) => tokens.push(token),
        }
    }
    let Some(columns) = inst.core.matrix_columns() else {
        std::process::abort();
    };
    let count = (count != -1).then(|| u64::from(count.cast_unsigned()));
    // Each callback borrows the context for its own dictionary read or store write.
    let mut pronounces = |key: MatrixKey, index: usize| -> Result<bool, EngineError> {
        // SAFETY: `instance` is live; no other borrow of it is held while
        // this closure runs, and the C caller serializes the call.
        let read = unsafe { instance_ref(instance) };
        let Some(&token) = tokens.get(index) else {
            return Ok(false);
        };
        key_pronounced_by(read.core.dict(), key, token)
    };
    let mut add = |path: &[MatrixKey]| -> Result<bool, EngineError> {
        Ok(
            // SAFETY: Handles guarantees a live instance. Serial calls excludes other calls; the initial
            // reader and each pronunciation reader have ended before this add callback.
            unsafe { add_remembered_path(instance, &phrase, path, count) },
        )
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
/// `:571`, `reduce_tokens`), then the store write. The
/// import path's preflight serves both.
///
/// # Safety
///
/// Under the crate-level rules, Handles: instance is non-NULL and live. Serial
/// calls grant exclusive access to its shared runtime; all earlier instance and
/// runtime readers must have ended.
unsafe fn add_remembered_path(
    instance: *mut PinyinInstance,
    phrase: &oxpinyin::facade::PhraseText,
    path: &[MatrixKey],
    count: Option<u64>,
) -> bool {
    if path.is_empty() || path.len() > oxpinyin::facade::MAX_PHRASE_LENGTH {
        return false;
    }
    let Some(keys) = path
        .iter()
        .map(|key| {
            // The matrix key as it is, with or without a syllable-table
            // row: `_remember_phrase_recur` pushes the matrix's own key and
            // `_add_phrase` stores it (`pinyin.cpp:3594-3606`, `:3647-3663`;
            // `phonetic_key_matrix.cpp:285-306` at 074a2219).
            toned_key(
                oxpinyin::facade::SyllableKey::from_matrix_key(key.key()).index(),
                key.tone(),
            )
        })
        .collect::<Option<Vec<PinyinKey>>>()
    else {
        return false;
    };
    // SAFETY: Handles guarantees a live instance; the helper contract ends
    // every earlier reader, and Serial calls grants exclusive runtime access.
    let ctx = unsafe { crate::state::instance_context_mut(instance) };
    let (phrase, keys) =
        match ctx
            .core
            .plan_add(oxpinyin::facade::USER_DICTIONARY, phrase.clone(), keys)
        {
            ImportPlan::Abort => std::process::abort(),
            ImportPlan::Refuse | ImportPlan::LookupFailed => return false,
            // The plan hands the phrase and keys back unchanged.
            ImportPlan::Add { phrase, keys, .. } => (phrase, keys),
        };
    let user = ctx.core.user_store_mut();
    // `if (-1 == count) count = default_count;` otherwise the `gint` is
    // used as the `guint32` it becomes (`pinyin.cpp:520-524`, the same
    // `_add_phrase` the import path calls): -2 is 4294967294, so a count
    // below -1 is accepted and exported as it went in.
    user.add_phrase(&phrase, &keys, count).is_ok()
}

#[cfg(test)]
mod tests;
