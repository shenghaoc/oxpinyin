//! Scheme-setter contract tests: the abort / no-op slots of the pinned
//! oracle, locked as `false` + unchanged state on the oxpinyin side
//! (`docs/findings/upstream-divergences.md`, the #109 contract-lock).
//!
//! Oracle differentials are impossible for these inputs — the pin-built
//! `.so` SIGABRTs (double 30, zhuyin 7 and out-of-enum, full-pinyin
//! out-of-enum) or half-mutates behind an unconditional `true`
//! (double out-of-enum clears the live scheme's fallback table). These
//! tests are oxpinyin-side only.

use std::os::raw::c_int;

use pinyin_capi::{
    GChar, pinyin_parse_more_chewings, pinyin_parse_more_double_pinyins,
    pinyin_parse_more_full_pinyins, pinyin_set_double_pinyin_scheme, pinyin_set_full_pinyin_scheme,
    pinyin_set_zhuyin_scheme,
};

use crate::common::{TempUserDir, cstr, open};

/// DOUBLE_PINYIN_CUSTOMIZED: upstream aborts inside
/// `DoublePinyinParser2::set_scheme` (`pinyin_parser2.cpp:611-612`)
/// mid-call, after the table pointers for the *current* scheme were
/// already disturbed. oxpinyin: `false`, nothing changes.
#[test]
fn double_customized_is_rejected_without_disturbing_the_live_scheme() {
    let user_dir = TempUserDir::new("contract-double-30");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // ZRM is the fallback-bearing scheme: parse "aa" through its
    // fallback table ("aa" -> a).
    assert!(pinyin_set_double_pinyin_scheme(context, 1));
    let aa = cstr("aa");
    assert_eq!(pinyin_parse_more_double_pinyins(instance, aa.as_ptr()), 2);

    assert!(!pinyin_set_double_pinyin_scheme(context, 30));
    // The live scheme — fallback included — is unchanged.
    assert_eq!(pinyin_parse_more_double_pinyins(instance, aa.as_ptr()), 2);

    // A later valid set still sticks after the rejection (MS: "ni"
    // is n+i; MS has no fallback table, unlike ZRM).
    assert!(pinyin_set_double_pinyin_scheme(context, 2));
    let ni = cstr("ni");
    assert_eq!(pinyin_parse_more_double_pinyins(instance, ni.as_ptr()), 2);

    pinyin_capi::pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

/// Out-of-enum double values (0, 7–29, 31+): upstream clears
/// `m_fallback_table` first (`pinyin_parser2.cpp:582`), returns `false`
/// from the parser, and the API wrapper still answers `true`
/// (`pinyin.cpp:1154-1159`) — the caller believes the call failed or
/// succeeded unreadably while the live scheme silently lost its
/// fallback. oxpinyin: `false` and the whole scheme state is unchanged.
#[test]
fn double_out_of_enum_is_rejected_without_the_fallback_half_mutation() {
    let user_dir = TempUserDir::new("contract-double-enum");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert!(pinyin_set_double_pinyin_scheme(context, 1));
    let aa = cstr("aa");
    assert_eq!(pinyin_parse_more_double_pinyins(instance, aa.as_ptr()), 2);

    for bad in [0, 7, 29, 31, 100, -1] {
        assert!(
            !pinyin_set_double_pinyin_scheme(context, c_int::from(bad)),
            "double scheme {bad} must be rejected"
        );
        // Upstream would have cleared the ZRM fallback here; oxpinyin
        // keeps parsing through it.
        assert_eq!(
            pinyin_parse_more_double_pinyins(instance, aa.as_ptr()),
            2,
            "fallback must survive the rejected double scheme {bad}"
        );
    }

    pinyin_capi::pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

/// ZHUYIN_STANDARD_DVORAK: upstream routes 7 into
/// `ZhuyinSimpleParser2::set_scheme`, which assigns the dvorak tables
/// and falls through into `default: abort()`
/// (`zhuyin_parser2.cpp:291-295` — the fallthrough is still present at
/// libpinyin tip `95e3af7`). oxpinyin: `false`, scheme unchanged. This
/// is a report-back candidate, not a keyboard to port.
#[test]
fn zhuyin_standard_dvorak_is_rejected_as_the_fallthrough_abort_slot() {
    let user_dir = TempUserDir::new("contract-zhuyin-7");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert!(pinyin_set_zhuyin_scheme(context, 9)); // CP26
    let taps = cstr("t");
    assert_eq!(pinyin_parse_more_chewings(instance, taps.as_ptr()), 1);

    assert!(!pinyin_set_zhuyin_scheme(context, 7));
    // CP26 is still in force — the rejected 7 did not disturb it.
    assert_eq!(pinyin_parse_more_chewings(instance, taps.as_ptr()), 1);

    // Every implemented keyboard still switches after the rejection.
    assert!(pinyin_set_zhuyin_scheme(context, 2)); // HSU
    let ge = cstr("ge");
    assert_eq!(pinyin_parse_more_chewings(instance, ge.as_ptr()), 2);

    pinyin_capi::pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

/// Out-of-enum zhuyin values: upstream aborts at the API layer's
/// `default:` (`pinyin.cpp:1188`). oxpinyin: `false`, unchanged.
#[test]
fn zhuyin_out_of_enum_is_rejected_without_the_api_abort() {
    let user_dir = TempUserDir::new("contract-zhuyin-enum");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert!(pinyin_set_zhuyin_scheme(context, 1));
    let su = cstr("su");
    assert_eq!(pinyin_parse_more_chewings(instance, su.as_ptr()), 2);

    for bad in [0, 10, 100, -1] {
        assert!(
            !pinyin_set_zhuyin_scheme(context, c_int::from(bad)),
            "zhuyin scheme {bad} must be rejected"
        );
        assert_eq!(
            pinyin_parse_more_chewings(instance, su.as_ptr()),
            2,
            "STANDARD must survive the rejected zhuyin scheme {bad}"
        );
    }

    pinyin_capi::pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

/// Out-of-enum full-pinyin values: upstream aborts inside
/// `FullPinyinParser2::set_scheme` (`pinyin_parser2.cpp:398`) while the
/// API wrapper (`pinyin.cpp:1148-1153`) would answer `true`
/// unconditionally. oxpinyin: `false`, scheme unchanged.
#[test]
fn full_pinyin_out_of_enum_is_rejected_without_the_abort() {
    let user_dir = TempUserDir::new("contract-full-enum");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let nihao = cstr("nihao");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, nihao.as_ptr()), 5);

    for bad in [0, 4, 7, 30, 100, -1] {
        assert!(
            !pinyin_set_full_pinyin_scheme(context, c_int::from(bad)),
            "full scheme {bad} must be rejected"
        );
        // HANYU (the default) is still in force.
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, nihao.as_ptr()),
            5,
            "HANYU must survive the rejected full scheme {bad}"
        );
    }

    // The LUOMA switch still sticks after the rejections, and its own
    // out-of-enum rejection keeps it in force.
    assert!(pinyin_set_full_pinyin_scheme(context, 2));
    let luoma = cstr("jhih");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, luoma.as_ptr()), 4);
    assert!(!pinyin_set_full_pinyin_scheme(context, 0));
    assert_eq!(pinyin_parse_more_full_pinyins(instance, luoma.as_ptr()), 4);

    pinyin_capi::pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

/// The full setter contract for every implemented scheme value: 1/2/3
/// succeed and stick on both parsers of the pinned oracle's surface.
#[test]
fn implemented_scheme_values_stick() {
    let user_dir = TempUserDir::new("contract-stick");
    let (context, _instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    for scheme in 1..=3 {
        assert!(pinyin_set_full_pinyin_scheme(context, scheme));
    }
    for scheme in 1..=6 {
        assert!(pinyin_set_double_pinyin_scheme(context, scheme));
    }
    for scheme in [1, 2, 3, 4, 5, 6, 8, 9] {
        assert!(pinyin_set_zhuyin_scheme(context, scheme));
    }

    let _: *const GChar = std::ptr::null();
    pinyin_capi::pinyin_fini(context);
}
