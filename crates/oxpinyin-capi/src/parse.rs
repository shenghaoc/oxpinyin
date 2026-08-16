//! Parsing symbols: full pinyin, double pinyin, chewing.

use std::os::raw::c_char;
use std::ptr;

use std::sync::atomic::Ordering;

use crate::ffi::{cstr_to_string, ffi_catch};
use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, PinyinInstance};

/// Shared batch-parse path: reset the instance, type `text`, store the
/// parsed prefix length, and return it.
///
/// The getter must return this snapshot, not a length recomputed from the
/// current session: the fork compares `pinyin_choose_candidate`'s cursor
/// against it (`docs/findings/abi-subset.md` W8 contract).
///
/// The stored length is the session's filtered `fewest_keys` prefix, so
/// `PINYIN_INCOMPLETE` off reports 2 for `nih` rather than the unfiltered
/// graph `consumed()` of 3.
///
/// Resets and clears the candidate snapshot even for empty input, so a
/// prior composition is discarded and the stored length returns to 0.
fn parse_more(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    inst.reset_parse_state();
    if inst
        .session
        .set_incomplete_pinyin(inst.incomplete.load(Ordering::Relaxed))
        .is_err()
    {
        return 0;
    }
    if text.is_empty() {
        return 0;
    }
    let consumed = match inst.session.type_pinyin(text) {
        Ok(_) => inst.session.parsed_prefix_len(),
        Err(_) => 0,
    };
    inst.parsed_len = consumed;
    consumed
}

fn parse_c_string(instance: *mut PinyinInstance, text: *const c_char) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `text` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(text) };
        parse_more(instance, &text)
    })
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
/// Provisional: routes through the same full-pinyin parse path until
/// the engine gains a dedicated double-pinyin parser.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_double_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    parse_c_string(instance, pinyins)
}

/// Parse multiple chewing (bopomofo) inputs.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_chewings(pinyin_instance_t * instance,
///                                    const char * chewings);
/// ```
///
/// Provisional: routes through the same full-pinyin parse path until
/// the engine gains a dedicated chewing parser.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_chewings(
    instance: *mut PinyinInstance,
    chewings: *const c_char,
) -> usize {
    parse_c_string(instance, chewings)
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
    ffi_catch(0, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        inst.parsed_len
    })
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
/// Provisional: always returns false (no chewing keyboard tables yet).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_in_chewing_keyboard(
    instance: *mut PinyinInstance,
    _key: c_char,
    symbols: *mut *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !symbols.is_null() {
        // SAFETY: Null-checked above. Write NULL to indicate no results.
        unsafe {
            *symbols = ptr::null_mut();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::{pinyin_get_parsed_input_length, pinyin_parse_more_full_pinyins};
    use crate::candidates::pinyin_get_candidate;
    use crate::instance::pinyin_reset;
    use crate::sentence::pinyin_guess_candidates;
    use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open};

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
        assert!(pinyin_get_candidate(instance, 0, &mut first));
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
