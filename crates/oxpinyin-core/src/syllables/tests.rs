use std::collections::HashSet;

use super::{
    FULL_PINYIN_SYLLABLE_COUNT, INCOMPLETE_PINYIN_KEY_COUNT, INCOMPLETE_PINYIN_KEYS,
    MAX_SYLLABLE_LEN, full_pinyin_syllables,
};

/// Whether `byte` is a vowel for the initial-only key rule.
///
/// `v` counts as a vowel because it spells `ü` in `lv`, `lve`, `nv` and
/// `nve`, so `lv` is not an initial.
const fn is_vowel(byte: u8) -> bool {
    matches!(byte, b'a' | b'e' | b'i' | b'o' | b'u' | b'v')
}

#[test]
fn frozen_inventory_has_expected_shape() {
    assert_eq!(full_pinyin_syllables().len(), FULL_PINYIN_SYLLABLE_COUNT);

    let mut seen = HashSet::with_capacity(FULL_PINYIN_SYLLABLE_COUNT);
    for syllable in full_pinyin_syllables() {
        assert!(!syllable.is_empty(), "syllables are non-empty");
        assert!(
            syllable.bytes().all(|byte| byte.is_ascii_lowercase()),
            "{syllable:?} is not lowercase ASCII"
        );
        assert!(seen.insert(syllable), "duplicate syllable {syllable:?}");
    }

    assert_eq!(
        full_pinyin_syllables().map(|syllable| syllable.len()).max(),
        Some(MAX_SYLLABLE_LEN)
    );

    for required in ["ng", "lv", "lve", "nv", "nve", "zhuan"] {
        assert!(full_pinyin_syllables().any(|entry| entry == required));
    }
    for excluded in [
        "b", "zh", "den", "kei", "lue", "nue", "tei", "eng", "chua", "din", "fe", "len", "nia",
        "rua", "yai", "zhei",
    ] {
        assert!(!full_pinyin_syllables().any(|entry| entry == excluded));
    }
}

#[test]
fn zhuyin_only_syllables_stay_off_the_full_pinyin_surface() {
    use super::ZHUYIN_ONLY_COMPLETE_SYLLABLES;

    let mut previous = "";
    for syllable in ZHUYIN_ONLY_COMPLETE_SYLLABLES {
        assert!(
            previous < syllable,
            "{syllable:?} is out of ascending order"
        );
        assert!(
            !full_pinyin_syllables().any(|entry| entry == syllable),
            "{syllable:?} leaked into the full-pinyin inventory"
        );
        previous = syllable;
    }
}

#[test]
fn incomplete_keys_follow_the_frozen_rule() {
    let complete: HashSet<&str> = full_pinyin_syllables().collect();
    let mut derived: Vec<&str> = Vec::new();

    for syllable in full_pinyin_syllables() {
        for length in 1..syllable.len() {
            let prefix = &syllable[..length];
            if prefix.bytes().any(is_vowel)
                || complete.contains(prefix)
                || derived.contains(&prefix)
            {
                continue;
            }
            derived.push(prefix);
        }
    }
    derived.sort_unstable();

    assert_eq!(derived, INCOMPLETE_PINYIN_KEYS);
    assert_eq!(INCOMPLETE_PINYIN_KEYS.len(), INCOMPLETE_PINYIN_KEY_COUNT);
}

#[test]
fn correction_aliases_resolve_to_their_canonical_syllable() {
    use super::{OptionBits, option_alias_canonical};

    let cases = [
        ("agn", "ang", OptionBits::PINYIN_CORRECT_GN_NG),
        ("amg", "ang", OptionBits::PINYIN_CORRECT_MG_NG),
        ("diou", "diu", OptionBits::PINYIN_CORRECT_IOU_IU),
        ("duei", "dui", OptionBits::PINYIN_CORRECT_UEI_UI),
        ("duen", "dun", OptionBits::PINYIN_CORRECT_UEN_UN),
        ("lue", "lve", OptionBits::PINYIN_CORRECT_UE_VE),
        ("jv", "ju", OptionBits::PINYIN_CORRECT_V_U),
    ];
    for (alias, canonical, bit) in cases {
        assert_eq!(option_alias_canonical(alias, bit), Some(canonical));
        assert_eq!(option_alias_canonical(alias, OptionBits::default()), None);
    }
    assert_eq!(
        option_alias_canonical("zon", OptionBits::PINYIN_CORRECT_ON_ONG),
        Some("zong")
    );
    // Bare gn/mg/on are not in pinyin_index (80/80/17 stemmed rows).
    assert_eq!(
        option_alias_canonical("gn", OptionBits::PINYIN_CORRECT_GN_NG),
        None
    );
    assert_eq!(
        option_alias_canonical("mg", OptionBits::PINYIN_CORRECT_MG_NG),
        None
    );
    assert_eq!(
        option_alias_canonical("on", OptionBits::PINYIN_CORRECT_ON_ONG),
        None
    );
}

#[test]
fn full_pinyin_syllables_match_original_rows() {
    assert_eq!(
        super::FULL_PINYIN_SYLLABLE_COUNT,
        super::reference::FULL_PINYIN_SYLLABLE_COUNT
    );
    assert!(super::full_pinyin_syllables().eq(super::reference::FULL_PINYIN_SYLLABLES));
}
