//! `newword.py` — filter partial words to new words by prefix/postfix
//! information entropy over the (post-merge) bigram table.
//!
//! A partial word is a *new word* when it is preceded by, and followed by,
//! a sufficiently diverse set of words — measured by the Shannon entropy
//! `H = -Σ p·ln p` of its neighbours' bigram frequencies
//! (`computeEntropy`, `newword.py:116-124`) — clearing thresholds derived
//! from the dictionary words the same way the partial-word threshold is
//! (`computeThreshold`, `:170-208`; the `int(len*0.6)`-from-the-end
//! position). The bigram is built from the order-2 table *after* the
//! partial-word merge, so merged words appear as nodes.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{MINIMUM_ENTROPY, NEW_WORD_THRESHOLD};
use crate::error::WordError;
use crate::ngram::NgramTables;
use crate::partial::{PartialWord, threshold_index};

/// Predecessor/successor frequency lists for each word, from the order-2
/// table.
#[derive(Clone, Debug, Default)]
pub struct BigramIndex {
    preceded_by: BTreeMap<String, Vec<u64>>,
    followed_by: BTreeMap<String, Vec<u64>>,
}

impl BigramIndex {
    /// Builds the index from the current order-2 table
    /// (`populateBigramSqlite`, `newword.py:72-109`).
    #[must_use]
    pub fn from_tables(tables: &NgramTables) -> Self {
        let mut index = Self::default();
        for (row, freq) in tables.rows(2) {
            let words: Vec<&str> = row.trim_matches(' ').splitn(2, ' ').collect();
            if words.len() != 2 {
                continue;
            }
            index
                .preceded_by
                .entry(words[1].to_owned())
                .or_default()
                .push(freq);
            index
                .followed_by
                .entry(words[0].to_owned())
                .or_default()
                .push(freq);
        }
        index
    }

    /// Prefix entropy: entropy over the frequencies of the words that
    /// precede `word` (`computePrefixEntropy`, `newword.py:141-152`).
    #[must_use]
    pub fn prefix_entropy(&self, word: &str) -> f64 {
        entropy(self.preceded_by.get(word))
    }

    /// Postfix entropy: entropy over the frequencies of the words that
    /// follow `word` (`computePostfixEntropy`, `:155-167`).
    #[must_use]
    pub fn postfix_entropy(&self, word: &str) -> f64 {
        entropy(self.followed_by.get(word))
    }
}

/// `H = -Σ (f/total)·ln(f/total)`; empty input is 0 (`newword.py:116-124`,
/// `:143-144`).
fn entropy(freqs: Option<&Vec<u64>>) -> f64 {
    let Some(freqs) = freqs else {
        return 0.0;
    };
    if freqs.is_empty() {
        return 0.0;
    }
    let total: u64 = freqs.iter().sum();
    let total = total as f64;
    -freqs
        .iter()
        .map(|&freq| {
            let probability = freq as f64 / total;
            probability * probability.ln()
        })
        .sum::<f64>()
}

/// Which side's entropy to threshold.
#[derive(Clone, Copy)]
enum Side {
    Prefix,
    Postfix,
}

/// The entropy threshold for one side (`computeThreshold`,
/// `newword.py:170-208`): dictionary words with entropy ≥
/// [`MINIMUM_ENTROPY`], sorted ascending, indexed at the
/// `int(len*0.6)`-from-the-end position.
fn entropy_threshold(
    index: &BigramIndex,
    dict_words: &BTreeSet<String>,
    side: Side,
) -> Result<f64, WordError> {
    let mut entropies: Vec<f64> = dict_words
        .iter()
        .map(|word| match side {
            Side::Prefix => index.prefix_entropy(word),
            Side::Postfix => index.postfix_entropy(word),
        })
        .filter(|entropy| *entropy >= MINIMUM_ENTROPY)
        .collect();
    if entropies.is_empty() {
        return Err(WordError::Malformed {
            detail: "no dictionary word meets the minimum entropy".to_owned(),
        });
    }
    entropies.sort_by(|a, b| a.partial_cmp(b).expect("entropies are finite"));
    Ok(entropies[threshold_index(entropies.len(), NEW_WORD_THRESHOLD)])
}

/// Filters the partial words to new words (`filterPartialWord`,
/// `newword.py:215-249`): keep a word whose prefix and postfix entropy both
/// clear their thresholds, deduplicating by word in first-seen order.
///
/// # Errors
///
/// Returns [`WordError::Malformed`] when a threshold cannot be derived
/// (no dictionary word clears the minimum entropy).
pub fn recognize_new_words(
    tables: &NgramTables,
    dict_words: &BTreeSet<String>,
    partials: &[PartialWord],
) -> Result<Vec<String>, WordError> {
    let index = BigramIndex::from_tables(tables);
    let prefix_threshold = entropy_threshold(&index, dict_words, Side::Prefix)?;
    let postfix_threshold = entropy_threshold(&index, dict_words, Side::Postfix)?;

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut new_words = Vec::new();
    for partial in partials {
        let word = &partial.merged;
        if seen.contains(word) {
            continue;
        }
        if index.prefix_entropy(word) < prefix_threshold {
            continue;
        }
        if index.postfix_entropy(word) < postfix_threshold {
            continue;
        }
        new_words.push(word.clone());
        seen.insert(word.clone());
    }
    Ok(new_words)
}

#[cfg(test)]
mod tests {
    use super::{BigramIndex, entropy};
    use crate::config::MAX_COMBINE;
    use crate::ngram::NgramTables;

    #[test]
    fn entropy_of_uniform_two_is_ln_two() {
        // Two equally frequent neighbours: H = ln 2.
        let freqs = vec![5, 5];
        assert!((entropy(Some(&freqs)) - std::f64::consts::LN_2).abs() < 1e-12);
    }

    #[test]
    fn entropy_of_single_neighbour_is_zero() {
        assert_eq!(entropy(Some(&vec![7])), 0.0);
        assert_eq!(entropy(None), 0.0);
    }

    #[test]
    fn prefix_and_postfix_entropy_read_the_bigram() {
        let mut tables = NgramTables::new(MAX_COMBINE);
        // 甲 中, 乙 中 (中 preceded by 甲 and 乙 equally) — need freq ≥ 2 to
        // survive a later prune, but the index reads the raw order-2 table.
        for _ in 0..3 {
            tables.populate_document("1 甲\n9 中\n").expect("count");
        }
        for _ in 0..3 {
            tables.populate_document("2 乙\n9 中\n").expect("count");
        }
        let index = BigramIndex::from_tables(&tables);
        // 中 preceded by 甲(3) and 乙(3): H = ln 2.
        assert!((index.prefix_entropy("中") - std::f64::consts::LN_2).abs() < 1e-12);
        // 甲 followed only by 中: H = 0.
        assert_eq!(index.postfix_entropy("甲"), 0.0);
    }
}
