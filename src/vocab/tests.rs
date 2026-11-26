use super::{PhraseEntry, SYLLABLE_KEY_COUNT, SyllableKey};
use crate::parser::{ChewingKey, Completeness};

// Rust dense IDs cover the frozen inventory; 074a2219 chewing_key.cpp:27–35 resolves each row's fields.
#[test]
fn every_frozen_key_round_trips_through_its_id() {
    for index in 0..SYLLABLE_KEY_COUNT {
        let key = SyllableKey::from_index(index).expect("index is in range");
        assert_eq!(key.index(), index);
        assert_eq!(SyllableKey::from_canonical_text(key.text()), Some(key));
        assert!(!key.text().is_empty());
        let chewing = ChewingKey::from_pinyin(key.text()).expect("content row");
        assert_eq!(SyllableKey::from_chewing_key(chewing), Some(key));
        assert_eq!(key.chewing_key(), chewing);
    }
    assert_eq!(SyllableKey::from_index(SYLLABLE_KEY_COUNT), None);
    assert_eq!(SyllableKey::from_chewing_key(ChewingKey::ZERO), None);
}

// Rust dense-ID contract makes 405..428 incomplete; canonical-only tiers stay off the raw pinyin surface.
#[test]
fn completeness_follows_the_frozen_id_ranges() {
    for (index, completeness) in [
        (404, Completeness::Complete),
        (405, Completeness::Partial),
        (406, Completeness::Partial),
        (427, Completeness::Partial),
        (428, Completeness::Complete),
        (429, Completeness::Complete),
    ] {
        let spelling = SyllableKey::from_index(index).unwrap().text();
        let key = if index < 428 {
            SyllableKey::from_text(spelling)
        } else {
            SyllableKey::from_canonical_text(spelling)
        }
        .unwrap();
        assert_eq!(key.completeness(), completeness);
    }
    for spelling in crate::syllables::ZHUYIN_ONLY_COMPLETE_SYLLABLES {
        let key = SyllableKey::from_canonical_text(spelling).expect("zhuyin-only key");
        assert_eq!(key.completeness(), Completeness::Complete);
        assert!(key.index() >= 430);
        assert_eq!(SyllableKey::from_text(spelling), None);
    }
}

// Rust from_text performs exact inventory lookup without case, tone or fuzzy normalization.
#[test]
fn unknown_spellings_have_no_key() {
    assert_eq!(SyllableKey::from_text("NI"), None);
}

// Rust PhraseEntry preserves its token and transfers ownership of its text.
#[test]
fn phrase_entries_carry_a_token_and_text() {
    let entry = PhraseEntry::new(7, "你好".to_owned());
    assert_eq!(entry.token(), 7);
    assert_eq!(entry.text(), "你好");
    assert_eq!(entry.into_text().as_str(), Some("你好"));
}
