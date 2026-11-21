use super::*;
use crate::OptionBits;

#[test]
fn direct_spans_keep_skipped_separators_and_partial_failure() {
    let parsed = parse_direct(b"ni3'' hao3  invalid", OptionBits::USE_TONE, false);
    assert_eq!(parsed.consumed, 12);
    assert_eq!(
        parsed
            .keys
            .iter()
            .map(|key| key.span.clone())
            .collect::<Vec<_>>(),
        [0..3, 6..10]
    );
    assert_eq!(parsed.keys[0].key.pinyin_string(), "ni3");
    let corrected = parse_direct(
        b"bagn n",
        OptionBits::USE_TONE | OptionBits::PINYIN_CORRECT_ALL | OptionBits::PINYIN_INCOMPLETE,
        false,
    );
    assert_eq!(corrected.consumed, 6);
    assert_eq!(corrected.keys[0].key.pinyin_string(), "bang");
    assert_eq!(corrected.keys[1].key.pinyin_string(), "n");
}

#[test]
fn zhuyin_spans_are_utf8_bytes_from_the_shipping_key_parser() {
    let input = "ㄋㄧˇ' ㄏㄠˇ  bad";
    let parsed = parse_direct(input.as_bytes(), OptionBits::USE_TONE, true);
    assert_eq!(parsed.consumed, 20);
    assert_eq!(
        parsed
            .keys
            .iter()
            .map(|key| key.span.clone())
            .collect::<Vec<_>>(),
        [0..8, 10..18]
    );
    let shipping = crate::parser::parse_zhuyin_direct(input, OptionBits::USE_TONE);
    assert_eq!(shipping.len(), parsed.keys.len());
    for ((key, tone), with_span) in shipping.iter().zip(parsed.keys) {
        assert_eq!(
            ChewingKey::from_pinyin(key.text())
                .unwrap()
                .with_tone(*tone),
            with_span.key
        );
    }
}
