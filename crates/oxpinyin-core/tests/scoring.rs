//! Scorer behaviour against the shared fixture doubles.
//!
//! Moved out of `src/scoring.rs`'s inline tests: they exercise the public
//! `Scorer` through the fixture-backed `Dictionary`/`LanguageModel` from
//! `oxpinyin-testsupport`, which a `cfg(test)` build of `oxpinyin-core`
//! itself cannot link (the dev-dependency cycle would build two instances
//! of the crate, and trait impls would not unify). These are integration
//! tests in the Rust sense anyway — components working together across the
//! public seam.

use oxpinyin_core::cost::UNKNOWN_COST;
use oxpinyin_core::graph::{EdgeKind, SegmentGraph};
use oxpinyin_core::kbest::{EdgeCost, k_best};
use oxpinyin_core::scoring::{Scorer, ScoringConfig, ScoringError};
use oxpinyin_core::{Cost, Dictionary, PhraseEntry, PhraseToken, SyllableKey};
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

fn keys(text: &str) -> Vec<SyllableKey> {
    text.split(',')
        .map(|key| SyllableKey::from_text(key).expect("frozen key"))
        .collect()
}

fn backends() -> (FixtureDictionary, FixtureLanguageModel) {
    (
        FixtureDictionary::parse(VOCAB).expect("committed fixture"),
        FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures"),
    )
}

#[test]
fn the_edge_cost_charges_the_kind_and_the_key() {
    let (dictionary, model) = backends();
    let scorer = Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

    let graph = SegmentGraph::build(b"xian").expect("short input");
    let mut by_key = Vec::new();
    for edge in graph.edges() {
        by_key.push((edge.key().text(), edge.kind(), scorer.cost(None, edge)));
    }

    let exact = by_key
        .iter()
        .find(|(key, ..)| *key == "xian")
        .expect("xian edge");
    let split = by_key
        .iter()
        .find(|(key, ..)| *key == "xi")
        .expect("xi edge");
    assert_eq!(split.1, EdgeKind::Segmentation);
    assert_eq!(
        split.2 - scorer.key_cost(keys("xi")[0]),
        ScoringConfig::default().segmentation_penalty,
        "a segmentation edge carries its penalty"
    );
    assert_eq!(exact.2, scorer.key_cost(keys("xian")[0]));

    let unknown = by_key.iter().find(|(key, ..)| *key == "x").expect("x edge");
    assert!(
        unknown.2 >= UNKNOWN_COST,
        "an initial with no phrase of its own costs the floor plus its penalty"
    );
}

#[test]
fn worked_example_a_longer_phrase_beats_its_first_syllable() {
    // The pin's list for nihao opens 你好 then 你; for zhongguoren it is
    // 中国人, 中国, 中. Coverage credit is what reproduces that.
    let (dictionary, model) = backends();
    let scorer = Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

    let whole = scorer
        .rank_phrases(&[], &keys("ni,hao"), &[EdgeKind::Exact; 2])
        .expect("ranking cannot fail here");
    let first = scorer
        .rank_phrases(&[], &keys("ni"), &[EdgeKind::Exact])
        .expect("ranking cannot fail here");
    assert_eq!(whole[0].0.text(), "你好");
    assert_eq!(first[0].0.text(), "你");
    assert!(
        whole[0].1 < first[0].1,
        "你好 covers two keys and must outrank 你"
    );

    let three = scorer
        .rank_phrases(&[], &keys("zhong,guo,ren"), &[EdgeKind::Exact; 3])
        .expect("ranking cannot fail here");
    let two = scorer
        .rank_phrases(&[], &keys("zhong,guo"), &[EdgeKind::Exact; 2])
        .expect("ranking cannot fail here");
    assert_eq!(three[0].0.text(), "中国人");
    assert!(three[0].1 < two[0].1);
}

#[test]
fn worked_example_the_segmentation_penalty_orders_fangan() {
    // Measured: the pin ranks 方案 (fang + an, both exact) ahead of
    // 反感 (fan + gan, where fan is the shorter split at position 0).
    let (dictionary, model) = backends();
    let scorer = Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

    let fangan = scorer
        .rank_phrases(&[], &keys("fang,an"), &[EdgeKind::Exact, EdgeKind::Exact])
        .expect("ranking cannot fail here");
    let fangan_alt = scorer
        .rank_phrases(
            &[],
            &keys("fan,gan"),
            &[EdgeKind::Segmentation, EdgeKind::Exact],
        )
        .expect("ranking cannot fail here");

    assert_eq!(fangan[0].0.text(), "方案");
    assert_eq!(fangan_alt[0].0.text(), "反感");
    assert!(
        fangan[0].1 < fangan_alt[0].1,
        "方案 {} must beat 反感 {}",
        fangan[0].1,
        fangan_alt[0].1
    );
}

#[test]
fn worked_example_an_incomplete_key_reaches_the_pins_phrases() {
    // nih: the pin offers 你好, 霓虹, 拟合 — two-key phrases whose second
    // syllable starts with h.
    let (dictionary, model) = backends();
    let scorer = Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

    let ranked = scorer
        .rank_phrases(&[], &keys("ni,h"), &[EdgeKind::Exact, EdgeKind::Incomplete])
        .expect("ranking cannot fail here");
    let texts: Vec<&str> = ranked.iter().map(|(entry, _)| entry.text()).collect();

    assert_eq!(texts[0], "你好");
    for wanted in ["霓虹", "拟合", "你和", "你很", "你会"] {
        assert!(texts.contains(&wanted), "missing {wanted}");
    }
}

#[test]
fn a_bigram_context_changes_the_cost() {
    let (dictionary, model) = backends();
    let scorer = Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

    let alone = scorer
        .rank_phrases(&[], &keys("zhong,guo"), &[EdgeKind::Exact; 2])
        .expect("ranking cannot fail here");
    let after = scorer
        .rank_phrases(
            &[PhraseToken::new(11)],
            &keys("zhong,guo"),
            &[EdgeKind::Exact; 2],
        )
        .expect("ranking cannot fail here");

    assert_eq!(alone[0].0.text(), "中国");
    assert_eq!(after[0].0.text(), "中国");
    assert!(after[0].1 < alone[0].1, "你好 -> 中国 is a known bigram");
}

#[test]
fn the_scorer_drives_k_best() {
    let (dictionary, model) = backends();
    let scorer = Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");
    let graph = SegmentGraph::build(b"nihao").expect("short input");

    let paths = k_best(&graph, &scorer, 4).expect("k is small");
    assert!(!paths.is_empty());
    let spelled: Vec<&str> = paths[0]
        .edges()
        .iter()
        .map(|id| graph.edge(*id).expect("id from this graph").key().text())
        .collect();
    assert_eq!(spelled, ["ni", "hao"], "the phrase-bearing split wins");
}

#[test]
fn scoring_is_deterministic_and_never_panics_on_an_empty_backend() {
    let empty = FixtureDictionary::default();
    let model = FixtureLanguageModel::default();
    let scorer = Scorer::new(ScoringConfig::default(), &empty, &model).expect("empty loads");

    assert_eq!(scorer.key_cost(keys("ni")[0]), UNKNOWN_COST);
    let ranked = scorer
        .rank_phrases(&[], &keys("ni,hao"), &[EdgeKind::Exact; 2])
        .expect("an empty dictionary is not an error");
    assert!(ranked.is_empty());

    let graph = SegmentGraph::build(b"nihao").expect("short input");
    let first: Vec<Cost> = graph.edges().iter().map(|e| scorer.cost(None, e)).collect();
    let second: Vec<Cost> = graph.edges().iter().map(|e| scorer.cost(None, e)).collect();
    assert_eq!(first, second);
}

#[test]
fn a_failing_backend_is_reported_not_swallowed() {
    #[derive(Debug)]
    struct Broken;

    impl Dictionary for Broken {
        type Entry = PhraseEntry;
        type Error = &'static str;
        type Syllable = SyllableKey;

        fn lookup(&self, _: &[SyllableKey]) -> Result<Vec<PhraseEntry>, &'static str> {
            Err("closed")
        }
    }

    let model = FixtureLanguageModel::default();
    assert_eq!(
        Scorer::new(ScoringConfig::default(), &Broken, &model)
            .expect_err("the dictionary is broken"),
        ScoringError::Dictionary("closed".to_owned())
    );
}
