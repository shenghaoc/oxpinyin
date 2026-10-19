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
    assert_eq!(whole[0].0.string(), "你好");
    assert_eq!(first[0].0.string(), "你");
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
    assert_eq!(three[0].0.string(), "中国人");
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

    assert_eq!(fangan[0].0.string(), "方案");
    assert_eq!(fangan_alt[0].0.string(), "反感");
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
    let texts: Vec<&str> = ranked
        .iter()
        .filter_map(|(entry, _)| entry.string().as_str())
        .collect();

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

    assert_eq!(alone[0].0.string(), "中国");
    assert_eq!(after[0].0.string(), "中国");
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
        type PathMemo = ();
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

#[test]
fn step_row_reuses_hits_and_misses_but_retries_errors() {
    use oxpinyin_core::{MergedGram, NbestStepRow};
    let mut row = NbestStepRow::default();
    assert_eq!(
        row.get_or_try_init::<&str>(|| Err("empty value")).err(),
        Some("empty value")
    );
    // The row keeps the order given: `lower_bound(9)` over `[9, 3]` lands
    // past the end and `lower_bound(3)` on the 9, so the pin's `get_freq`
    // misses both (`ngram.cpp:231-250`).
    let gram = row
        .get_or_try_init::<&str>(|| Ok(Some(MergedGram::new(0, vec![(9, 0), (3, 7)]))))
        .unwrap()
        .unwrap();
    assert_eq!(gram.get_freq(9), Ok(None));
    assert_eq!(gram.get_freq(4), Ok(None));
    assert_eq!(
        row.get_or_try_init::<&str>(|| panic!("hit reloaded"))
            .unwrap()
            .unwrap()
            .get_freq(3),
        Ok(None)
    );
    let mut miss = NbestStepRow::default();
    assert!(miss.get_or_try_init::<&str>(|| Ok(None)).unwrap().is_none());
    assert!(
        miss.get_or_try_init::<&str>(|| panic!("miss reloaded"))
            .unwrap()
            .is_none()
    );
}

/// `merge_single_gram` over an unsorted system row and a user row
/// (`ngram.cpp:308-349`): `3 > 2` appends the user's 2, the equal 3s sum,
/// and the system's remainder follows, giving `[2, 3, 1, 3]`. The pin's
/// `get_freq` then misses 1 and 2 and finds the second 3; `search` over
/// `1..=3` yields every item, duplicates included.
#[test]
fn a_merged_row_answers_as_the_pins_merged_chunk() {
    use oxpinyin_core::{MergedGram, NbestStepRow, NbestSuccessors};
    let chunk = |total: u32, items: &[(u32, u32)]| {
        let mut value = total.to_le_bytes().to_vec();
        for (token, count) in items {
            value.extend_from_slice(&token.to_le_bytes());
            value.extend_from_slice(&count.to_le_bytes());
        }
        value
    };
    let system = chunk(0, &[(3, 0), (1, 4), (3, 9)]);
    let user = chunk(0, &[(2, 7), (3, u32::MAX)]);
    let mut row = NbestStepRow::default();
    let gram = row
        .get_or_try_init::<&str>(|| Ok(MergedGram::merge(Some(system), Some(user))))
        .unwrap()
        .unwrap();
    assert_eq!(
        gram.as_bytes(),
        chunk(0, &[(2, 7), (3, u32::MAX), (1, 4), (3, 9)])
    );
    assert_eq!(gram.total(), Ok(0));
    assert_eq!(gram.get_freq(1), Ok(None));
    assert_eq!(gram.get_freq(2), Ok(None));
    assert_eq!(gram.get_freq(3), Ok(Some(9)));
    assert_eq!(gram.get_freq(4), Ok(None));
    let mut items = Vec::new();
    gram.search_into(1, 3, &mut items).unwrap();
    assert_eq!(items, [(2, 7), (3, u32::MAX), (1, 4), (3, 9)]);
    assert!(matches!(
        NbestSuccessors::Row(gram),
        NbestSuccessors::Row(row) if row.fault().is_none()
    ));
    assert!(MergedGram::merge(None, None).is_none());
}

/// A range ending at `u32::MAX`: the pin's exclusive end is the `guint32`
/// `token + 1` (`chewing_large_table2.h:115`, `:120` at the pin
/// `074a2219`), zero, and `search` stops at the first item at or past it
/// (`ngram.cpp:168`) — every item — so the walk yields nothing.
#[test]
fn a_range_ending_at_the_last_token_searches_nothing_as_the_pin_does() {
    use oxpinyin_core::MergedGram;
    let gram = MergedGram::new(2, vec![(u32::MAX - 1, 1), (u32::MAX, 1)]);
    let mut items = Vec::new();
    gram.search_into(u32::MAX, u32::MAX, &mut items).unwrap();
    gram.search_into(u32::MAX - 1, u32::MAX, &mut items)
        .unwrap();
    assert!(items.is_empty());
    gram.search_into(u32::MAX - 1, u32::MAX - 1, &mut items)
        .unwrap();
    assert_eq!(items, [(u32::MAX - 1, 1)]);
}

/// N2 refusals 1 and 5 at the merge (`ngram.cpp:303-349`): both grams
/// present, a user value shorter than its total asserts and a partial
/// tail is consumed past its end; every read of the row then answers the
/// fault. With no system gram the user value is the row as stored, and
/// the fault surfaces at the first read that meets it.
#[test]
fn a_merge_the_pin_cannot_read_answers_its_fault() {
    use oxpinyin_core::{GramFault, MergedGram};
    let system = {
        let mut value = 4_u32.to_le_bytes().to_vec();
        value.extend_from_slice(&2_u32.to_le_bytes());
        value.extend_from_slice(&4_u32.to_le_bytes());
        value
    };
    let short = MergedGram::merge(Some(system.clone()), Some(vec![1, 2])).unwrap();
    assert_eq!(short.fault(), Some(GramFault::Short));
    assert_eq!(short.total(), Err(GramFault::Short));
    assert_eq!(short.get_freq(2), Err(GramFault::Short));
    let mut tailed = 0_u32.to_le_bytes().to_vec();
    tailed.push(9);
    let over = MergedGram::merge(Some(system.clone()), Some(tailed.clone())).unwrap();
    assert_eq!(over.fault(), Some(GramFault::OverRead));
    let mut items = Vec::new();
    assert_eq!(over.search_into(0, 9, &mut items), Err(GramFault::OverRead));
    // No system gram: the user value itself, read as the pin reads it.
    let alone = MergedGram::merge(None, Some(vec![1, 2])).unwrap();
    assert_eq!(alone.fault(), None);
    assert_eq!(alone.as_bytes(), [1, 2]);
    assert_eq!(alone.total(), Err(GramFault::Short));
    assert_eq!(alone.get_freq(2), Err(GramFault::OverRead));
    assert_eq!(alone.search_into(0, 9, &mut items), Err(GramFault::Short));
    let alone = MergedGram::merge(None, Some(tailed)).unwrap();
    assert_eq!(alone.total(), Ok(0));
    assert_eq!(alone.get_freq(2), Err(GramFault::OverRead));
    // The system gram alone is its own bytes.
    assert_eq!(
        MergedGram::merge(Some(system.clone()), None)
            .unwrap()
            .as_bytes(),
        system
    );
}
