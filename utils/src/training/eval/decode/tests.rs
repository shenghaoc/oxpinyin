use super::super::tests::FixtureDictionary;
use super::{EvalLanguageModel, LibraryPhraseSource, correction_rate};
use oxpinyin::__internal::data::PhraseLibraries;
use oxpinyin::__internal::data::phrase_library::{ChunkItem, build_chunk};
use oxpinyin::facade::ChewingKey;

// The hand-computable homophone fixture in eval/tests.rs exercises the
// full decode. These tests cover the error paths.

fn fixture() -> (FixtureDictionary, PhraseLibraries) {
    let vocab = include_str!("../../../../../fixtures/w4/mini-vocab.txt");
    let dict = FixtureDictionary::parse(vocab).expect("vocab");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gb.bin");
    let uncovered = ChunkItem {
        phrase: vec!['国' as u32],
        unigram: 1,
        prons: vec![(vec![ChewingKey::from_pinyin("guo").unwrap().to_packed()], 1)],
    };
    let invalid_text = ChunkItem {
        phrase: vec![0x110000],
        unigram: 1,
        prons: vec![(vec![ChewingKey::from_pinyin("ni").unwrap().to_packed()], 1)],
    };
    std::fs::write(
        &path,
        build_chunk(&[(10, &uncovered), (20, &invalid_text)]).unwrap(),
    )
    .unwrap();
    let mut libraries = PhraseLibraries::empty();
    oxpinyin_tool_support::data::load_heap_library(&mut libraries, 1, &path).unwrap();
    (dict, libraries)
}

#[test]
fn a_token_without_pronunciation_is_an_error() {
    let (dict, libraries) = fixture();
    let model = EvalLanguageModel::new(&libraries, None, 0.312_699);
    let source = LibraryPhraseSource {
        libraries: &libraries,
    };
    let sentences = vec![vec![1]];
    assert!(correction_rate(&dict, &model, &source, &sentences).is_none());
}

#[test]
fn a_token_without_text_is_an_error_not_a_shorter_expected_sentence() {
    let (dict, libraries) = fixture();
    let model = EvalLanguageModel::new(&libraries, None, 0.312_699);
    let source = LibraryPhraseSource {
        libraries: &libraries,
    };
    // A stored non-scalar UCS-4 word has a pronunciation but no Rust text.
    assert!(source.best_keys(0x0100_0014).is_some());
    let sentences = vec![vec![0x0100_0014]];
    assert!(correction_rate(&dict, &model, &source, &sentences).is_none());
}

#[test]
fn a_key_chain_without_a_phrase_cover_is_an_error_not_a_mismatch() {
    let (dict, libraries) = fixture();
    let model = EvalLanguageModel::new(&libraries, None, 0.312_699);
    // guo is not in mini-vocab, so no phrase covers the real item's reading.
    let source = LibraryPhraseSource {
        libraries: &libraries,
    };
    let sentences = vec![vec![0x0100_000a]];
    assert!(correction_rate(&dict, &model, &source, &sentences).is_none());
}
