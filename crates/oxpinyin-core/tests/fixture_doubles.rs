//! The fixture doubles' own contract: parsing, lookup and the interpolated cost.

#[test]
fn extreme_counts_saturate_and_degrade_to_unknown_cost() {
    // Regression (docs/safety/oxpinyin-audit.md F-1/F-2): two
    // u64::MAX counts sharing a prev used to wrap the bigram total
    // (overflow panic in debug, silent wrap in release), and the u128
    // interpolation products overflowed unchecked. Parsing must
    // saturate the totals and the cost must fall back to
    // UNKNOWN_COST rather than panic or wrap.
    let vocab = "token=1\tkeys=ni\ttext=你\tunigram=18446744073709551615\n\
                 token=2\tkeys=ni\ttext=尼\tunigram=18446744073709551615\n";
    let bigrams = "prev=1\tnext=2\tcount=18446744073709551615\n\
                   prev=1\tnext=1\tcount=18446744073709551615\n";
    let model =
        FixtureLanguageModel::parse(vocab, bigrams).expect("the extreme-count fixture parses");
    // Pin the saturation itself: a wrapped total would also degrade
    // to UNKNOWN_COST below, so only this assertion distinguishes
    // saturating from wrapping.
    assert_eq!(
        model.bigram_totals.get(&PhraseToken::new(1)),
        Some(&u64::MAX)
    );
    assert_eq!(
        model.model_cost(&[PhraseToken::new(1)], &PhraseToken::new(2)),
        UNKNOWN_COST
    );
}

use crate::common::fixture_doubles::{FixtureDictionary, FixtureError, FixtureLanguageModel};
use oxpinyin_core::cost::UNKNOWN_COST;
use oxpinyin_core::{Dictionary, LanguageModel, PhraseToken, SyllableKey};

const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

fn keys(text: &str) -> Vec<SyllableKey> {
    text.split(',')
        .map(|key| SyllableKey::from_text(key).expect("frozen key"))
        .collect()
}

fn dictionary() -> FixtureDictionary {
    FixtureDictionary::parse(VOCAB).expect("the committed fixture parses")
}

fn model() -> FixtureLanguageModel {
    FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("the committed fixtures parse")
}

#[test]
fn the_committed_vocabulary_loads() {
    let dictionary = dictionary();
    assert_eq!(dictionary.len(), 90);
    assert!(!dictionary.is_empty());
    assert_eq!(dictionary.max_keys(), 3);
}

#[test]
fn lookup_preserves_the_captured_candidate_order() {
    let dictionary = dictionary();
    let entries = dictionary.lookup(&keys("ni")).expect("lookup cannot fail");
    let texts: Vec<&str> = entries
        .iter()
        .filter_map(oxpinyin_core::PhraseEntry::text_str)
        .collect();
    assert_eq!(
        texts,
        ["你", "尼", "呢", "泥", "妮", "拟", "逆", "倪", "腻", "溺"]
    );

    let phrase = dictionary.lookup(&keys("zhong,guo,ren")).expect("lookup");
    assert_eq!(phrase.len(), 1);
    assert_eq!(phrase[0].text(), "中国人");
}

#[test]
fn an_absent_key_sequence_is_an_empty_lookup() {
    let dictionary = dictionary();
    assert!(
        dictionary
            .lookup(&keys("zhuang"))
            .expect("lookup cannot fail")
            .is_empty()
    );
    assert!(
        dictionary
            .lookup(&[])
            .expect("lookup cannot fail")
            .is_empty()
    );
}

#[test]
fn lookups_are_deterministic() {
    let dictionary = dictionary();
    for sequence in ["ni", "ni,hao", "zhong,guo", "xi,an", "fang,an"] {
        let first = dictionary.lookup(&keys(sequence)).expect("lookup");
        let second = dictionary.lookup(&keys(sequence)).expect("lookup");
        assert_eq!(first, second, "sequence: {sequence}");
    }
}

#[test]
fn the_model_ranks_the_captured_order() {
    let model = model();
    assert_eq!(model.len(), 90);
    assert_eq!(model.bigram_count(), 8);

    let dictionary = dictionary();
    let entries = dictionary.lookup(&keys("ni")).expect("lookup");
    let mut previous = None;
    for entry in &entries {
        let cost = model.model_cost(&[], &entry.token());
        if let Some(previous) = previous {
            assert!(
                cost >= previous,
                "{} scored better than a higher-ranked phrase",
                entry.text()
            );
        }
        previous = Some(cost);
    }
}

#[test]
fn a_known_bigram_beats_the_unigram_estimate() {
    let model = model();
    let nihao = PhraseToken::new(11);
    let zhongguo = PhraseToken::new(33);
    let xian = PhraseToken::new(59);

    let alone = model.model_cost(&[], &zhongguo);
    let after = model.model_cost(&[nihao], &zhongguo);
    assert!(after < alone, "the bigram must help: {after} vs {alone}");

    let unrelated = model.model_cost(&[xian], &zhongguo);
    assert_eq!(unrelated, alone, "an absent pair falls back to the unigram");
}

#[test]
fn an_unknown_token_costs_the_documented_floor() {
    let model = model();
    assert_eq!(
        model.model_cost(&[], &PhraseToken::new(9_999)),
        UNKNOWN_COST
    );
    assert_eq!(
        model
            .score(&[], &PhraseToken::new(9_999), 7)
            .expect("scoring cannot fail"),
        7 + UNKNOWN_COST
    );
}

#[test]
fn scoring_adds_the_edge_cost_and_is_deterministic() {
    let model = model();
    let token = PhraseToken::new(1);
    let base = model.score(&[], &token, 0).expect("scoring");
    assert_eq!(model.score(&[], &token, 250).expect("scoring"), base + 250);
    assert_eq!(model.score(&[], &token, 0).expect("scoring"), base);
}

#[test]
fn malformed_records_are_reported_with_their_line() {
    let failure = |text: &str| {
        FixtureDictionary::parse(text).expect_err("the record is malformed on purpose")
    };

    assert_eq!(
        failure("token=1\tkeys=ni"),
        FixtureError::MissingField {
            line: 1,
            field: "text"
        }
    );
    assert_eq!(
        failure("token=1\tkeys=qqq\ttext=x\tunigram=1"),
        FixtureError::UnknownKey {
            line: 1,
            key: "qqq".to_owned()
        }
    );
    assert_eq!(
        failure("token=1\tkeys=ni\ttext=x\tunigram=1\ntoken=1\tkeys=ni\ttext=y\tunigram=1"),
        FixtureError::DuplicateToken { line: 2, token: 1 }
    );
    assert_eq!(
        failure("token=1\tkeys=ni\ttext=x\tunigram=nope"),
        FixtureError::InvalidNumber {
            line: 1,
            field: "unigram",
            value: "nope".to_owned()
        }
    );
    assert_eq!(
        FixtureLanguageModel::parse(
            "token=1\tkeys=ni\ttext=x\tunigram=1",
            "prev=1\tnext=2\tcount=1"
        )
        .expect_err("the bigram names an undefined token"),
        FixtureError::UnknownToken { line: 1, token: 2 }
    );

    for message in [
        FixtureError::MissingField {
            line: 1,
            field: "text",
        }
        .to_string(),
        FixtureError::UnknownKey {
            line: 2,
            key: "qqq".to_owned(),
        }
        .to_string(),
        FixtureError::DuplicateToken { line: 3, token: 1 }.to_string(),
        FixtureError::InvalidNumber {
            line: 4,
            field: "unigram",
            value: "nope".to_owned(),
        }
        .to_string(),
        FixtureError::UnknownToken { line: 5, token: 2 }.to_string(),
    ] {
        assert!(message.starts_with("line "), "message: {message}");
    }
}

#[test]
fn comments_and_blank_lines_are_skipped() {
    let dictionary = FixtureDictionary::parse(
        "# header\n\n  \ntoken=1\tkeys=ni\ttext=x\tunigram=1\t# trailing\n",
    )
    .expect("comments parse");
    assert_eq!(dictionary.len(), 1);
}
