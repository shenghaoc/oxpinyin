//! `SystemDictionary` behaviour over the committed mini fixtures.
//!
//! Public-API integration tests moved out of `src/dict.rs`'s inline tests;
//! the source file keeps the tests that pin private invariants of the
//! loaded index structures (lazy reverse map, sorted rows, binary search).

use oxpinyin_core::{Dictionary, SyllableKey};
use oxpinyin_data::SystemDictionary;

fn fixtures_dir() -> std::path::PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    std::path::PathBuf::from(manifest)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("w3")
}

fn dict() -> SystemDictionary {
    SystemDictionary::open(
        &fixtures_dir().join(oxpinyin_data::default_store_file("pinyin_index")),
        &fixtures_dir().join(oxpinyin_data::default_store_file("phrase_index")),
    )
    .unwrap()
}

fn key(text: &str) -> SyllableKey {
    SyllableKey::from_text(text).expect("frozen syllable")
}

#[test]
fn mini_fixture_opens() {
    assert_eq!(dict().key_count().unwrap(), 10);
}

#[test]
fn single_syllable_is_frequency_ranked() {
    let entries = dict().lookup(&[key("ni")]).unwrap();
    assert!(!entries.is_empty());
    // 你 dominates the pin's ni column; the exporter froze
    // frequency-descending order.
    assert_eq!(entries[0].text(), "你");
}

#[test]
fn multi_syllable_lookup_is_one_string_key() {
    let entries = dict().lookup(&[key("ni"), key("hao")]).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text(), "你好");

    let entries = dict().lookup(&[key("zhong"), key("guo")]).unwrap();
    assert!(entries.iter().any(|entry| entry.text() == "中国"));
}

#[test]
fn apostrophe_keeps_xian_and_xi_an_apart() {
    let xian = dict().lookup(&[key("xian")]).unwrap();
    assert!(xian.iter().any(|entry| entry.text() == "现"));
    assert!(!xian.iter().any(|entry| entry.text() == "西安"));

    let xi_an = dict().lookup(&[key("xi"), key("an")]).unwrap();
    assert!(xi_an.iter().any(|entry| entry.text() == "西安"));
    assert!(!xi_an.iter().any(|entry| entry.text() == "现"));
}

#[test]
fn unknown_sequence_is_empty_not_an_error() {
    let entries = dict().lookup(&[key("zhuang"), key("zhuang")]).unwrap();
    assert!(entries.is_empty());
    let entries = dict().lookup(&[]).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn lookup_into_matches_lookup_and_keeps_capacity() {
    let dict = dict();
    let via_lookup = dict.lookup(&[key("ni")]).unwrap();
    let mut buf = Vec::with_capacity(via_lookup.len().max(1));
    dict.lookup_into(&[key("ni")], &mut buf).unwrap();
    assert_eq!(buf, via_lookup);
    let cap = buf.capacity();
    dict.lookup_into(&[key("ni"), key("hao")], &mut buf)
        .unwrap();
    assert_eq!(buf, dict.lookup(&[key("ni"), key("hao")]).unwrap());
    assert!(buf.capacity() >= cap);
}

#[test]
fn phrase_text_and_pronunciations_reverse_the_index() {
    let dict = dict();
    let entries = dict.lookup(&[key("ni")]).unwrap();
    let lead = &entries[0];
    assert_eq!(
        dict.phrase_text(lead.token().value()).unwrap().as_deref(),
        Some(lead.text())
    );
    // The lead's pronunciation list contains the lookup key itself.
    let pronunciations = dict.pronunciations(lead.token().value()).unwrap();
    assert!(
        pronunciations.iter().any(|(pinyin, _)| pinyin == "ni"),
        "lead token must carry the ni reading"
    );
    // Unknown tokens reverse to nothing, not to an error.
    assert!(dict.phrase_text(u32::MAX).unwrap().is_none());
    assert!(dict.pronunciations(u32::MAX).unwrap().is_empty());
}
