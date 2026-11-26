use super::{OptionBits, full_pinyin_syllable, option_alias_canonical};

// Pin scripts2/options.py:23-39 and pinyin_parser2.cpp:38-58 at 074a2219 define these alias gates.
#[test]
fn correction_aliases_resolve_to_their_canonical_syllable() {
    for (alias, canonical, bit) in [
        ("agn", "ang", OptionBits::PINYIN_CORRECT_GN_NG),
        ("amg", "ang", OptionBits::PINYIN_CORRECT_MG_NG),
        ("diou", "diu", OptionBits::PINYIN_CORRECT_IOU_IU),
        ("duei", "dui", OptionBits::PINYIN_CORRECT_UEI_UI),
        ("duen", "dun", OptionBits::PINYIN_CORRECT_UEN_UN),
        ("lue", "lve", OptionBits::PINYIN_CORRECT_UE_VE),
        ("jv", "ju", OptionBits::PINYIN_CORRECT_V_U),
    ] {
        assert_eq!(option_alias_canonical(alias, bit), Some(canonical));
        assert_eq!(option_alias_canonical(alias, OptionBits::empty()), None);
    }
    assert_eq!(
        option_alias_canonical("zon", OptionBits::PINYIN_CORRECT_ON_ONG),
        Some("zong")
    );
    for (alias, bit) in [
        ("gn", OptionBits::PINYIN_CORRECT_GN_NG),
        ("on", OptionBits::PINYIN_CORRECT_ON_ONG),
    ] {
        assert_eq!(option_alias_canonical(alias, bit), None);
    }
}

// Frozen full_pinyin_syllables.rs rows at pin 074a2219 retain their numeric IDs.
#[test]
fn full_pinyin_syllables_match_original_rows() {
    for (index, &expected) in super::reference::FULL_PINYIN_SYLLABLES.iter().enumerate() {
        assert_eq!(full_pinyin_syllable(index), Some(expected));
    }
}
