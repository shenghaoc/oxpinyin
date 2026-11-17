use oxpinyin_core::{ZhuyinParser, ZhuyinScheme};

use super::*;

#[test]
fn den_matrix_keeps_key_and_fuzzy_deng() {
    let parse = ZhuyinParser::with_scheme(ZhuyinScheme::Standard)
        .parse_with_options(b"2p", OptionBits::USE_TONE);
    let keys = crate::cursor::ModeKeys::Zhuyin(parse.keys())
        .iter()
        .collect::<Vec<_>>();
    let keys = crate::cursor::matrix_keys_of(&keys);
    let matrix = oxpinyin_engine::parse_time_keys(&keys, 2, OptionBits::PINYIN_AMB_EN_ENG, false);
    assert_eq!(
        matrix
            .iter()
            .map(|k| k.key().pinyin_spelling())
            .collect::<Vec<_>>(),
        ["den", "deng"]
    );
}

#[test]
fn scheme_dispatch_covers_the_header_discriminants_and_nothing_else() {
    assert!((1..=3).all(|v| full_scheme(v).is_some()));
    assert!(full_scheme(0).is_none() && full_scheme(4).is_none());
    // Zhuyin: total over 1..=9 including the STANDARD_DVORAK slot (7).
    assert!((1..=9).all(|v| zhuyin_scheme(v).is_some()));
    assert_eq!(zhuyin_scheme(7), Some(ZhuyinScheme::StandardDvorak));
    assert!(zhuyin_scheme(0).is_none() && zhuyin_scheme(10).is_none());
    assert!((1..=6).all(|v| double_scheme(v).is_some()));
    assert!(double_scheme(0).is_none() && double_scheme(7).is_none());
    assert!(double_scheme(-1).is_none());
}

#[test]
fn exact_input_joins_keys_with_apostrophes_and_spans_each_one() {
    let parse = ZhuyinParser::with_scheme(ZhuyinScheme::Standard)
        .parse_with_options(b"su3cl3", OptionBits::USE_TONE);
    let (text, segments) = exact_input(parse.keys());
    assert_eq!(text, "ni'hao");
    assert_eq!(segments.len(), 2);
    assert_eq!((segments[0].start(), segments[0].end()), (0, 2));
    assert_eq!((segments[1].start(), segments[1].end()), (3, 6));
    assert_eq!(segments[0].tone(), 3);
    assert_eq!(segments[1].tone(), 3);
    let no_keys: &[ZhuyinKey] = &[];
    let (empty, none) = exact_input(no_keys);
    assert!(empty.is_empty() && none.is_empty());
}

// From the facade-orchestration laws (see instance/tests.rs).
use crate::PinyinCorrections;

use crate::test_fixture::{pinyin_instance, zhuyin_instance};

#[test]
fn full_pinyin_parse_consumes_and_keys_sit_at_their_syllable_starts() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    assert_eq!(inst.bind(&context.runtime).parsed_len, 5);
    assert!(inst.bind(&context.runtime).session.is_composing());
    let ni = inst
        .bind(&context.runtime)
        .key_at(0)
        .expect("a key starts at 0");
    assert_eq!((ni.text, ni.begin, ni.end), ("ni", 0, 2));
    let hao = inst
        .bind(&context.runtime)
        .key_at(2)
        .expect("a key starts at 2");
    assert_eq!((hao.text, hao.begin, hao.end), ("hao", 2, 5));
    assert!(
        inst.bind(&context.runtime).key_at(1).is_none(),
        "a mid-syllable column is empty"
    );
    assert!(
        inst.bind(&context.runtime).key_at(5).is_none(),
        "the reserved slot answers nothing"
    );
}

#[test]
fn full_pinyin_skips_a_consumed_separator_column() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("ni'hao"), 6);
    // Offset 2 is the apostrophe: a lone zero-key column the pin walks past.
    let hao = inst
        .bind(&context.runtime)
        .key_at(2)
        .expect("the walk lands on `hao`");
    assert_eq!((hao.text, hao.begin, hao.end), ("hao", 3, 6));
}

#[test]
fn one_key_probes_follow_the_live_schemes() {
    let (context, mut inst) = pinyin_instance();
    assert!(
        inst.bind(&context.runtime)
            .parse_one_full_pinyin("ni", PinyinCorrections::FromOptions)
            .is_some()
    );
    assert!(
        inst.bind(&context.runtime)
            .parse_one_full_pinyin("xyz", PinyinCorrections::FromOptions)
            .is_none()
    );
    let (zhuyin_context, mut zhuyin) = zhuyin_instance();
    // Standard keyboard: `s` is ㄋ; `q` is not a chewing symbol.
    assert!(
        !zhuyin
            .bind(&zhuyin_context.runtime)
            .in_keyboard(b's')
            .is_empty()
    );
    assert!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_one_chewing("su3")
            .is_some()
    );
    assert!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_one_chewing("")
            .is_none()
    );
}
