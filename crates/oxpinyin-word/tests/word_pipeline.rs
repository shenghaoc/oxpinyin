//! End-to-end word-recognition pipeline over a hand-traced corpus.
//!
//! The corpus places `甲 乙` between four distinct left and right neighbours,
//! each sentence repeated 12 times (above the order-9 merge floor). The
//! bigram `甲 乙` (freq 48) clears the partial-word threshold (24, the median
//! of the dictionary unigram frequencies); after the merge, `甲乙` is
//! preceded and followed by four words each, so its prefix and postfix
//! entropy (ln 4) clear the new-word thresholds (also ln 4, from the only
//! dictionary words with entropy). Marking `甲乙 = 甲 + 乙` yields a single
//! combined pinyin at the full default frequency.

use std::collections::BTreeSet;

use oxpinyin_word::recognize;

fn corpus() -> Vec<String> {
    let sentences = [
        ["丙", "甲", "乙", "戊"],
        ["丁", "甲", "乙", "己"],
        ["戊", "甲", "乙", "丙"],
        ["己", "甲", "乙", "丁"],
    ];
    let mut text = String::new();
    for _ in 0..12 {
        for sentence in &sentences {
            for word in sentence {
                text.push_str("1 ");
                text.push_str(word);
                text.push('\n');
            }
            text.push_str("0 \n");
        }
    }
    vec![text]
}

fn dictionary() -> BTreeSet<String> {
    ["甲", "乙", "丙", "丁", "戊", "己"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

const OLDWORDS: &str = "甲\tjia\t100\n乙\tyi\t100\n";

#[test]
fn recognizes_the_merged_word_with_combined_pinyin() {
    let recognized = recognize(&corpus(), &dictionary(), OLDWORDS).expect("recognize");
    assert_eq!(recognized, "甲乙\tjia'yi\t100\n");
}

#[test]
fn pipeline_is_deterministic() {
    let a = recognize(&corpus(), &dictionary(), OLDWORDS).expect("a");
    let b = recognize(&corpus(), &dictionary(), OLDWORDS).expect("b");
    assert_eq!(a, b);
}
