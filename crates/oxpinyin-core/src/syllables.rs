//! Frozen complete full-pinyin syllable inventory.

use crate::options::OptionBits;

include!("table_data/syllable_inventory.rs");

/// Complete full-pinyin syllables in pinned upstream numeric-ID order.
pub fn full_pinyin_syllables()
-> impl ExactSizeIterator<Item = &'static str> + DoubleEndedIterator + Clone {
    crate::string_tables::full_pinyin_syllables()
        .entries()
        .map(|[text]| text)
}

/// A complete full-pinyin spelling by its frozen numeric ID.
#[must_use]
pub fn full_pinyin_syllable(index: usize) -> Option<&'static str> {
    crate::string_tables::full_pinyin_syllables().field(index, 0)
}

include!(concat!(env!("OUT_DIR"), "/syllable_maps.rs"));

/// A canonical complete syllable spelling, whether it is in the frozen
/// untuned inventory or an option-only correction target.
#[must_use]
pub fn canonical_complete_syllable(text: &str) -> Option<&'static str> {
    FULL_PINYIN_SYLLABLE_MAP
        .get(text)
        .and_then(|&index| full_pinyin_syllable(index as usize))
        .or_else(|| {
            OPTION_ONLY_COMPLETE_SYLLABLE_MAP
                .get(text)
                .map(|&index| OPTION_ONLY_COMPLETE_SYLLABLES[index as usize])
        })
}

/// The canonical spelling for an option-gated spelling alias, if enabled.
///
/// This is the parser-table half of the option bits: correction aliases and
/// the two parser-table ambiguity aliases (`sua`→`shua`, `zua`→`zhua`).
/// Matrix-level fuzzy substitutions live on [`crate::SyllableKey`] instead.
#[must_use]
pub fn option_alias_canonical(text: &str, options: OptionBits) -> Option<&'static str> {
    // Most parser option words enable neither spelling corrections nor
    // the two parser-table ambiguity aliases. Avoid their suffix scans;
    // matrix-only fuzzy bits do not admit a spelling alias here.
    let alias_bits =
        OptionBits::PINYIN_CORRECT_ALL | OptionBits::PINYIN_AMB_S_SH | OptionBits::PINYIN_AMB_Z_ZH;
    if !options.intersects(alias_bits) {
        return None;
    }
    correction_canonical(text, options).or_else(|| ambiguity_canonical(text, options))
}

/// The suffix corrections: the option, the wrong suffix and the correct
/// one, in the pin's row order (`auto_correct`, `scripts2/options.py:23-32`).
/// The pin's last row, `on` for `ong`, has its own length rule below.
const SUFFIX_CORRECTIONS: [(OptionBits, &str, &str); 6] = [
    (OptionBits::PINYIN_CORRECT_GN_NG, "gn", "ng"),
    (OptionBits::PINYIN_CORRECT_MG_NG, "mg", "ng"),
    (OptionBits::PINYIN_CORRECT_IOU_IU, "iou", "iu"),
    (OptionBits::PINYIN_CORRECT_UEI_UI, "uei", "ui"),
    (OptionBits::PINYIN_CORRECT_UEN_UN, "uen", "un"),
    (OptionBits::PINYIN_CORRECT_UE_VE, "ue", "ve"),
];

fn correction_canonical(text: &str, options: OptionBits) -> Option<&'static str> {
    let bytes = text.as_bytes();

    // The first row that matches decides, also when its replacement is not
    // a syllable.
    for (option, wrong, correct) in SUFFIX_CORRECTIONS {
        if options.contains(option) && text.ends_with(wrong) {
            return checked_canonical(text, wrong.len(), correct);
        }
    }
    if options.contains(OptionBits::PINYIN_CORRECT_V_U)
        && bytes.len() >= 2
        && matches!(bytes[0], b'j' | b'q' | b'x' | b'y')
        && bytes[1] == b'v'
    {
        let mut candidate = String::with_capacity(text.len());
        candidate.push(char::from(bytes[0]));
        candidate.push('u');
        candidate.push_str(&text[2..]);
        return canonical_complete_syllable(&candidate);
    }
    if options.contains(OptionBits::PINYIN_CORRECT_ON_ONG) && text.ends_with("on") && text.len() > 2
    {
        let mut candidate = String::with_capacity(text.len() + 1);
        candidate.push_str(text);
        candidate.push('g');
        return canonical_complete_syllable(&candidate);
    }

    None
}

fn ambiguity_canonical(text: &str, options: OptionBits) -> Option<&'static str> {
    match text {
        "sua" if options.contains(OptionBits::PINYIN_AMB_S_SH) => Some("shua"),
        "zua" if options.contains(OptionBits::PINYIN_AMB_Z_ZH) => Some("zhua"),
        _ => None,
    }
}

fn checked_canonical(text: &str, suffix_len: usize, suffix: &str) -> Option<&'static str> {
    // Bare `gn`/`mg`/`ue`/… are not in `pinyin_index`; only the 80/80/…
    // stemmed rows are. An empty stem would invent `ng`/`ve`.
    let base_len = text.len().checked_sub(suffix_len)?;
    if base_len == 0 {
        return None;
    }
    let mut candidate = String::with_capacity(base_len + suffix.len());
    candidate.push_str(&text[..base_len]);
    candidate.push_str(suffix);
    canonical_complete_syllable(&candidate)
}

#[cfg(test)]
mod tests {
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
}

#[cfg(test)]
mod identity_tests {
    #[test]
    fn full_pinyin_syllables_match_original_rows() {
        assert_eq!(
            super::FULL_PINYIN_SYLLABLE_COUNT,
            super::reference::FULL_PINYIN_SYLLABLE_COUNT
        );
        assert!(super::full_pinyin_syllables().eq(super::reference::FULL_PINYIN_SYLLABLES));
    }
}

#[cfg(test)]
mod reference {
    include!("table_data/full_pinyin_syllables.rs");
}
