//! Word→punctuation counting, pruning, merging, and the `puncts.table`
//! emission — the native reproduction of the trainer's `genpunct.py`.
//!
//! For each consecutive `(prev, cur)` line pair in a segmented document,
//! when `cur` is a `null_token` separator line whose raw text *starts with*
//! one of the fixed punctuation strings (first match in [`PUNCT_SEARCH`]
//! order wins), the pair `(prev_token, prev_word) → punct` is counted
//! (`genpunct.py:27-79`). Per-index counts are pruned at
//! [`PER_INDEX_THRESHOLD`], merged across indexes, pruned again at
//! [`ALL_INDEX_THRESHOLD`], and emitted as `token word punct freq` lines
//! with each word's punctuation sorted by frequency, descending
//! (`:211-226`).
//!
//! Determinism: the Python original iterates a `dict` in filesystem-walk
//! order, so its inter-word line order is not a stable compatibility
//! target. This port emits a canonical order — words ascending by
//! `(token, word)`, and within a word by `(freq desc, PUNCT_SEARCH
//! priority)` — so the output is a pure function of the input. The
//! per-word frequencies are the value-level compatibility target.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::error::PunctError;

/// The fixed punctuation search list, order-significant
/// (`genpunct.py:19`). Multi-character puncts precede their prefixes
/// (`……` before `…`) so the longest match wins.
pub const PUNCT_SEARCH: &[&str] = &[
    "……", "…", "，", "。", "；", "？", "！", "：", "\u{201c}", "\u{201d}", "、",
];

/// Per-index prune threshold, `5 * 100` (`myconfig.py:213-214`).
pub const PER_INDEX_THRESHOLD: u64 = 500;
/// Cross-index prune threshold, `PER_INDEX_THRESHOLD * 20`
/// (`myconfig.py:216-217`).
pub const ALL_INDEX_THRESHOLD: u64 = ALL_INDEX_MULTIPLIER * PER_INDEX_THRESHOLD;
const ALL_INDEX_MULTIPLIER: u64 = 20;

/// `null_token` (`novel_types.h:121`).
const NULL_TOKEN: u32 = 0;

/// The `(token, word)` key of a punctuation record.
type WordKey = (u32, String);

/// Word→punctuation counts. Per key, punctuations are kept in first-
/// encounter order (as the Python list is appended), which the canonical
/// export then reorders.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PunctCounts {
    pairs: BTreeMap<WordKey, Vec<(String, u64)>>,
}

impl PunctCounts {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts one segmented document's word→punctuation pairs
    /// (`handleOneText`, `genpunct.py:27-81`).
    ///
    /// # Errors
    ///
    /// Returns [`PunctError::Malformed`] when a non-empty line lacks the
    /// `token text` separator.
    pub fn add_document(&mut self, text: &str) -> Result<(), PunctError> {
        // (prev_token, prev_text) start at the sentinel (0, "").
        let mut prev: Option<(u32, String)> = None;
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let (cur_token, cur_text) = split_line(line)?;

            let Some((prev_token, prev_text)) = prev.clone() else {
                // First line becomes prev without recording.
                prev = Some((cur_token, cur_text.to_owned()));
                continue;
            };
            // prev is a real word only when its token is non-null; a null
            // prev is skipped by the `prev_token == 0` guard upstream.
            if prev_token == NULL_TOKEN {
                prev = Some((cur_token, cur_text.to_owned()));
                continue;
            }

            // A punctuation appears only on a null-token separator line
            // whose text starts with one of the search puncts.
            if cur_token == NULL_TOKEN
                && let Some(punct) = match_punct(cur_text)
            {
                self.record(prev_token, &prev_text, punct);
            }
            prev = Some((cur_token, cur_text.to_owned()));
        }
        Ok(())
    }

    /// Increment `(token, word) → punct`, appending the punct in encounter
    /// order on first sight (`genpunct.py:64-77`).
    fn record(&mut self, token: u32, word: &str, punct: &str) {
        let puncts = self.pairs.entry((token, word.to_owned())).or_default();
        if let Some(entry) = puncts.iter_mut().find(|(p, _)| p == punct) {
            entry.1 += 1;
        } else {
            puncts.push((punct.to_owned(), 1));
        }
    }

    /// Drops punctuations with `freq < threshold`, then drops words left
    /// with no punctuations (`prunePunctPair`, `genpunct.py:83-105`).
    pub fn prune(&mut self, threshold: u64) {
        for puncts in self.pairs.values_mut() {
            puncts.retain(|(_, freq)| *freq >= threshold);
        }
        self.pairs.retain(|_, puncts| !puncts.is_empty());
    }

    /// Merges another table into this one, summing the frequency of each
    /// `(word, punct)` pair (`loadOnePrune`, `genpunct.py:170-208`).
    pub fn merge(&mut self, other: &PunctCounts) {
        for (key, puncts) in &other.pairs {
            let target = self.pairs.entry(key.clone()).or_default();
            for (punct, freq) in puncts {
                if let Some(entry) = target.iter_mut().find(|(p, _)| p == punct) {
                    entry.1 += *freq;
                } else {
                    target.push((punct.clone(), *freq));
                }
            }
        }
    }

    /// Emits the canonical `token word punct freq` table
    /// (`exportAllPunctPairs`, `genpunct.py:211-226`): words ascending by
    /// `(token, word)`, and within a word by `(freq desc, PUNCT_SEARCH
    /// priority)`.
    #[must_use]
    pub fn to_table(&self) -> String {
        let mut out = String::new();
        for ((token, word), puncts) in &self.pairs {
            let mut sorted: Vec<&(String, u64)> = puncts.iter().collect();
            sorted.sort_by(|a, b| {
                b.1.cmp(&a.1)
                    .then_with(|| punct_priority(&a.0).cmp(&punct_priority(&b.0)))
            });
            for (punct, freq) in sorted {
                let _ = writeln!(out, "{token} {word} {punct} {freq}");
            }
        }
        out
    }

    /// Parses a `token word punct freq` table (the inverse of [`to_table`],
    /// for reading per-index intermediates back in the merge stage).
    ///
    /// # Errors
    ///
    /// Returns [`PunctError::Malformed`] on a line that is not four fields.
    pub fn from_table(text: &str) -> Result<Self, PunctError> {
        let mut counts = Self::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() != 4 {
                return Err(PunctError::Malformed {
                    detail: format!("expected `token word punct freq`: {line:?}"),
                });
            }
            let token = fields[0]
                .parse::<u32>()
                .map_err(|_| PunctError::Malformed {
                    detail: format!("token field {:?} is not an integer", fields[0]),
                })?;
            let freq = fields[3]
                .parse::<u64>()
                .map_err(|_| PunctError::Malformed {
                    detail: format!("freq field {:?} is not an integer", fields[3]),
                })?;
            counts
                .pairs
                .entry((token, fields[1].to_owned()))
                .or_default()
                .push((fields[2].to_owned(), freq));
        }
        Ok(counts)
    }

    /// Number of distinct `(token, word)` keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// The frequency recorded for `(token, word) → punct`, or 0.
    #[must_use]
    pub fn frequency(&self, token: u32, word: &str, punct: &str) -> u64 {
        self.pairs
            .get(&(token, word.to_owned()))
            .and_then(|puncts| puncts.iter().find(|(p, _)| p == punct))
            .map_or(0, |(_, freq)| *freq)
    }
}

/// Splits `token text` on the first space or tab.
fn split_line(line: &str) -> Result<(u32, &str), PunctError> {
    let (head, rest) = line
        .split_once([' ', '\t'])
        .ok_or_else(|| PunctError::Malformed {
            detail: format!("no separator in {line:?}"),
        })?;
    let token = head.parse::<u32>().map_err(|_| PunctError::Malformed {
        detail: format!("token field {head:?} is not an integer"),
    })?;
    Ok((token, rest))
}

/// The first [`PUNCT_SEARCH`] punct the text starts with, if any.
fn match_punct(text: &str) -> Option<&'static str> {
    PUNCT_SEARCH
        .iter()
        .copied()
        .find(|punct| text.starts_with(punct))
}

/// The `PUNCT_SEARCH` index of a punct (its priority; unknown puncts sort
/// last). Used only to break frequency ties deterministically.
fn punct_priority(punct: &str) -> usize {
    PUNCT_SEARCH
        .iter()
        .position(|p| *p == punct)
        .unwrap_or(PUNCT_SEARCH.len())
}

#[cfg(test)]
mod tests {
    use super::{ALL_INDEX_THRESHOLD, PER_INDEX_THRESHOLD, PunctCounts};

    fn doc(lines: &[&str]) -> String {
        let mut text = String::new();
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    #[test]
    fn thresholds_match_myconfig() {
        assert_eq!(PER_INDEX_THRESHOLD, 500);
        assert_eq!(ALL_INDEX_THRESHOLD, 10_000);
    }

    #[test]
    fn counts_word_followed_by_punctuation() {
        // 中国 then a separator line starting with 。
        let mut counts = PunctCounts::new();
        counts
            .add_document(&doc(&["10 中国", "0 。foo"]))
            .expect("count");
        assert_eq!(counts.frequency(10, "中国", "。"), 1);
    }

    #[test]
    fn first_matching_punct_in_search_order_wins() {
        // "……" must win over "…" because it is earlier in PUNCT_SEARCH.
        let mut counts = PunctCounts::new();
        counts
            .add_document(&doc(&["10 中国", "0 ……rest"]))
            .expect("count");
        assert_eq!(counts.frequency(10, "中国", "……"), 1);
        assert_eq!(counts.frequency(10, "中国", "…"), 0);
    }

    #[test]
    fn non_punct_separator_records_nothing() {
        let mut counts = PunctCounts::new();
        counts
            .add_document(&doc(&["10 中国", "0 abc"]))
            .expect("count");
        assert!(counts.is_empty());
    }

    #[test]
    fn null_prev_word_is_not_recorded() {
        // A separator followed by a punct separator: prev is null, skip.
        let mut counts = PunctCounts::new();
        counts.add_document(&doc(&["0 x", "0 。y"])).expect("count");
        assert!(counts.is_empty());
    }

    #[test]
    fn repeated_pairs_accumulate() {
        let mut counts = PunctCounts::new();
        counts
            .add_document(&doc(&["10 甲", "0 。", "10 甲", "0 。"]))
            .expect("count");
        assert_eq!(counts.frequency(10, "甲", "。"), 2);
    }

    #[test]
    fn prune_drops_below_threshold_and_empties() {
        let mut counts = PunctCounts::new();
        for _ in 0..3 {
            counts
                .add_document(&doc(&["10 甲", "0 。"]))
                .expect("count");
        }
        counts.prune(5);
        assert!(counts.is_empty(), "freq 3 < 5 pruned to empty");
        // freq 3 >= 3 survives.
        let mut kept = PunctCounts::new();
        for _ in 0..3 {
            kept.add_document(&doc(&["10 甲", "0 。"])).expect("count");
        }
        kept.prune(3);
        assert_eq!(kept.frequency(10, "甲", "。"), 3);
    }

    #[test]
    fn merge_sums_frequencies() {
        let mut a = PunctCounts::new();
        a.add_document(&doc(&["10 甲", "0 。"])).expect("count");
        let mut b = PunctCounts::new();
        b.add_document(&doc(&["10 甲", "0 。", "10 甲", "0 ，"]))
            .expect("count");
        a.merge(&b);
        assert_eq!(a.frequency(10, "甲", "。"), 2);
        assert_eq!(a.frequency(10, "甲", "，"), 1);
    }

    #[test]
    fn table_sorts_by_key_then_freq_desc() {
        let mut counts = PunctCounts::new();
        // 甲: 。×1, ，×3  → ， first (higher freq). 乙: 。×2.
        counts
            .add_document(&doc(&[
                "10 甲", "0 。", "10 甲", "0 ，", "10 甲", "0 ，", "10 甲", "0 ，", "20 乙",
                "0 。", "20 乙", "0 。",
            ]))
            .expect("count");
        let table = counts.to_table();
        assert_eq!(table, "10 甲 ， 3\n10 甲 。 1\n20 乙 。 2\n");
    }

    #[test]
    fn table_round_trips_through_from_table() {
        let mut counts = PunctCounts::new();
        counts
            .add_document(&doc(&["10 甲", "0 。", "20 乙", "0 ，"]))
            .expect("count");
        let table = counts.to_table();
        let reparsed = PunctCounts::from_table(&table).expect("parse");
        assert_eq!(reparsed.to_table(), table);
    }

    #[test]
    fn malformed_line_is_an_error() {
        let mut counts = PunctCounts::new();
        assert!(counts.add_document("10\n").is_err());
    }
}
