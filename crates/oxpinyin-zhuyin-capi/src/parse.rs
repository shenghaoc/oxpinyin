//! Parsing symbols: full pinyin, chewing (bopomofo), and the shared
//! batch-parse shell.

use std::os::raw::c_char;

use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, ZhuyinInstance};

fn parse_chewing_more(instance: *mut ZhuyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    // The pin preserves candidate rows and strings across parsing; only a
    // candidate guess, reset or instance destruction invalidates them.
    inst.core
        .parse_chewing_more(text, oxpinyin_facade::ToneForwarding::ZhuyinFacade)
}

fn parse_full_more(instance: *mut ZhuyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    inst.core.parse_full_more_zhuyin(text)
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

    // At 074a2219, zhuyin.cpp:1017-1081,1460-1471,1718-1729,1865-1870: parsing
    // leaves candidate storage alone; guessing and reset free it. Keep the
    // original C pointers, not just copied text or a row fetched after parse.
    #[rstest]
    #[case::full(crate::parse::zhuyin_parse_more_full_pinyins, "ni3")]
    #[case::chewing(crate::parse::zhuyin_parse_more_chewings, "su3")]
    fn reparse_preserves_candidate_strings_until_guess_or_reset(
        #[case] parse: extern "C" fn(
            *mut crate::types::ZhuyinInstance,
            *const std::ffi::c_char,
        ) -> usize,
        #[case] input: &str,
    ) {
        use crate::candidates::{
            zhuyin_get_candidate, zhuyin_get_candidate_string, zhuyin_get_n_candidate,
        };
        use std::ffi::CStr;
        use std::ptr;

        let (context, instance) = open();
        assert_eq!(
            zhuyin_parse_more_full_pinyins(instance, cstr("ni3hao3").as_ptr()),
            7
        );
        assert!(crate::sentence::zhuyin_guess_candidates_after_cursor(
            instance, 0
        ));
        let mut count = 0;
        assert!(zhuyin_get_n_candidate(instance, &raw mut count));
        assert!(count > 0);
        let mut original = ptr::null_mut();
        assert!(zhuyin_get_candidate(instance, 0, &raw mut original));
        let mut string = ptr::null();
        assert!(zhuyin_get_candidate_string(
            instance,
            original,
            &raw mut string
        ));
        assert!(!string.is_null());
        // SAFETY: the freshly guessed candidate owns a NUL-terminated string.
        let expected = unsafe { CStr::from_ptr(string) }.to_owned();

        assert_eq!(parse(instance, cstr(input).as_ptr()), input.len());
        let mut after = 0;
        assert!(zhuyin_get_n_candidate(instance, &raw mut after));
        assert_eq!(after, count, "a reparse preserves the original list");
        let mut current = ptr::null_mut();
        assert!(zhuyin_get_candidate(instance, 0, &raw mut current));
        assert_eq!(current, original);
        let mut current_string = ptr::null();
        assert!(zhuyin_get_candidate_string(
            instance,
            original,
            &raw mut current_string
        ));
        assert_eq!(current_string, string);
        // SAFETY: parsing has preserved the original snapshot, as checked above;
        // no guess or reset has invalidated either saved pointer.
        assert_eq!(unsafe { CStr::from_ptr(string) }, expected.as_c_str());

        // A successful guess replaces the old list with the reparsed input's
        // candidates. Read only newly obtained pointers after this boundary.
        assert!(crate::sentence::zhuyin_guess_candidates_after_cursor(
            instance, 0
        ));
        assert!(zhuyin_get_candidate(instance, 0, &raw mut current));
        assert!(zhuyin_get_candidate_string(
            instance,
            current,
            &raw mut current_string
        ));
        assert_ne!(
            // SAFETY: this pointer comes from the new candidate list.
            unsafe { CStr::from_ptr(current_string) },
            expected.as_c_str()
        );

        // Even a refused empty-input guess invalidates the preceding list.
        assert!(zhuyin_get_n_candidate(instance, &raw mut count));
        assert!(count > 0);
        assert_eq!(parse(instance, cstr("").as_ptr()), 0);
        assert!(zhuyin_get_n_candidate(instance, &raw mut after));
        assert_eq!(after, count);
        assert!(!crate::sentence::zhuyin_guess_candidates_after_cursor(
            instance, 0
        ));
        assert!(zhuyin_get_n_candidate(instance, &raw mut after));
        assert_eq!(after, 0);
        assert!(!zhuyin_get_candidate(instance, 0, &raw mut current));
        assert!(current.is_null());

        assert_eq!(
            zhuyin_parse_more_full_pinyins(instance, cstr("ni3hao3").as_ptr()),
            7
        );
        assert!(crate::sentence::zhuyin_guess_candidates_after_cursor(
            instance, 0
        ));
        assert!(crate::instance::zhuyin_reset(instance));
        assert!(zhuyin_get_n_candidate(instance, &raw mut after));
        assert_eq!(after, 0);
        assert!(!zhuyin_get_candidate(instance, 0, &raw mut current));
        assert!(current.is_null());
        close(context, instance);
    }

    // 074a2219 zhuyin.cpp:1058-1081 preserves the candidate;
    // :1646-1666 chooses its own token/span and returns its original end.
    #[rstest]
    #[case::unchanged("su3cl3", false, 6, false)]
    #[case::shorter("su3", false, 3, true)]
    #[case::unconsumed_suffix("ni3!hao3", true, 3, true)]
    #[case::zero_keys("''''''''", true, 8, true)]
    fn reparse_keeps_the_reanchored_candidates_selection(
        #[case] reparse: &str,
        #[case] full_pinyin: bool,
        #[case] consumed: usize,
        #[case] refused: bool,
        #[values(false, true)] before: bool,
    ) {
        use crate::candidates::{
            zhuyin_choose_candidate, zhuyin_clear_constraint, zhuyin_get_candidate,
        };
        use crate::sentence::{
            zhuyin_guess_candidates_after_cursor, zhuyin_guess_candidates_before_cursor,
        };
        use oxpinyin_core::Dictionary;
        use std::ptr;

        let (context, instance) = open();
        let input = cstr("su3cl3");
        assert_eq!(
            super::zhuyin_parse_more_chewings(instance, input.as_ptr()),
            6
        );
        assert!(if before {
            zhuyin_guess_candidates_before_cursor(instance, 6)
        } else {
            zhuyin_guess_candidates_after_cursor(instance, 3)
        });
        let (index, token) = {
            // SAFETY: live instance immediately after the guess.
            let inst = unsafe { crate::state::instance_ref(instance) };
            let index = inst
                .state
                .candidates
                .iter()
                .position(|row| row.text_str() == Ok("好"))
                .expect("好 at offset 3");
            (index, inst.core.dict.tokens_for_text("好")[0])
        };
        let mut original = ptr::null_mut();
        assert!(zhuyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut original
        ));
        let reparsed = cstr(reparse);
        assert_eq!(
            if full_pinyin {
                super::zhuyin_parse_more_full_pinyins(instance, reparsed.as_ptr())
            } else {
                super::zhuyin_parse_more_chewings(instance, reparsed.as_ptr())
            },
            consumed
        );
        // No candidate guess: selection must still use 好 over the second key.
        // Unlike Pinyin, refusal still returns the original m_begin/m_end.
        let expected_tokens = [token];
        // The pin leaves the candidates intact on choose, so a second
        // choose still sees this same token and span (zhuyin.cpp:1646-1666).
        for _ in 0..2 {
            assert_eq!(
                zhuyin_choose_candidate(instance, if before { 6 } else { 3 }, original),
                if before { 3 } else { 6 }
            );
            assert_eq!(
                // SAFETY: the choose completed and the instance is still live.
                unsafe { crate::state::instance_ref(instance) }
                    .core
                    .session
                    .selected_tokens(),
                if refused {
                    &[][..]
                } else {
                    &expected_tokens[..]
                }
            );
        }
        assert!(!zhuyin_clear_constraint(instance, 0));
        assert_eq!(zhuyin_clear_constraint(instance, 3), !refused);
        close(context, instance);
    }

    /// A choose answers the row's own `m_end` (after the cursor) or
    /// `m_begin` (before it), fixed at the guess, and never the caller's
    /// offset, which `zhuyin_choose_candidate` overwrites (074a2219
    /// `zhuyin.cpp:1649-1666`); here the caller names 0 for a row over
    /// `[3, 6)`. A reparse that shortens the first key moves the key spans
    /// under the row, and the answer stays the row's own.
    #[rstest]
    fn a_choose_at_another_offset_answers_the_rows_own_end(#[values(false, true)] before: bool) {
        use crate::candidates::{zhuyin_choose_candidate, zhuyin_get_candidate};
        use crate::sentence::{
            zhuyin_guess_candidates_after_cursor, zhuyin_guess_candidates_before_cursor,
        };
        use std::ptr;

        let (context, instance) = open();
        let input = cstr("su3cl3");
        assert_eq!(
            super::zhuyin_parse_more_chewings(instance, input.as_ptr()),
            6
        );
        assert!(if before {
            zhuyin_guess_candidates_before_cursor(instance, 6)
        } else {
            zhuyin_guess_candidates_after_cursor(instance, 3)
        });
        let index = {
            // SAFETY: live instance immediately after the guess.
            let inst = unsafe { crate::state::instance_ref(instance) };
            inst.state
                .candidates
                .iter()
                .position(|row| row.text_str() == Ok("好"))
                .expect("好 over the second key")
        };
        let mut row = ptr::null_mut();
        assert!(zhuyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut row
        ));
        let expected = if before { 3 } else { 6 };
        assert_eq!(zhuyin_choose_candidate(instance, 0, row), expected);
        let shorter = cstr("83cl3");
        assert_eq!(
            super::zhuyin_parse_more_chewings(instance, shorter.as_ptr()),
            5
        );
        assert_eq!(zhuyin_choose_candidate(instance, 0, row), expected);
        close(context, instance);
    }

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
