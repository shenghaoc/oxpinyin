//! Hand-computable correction-rate fixture and the full evaluate.py flow.
//!
//! A three-token vocabulary with one homophone pair pins the decode exactly:
//! `中`(10) and `钟`(20) both spell `zhong`, `国`(30) spells `guo`. `中` is
//! far more frequent than `钟` in both the unigram (100 vs 10) and after
//! `sentence_start` (bigram 30 vs 1), so `zhong` always decodes to `中` for
//! **any** λ ∈ [0, 1]. Therefore:
//!
//! * `中国` (中→国, bigram 40) decodes back to `中国` — passes;
//! * `钟` decodes to `中`, not `钟` — fails.
//!
//! Correction rate = 1 passed / 2 tested = 0.5, independent of the estimated
//! λ, which the test also exercises end to end (estimate → apply → decode).

use std::collections::BTreeMap;

use oxpinyin_core::{PhraseToken, SyllableKey};
use oxpinyin_eval::{
    EvalLanguageModel, PhraseSource, build_model, correction_rate, estimate_lambda,
    parse_interpolation2,
};
use oxpinyin_lambda::count_deleted;
use oxpinyin_testsupport::FixtureDictionary;

const VOCAB: &str = "\
token=10\tkeys=zhong\ttext=中\tunigram=100
token=20\tkeys=zhong\ttext=钟\tunigram=10
token=30\tkeys=guo\ttext=国\tunigram=50
";

/// The candidate `interpolation2.text` for the same model.
const INTERPOLATION2: &str = "\
\\data model interpolation
\\1-gram
\\item 10 中 count 100
\\item 20 钟 count 10
\\item 30 国 count 50
\\2-gram
\\item 1 <start> 10 中 count 30
\\item 1 <start> 20 钟 count 1
\\item 10 中 30 国 count 40
\\end
";

fn keys(text: &str) -> Vec<SyllableKey> {
    text.split(',')
        .map(|k| SyllableKey::from_text(k).expect("frozen key"))
        .collect()
}

/// A `PhraseSource` mirroring the fixture: token → best keys + text.
struct FixtureSource {
    best: BTreeMap<u32, Vec<SyllableKey>>,
    text: BTreeMap<u32, String>,
}

impl FixtureSource {
    fn new() -> Self {
        let mut best = BTreeMap::new();
        best.insert(10, keys("zhong"));
        best.insert(20, keys("zhong"));
        best.insert(30, keys("guo"));
        let mut text = BTreeMap::new();
        text.insert(10, "中".to_owned());
        text.insert(20, "钟".to_owned());
        text.insert(30, "国".to_owned());
        Self { best, text }
    }
}

impl PhraseSource for FixtureSource {
    fn best_keys(&self, token: PhraseToken) -> Option<Vec<SyllableKey>> {
        self.best.get(&token.value()).cloned()
    }
    fn text(&self, token: PhraseToken) -> Option<String> {
        self.text.get(&token.value()).cloned()
    }
    fn lexicon_tokens(&self) -> Vec<PhraseToken> {
        self.text
            .keys()
            .map(|&token| PhraseToken::new(token))
            .collect()
    }
}

fn sentences() -> Vec<Vec<PhraseToken>> {
    vec![
        vec![PhraseToken::new(10), PhraseToken::new(30)], // 中国
        vec![PhraseToken::new(20)],                       // 钟
    ]
}

#[test]
fn interpolation2_parses_to_the_expected_counts() {
    let counts = parse_interpolation2(INTERPOLATION2);
    assert_eq!(counts.unigrams.get(&10), Some(&100));
    assert_eq!(counts.bigrams.get(&(10, 30)), Some(&40));
    assert_eq!(counts.bigrams.get(&(1, 20)), Some(&1));
}

#[test]
fn full_flow_estimate_apply_decode_correction_rate() {
    // interpolation2.text → counts.
    let counts = parse_interpolation2(INTERPOLATION2);

    // estimate λ over a held-out slice (中 国), then apply it.
    let deleted = count_deleted("10 中\n30 国\n", true).expect("held-out counts");
    let lambda = estimate_lambda(&counts, &deleted).expect("lambda estimates");

    // decode the evaluation corpus.
    let dictionary = FixtureDictionary::parse(VOCAB).expect("vocab");
    let source = FixtureSource::new();
    let model = build_model(&counts, lambda, source.lexicon_tokens());
    let report = correction_rate(&dictionary, &model, &source, &sentences()).expect("evaluate");

    assert_eq!(report.tested, 2);
    assert_eq!(report.passed, 1, "中国 passes, 钟 fails");
    assert!((report.rate - 0.5).abs() < 1e-12, "rate {}", report.rate);
    assert_eq!(report.correction_rate_line(), "correction rate:0.500000");

    // The one failure is 钟 decoded as the more frequent homophone 中.
    assert_eq!(report.mismatches, vec![("钟".to_owned(), "中".to_owned())]);
}

#[test]
fn correction_rate_is_lambda_independent_here() {
    // The homophone ranking holds for every λ, so the rate is 0.5 whether λ
    // is 0 (pure unigram), 1 (pure bigram), or anything between.
    let counts = parse_interpolation2(INTERPOLATION2);
    let dictionary = FixtureDictionary::parse(VOCAB).expect("vocab");
    let source = FixtureSource::new();
    for value in ["0.000000", "0.312699", "1.000000"] {
        let lambda = oxpinyin_data::parse_table_conf_lambda(&format!("lambda parameter:{value}\n"))
            .expect("lambda");
        let model = EvalLanguageModel::from_counts(&counts, lambda);
        let report = correction_rate(&dictionary, &model, &source, &sentences()).expect("evaluate");
        assert_eq!(report.passed, 1, "λ={value}");
        assert!(
            (report.rate - 0.5).abs() < 1e-12,
            "λ={value} rate {}",
            report.rate
        );
    }
}
