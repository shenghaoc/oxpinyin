//! The real-model [`PhraseSource`]: a system phrase index answers each
//! token's best pronunciation and text.
//!
//! `get_possible_pinyin` picks the highest-frequency pronunciation
//! (`eval_correction_rate.cpp:34-61`); [`oxpinyin_data::SystemDictionary`]
//! exposes `pronunciations(token) → [(pinyin, freq)]` and `phrase_text`,
//! which is exactly that data. The pinyin string is apostrophe-joined
//! syllables, each parsed to a [`SyllableKey`].

use oxpinyin_core::{PhraseToken, SyllableKey};
use oxpinyin_data::SystemDictionary;

use crate::decode::PhraseSource;

/// A [`PhraseSource`] over a system phrase index.
pub struct SystemPhraseSource<'a> {
    dictionary: &'a SystemDictionary,
}

impl<'a> SystemPhraseSource<'a> {
    /// Wraps a system dictionary.
    #[must_use]
    pub fn new(dictionary: &'a SystemDictionary) -> Self {
        Self { dictionary }
    }
}

impl PhraseSource for SystemPhraseSource<'_> {
    fn best_keys(&self, token: PhraseToken) -> Option<Vec<SyllableKey>> {
        let pronunciations = self.dictionary.pronunciations(token.value()).ok()?;
        // The highest-frequency pronunciation; ties keep the earlier one, as
        // `get_possible_pinyin`'s `freq > max_freq` strict test does.
        let best = pronunciations
            .into_iter()
            .reduce(|best, next| if next.1 > best.1 { next } else { best })?;
        parse_pinyin_keys(&best.0)
    }

    fn text(&self, token: PhraseToken) -> Option<String> {
        self.dictionary.phrase_text(token.value()).ok().flatten()
    }

    fn lexicon_tokens(&self) -> Vec<PhraseToken> {
        // Every token of the loaded phrase index (the default tables the pin's
        // eval loads), aggregated by the dictionary at open.
        self.dictionary
            .unigram_records()
            .iter()
            .map(|&(token, _)| PhraseToken::new(token))
            .collect()
    }
}

/// Parses an apostrophe-joined pinyin string into syllable keys, or `None`
/// when any syllable is not a known key.
fn parse_pinyin_keys(pinyin: &str) -> Option<Vec<SyllableKey>> {
    pinyin
        .split('\'')
        .map(SyllableKey::from_text)
        .collect::<Option<Vec<_>>>()
}

#[cfg(test)]
mod tests {
    use super::parse_pinyin_keys;

    #[test]
    fn splits_apostrophe_joined_syllables() {
        let keys = parse_pinyin_keys("zhong'guo").expect("valid");
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn a_bad_syllable_is_none() {
        assert!(parse_pinyin_keys("zhong'qqq").is_none());
    }
}
