//! Scheme-setter contract tests: the abort / no-op / half-mutation
//! slots of the pinned oracle
//! (`docs/findings/upstream-divergences.md`, the #109 contract-lock).
//!
//! The pin-built `.so` SIGABRTs on double 30, zhuyin 7 and out-of-enum,
//! and full-pinyin out-of-enum, and so does this facade (register rows
//! 5a, 5c and 5d; the contract-diff cases hold the comparison); the death
//! tests below check the facade side. Double out-of-enum half-mutates
//! behind an unconditional `true` (the live scheme's fallback table is
//! cleared — reproduced here as of the row-5b revert).

use std::os::raw::c_int;

use pinyin_capi::{
    GChar, pinyin_parse_more_double_pinyins, pinyin_set_double_pinyin_scheme,
    pinyin_set_full_pinyin_scheme, pinyin_set_zhuyin_scheme,
};

use crate::common::{TempUserDir, assert_aborts, cstr, open};

/// `DOUBLE_PINYIN_CUSTOMIZED`: upstream aborts inside
/// `DoublePinyinParser2::set_scheme` (`pinyin_parser2.cpp:611`), and the
/// facade aborts the process too (register row 5a).
#[test]
fn double_customized_aborts() {
    assert_aborts("contract::double_customized_aborts", || {
        let user_dir = TempUserDir::new("contract-double-30");
        let (context, _instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        pinyin_set_double_pinyin_scheme(context, 30);
    });
}

/// Out-of-enum double values (negatives, 0, 7–29, 31+): upstream clears
/// `m_fallback_table` unconditionally (`pinyin_parser2.cpp:580`),
/// returns `false` from the parser (`pinyin_parser2.cpp:614`), and the
/// API wrapper answers `true` regardless (`pinyin.cpp:1155–1159`).
/// oxpinyin reproduces the half-mutation: `true`, fallback cleared,
/// shengmu/yunmu tables unchanged.
#[test]
fn double_out_of_enum_reproduces_the_half_mutation() {
    let user_dir = TempUserDir::new("contract-double-enum");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // ZRM is the fallback-bearing scheme: "aa" resolves through its
    // fallback table ("aa" -> a, parsed_len 2).
    assert!(pinyin_set_double_pinyin_scheme(context, 1));
    let aa = cstr("aa");
    assert_eq!(pinyin_parse_more_double_pinyins(instance, aa.as_ptr()), 2);

    // A non-fallback input still works after the fallback is cleared:
    // "ni" (n+i) uses the shengmu/yunmu tables, not the fallback.
    let ni = cstr("ni");

    for bad in [0, 7, 29, 31, 100, -1] {
        // Re-set ZRM so the fallback is live again before each probe.
        assert!(pinyin_set_double_pinyin_scheme(context, 1));
        assert_eq!(
            pinyin_parse_more_double_pinyins(instance, aa.as_ptr()),
            2,
            "ZRM fallback must be live before the out-of-enum call ({bad})"
        );

        // The half-mutation: returns `true` (the upstream wrapper's lie).
        assert!(
            pinyin_set_double_pinyin_scheme(context, c_int::from(bad)),
            "double scheme {bad} must answer true (upstream's wrapper lie)"
        );

        // The fallback is now cleared — "aa" no longer resolves.
        assert_eq!(
            pinyin_parse_more_double_pinyins(instance, aa.as_ptr()),
            0,
            "fallback must be cleared after out-of-enum {bad}"
        );

        // The shengmu/yunmu tables are intact — "ni" still parses.
        assert_eq!(
            pinyin_parse_more_double_pinyins(instance, ni.as_ptr()),
            2,
            "shengmu/yunmu must survive the out-of-enum {bad}"
        );
    }

    // A later valid set restores the fallback.
    assert!(pinyin_set_double_pinyin_scheme(context, 1));
    assert_eq!(
        pinyin_parse_more_double_pinyins(instance, aa.as_ptr()),
        2,
        "ZRM fallback must be restored after a valid set"
    );

    pinyin_capi::pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

/// `ZHUYIN_STANDARD_DVORAK`: upstream routes 7 into
/// `ZhuyinSimpleParser2::set_scheme`, which assigns the dvorak tables
/// and falls through into `default: abort()`
/// (`zhuyin_parser2.cpp:291-295`); the facade aborts the process too
/// (register row 5c).
#[test]
fn zhuyin_standard_dvorak_aborts() {
    assert_aborts("contract::zhuyin_standard_dvorak_aborts", || {
        let user_dir = TempUserDir::new("contract-zhuyin-7");
        let (context, _instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        pinyin_set_zhuyin_scheme(context, 7);
    });
}

/// Out-of-enum zhuyin values: upstream aborts at the API layer's
/// `default:` (`pinyin.cpp:1189`), and so does the facade (register row
/// 5d).
#[test]
fn zhuyin_out_of_enum_aborts() {
    assert_aborts("contract::zhuyin_out_of_enum_aborts", || {
        let user_dir = TempUserDir::new("contract-zhuyin-enum");
        let (context, _instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        pinyin_set_zhuyin_scheme(context, 10);
    });
}

/// Out-of-enum full-pinyin values: upstream aborts inside
/// `FullPinyinParser2::set_scheme` (`pinyin_parser2.cpp:398`), and so does
/// the facade (register row 5d).
#[test]
fn full_pinyin_out_of_enum_aborts() {
    assert_aborts("contract::full_pinyin_out_of_enum_aborts", || {
        let user_dir = TempUserDir::new("contract-full-enum");
        let (context, _instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        pinyin_set_full_pinyin_scheme(context, 0);
    });
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
