use super::super::tests::FixtureDictionary;
use super::{EvalLanguageModel, PhraseSource, correction_rate};
use oxpinyin::data::PhraseLibraries;
use oxpinyin::parser::ChewingKey;
use oxpinyin::vocab::PhraseToken;
use std::collections::BTreeMap;

// The hand-computable homophone fixture in eval/tests.rs exercises the
// full decode. These tests cover the error paths.

/// A minimal map-backed `PhraseSource` for the error tests: keys and text
/// per token, either absent.
struct MapSource {
    keys: BTreeMap<u32, Vec<ChewingKey>>,
    text: BTreeMap<u32, String>,
}
impl PhraseSource for MapSource {
    fn best_keys(&self, token: PhraseToken) -> Option<Vec<ChewingKey>> {
        self.keys.get(&token).cloned()
    }
    fn text(&self, token: PhraseToken) -> Option<String> {
        self.text.get(&token).cloned()
    }
}

fn fixture() -> (FixtureDictionary, PhraseLibraries) {
    let vocab = include_str!("../../../../../fixtures/w4/mini-vocab.txt");
    let dict = FixtureDictionary::parse(vocab).expect("vocab");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/w3")
        .join(oxpinyin::__internal::DEFAULT_STORE_EXT)
        .join("gb_char.bin");
    let mut libraries = PhraseLibraries::empty();
    oxpinyin_tool_support::data::load_heap_library(&mut libraries, 1, &path).unwrap();
    (dict, libraries)
}

#[test]
fn a_token_without_pronunciation_is_an_error() {
    let (dict, libraries) = fixture();
    let model = EvalLanguageModel::new(&libraries, None, 0.312_699);
    let source = MapSource {
        keys: BTreeMap::new(),
        text: BTreeMap::new(),
    };
    let sentences = vec![vec![1]];
    assert!(correction_rate(&dict, &model, &source, &sentences).is_none());
}

#[test]
fn a_token_without_text_is_an_error_not_a_shorter_expected_sentence() {
    let (dict, libraries) = fixture();
    let model = EvalLanguageModel::new(&libraries, None, 0.312_699);
    let mut keys = BTreeMap::new();
    keys.insert(1, vec![ChewingKey::from_pinyin("ni").expect("key")]);
    let source = MapSource {
        keys,
        text: BTreeMap::new(),
    };
    let sentences = vec![vec![1]];
    assert!(correction_rate(&dict, &model, &source, &sentences).is_none());
}

#[test]
fn a_key_chain_without_a_phrase_cover_is_an_error_not_a_mismatch() {
    let (dict, libraries) = fixture();
    let model = EvalLanguageModel::new(&libraries, None, 0.312_699);
    let mut keys = BTreeMap::new();
    // `guo` is not in the fixture vocabulary, so no phrase covers the
    // chain.
    keys.insert(1, vec![ChewingKey::from_pinyin("guo").expect("key")]);
    let mut text = BTreeMap::new();
    text.insert(1, "国".to_owned());
    let source = MapSource { keys, text };
    let sentences = vec![vec![1]];
    assert!(correction_rate(&dict, &model, &source, &sentences).is_none());
}
