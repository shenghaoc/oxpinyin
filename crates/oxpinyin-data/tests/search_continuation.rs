//! Behavioural port of the upstream table-search suites.
//!
//! - `libpinyin/tests/storage/test_phrase_table.cpp` and
//!   `test_chewing_table.cpp`: a full-length search answers `SEARCH_OK`
//!   with the phrase's tokens, and sub-length searches answer
//!   `SEARCH_CONTINUED` — the probe that lets the candidate window scan
//!   stop widening.
//! - `libpinyin/tests/storage/test_phrase_index.cpp`: token lookups
//!   reverse to phrase text and pronunciations, unknown keys/tokens
//!   answer empty rather than erroring.
//! - `libpinyin/tests/test_phrase.cpp`: phrase text resolves to tokens
//!   through the index.
//!
//! The oxpinyin seam is `SystemDictionary` over the committed `fixtures/w3`
//! mini tables: [`Dictionary::lookup`] is the `SEARCH_OK` answer,
//! [`Dictionary::phrase_prefix_exists`] is the `SEARCH_CONTINUED` probe
//! (`docs/findings/core-trait-seam.md`), and `tokens_for_text` is the
//! phrase-index text reverse map.

use oxpinyin_core::{Completeness, Dictionary, OptionBits, PINYIN_INCOMPLETE, SyllableKey};
use oxpinyin_data::SystemDictionary;

fn dict() -> SystemDictionary {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("w3");
    SystemDictionary::open(
        &dir.join("pinyin_index.redb"),
        &dir.join("phrase_index.redb"),
    )
    .unwrap()
}

fn keys(text: &str) -> Vec<SyllableKey> {
    text.split(',')
        .map(|k| SyllableKey::from_text(k).unwrap())
        .collect()
}

#[test]
fn a_full_key_sequence_answers_ok_with_its_phrases() {
    // `SEARCH_OK`: the full key sequence resolves to entries.
    let dict = dict();
    let entries = dict.lookup(&keys("ni,hao")).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text(), "你好");
}

#[test]
fn every_prefix_of_a_stored_phrase_reports_continuation() {
    // `SEARCH_CONTINUED` for sub-lengths: each proper prefix of a stored
    // phrase's key sequence must report that some stored phrase extends
    // it — this is what bounds the decoder's window scan.
    let dict = dict();
    let ni_hao = keys("ni,hao");
    assert!(
        dict.phrase_prefix_exists(&ni_hao[..1]).unwrap(),
        "ni prefixes ni'hao"
    );
    assert!(dict.phrase_prefix_exists(&ni_hao).unwrap(), "equal key");
    assert!(
        dict.phrase_prefix_exists(&keys("zhong")).unwrap(),
        "zhong prefixes zhong'guo/zhong'guo'ren"
    );
    assert!(
        dict.phrase_prefix_exists(&keys("zhong,guo")).unwrap(),
        "zhong,guo prefixes zhong'guo'ren"
    );
}

#[test]
fn a_dead_end_sequence_reports_no_continuation() {
    // The negative of the probe: a key sequence nothing extends must say
    // so, or the window scan would keep searching past the last phrase.
    let dict = dict();
    assert!(
        !dict.phrase_prefix_exists(&keys("zhuang,zhuang")).unwrap(),
        "no mini phrase carries zhuang,zhuang"
    );
    assert!(
        !dict.phrase_prefix_exists(&keys("guo,guo")).unwrap(),
        "no mini phrase continues guo with guo"
    );
}

#[test]
fn an_initial_only_sequence_probes_the_initial_index() {
    // The incomplete path probes the initial index: an initial-only key
    // stands for every syllable sharing its initial, so `n` must report
    // continuation while the mini tables hold any n-initial phrase.
    let dict = dict();
    let n_initial = SyllableKey::from_option_text("n", OptionBits::from_bits(PINYIN_INCOMPLETE))
        .expect("initial-only key");
    assert!(
        dict.phrase_prefix_exists(&[n_initial]).unwrap(),
        "the initial n reaches n-initial phrases"
    );
}

#[test]
fn phrase_text_reverse_resolves_to_its_token() {
    // test_phrase.cpp's `get_phrase_token` direction: text in, token out.
    let dict = dict();
    let entries = dict.lookup(&keys("ni,hao")).unwrap();
    let token = entries[0].token().value();
    let tokens = dict.tokens_for_text("你好");
    assert!(
        tokens.contains(&token),
        "text reverse map must resolve the looked-up phrase: {tokens:?}"
    );
}

#[test]
fn phrase_text_and_pronunciations_roundtrip_the_index() {
    // test_phrase_index.cpp's item invariants: a token carries its phrase
    // text and its pronunciation list; unknown tokens answer empty.
    let dict = dict();
    let entries = dict.lookup(&keys("ni")).unwrap();
    let lead = &entries[0];
    assert_eq!(
        dict.phrase_text(lead.token().value()).unwrap().as_deref(),
        Some(lead.text())
    );
    let pronunciations = dict.pronunciations(lead.token().value()).unwrap();
    assert!(
        pronunciations.iter().any(|(pinyin, _)| pinyin == "ni"),
        "the lead token must carry its ni reading"
    );
    assert_eq!(dict.phrase_text(u32::MAX).unwrap(), None);
    assert!(dict.pronunciations(u32::MAX).unwrap().is_empty());
}

#[test]
fn reopen_rebuilds_the_same_answers() {
    // test_phrase_index.cpp's store/load roundtrip: a fresh open of the
    // same tables answers identically.
    let answers = |keys_text: &str| {
        let dict = dict();
        let entries = dict.lookup(&keys(keys_text)).unwrap();
        let texts: Vec<String> = entries.iter().map(|e| e.text().to_owned()).collect();
        texts
    };
    assert_eq!(answers("ni"), answers("ni"));
    assert_eq!(answers("fang,an"), answers("fang,an"));
}

#[test]
fn a_partial_key_cannot_answer_ok_but_the_probe_survives() {
    // Initial-only keys never appear in the complete inventory (their
    // completeness is Partial), and lookups with them stay empty, while
    // the continuation probe still routes through the initial index.
    let dict = dict();
    let partial =
        SyllableKey::from_option_text("n", OptionBits::from_bits(PINYIN_INCOMPLETE)).unwrap();
    assert_eq!(partial.completeness(), Completeness::Partial);
    assert!(dict.lookup(&[partial]).unwrap().is_empty());
}
