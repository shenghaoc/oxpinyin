use super::*;

#[test]
fn default_zhuyin_options_use_and_force_tone_without_incomplete() {
    assert!(ZHUYIN_DEFAULT_OPTION_WORD.contains(crate::OptionBits::USE_TONE));
    assert!(ZHUYIN_DEFAULT_OPTION_WORD.contains(crate::OptionBits::FORCE_TONE));
    assert!(!ZHUYIN_DEFAULT_OPTION_WORD.contains(crate::OptionBits::PINYIN_INCOMPLETE));
}
