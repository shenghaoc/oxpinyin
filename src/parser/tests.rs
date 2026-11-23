use super::FullPinyinParser;
use crate::OptionBits;

// Pin pinyin_parser2.cpp:38-58,164-214 at 074a2219; the Rust parser rejects empty input.
#[test]
fn full_pinyin_one_key_law() {
    let use_tone = OptionBits::USE_TONE;
    let force_tone = OptionBits::FORCE_TONE;
    for (input, options, expected) in [
        ("ni", OptionBits::empty(), Some(("ni", 0))),
        ("nihao", OptionBits::empty(), None),
        ("", OptionBits::empty(), None),
        ("zai4", use_tone, Some(("zai", 4))),
        ("zai4", OptionBits::empty(), None),
        ("zai", force_tone, Some(("zai", 0))),
        ("zai", use_tone | force_tone, None),
        ("zai4", use_tone | force_tone, Some(("zai", 4))),
        ("zai6", use_tone, None),
        ("b", OptionBits::empty(), None),
        ("b", OptionBits::PINYIN_INCOMPLETE, Some(("b", 0))),
        ("ni3", use_tone, Some(("ni", 3))),
        ("ni", use_tone, Some(("ni", 0))),
    ] {
        let parsed = FullPinyinParser::parse_one_key(options, input.as_bytes());
        assert_eq!(
            parsed.map(|key| (key.pinyin_spelling(), key.tone)),
            expected,
            "{input:?}, {options:?}"
        );
    }
}
