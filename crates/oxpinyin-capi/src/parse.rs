//! Parsing symbols: full pinyin, double pinyin, chewing.
//!
//! C wrappers over the shared facade-orchestration seams
//! (`oxpinyin_facade::InstanceCore`); the laws themselves — the
//! continue-or-restart rule, the scheme dispatch, the exact-input drive,
//! the LUOMA / `SECONDARY_ZHUYIN` index branch — live there once, shared
//! with the zhuyin facade. This facade's chewing seam forwards
//! [`ToneForwarding::PinFacade`] (`FORCE_TONE` does not cross it — the
//! recorded open divergence); the zhuyin facade forwards the whole word.

use std::os::raw::c_char;

use oxpinyin_facade::ToneForwarding;

use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, PinyinInstance};

fn parse_more(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    // The pin preserves candidate rows and strings across parsing; only a
    // candidate guess, reset or instance destruction invalidates them.
    let parsed = inst.core.parse_full_more(text);
    inst.state.full_aux_options = inst.core.session.options();
    parsed
}

fn parse_c_string(instance: *mut PinyinInstance, text: *const c_char) -> usize {
    if instance.is_null() {
        return 0;
    }

    // The pin parses the raw bytes: what follows an invalid sequence is
    // never reached (`input::parser_prefix`).
    let text =
        oxpinyin_capi_marshal::input::parser_prefix(oxpinyin_capi_marshal::borrow_c_string!(text));
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
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `pinyins` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_parse_more_full_pinyins(
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
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `pinyins` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_parse_more_double_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }

    let text = oxpinyin_capi_marshal::input::parser_prefix(
        oxpinyin_capi_marshal::borrow_c_string!(pinyins),
    );
    // SAFETY: `instance` is non-null (checked above).
    let mut inst = unsafe { instance_mut(instance) };
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
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `chewings` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_parse_more_chewings(
    instance: *mut PinyinInstance,
    chewings: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }

    let text = oxpinyin_capi_marshal::input::parser_prefix(
        oxpinyin_capi_marshal::borrow_c_string!(chewings),
    );
    // SAFETY: `instance` is non-null (checked above).
    let mut inst = unsafe { instance_mut(instance) };
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
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_parsed_input_length(instance: *mut PinyinInstance) -> usize {
    if instance.is_null() {
        return 0;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    inst.core.parsed_input_length()
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
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `symbols` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_in_chewing_keyboard(
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
    #![expect(
        clippy::undocumented_unsafe_blocks,
        reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
    )]

    use std::ptr;

    use rstest::rstest;

    use super::{pinyin_get_parsed_input_length, pinyin_parse_more_full_pinyins};

    use crate::candidates::pinyin_get_candidate;

    use crate::config::pinyin_set_options;

    use crate::cursor::{
        pinyin_get_pinyin_key, pinyin_get_pinyin_key_rest, pinyin_get_pinyin_key_rest_positions,
        pinyin_get_pinyin_string,
    };

    use crate::instance::pinyin_reset;

    use crate::sentence::pinyin_guess_candidates;

    use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, key_slot_guard, open};

    use crate::text::pinyin_get_full_pinyin_auxiliary_text;

    use crate::types::{ChewingKey, ChewingKeyRest, GChar};

    /// An apostrophe-only parse under the default options. The pin's
    /// separator branch moves the parse one byte per `'` and places no key
    /// (`storage/pinyin_parser2.cpp:238-250` at 074a2219);
    /// `pinyin_parse_more_full_pinyins` answers that length and stores it for
    /// `pinyin_get_parsed_input_length` (`pinyin.cpp:1507-1511`, `:1524`,
    /// `:1611-1613`). Each case holds the pin's consumed and parsed lengths
    /// from its frozen observation (pin 074a2219, the pinned
    /// libpinyin-data). No
    /// database is read, so every cell runs this.
    #[rstest]
    // zero-key-guess/pinyin/default/'/guess_sentence; pinyin_parser2.cpp:238-250.
    #[case::one("'", 1, 1)]
    // zero-key-guess/pinyin/default/''/guess_sentence; pinyin_parser2.cpp:238-250.
    #[case::two("''", 2, 2)]
    // zero-key-guess/pinyin/default/'''/guess_sentence; pinyin_parser2.cpp:238-250.
    #[case::three("'''", 3, 3)]
    fn an_apostrophe_only_parse_matches_the_pin(
        #[case] input: &str,
        #[case] consumed: usize,
        #[case] parsed_length: usize,
    ) {
        let user_dir = TempUserDir::new("apostrophe-parse");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        let text = cstr(input);
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) },
            consumed
        );
        assert_eq!(
            unsafe { pinyin_get_parsed_input_length(instance) },
            parsed_length
        );

        unsafe { crate::instance::pinyin_free_instance(instance) };
        unsafe { crate::context::pinyin_fini(context) };
    }

    #[test]
    fn parsed_input_length_stores_the_parse_result_and_clears_on_reset() {
        let user_dir = TempUserDir::new("parsed-len");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        assert_eq!(unsafe { pinyin_get_parsed_input_length(instance) }, 0);

        let nihao = cstr("nihao");
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, nihao.as_ptr()) },
            5
        );
        assert_eq!(unsafe { pinyin_get_parsed_input_length(instance) }, 5);

        assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
        let mut first = ptr::null_mut();
        assert!(unsafe { pinyin_get_candidate(instance, 0, &raw mut first) });
        assert!(unsafe { crate::candidates::pinyin_choose_candidate(instance, 0, first) } > 0);
        assert_eq!(unsafe { pinyin_get_parsed_input_length(instance) }, 5);

        let partial = cstr("nihaoXYZ");
        let consumed = unsafe { pinyin_parse_more_full_pinyins(instance, partial.as_ptr()) };
        assert_eq!(
            unsafe { pinyin_get_parsed_input_length(instance) },
            consumed
        );

        assert!(unsafe { pinyin_reset(instance) });
        assert_eq!(unsafe { pinyin_get_parsed_input_length(instance) }, 0);

        let empty = cstr("");
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, empty.as_ptr()) },
            0
        );
        assert_eq!(unsafe { pinyin_get_parsed_input_length(instance) }, 0);
        assert_eq!(
            unsafe { pinyin_get_parsed_input_length(ptr::null_mut()) },
            0
        );

        unsafe { crate::instance::pinyin_free_instance(instance) };
        unsafe { crate::context::pinyin_fini(context) };
    }

    /// One offset of a key walk as `tools/oracle/pin_capture.py`'s `key_walk` observes it: the
    /// key's packed word (`None` when the query answers false), the key rest's raw span (asked only
    /// of a non-null rest) and the key's pinyin string (`None` when the renderer answers false).
    #[derive(Debug, PartialEq)]
    struct Step {
        key: Option<u16>,
        span: Option<(u16, u16)>,
        pinyin: Option<String>,
    }

    /// Every offset from 0 to the parsed length, the calls in `key_walk`'s order.
    fn walk(instance: *mut crate::types::PinyinInstance, parsed: usize) -> Vec<Step> {
        (0..=parsed)
            .map(|offset| {
                let mut key: *mut ChewingKey = ptr::null_mut();
                let found = unsafe { pinyin_get_pinyin_key(instance, offset, &raw mut key) };
                assert_eq!(
                    found,
                    !key.is_null(),
                    "offset {offset}: answer and out-param"
                );
                let mut rest: *mut ChewingKeyRest = ptr::null_mut();
                let rest_found =
                    unsafe { pinyin_get_pinyin_key_rest(instance, offset, &raw mut rest) };
                assert_eq!(
                    rest_found,
                    !rest.is_null(),
                    "offset {offset}: rest and out-param"
                );
                let span = (!rest.is_null()).then(|| {
                    let (mut begin, mut end) = (0_u16, 0_u16);
                    assert!(unsafe {
                        pinyin_get_pinyin_key_rest_positions(
                            instance,
                            rest,
                            &raw mut begin,
                            &raw mut end,
                        )
                    });
                    (begin, end)
                });
                let (packed, pinyin) = if key.is_null() {
                    (None, None)
                } else {
                    // SAFETY: a true answer points `key` at the instance's key slot.
                    let packed = unsafe { (*key).packed };
                    let mut text: *mut GChar = ptr::null_mut();
                    let rendered =
                        unsafe { pinyin_get_pinyin_string(instance, key, &raw mut text) };
                    (
                        Some(packed),
                        rendered.then(|| crate::ffi::take_owned_cstr(text)),
                    )
                };
                Step {
                    key: packed,
                    span,
                    pinyin,
                }
            })
            .collect()
    }

    /// `OPTION_BASE` of `tools/oracle/pin_capture.py`: PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
    /// USE_RESPLIT_TABLE, the base word of the option cases (`pinyin_set_options`, `pinyin.cpp:1299-1307` at 074a2219).
    const OPTION_BASE: u32 = 0x188;

    /// PINYIN_CORRECT_ALL | PINYIN_AMB_S_SH | PINYIN_AMB_Z_ZH (`pinyin_custom2.h:51-52,79`).
    const OPTION_GATES: u32 = 0x1FE0_1800;

    /// The parser-table rows one option bit gates, each with its bit on, then with every other gate
    /// bit on and its own off. A gated row is admitted only when its flag is in the options
    /// (`storage/pinyin_parser2.cpp:49-55` at 074a2219); it then parses to its own key
    /// (`:143-147`), and the DP prefers the fewest keys, then the least distance (`:298-309`).
    /// With the row refused, PINYIN_INCOMPLETE (in the base word) lets an initial stand alone
    /// (`:43-44`). The auxiliary text renders the keys at the parsed length
    /// (`pinyin.cpp:3381-3437`). Pin capture of 2026-11-07, runs 1 and 2 identical, observations
    /// `option-row/on/<input>`, `option-row/off/<input>` and `option-distance/chonga`.
    #[rstest]
    #[case::gn_ng_on("agn", OPTION_BASE | 1 << 21, 3, &[(512, 0, 3, "ang")], "ang |")]
    #[case::gn_ng_off(
        "agn",
        (OPTION_BASE | OPTION_GATES) & !(1 << 21),
        3,
        &[(128, 0, 1, "a"), (7, 1, 2, "g"), (11, 2, 3, "n")],
        "a g n |"
    )]
    #[case::gn_ng_target_outside_the_inventory("egn", OPTION_BASE | 1 << 21, 3, &[(1280, 0, 3, "eng")], "eng |")]
    #[case::mg_ng_on("amg", OPTION_BASE | 1 << 22, 3, &[(512, 0, 3, "ang")], "ang |")]
    #[case::mg_ng_off(
        "amg",
        (OPTION_BASE | OPTION_GATES) & !(1 << 22),
        3,
        &[(128, 0, 1, "a"), (10, 1, 2, "m"), (7, 2, 3, "g")],
        "a m g |"
    )]
    #[case::iou_iu_on("diou", OPTION_BASE | 1 << 23, 4, &[(1956, 0, 4, "diu")], "diu |")]
    #[case::iou_iu_off(
        "diou",
        (OPTION_BASE | OPTION_GATES) & !(1 << 23),
        4,
        &[(36, 0, 2, "di"), (1920, 2, 4, "ou")],
        "di ou |"
    )]
    #[case::uei_ui_on("duei", OPTION_BASE | 1 << 24, 4, &[(1092, 0, 4, "dui")], "dui |")]
    #[case::uei_ui_off(
        "duei",
        (OPTION_BASE | OPTION_GATES) & !(1 << 24),
        4,
        &[(68, 0, 2, "du"), (1024, 2, 4, "ei")],
        "du ei |"
    )]
    #[case::uen_un_on("duen", OPTION_BASE | 1 << 25, 4, &[(1220, 0, 4, "dun")], "dun |")]
    #[case::uen_un_off(
        "duen",
        (OPTION_BASE | OPTION_GATES) & !(1 << 25),
        4,
        &[(68, 0, 2, "du"), (1152, 2, 4, "en")],
        "du en |"
    )]
    #[case::ue_ve_on("lue", OPTION_BASE | 1 << 26, 3, &[(876, 0, 3, "lve")], "lve |")]
    #[case::ue_ve_off(
        "lue",
        (OPTION_BASE | OPTION_GATES) & !(1 << 26),
        3,
        &[(76, 0, 2, "lu"), (768, 2, 3, "e")],
        "lu e |"
    )]
    #[case::v_u_on("jv", OPTION_BASE | 1 << 27, 2, &[(105, 0, 2, "ju")], "ju |")]
    #[case::v_u_off("jv", (OPTION_BASE | OPTION_GATES) & !(1 << 27), 1, &[(9, 0, 1, "j")], "j |")]
    #[case::on_ong_on("zon", OPTION_BASE | 1 << 28, 3, &[(1814, 0, 3, "zong")], "zong |")]
    #[case::on_ong_off(
        "zon",
        (OPTION_BASE | OPTION_GATES) & !(1 << 28),
        3,
        &[(22, 0, 1, "z"), (1664, 1, 2, "o"), (11, 2, 3, "n")],
        "z o n |"
    )]
    #[case::s_sh_on("sua", OPTION_BASE | 1 << 11, 3, &[(209, 0, 3, "shua")], "shua |")]
    #[case::s_sh_off(
        "sua",
        (OPTION_BASE | OPTION_GATES) & !(1 << 11),
        3,
        &[(80, 0, 2, "su"), (128, 2, 3, "a")],
        "su a |"
    )]
    #[case::z_zh_on("zua", OPTION_BASE | 1 << 12, 3, &[(215, 0, 3, "zhua")], "zhua |")]
    #[case::z_zh_off(
        "zua",
        (OPTION_BASE | OPTION_GATES) & !(1 << 12),
        3,
        &[(86, 0, 2, "zu"), (128, 2, 3, "a")],
        "zu a |"
    )]
    #[case::distance_tie(
        "chonga",
        OPTION_BASE | 1 << 28,
        6,
        &[(1795, 0, 5, "chong"), (128, 5, 6, "a")],
        "chong a |"
    )]
    fn a_gated_parser_row_follows_its_option_bit(
        #[case] input: &str,
        #[case] options: u32,
        #[case] parsed: usize,
        #[case] keys: &[(u16, u16, u16, &str)],
        #[case] aux: &str,
    ) {
        let _slots = key_slot_guard();
        let user_dir = TempUserDir::new("gated-row");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert!(unsafe { pinyin_set_options(context, options) });
        let text = cstr(input);
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) },
            parsed
        );
        let found: Vec<(u16, u16, u16, String)> = walk(instance, parsed)
            .into_iter()
            .filter_map(|step| {
                let (begin, end) = step.span?;
                Some((step.key?, begin, end, step.pinyin?))
            })
            .collect();
        let expected: Vec<(u16, u16, u16, String)> = keys
            .iter()
            .map(|&(key, begin, end, pinyin)| (key, begin, end, pinyin.to_owned()))
            .collect();
        assert_eq!(found, expected);
        let mut text: *mut GChar = ptr::null_mut();
        assert!(unsafe { pinyin_get_full_pinyin_auxiliary_text(instance, parsed, &raw mut text) });
        assert_eq!(crate::ffi::take_owned_cstr(text), aux);
        unsafe { crate::instance::pinyin_free_instance(instance) };
        unsafe { crate::context::pinyin_fini(context) };
    }
}
