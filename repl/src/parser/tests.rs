use super::*;
#[test]
fn explicit_nul_spans_keep_the_pin_length_and_zero_pad_the_tail() {
    for options in [
        OptionBits::PINYIN_INCOMPLETE,
        OptionBits::USE_TONE | OptionBits::PINYIN_INCOMPLETE,
    ] {
        let parsed = parse_full(b"ni\0'hao", options);
        assert_eq!(parsed.consumed, 7);
        assert_eq!(parsed.keys.len(), 1);
        assert_eq!(parsed.keys[0].key.pinyin_string(), "ni");
        assert_eq!(parsed.keys[0].span, 0..7);
    }
}
