use super::OptionBits;

// 074a2219 pinyin_custom2.h:31–92 assigns the option word's flag values.
#[test]
fn bit_values_match_the_frozen_header() {
    for (flag, bits) in [
        (OptionBits::PINYIN_INCOMPLETE, 0x8),
        (OptionBits::ZHUYIN_INCOMPLETE, 0x10),
        (OptionBits::USE_TONE, 0x20),
        (OptionBits::FORCE_TONE, 0x40),
        (OptionBits::USE_DIVIDED_TABLE, 0x80),
        (OptionBits::USE_RESPLIT_TABLE, 0x100),
        (OptionBits::DYNAMIC_ADJUST, 0x200),
        (OptionBits::PINYIN_AMB_ALL, 0x3ff << 10),
        (OptionBits::PINYIN_CORRECT_ALL, 0xff << 21),
        (OptionBits::ZHUYIN_CORRECT_SHUFFLE, 0x8000_0000),
        (OptionBits::ZHUYIN_CORRECT_ALL, 0x7 << 29),
    ] {
        assert_eq!(flag.bits(), bits);
    }
}

// Rust OptionBits preserves both named and unnamed bits in a C option word.
#[test]
fn unnamed_bits_round_trip() {
    let word = 0x0010_0128;
    assert_eq!(OptionBits::from_bits_retain(word).bits(), word);
}
