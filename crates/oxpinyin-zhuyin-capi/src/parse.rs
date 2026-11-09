//! Parsing symbols: full pinyin, chewing (bopomofo), and the shared
//! batch-parse shell.

use std::os::raw::c_char;

use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, ZhuyinInstance};

fn parse_chewing_more(instance: *mut ZhuyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    // The parse path clears the candidate snapshot before anything else —
    // main's `begin_parse` did this through `reset_parse_state`; the core
    // seam cannot see this layer's snapshot, so the clear lives here.
    inst.state.candidates.clear();
    inst.core
        .parse_chewing_more(text, oxpinyin_facade::ToneForwarding::ZhuyinFacade)
}

fn parse_full_more(instance: *mut ZhuyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    // Parse-path snapshot clear (main's begin_parse law).
    inst.state.candidates.clear();
    inst.core.parse_full_more(text)
}

/// Parse multiple full pinyins.
/// # C signature
/// ```c
/// size_t zhuyin_parse_more_full_pinyins(zhuyin_instance_t * instance,
///                                       const char * pinyins);
/// ```
///
/// Returns number of bytes consumed, 0 on failure.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_parse_more_full_pinyins(
    instance: *mut ZhuyinInstance,
    pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }

    let text = oxpinyin_capi_marshal::input::parser_prefix(
        oxpinyin_capi_marshal::borrow_c_string!(pinyins),
    );
    parse_full_more(instance, &text)
}

/// Parse multiple chewing (bopomofo) inputs.
///
/// # C signature
/// ```c
/// size_t zhuyin_parse_more_chewings(zhuyin_instance_t * instance,
///                                   const char * chewings);
/// ```
///
/// Parses through [`oxpinyin_core::ZhuyinParser`] and drives the session
/// with the apostrophe-joined full-pinyin spelling.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_parse_more_chewings(
    instance: *mut ZhuyinInstance,
    chewings: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }

    let text = oxpinyin_capi_marshal::input::parser_prefix(
        oxpinyin_capi_marshal::borrow_c_string!(chewings),
    );
    parse_chewing_more(instance, &text)
}

/// Get the parsed length of the input.
///
/// # C signature
/// ```c
/// size_t zhuyin_get_parsed_input_length(zhuyin_instance_t * instance);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_parsed_input_length(instance: *mut ZhuyinInstance) -> usize {
    if instance.is_null() {
        return 0;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    inst.core.parsed_input_length()
}

/// Check whether an input key is in the current chewing keyboard scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_in_chewing_keyboard(zhuyin_instance_t * instance,
///                                 const char key,
///                                 gchar *** symbols);
/// ```
///
/// `key` is a plain `char` value (not a pointer).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_in_chewing_keyboard(
    instance: *mut ZhuyinInstance,
    key: std::os::raw::c_char,
    symbols: *mut *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // `c_char` is `i8` on some targets and `u8` on others (aarch64
    // Linux among them); `as u8` is a lossless reinterpret on both,
    // and the cast is not "unnecessary" on the targets where it is
    // `i8`.
    #[allow(
        clippy::unnecessary_cast,
        clippy::cast_sign_loss,
        reason = "c_char is u8 on aarch64 Linux and i8 elsewhere, so a                   TryFrom conversion would fail to compile on one of them;                   the i8-to-u8 wrap is byte identity, and wrapped negatives                   (>= 0x80) match no keyboard table entry"
    )]
    let mapped = inst.core.in_keyboard(key as u8);
    if symbols.is_null() {
        return !mapped.is_empty();
    }
    // `in_chewing_scheme` (`zhuyin_parser2.cpp:301-332`, `:496-544`,
    // `:798-851`): the symbols NULL, then the `GPtrArray` built with the
    // pin's GLib calls, handed out for `g_strfreev`; NULL with `false`
    // where no symbol matched.
    let vector = crate::ffi::symbol_vector(&mapped);
    // SAFETY: Null-checked above.
    unsafe {
        *symbols = vector;
    }
    !mapped.is_empty()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{zhuyin_get_parsed_input_length, zhuyin_parse_more_full_pinyins};
    use crate::test_support::{close, cstr, open};

    /// An apostrophe-only parse under the default options. libzhuyin's
    /// full-pinyin parse runs the same separator branch, which moves the
    /// parse one byte per `'` and places no key
    /// (`storage/pinyin_parser2.cpp:238-250` at 074a2219);
    /// `zhuyin_parse_more_full_pinyins` answers that length and stores it for
    /// `zhuyin_get_parsed_input_length` (`zhuyin.cpp:1030`, `:1034`, `:1042`,
    /// `:1083-1085`). Each case holds the pin's consumed and parsed lengths
    /// from its frozen observation (pin 074a2219, the pinned
    /// libpinyin-data). No
    /// database is read, so every cell runs this.
    #[rstest]
    // zero-key-guess/zhuyin/default/'/guess_sentence; pinyin_parser2.cpp:238-250.
    #[case::one("'", 1, 1)]
    // zero-key-guess/zhuyin/default/''/guess_sentence; pinyin_parser2.cpp:238-250.
    #[case::two("''", 2, 2)]
    // zero-key-guess/zhuyin/default/'''/guess_sentence; pinyin_parser2.cpp:238-250.
    #[case::three("'''", 3, 3)]
    fn an_apostrophe_only_parse_matches_the_pin(
        #[case] input: &str,
        #[case] consumed: usize,
        #[case] parsed_length: usize,
    ) {
        let (context, instance) = open();

        let text = cstr(input);
        assert_eq!(
            zhuyin_parse_more_full_pinyins(instance, text.as_ptr()),
            consumed
        );
        assert_eq!(zhuyin_get_parsed_input_length(instance), parsed_length);

        close(context, instance);
    }
}
