//! `populate.py` — count 1..N-gram word-history counts from the segmented
//! corpus, pruning rare rows.
//!
//! Upstream keeps one SQLite database per n-gram order, keyed by the
//! space-fenced word sequence `" w1 w2 … "`; this port keeps ordered maps
//! with the same string keys, so the partial-word merge (`partial.rs`) can
//! do the same space-fenced substitution. Each order slides a window of
//! that length over each sentence (reset at a `null_token` separator) and
//! increments the sequence's count (`populate.py:34-90`); after populating,
//! rows with `freq ≤ 1` are pruned (`:124-140`).

use std::collections::BTreeMap;

use crate::config::{NULL_TOKEN, PRUNE_MINIMUM_OCCURRENCE, SEP};
use crate::error::WordError;

/// The 1..N-gram tables, indexed by order (index 0 unused).
#[derive(Clone, Debug)]
pub struct NgramTables {
    tables: Vec<BTreeMap<String, u64>>,
    max_order: usize,
}

impl NgramTables {
    /// Empty tables for orders `1..=max_order`.
    #[must_use]
    pub fn new(max_order: usize) -> Self {
        Self {
            tables: vec![BTreeMap::new(); max_order + 1],
            max_order,
        }
    }

    /// The highest n-gram order tracked.
    #[must_use]
    pub fn max_order(&self) -> usize {
        self.max_order
    }

    /// Counts one segmented document into every order (`handleOneDocument`,
    /// `populate.py:34-90`).
    ///
    /// # Errors
    ///
    /// Returns [`WordError::Malformed`] when a non-empty line is not a
    /// `token word` record.
    pub fn populate_document(&mut self, text: &str) -> Result<(), WordError> {
        let mut sentence: Vec<&str> = Vec::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let (token, word) = split_line(line)?;
            if token == NULL_TOKEN {
                sentence.clear();
                continue;
            }
            sentence.push(word);
            let have = sentence.len();
            for length in 1..=have.min(self.max_order) {
                let window = &sentence[have - length..];
                let key = fence(window);
                *self.tables[length].entry(key).or_insert(0) += 1;
            }
        }
        Ok(())
    }

    /// Prunes rows with `freq ≤ 1` from every order (`pruneNgramTable`,
    /// `populate.py:124-140`).
    pub fn prune(&mut self) {
        for table in &mut self.tables {
            table.retain(|_, freq| *freq > PRUNE_MINIMUM_OCCURRENCE);
        }
    }

    /// The unigram frequency of `word` (`getWordFrequency`,
    /// `partialword.py:25-36`), or 0.
    #[must_use]
    pub fn unigram_freq(&self, word: &str) -> u64 {
        self.tables[1].get(&fence(&[word])).copied().unwrap_or(0)
    }

    /// The rows of order `order` as `(sequence, freq)`, sequence still
    /// space-fenced.
    pub fn rows(&self, order: usize) -> impl Iterator<Item = (&str, u64)> {
        self.tables[order]
            .iter()
            .map(|(key, freq)| (key.as_str(), *freq))
    }

    /// The number of rows at `order`.
    #[must_use]
    pub fn len(&self, order: usize) -> usize {
        self.tables[order].len()
    }

    /// Whether order `order` has no rows.
    #[must_use]
    pub fn is_empty(&self, order: usize) -> bool {
        self.tables[order].is_empty()
    }

    /// The frequency of an exact fenced sequence at `order`.
    #[must_use]
    pub fn get(&self, order: usize, fenced: &str) -> Option<u64> {
        self.tables[order].get(fenced).copied()
    }

    /// Adds `delta` to a fenced sequence at `order`, inserting if absent
    /// (`UPDATE … OR INSERT`, `partialword.py:95-101`). Returns whether the
    /// row was newly inserted.
    pub fn add(&mut self, order: usize, fenced: &str, delta: u64) -> bool {
        match self.tables[order].get_mut(fenced) {
            Some(freq) => {
                *freq += delta;
                false
            }
            None => {
                self.tables[order].insert(fenced.to_owned(), delta);
                true
            }
        }
    }
}

/// Space-fences a word window: `" w1 w2 … "` (`getWordSep` fencing).
#[must_use]
pub fn fence(words: &[&str]) -> String {
    let mut out = String::with_capacity(words.iter().map(|w| w.len() + 1).sum::<usize>() + 1);
    out.push(SEP);
    for word in words {
        out.push_str(word);
        out.push(SEP);
    }
    out
}

/// Splits `token word` on the first space or tab.
fn split_line(line: &str) -> Result<(u32, &str), WordError> {
    let (head, rest) = line
        .split_once([' ', '\t'])
        .ok_or_else(|| WordError::Malformed {
            detail: format!("no separator in {line:?}"),
        })?;
    let token = head.parse::<u32>().map_err(|_| WordError::Malformed {
        detail: format!("token field {head:?} is not an integer"),
    })?;
    Ok((token, rest))
}

#[cfg(test)]
mod tests {
    use super::{NgramTables, fence};
    use crate::config::MAX_COMBINE;

    fn doc(lines: &[&str]) -> String {
        let mut text = String::new();
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    #[test]
    fn fence_wraps_in_spaces() {
        assert_eq!(fence(&["甲"]), " 甲 ");
        assert_eq!(fence(&["甲", "乙"]), " 甲 乙 ");
    }

    #[test]
    fn counts_all_orders_of_a_sentence() {
        let mut tables = NgramTables::new(MAX_COMBINE);
        // "甲 乙 丙" once.
        tables
            .populate_document(&doc(&["1 甲", "2 乙", "3 丙"]))
            .expect("populate");
        // Unigrams: 甲, 乙, 丙 each once.
        assert_eq!(tables.get(1, " 甲 "), Some(1));
        // Bigrams: 甲 乙, 乙 丙.
        assert_eq!(tables.get(2, " 甲 乙 "), Some(1));
        assert_eq!(tables.get(2, " 乙 丙 "), Some(1));
        assert_eq!(tables.get(2, " 甲 丙 "), None);
        // Trigram: 甲 乙 丙.
        assert_eq!(tables.get(3, " 甲 乙 丙 "), Some(1));
    }

    #[test]
    fn separator_breaks_windows() {
        let mut tables = NgramTables::new(MAX_COMBINE);
        tables
            .populate_document(&doc(&["1 甲", "0 。", "2 乙"]))
            .expect("populate");
        // No 甲 乙 bigram across the separator.
        assert_eq!(tables.get(2, " 甲 乙 "), None);
        assert_eq!(tables.get(1, " 甲 "), Some(1));
        assert_eq!(tables.get(1, " 乙 "), Some(1));
    }

    #[test]
    fn prune_drops_freq_one() {
        let mut tables = NgramTables::new(MAX_COMBINE);
        // 甲 乙 twice, 丙 once.
        tables
            .populate_document(&doc(&["1 甲", "2 乙", "0 ", "1 甲", "2 乙", "0 ", "3 丙"]))
            .expect("populate");
        tables.prune();
        assert_eq!(tables.get(2, " 甲 乙 "), Some(2), "freq 2 kept");
        assert_eq!(tables.get(1, " 丙 "), None, "freq 1 pruned");
        assert_eq!(tables.get(1, " 甲 "), Some(2));
    }

    #[test]
    fn unigram_freq_accessor() {
        let mut tables = NgramTables::new(MAX_COMBINE);
        tables
            .populate_document(&doc(&["1 甲", "1 甲", "1 甲"]))
            .expect("populate");
        assert_eq!(tables.unigram_freq("甲"), 3);
        assert_eq!(tables.unigram_freq("乙"), 0);
    }
}
