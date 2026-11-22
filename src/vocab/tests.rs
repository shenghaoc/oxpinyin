use super::{PhraseEntry, SYLLABLE_KEY_COUNT, SyllableKey};
use crate::parser::{ChewingKey, Completeness};
use crate::syllables::{
    INCOMPLETE_PINYIN_KEYS, OPTION_ONLY_COMPLETE_SYLLABLES, full_pinyin_syllables,
};

#[test]
fn every_frozen_key_round_trips_through_its_id() {
    for index in 0..SYLLABLE_KEY_COUNT {
        let key = SyllableKey::from_index(index).expect("index is in range");
        assert_eq!(key.index(), index);
        assert_eq!(SyllableKey::from_canonical_text(key.text()), Some(key));
        assert!(!key.text().is_empty());
    }
    assert_eq!(SyllableKey::from_index(SYLLABLE_KEY_COUNT), None);
}

/// Every key decodes from the fields of its own `content_table` row,
/// whatever its tier, and fields outside the table decode to nothing.
#[test]
fn every_key_decodes_from_its_chewing_fields() {
    for index in 0..SYLLABLE_KEY_COUNT {
        let key = SyllableKey::from_index(index).expect("index is in range");
        let chewing = ChewingKey::from_pinyin(key.text()).expect("content row");
        assert_eq!(SyllableKey::from_chewing_key(chewing), Some(key));
        assert_eq!(chewing.pinyin_spelling(), key.text());
        assert_eq!(key.chewing_key(), chewing);
        for tone in 1..=5 {
            assert_eq!(
                SyllableKey::from_chewing_key(chewing.with_tone(tone)),
                Some(key)
            );
        }
    }
    for spelling in ["eng", "nun", "den"] {
        let chewing = ChewingKey::from_pinyin(spelling).expect("content row");
        let key = SyllableKey::from_chewing_key(chewing).expect("decoded");
        assert_eq!(key.text(), spelling);
    }
    assert_eq!(SyllableKey::from_chewing_key(ChewingKey::ZERO), None);
    assert_eq!(
        SyllableKey::from_chewing_key(ChewingKey::new(31, 3, 31, 0)),
        None
    );
}

#[test]
fn completeness_follows_the_frozen_id_ranges() {
    for syllable in full_pinyin_syllables() {
        let key = SyllableKey::from_text(syllable).expect("complete syllable");
        assert_eq!(key.completeness(), Completeness::Complete);
    }
    for incomplete in INCOMPLETE_PINYIN_KEYS {
        let key = SyllableKey::from_text(incomplete).expect("initial-only key");
        assert_eq!(key.completeness(), Completeness::Partial);
        assert!(key.index() >= full_pinyin_syllables().len());
    }
    for option_only in OPTION_ONLY_COMPLETE_SYLLABLES {
        let key = SyllableKey::from_canonical_text(option_only).expect("option-only key");
        assert_eq!(key.completeness(), Completeness::Complete);
        assert!(key.index() >= full_pinyin_syllables().len() + INCOMPLETE_PINYIN_KEYS.len());
    }
    for zhuyin_only in crate::syllables::ZHUYIN_ONLY_COMPLETE_SYLLABLES {
        let key = SyllableKey::from_canonical_text(zhuyin_only).expect("zhuyin-only key");
        assert_eq!(key.completeness(), Completeness::Complete);
        assert!(key.index() >= 430);
        // The raw full-pinyin surface never sees tier 4: `from_text`
        // mirrors `pinyin_index` membership, which lacks these rows.
        assert_eq!(SyllableKey::from_text(zhuyin_only), None);
    }
}

#[test]
fn unknown_spellings_have_no_key() {
    for text in ["", "NI", "ni2", "qqq", "\u{4f60}", "lv1"] {
        assert_eq!(SyllableKey::from_text(text), None, "text: {text:?}");
    }
}

#[test]
fn option_text_resolves_correction_aliases_under_their_bits() {
    use crate::OptionBits;

    let gn = OptionBits::PINYIN_CORRECT_GN_NG;
    let mg = OptionBits::PINYIN_CORRECT_MG_NG;
    assert_eq!(
        SyllableKey::from_option_text("agn", gn).map(SyllableKey::text),
        Some("ang")
    );
    assert_eq!(
        SyllableKey::from_option_text("amg", mg).map(SyllableKey::text),
        Some("ang")
    );
    assert_eq!(
        SyllableKey::from_option_text("amg", OptionBits::default()),
        None
    );
}

#[test]
fn phrase_entries_carry_a_token_and_text() {
    let entry = PhraseEntry::new(7, "\u{4f60}\u{597d}".to_owned());
    assert_eq!(entry.token(), 7);
    assert_eq!(entry.text(), "\u{4f60}\u{597d}");
    assert_eq!(entry.clone().into_text().as_str(), Some("\u{4f60}\u{597d}"));
    assert_eq!(entry.token().to_string(), "7");
}
