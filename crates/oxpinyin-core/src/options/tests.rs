use super::OptionBits;

#[test]
fn bit_values_match_the_frozen_header() {
    assert_eq!(OptionBits::PINYIN_INCOMPLETE.bits(), 0x8);
    assert_eq!(OptionBits::ZHUYIN_INCOMPLETE.bits(), 0x10);
    assert_eq!(OptionBits::USE_TONE.bits(), 0x20);
    assert_eq!(OptionBits::FORCE_TONE.bits(), 0x40);
    assert_eq!(OptionBits::USE_DIVIDED_TABLE.bits(), 0x080);
    assert_eq!(OptionBits::USE_RESPLIT_TABLE.bits(), 0x100);
    assert_eq!(OptionBits::DYNAMIC_ADJUST.bits(), 0x200);
    assert_eq!(OptionBits::PINYIN_AMB_ALL.bits(), 0x3ff << 10);
    assert_eq!(OptionBits::PINYIN_CORRECT_ALL.bits(), 0xff << 21);
    assert_eq!(OptionBits::ZHUYIN_CORRECT_SHUFFLE.bits(), 0x8000_0000);
    assert_eq!(OptionBits::ZHUYIN_CORRECT_ALL.bits(), 0x7 << 29);
    assert_eq!(
        (OptionBits::PINYIN_INCOMPLETE
            | OptionBits::ZHUYIN_INCOMPLETE
            | OptionBits::PINYIN_CORRECT_ALL
            | OptionBits::USE_DIVIDED_TABLE
            | OptionBits::USE_RESPLIT_TABLE)
            .bits(),
        0x1fe0_0198
    );
}

#[test]
fn set_and_clear_is_a_bit_operation() {
    let mut options = OptionBits::default();
    options.set(OptionBits::PINYIN_INCOMPLETE, true);
    assert!(options.contains(OptionBits::PINYIN_INCOMPLETE));
    assert!(!options.contains(OptionBits::DYNAMIC_ADJUST));
    options.set(OptionBits::PINYIN_INCOMPLETE, false);
    assert_eq!(options, OptionBits::default());
}

#[test]
fn unnamed_bits_round_trip() {
    // Bits 0 to 2 and 20 have no `OptionBits` flag (`pinyin_custom2.h:31-32`
    // names bits 1 and 2 as table-row flags only).
    for word in [0x1, 0x7, 1 << 20, 0x0010_0128, u32::MAX] {
        assert_eq!(OptionBits::from_bits_retain(word).bits(), word);
    }
    let mut options = OptionBits::from_bits_retain(0x5 | (1 << 20));
    options.set(OptionBits::USE_TONE, true);
    options.remove(OptionBits::PINYIN_CORRECT_ALL);
    assert_eq!(options.bits(), 0x5 | (1 << 20) | 0x20);
}
