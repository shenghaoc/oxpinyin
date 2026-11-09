//! The fixture doubles' own contract: parsing and lookup.

use crate::common::fixture_doubles::{FixtureDictionary, FixtureError, FixtureLanguageModel};
use oxpinyin_core::{Dictionary, SyllableKey};

const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");

fn keys(text: &str) -> Vec<SyllableKey> {
    text.split(',')
        .map(|key| SyllableKey::from_text(key).expect("frozen key"))
        .collect()
}

fn dictionary() -> FixtureDictionary {
    FixtureDictionary::parse(VOCAB).expect("the committed fixture parses")
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
