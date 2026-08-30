//! User data persistence: `pinyin_remember_user_input`.

use std::os::raw::{c_char, c_int};

use oxpinyin_user::PinyinKey;

use crate::ffi::{cstr_to_string, ffi_catch};
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
/// `count` of -1 means use the default value.
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
/// a count other than -1 that is negative, an instance without a user store,
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
    ffi_catch(false, || {
        // SAFETY: `phrase` is a C string from the caller (null OK; a null
        // pointer reads as empty, which validation rejects).
        let phrase = unsafe { cstr_to_string(phrase) };
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        let Some(user) = inst.user.as_mut() else {
            return false;
        };
        // Key ids are the dense inventory index (< u16::MAX by
        // construction: 405 complete + 23 initial keys).
        let Some(keys) = inst.session.composition_keys().ok().and_then(|keys| {
            keys.into_iter()
                .map(|key| u16::try_from(key.index()).ok())
                .collect::<Option<Vec<PinyinKey>>>()
        }) else {
            return false;
        };
        let count = if count == -1 {
            None
        } else if count >= 0 {
            Some(count as u64)
        } else {
            return false;
        };
        user.add_phrase(&phrase, &keys, count).is_ok()
    })
}
