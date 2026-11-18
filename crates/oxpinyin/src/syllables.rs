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
mod tests;

#[cfg(test)]
mod reference {
    include!("table_data/full_pinyin_syllables.rs");
}
