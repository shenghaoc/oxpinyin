use super::super::counts::Counts;
use super::super::fixture_doubles::FixtureDictionary;
use super::{EvalError, EvalLanguageModel, PhraseSource, correction_rate};
use oxpinyin::user::data::PINNED_LAMBDA;
use oxpinyin::{PhraseToken, SyllableKey};
use std::collections::BTreeMap;

// The hand-computable homophone fixture in eval/tests.rs exercises the
// full decode. These tests cover the error paths.

/// A minimal map-backed `PhraseSource` for the error tests: keys and text
/// per token, either absent.
struct MapSource {
    keys: BTreeMap<u32, Vec<SyllableKey>>,
    text: BTreeMap<u32, String>,
}
impl PhraseSource for MapSource {
    fn best_keys(&self, token: PhraseToken) -> Option<Vec<SyllableKey>> {
        self.keys.get(&token.value()).cloned()
    }
    fn text(&self, token: PhraseToken) -> Option<String> {
        self.text.get(&token.value()).cloned()
    }
}

fn fixture() -> (FixtureDictionary, EvalLanguageModel) {
    // The real fixture dictionary; an evaluation model that gives every
    // fixture token a unigram count.
    let vocab = include_str!("../../../../../fixtures/w4/mini-vocab.txt");
    let dict = FixtureDictionary::parse(vocab).expect("vocab");
    let mut counts = Counts::default();
    counts.unigrams.insert(1, 10);
    let model = EvalLanguageModel::from_counts(&counts, PINNED_LAMBDA);
    (dict, model)
}

#[test]
fn a_token_without_pronunciation_is_an_error() {
    let (dict, model) = fixture();
    let source = MapSource {
        keys: BTreeMap::new(),
        text: BTreeMap::new(),
    };
    let sentences = vec![vec![PhraseToken::new(1)]];
    let err = correction_rate(&dict, &model, &source, &sentences).unwrap_err();
    assert!(matches!(err, EvalError::NoPronunciation { token: 1 }));
}

#[test]
fn a_token_without_text_is_an_error_not_a_shorter_expected_sentence() {
    let (dict, model) = fixture();
    let mut keys = BTreeMap::new();
    keys.insert(1, vec![SyllableKey::from_text("ni").expect("key")]);
    let source = MapSource {
        keys,
        text: BTreeMap::new(),
    };
    let sentences = vec![vec![PhraseToken::new(1)]];
    let err = correction_rate(&dict, &model, &source, &sentences).unwrap_err();
    assert!(matches!(err, EvalError::NoText { token: 1 }), "{err}");
}

#[test]
fn a_key_chain_without_a_phrase_cover_is_an_error_not_a_mismatch() {
    let (dict, model) = fixture();
    let mut keys = BTreeMap::new();
    // `guo` is not in the fixture vocabulary, so no phrase covers the
    // chain.
    keys.insert(1, vec![SyllableKey::from_text("guo").expect("key")]);
    let mut text = BTreeMap::new();
    text.insert(1, "国".to_owned());
    let source = MapSource { keys, text };
    let sentences = vec![vec![PhraseToken::new(1)]];
    let err = correction_rate(&dict, &model, &source, &sentences).unwrap_err();
    assert!(matches!(err, EvalError::Undecodable { keys: 1 }), "{err}");
}
