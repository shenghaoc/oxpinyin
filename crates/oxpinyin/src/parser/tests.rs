use super::FullPinyinParser;
use crate::OptionBits;

/// `FullPinyinParser2::parse_one_key`: the whole stripped input is
/// one spelling probe — multi-syllable and apostrophe inputs are
/// refused, a trailing `1..=5` is the tone under `USE_TONE`, and
/// `FORCE_TONE` (nested inside `USE_TONE`) rejects toneless input.
#[test]
fn full_pinyin_one_key_law() {
    let parser = FullPinyinParser;

    let key = parser
        .parse_one_key(OptionBits::empty(), b"ni")
        .expect("ni parses");
    assert_eq!(key.pinyin_string(), "ni");

    // The probe covers the whole input: no forward maximum match.
    assert_eq!(parser.parse_one_key(OptionBits::empty(), b"nihao"), None);
    // Apostrophes are refused (the pin asserts on them).
    assert_eq!(parser.parse_one_key(OptionBits::empty(), b"ni'hao"), None);
    assert_eq!(parser.parse_one_key(OptionBits::empty(), b""), None);

    // Toneless without USE_TONE; tone carried with it.
    let toned = parser
        .parse_one_key(OptionBits::USE_TONE, b"zai4")
        .expect("zai4");
    assert_eq!(toned.pinyin_string(), "zai4");
    assert_eq!(parser.parse_one_key(OptionBits::empty(), b"zai4"), None);

    // FORCE_TONE is dead without USE_TONE, rejects the toneless
    // `zai` with it, and accepts the toned form.
    assert!(
        parser
            .parse_one_key(OptionBits::FORCE_TONE, b"zai")
            .is_some()
    );
    assert_eq!(
        parser.parse_one_key(OptionBits::USE_TONE | OptionBits::FORCE_TONE, b"zai"),
        None
    );
    assert!(
        parser
            .parse_one_key(OptionBits::USE_TONE | OptionBits::FORCE_TONE, b"zai4")
            .is_some()
    );
    // `6` is not a tone: the digit stays in the core and fails it.
    assert_eq!(parser.parse_one_key(OptionBits::USE_TONE, b"zai6"), None);

    // Initial-only keys are index rows flagged PINYIN_INCOMPLETE:
    // `check_pinyin_options` refuses them unless the option word
    // carries the bit (`pinyin_parser2.cpp:42-46`), so `b` parses
    // under PINYIN_INCOMPLETE and under nothing else (#547).
    assert_eq!(parser.parse_one_key(OptionBits::empty(), b"b"), None);
    assert_eq!(parser.parse_one_key(OptionBits::USE_TONE, b"b1"), None);
    let b = parser
        .parse_one_key(OptionBits::PINYIN_INCOMPLETE, b"b")
        .expect("initial-only under PINYIN_INCOMPLETE");
    assert_eq!(b.pinyin_string(), "b");
    assert_eq!(b.yunmu_string(), "");
    assert_eq!(
        parser
            .parse_one_key(OptionBits::PINYIN_INCOMPLETE, b"zh")
            .map(|k| k.to_packed()),
        Some(0x0017)
    );

    // Correction aliases resolve through the option gate.
    let ang = parser
        .parse_one_key(OptionBits::PINYIN_CORRECT_GN_NG, b"agn")
        .expect("agn corrects");
    assert_eq!(ang.pinyin_string(), "ang");
    assert_eq!(parser.parse_one_key(OptionBits::empty(), b"agn"), None);
}

/// The single-key tone law formerly exercised through the C facade.
#[test]
fn full_pinyin_tone_law() {
    let parser = FullPinyinParser;
    assert_eq!(
        parser.parse_one_key(OptionBits::PINYIN_INCOMPLETE, b"ni3"),
        None
    );
    let key = parser.parse_one_key(OptionBits::USE_TONE, b"ni3").unwrap();
    assert_eq!(key.pinyin_string(), "ni3");
    assert!(parser.parse_one_key(OptionBits::USE_TONE, b"ni").is_some());
    let forced = OptionBits::USE_TONE | OptionBits::FORCE_TONE;
    assert_eq!(parser.parse_one_key(forced, b"ni"), None);
    assert!(parser.parse_one_key(forced, b"ni3").is_some());
    assert!(
        parser
            .parse_one_key(OptionBits::FORCE_TONE, b"ni")
            .is_some()
    );
}
