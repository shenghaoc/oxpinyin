//! Parsing symbols: full pinyin, double pinyin, chewing.
//!
//! C wrappers over the shared facade-orchestration seams
//! (`oxpinyin_facade::InstanceCore`); the laws themselves — the
//! continue-or-restart rule, the scheme dispatch, the exact-input drive,
//! the LUOMA / `SECONDARY_ZHUYIN` index branch — live there once, shared
//! with the zhuyin facade. This facade's chewing seam forwards
//! [`ToneForwarding::PinFacade`] (FORCE_TONE does not cross it — the
//! recorded open divergence); the zhuyin facade forwards the whole word.

use std::os::raw::c_char;

use oxpinyin_facade::ToneForwarding;

use crate::ffi::cstr_to_string;
use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, PinyinInstance};

fn parse_more(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    // The parse path clears the candidate snapshot before anything else —
    // main's `begin_parse` did this through `reset_parse_state`; the core
    // seam cannot see this layer's snapshot, so the clear lives here.
    inst.candidates.clear();
    inst.core.parse_full_more(text)
}

fn parse_c_string(instance: *mut PinyinInstance, text: *const c_char) -> usize {
    if instance.is_null() {
        return 0;
    }

    // SAFETY: `text` is a C string from the caller (null OK).
    let text = unsafe { cstr_to_string(text) };
    parse_more(instance, &text)
}

/// Parse multiple full pinyins.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_full_pinyins(pinyin_instance_t * instance,
///                                       const char * pinyins);
/// ```
///
/// Returns number of bytes consumed, 0 on failure.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_full_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    parse_c_string(instance, pinyins)
}

/// Parse multiple double pinyins.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_double_pinyins(pinyin_instance_t * instance,
///                                         const char * pinyins);
/// ```
///
/// Parses through [`oxpinyin_core::DoublePinyinParser`] and drives the
/// session with the apostrophe-joined full-pinyin spelling.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_double_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }

    // SAFETY: `pinyins` is a C string from the caller (null OK).
    let text = unsafe { cstr_to_string(pinyins) };
    // SAFETY: `instance` is non-null (checked above).
    let inst = unsafe { instance_mut(instance) };
    // Parse-path snapshot clear (main's begin_parse law).
    inst.candidates.clear();
    inst.core.parse_double_more(&text)
}

/// Parse multiple chewing (bopomofo) inputs.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_chewings(pinyin_instance_t * instance,
///                                    const char * chewings);
/// ```
///
/// Parses through [`oxpinyin_core::ZhuyinParser`] (STANDARD) and drives
/// the session with the apostrophe-joined full-pinyin spelling.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_chewings(
    instance: *mut PinyinInstance,
    chewings: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }

    // SAFETY: `chewings` is a C string from the caller (null OK).
    let text = unsafe { cstr_to_string(chewings) };
    // SAFETY: `instance` is non-null (checked above).
    let inst = unsafe { instance_mut(instance) };
    // Parse-path snapshot clear (main's begin_parse law).
    inst.candidates.clear();
    inst.core
        .parse_chewing_more(&text, ToneForwarding::PinFacade)
}

/// Get the parsed length of the input.
///
/// # C signature
/// ```c
/// size_t pinyin_get_parsed_input_length(pinyin_instance_t * instance);
/// ```
///
/// Returns the byte count of raw input consumed by the most recent parse
/// call, `0` before any parse and after [`pinyin_reset`](crate::instance::pinyin_reset),
/// matching upstream `pinyin.cpp:1611-1613` and reset `pinyin.cpp:2692`.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_parsed_input_length(instance: *mut PinyinInstance) -> usize {
    if instance.is_null() {
        return 0;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    inst.core.parsed_len
}

/// Check whether an input key is in the current chewing keyboard scheme.
///
/// # C signature
/// ```c
/// bool pinyin_in_chewing_keyboard(pinyin_instance_t * instance,
///                                  const char key,
///                                  gchar *** symbols);
/// ```
///
/// `key` is a plain `char` value (not a pointer).
/// `symbols` receives a NULL-terminated string array; caller frees with
/// `g_strfreev`.
///
/// Returns the Zhuyin symbol(s) for `key` under the current scheme.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_in_chewing_keyboard(
    instance: *mut PinyinInstance,
    key: c_char,
    symbols: *mut *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // `c_char` is `i8` on some targets and `u8` on others (aarch64
    // Linux among them); `as u8` is a lossless reinterpret on both,
    // and the cast is not "unnecessary" on the targets where it is
    // `i8`.
    #[allow(clippy::unnecessary_cast)]
    let mapped = inst.core.in_keyboard(key as u8);
    if mapped.is_empty() {
        if !symbols.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *symbols = std::ptr::null_mut();
            }
        }
        return false;
    }

    if !symbols.is_null() {
        let ptr = crate::ffi::owned_cstr_list(&mapped);
        if ptr.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *symbols = std::ptr::null_mut();
            }
            return false;
        }
        // SAFETY: `owned_cstr_list` is a malloc array of malloc
        // strings; the caller releases both with g_strfreev.
        unsafe {
            *symbols = ptr;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::{pinyin_get_parsed_input_length, pinyin_parse_more_full_pinyins};
    use crate::candidates::{pinyin_get_candidate, pinyin_get_n_candidate};
    use crate::instance::pinyin_reset;
    use crate::sentence::pinyin_guess_candidates;
    use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open};

    /// A re-parse clears the candidate snapshot even when no guess follows —
    /// upstream's `begin_parse` reset law, which a reads-between-parse-and-
    /// guess consumer observes as an empty list. Regression pin for the
    /// extraction, where the clear moved out of `begin_parse`'s reach.
    #[test]
    fn reparse_clears_the_candidate_snapshot_before_any_guess() {
        let user_dir = TempUserDir::new("reparse-clears");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        let nihao = cstr("nihao");
        assert!(pinyin_parse_more_full_pinyins(instance, nihao.as_ptr()) > 0);
        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let mut populated = 0_u32;
        assert!(pinyin_get_n_candidate(instance, &raw mut populated));
        assert!(populated > 0, "a guess populates the snapshot");

        // Parse again with no guess in between: the snapshot must be empty.
        let ni = cstr("ni");
        assert!(pinyin_parse_more_full_pinyins(instance, ni.as_ptr()) > 0);
        let mut after = u32::MAX;
        assert!(pinyin_get_n_candidate(instance, &raw mut after));
        assert_eq!(after, 0, "parse clears the snapshot without a guess");

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    #[test]
    fn parsed_input_length_stores_the_parse_result_and_clears_on_reset() {
        let user_dir = TempUserDir::new("parsed-len");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        assert_eq!(pinyin_get_parsed_input_length(instance), 0);

        let nihao = cstr("nihao");
        assert_eq!(pinyin_parse_more_full_pinyins(instance, nihao.as_ptr()), 5);
        assert_eq!(pinyin_get_parsed_input_length(instance), 5);

        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let mut first = ptr::null_mut();
        assert!(pinyin_get_candidate(instance, 0, &raw mut first));
        assert!(crate::candidates::pinyin_choose_candidate(instance, 0, first) > 0);
        assert_eq!(pinyin_get_parsed_input_length(instance), 5);

        let partial = cstr("nihaoXYZ");
        let consumed = pinyin_parse_more_full_pinyins(instance, partial.as_ptr());
        assert_eq!(pinyin_get_parsed_input_length(instance), consumed);

        assert!(pinyin_reset(instance));
        assert_eq!(pinyin_get_parsed_input_length(instance), 0);

        let empty = cstr("");
        assert_eq!(pinyin_parse_more_full_pinyins(instance, empty.as_ptr()), 0);
        assert_eq!(pinyin_get_parsed_input_length(instance), 0);
        assert_eq!(pinyin_get_parsed_input_length(ptr::null_mut()), 0);

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
