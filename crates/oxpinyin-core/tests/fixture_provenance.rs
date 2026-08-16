//! The W4 fixture vocabulary is derived from the frozen captures.
//!
//! `docs/findings/fixture-adapters.md` states the rule; this test is what
//! makes it checkable rather than asserted. Every phrase in
//! `fixtures/w4/mini-vocab.txt` must be text the pinned oracle actually
//! emitted in `fixtures/foundation/f-a.txt` or `f-c.txt`, and every unigram
//! weight must be exactly `1000 - 100 × best captured rank`.
//!
//! Key sequences are the authored part and are deliberately not checked here:
//! the capture ranks candidates across every segmentation of an input, so it
//! cannot say which keys a candidate covers. That is recorded in the finding.

use std::collections::BTreeMap;

const F_A: &str = include_str!("../../../fixtures/foundation/f-a.txt");
const F_C: &str = include_str!("../../../fixtures/foundation/f-c.txt");
const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");

const BASE_WEIGHT: u64 = 1_000;
const RANK_STEP: u64 = 100;

/// Best (lowest) candidate rank at which each phrase was captured.
fn captured_ranks() -> BTreeMap<String, usize> {
    let mut ranks: BTreeMap<String, usize> = BTreeMap::new();

    for line in F_A.lines().chain(F_C.lines()) {
        let Some(encoded) = field(line, "candidates_hex") else {
            continue;
        };
        if encoded == "-" {
            continue;
        }

        for (rank, item) in encoded.split(',').enumerate() {
            let phrase = decode_hex(item);
            ranks
                .entry(phrase)
                .and_modify(|best| *best = (*best).min(rank))
                .or_insert(rank);
        }
    }

    ranks
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.split('\t')
        .filter_map(|field| field.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

fn decode_hex(text: &str) -> String {
    let bytes: Vec<u8> = (0..text.len() / 2)
        .map(|index| {
            u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).expect("capture hex is valid")
        })
        .collect();
    String::from_utf8(bytes).expect("capture candidates are UTF-8")
}

/// The vocabulary fixture as `(token, keys, text, unigram)` in file order.
fn vocab_records() -> Vec<(u32, String, String, u64)> {
    VOCAB
        .lines()
        .filter(|line| !line.trim_start().starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            (
                field(line, "token").expect("token").parse().expect("token"),
                field(line, "keys").expect("keys").to_owned(),
                field(line, "text").expect("text").to_owned(),
                field(line, "unigram")
                    .expect("unigram")
                    .parse()
                    .expect("unigram"),
            )
        })
        .collect()
}

#[test]
fn every_fixture_phrase_is_captured_oracle_output() {
    let ranks = captured_ranks();
    let records = vocab_records();
    assert!(!records.is_empty(), "the fixture must not be empty");

    for (_, keys, text, _) in &records {
        assert!(
            ranks.contains_key(text),
            "{text:?} (keys {keys}) is not in any captured candidate list"
        );
    }
}

#[test]
fn every_unigram_weight_follows_the_documented_rank_rule() {
    let ranks = captured_ranks();

    for (token, keys, text, unigram) in vocab_records() {
        let rank = u64::try_from(ranks[&text]).expect("captured ranks are small");
        let expected = BASE_WEIGHT - RANK_STEP * rank;
        assert_eq!(
            unigram, expected,
            "token {token} ({text:?}, keys {keys}) is at captured rank {rank}"
        );
    }
}

#[test]
fn tokens_are_dense_and_ordered() {
    for (index, (token, ..)) in vocab_records().into_iter().enumerate() {
        let expected = u32::try_from(index + 1).expect("the fixture is small");
        assert_eq!(token, expected, "tokens number 1..=n in file order");
    }
}

#[test]
fn entries_sharing_a_key_sequence_keep_the_captured_order() {
    let mut previous: Option<(String, u64)> = None;

    for (_, keys, text, unigram) in vocab_records() {
        if let Some((last_keys, last_weight)) = &previous
            && *last_keys == keys
        {
            assert!(
                *last_weight >= unigram,
                "{text:?} outranks an earlier entry for keys {keys}"
            );
        }
        previous = Some((keys, unigram));
    }
}
