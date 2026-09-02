//! `partialword.py` — the partial-word threshold and the iterative
//! partial-word discovery with cross-order sequence merging.
//!
//! A *partial word* is an adjacent pair `(prefix, postfix)` whose bigram
//! count exceeds a threshold derived from the dictionary words' unigram
//! frequencies (`computeThreshold`, `partialword.py:39-65`). Discovery
//! iterates: pull the qualifying bigrams, emit the new ones to
//! `partialword.txt`, then merge every occurrence of each new pair down
//! through the n-gram orders (`doCombineWord`, `:209-249`) so higher-order
//! sequences collapse and feed new bigrams into the next pass. It stops
//! when a pass finds nothing new or after [`MAXIMUM_ITERATION`] passes.
//!
//! Upstream backs the orders with SQLite and the phrase search with an
//! FTS3 table; this port uses the ordered maps of [`NgramTables`] and the
//! same space-fenced string substitution, so the merge is byte-faithful to
//! the Python `partition` walk (which merges one occurrence at a time and
//! consumes the shared fence space between adjacent pairs). The FTS3
//! phrase lookup is replaced by an index from each survivor's fenced pair
//! to its merged form: a fenced pair can only occur at an adjacent-word
//! position of a row, so each row is walked once over its adjacent pairs
//! instead of once per survivor.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{
    MAXIMUM_ITERATION, NGRAM_MINIMUM_OCCURRENCE, PARTIAL_WORD_THRESHOLD, SEP,
    WORD_MINIMUM_OCCURRENCE,
};
use crate::error::WordError;
use crate::ngram::{NgramTables, fence};

/// A partial-word record: the merged word and the pair it came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialWord {
    /// `prefix + postfix`.
    pub merged: String,
    /// The first component.
    pub prefix: String,
    /// The second component.
    pub postfix: String,
    /// The bigram frequency at discovery.
    pub freq: u64,
}

impl PartialWord {
    /// The `partialword.txt` line: `merged\tprefix\tpostfix\tfreq`.
    #[must_use]
    pub fn to_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.merged, self.prefix, self.postfix, self.freq
        )
    }
}

/// The partial-word frequency threshold (`computeThreshold`,
/// `partialword.py:39-65`): dictionary words with unigram freq ≥
/// [`WORD_MINIMUM_OCCURRENCE`], sorted ascending, indexed at the
/// `int(len * 0.5)`-from-the-end position.
///
/// # Errors
///
/// Returns [`WordError::Malformed`] when no dictionary word meets the
/// minimum occurrence (upstream would index an empty list).
pub fn compute_threshold(
    tables: &NgramTables,
    dict_words: &BTreeSet<String>,
) -> Result<u64, WordError> {
    let mut freqs: Vec<u64> = dict_words
        .iter()
        .map(|word| tables.unigram_freq(word))
        .filter(|freq| *freq >= WORD_MINIMUM_OCCURRENCE)
        .collect();
    if freqs.is_empty() {
        return Err(WordError::Malformed {
            detail: "no dictionary word meets the minimum occurrence".to_owned(),
        });
    }
    freqs.sort_unstable();
    Ok(freqs[threshold_index(freqs.len(), PARTIAL_WORD_THRESHOLD)])
}

/// The Python `[-int(len * ratio)]` index into an ascending list: `int()`
/// truncates, and `[-0]` is `[0]`.
pub(crate) fn threshold_index(len: usize, ratio: f64) -> usize {
    let pos = (len as f64 * ratio) as usize;
    if pos == 0 { 0 } else { len - pos }
}

/// Runs the partial-word discovery, returning the `partialword.txt` lines
/// and mutating `tables` by the cross-order merges.
#[must_use]
pub fn recognize_partial_words(
    tables: &mut NgramTables,
    dict_words: &BTreeSet<String>,
    threshold: u64,
) -> Vec<PartialWord> {
    let mut output = Vec::new();
    // Pairs already emitted as merged, so a later pass skips them.
    let mut merged_pairs: BTreeSet<(String, String)> = BTreeSet::new();

    for _pass in 0..MAXIMUM_ITERATION {
        let candidates = qualifying_bigrams(tables, threshold);

        // Survivors: not a known dictionary word, not an already-merged pair.
        let survivors: Vec<&PartialWord> = candidates
            .iter()
            .filter(|c| {
                !dict_words.contains(&c.merged)
                    && !merged_pairs.contains(&(c.prefix.clone(), c.postfix.clone()))
            })
            .collect();
        if survivors.is_empty() {
            break;
        }

        for survivor in &survivors {
            output.push((*survivor).clone());
        }

        merge_survivors_down(tables, &survivors);

        // Remember every candidate pair as merged (`:328-330`).
        for candidate in &candidates {
            merged_pairs.insert((candidate.prefix.clone(), candidate.postfix.clone()));
        }
    }

    output
}

/// The bigrams with `freq > threshold`, as `(merged, prefix, postfix, freq)`
/// (`getPartialWordList`, `partialword.py:172-188`).
fn qualifying_bigrams(tables: &NgramTables, threshold: u64) -> Vec<PartialWord> {
    let mut out = Vec::new();
    for (row, freq) in tables.rows(2) {
        if freq <= threshold {
            continue;
        }
        let words: Vec<&str> = row.trim_matches(' ').splitn(2, ' ').collect();
        if words.len() != 2 {
            continue;
        }
        let (prefix, postfix) = (words[0], words[1]);
        out.push(PartialWord {
            merged: format!("{prefix}{postfix}"),
            prefix: prefix.to_owned(),
            postfix: postfix.to_owned(),
            freq,
        });
    }
    out
}

/// Merge each survivor pair down through orders `N..2` into `N-1..1`
/// (`recognizePartialWord`'s `for i in range(N, 1, -1)` loop,
/// `partialword.py:296-326`), cascading within the pass.
fn merge_survivors_down(tables: &mut NgramTables, survivors: &[&PartialWord]) {
    // Fenced pair → merged fenced word (`" a b "` → `" ab "`), built once
    // for every order. Survivor pairs are distinct (each comes from its own
    // bigram row), so the map loses nothing.
    let by_pair: BTreeMap<String, String> = survivors
        .iter()
        .map(|survivor| {
            let merged_word = format!("{}{}", survivor.prefix, survivor.postfix);
            (
                fence(&[&survivor.prefix, &survivor.postfix]),
                fence(&[&merged_word]),
            )
        })
        .collect();

    for order in (2..=tables.max_order()).rev() {
        // Snapshot of the high order's qualifying rows (`freq > 9`), the
        // FTS clone. The high order is not modified (the upstream DELETE of
        // the merged low-order key from the high table is a no-op), so one
        // snapshot serves every survivor at this order.
        let snapshot: Vec<(String, u64)> = tables
            .rows(order)
            .filter(|(_, freq)| *freq > NGRAM_MINIMUM_OCCURRENCE)
            .map(|(row, freq)| (row.to_owned(), freq))
            .collect();

        for (row, freq) in &snapshot {
            // Words never contain the fence separator (`populate_document`
            // rejects them), so a fenced pair matches a row exactly when it
            // is one of the row's adjacent word pairs. A pair present more
            // than once is merged once per survivor, over every occurrence,
            // exactly as the per-survivor `contains` walk did.
            let words: Vec<&str> = row.split(SEP).filter(|w| !w.is_empty()).collect();
            let mut merged_here: BTreeSet<&str> = BTreeSet::new();
            for pair in words.windows(2) {
                let Some((words_str, merged_str)) = by_pair.get_key_value(fence(pair).as_str())
                else {
                    continue;
                };
                if !merged_here.insert(words_str) {
                    continue;
                }
                for merged in combine_occurrences(row, words_str, merged_str) {
                    tables.add(order - 1, &merged, *freq);
                }
            }
        }
    }
}

/// Produce one merged sequence per occurrence of `words_str` in `matched`,
/// each replacing that single occurrence with `merged_str`
/// (`doCombineWord`'s `partition` loop, `partialword.py:227-249`).
///
/// The Python loop searches each next occurrence in the suffix *after* the
/// previous match, so adjacent pairs sharing a fence space collapse to a
/// single merge — this reproduces that exactly.
fn combine_occurrences(matched: &str, words_str: &str, merged_str: &str) -> Vec<String> {
    let mut results = Vec::new();
    let Some(first) = matched.find(words_str) else {
        return results;
    };
    let mut left = matched[..first].to_owned();
    let mut right = matched[first + words_str.len()..].to_owned();

    loop {
        results.push(format!("{left}{merged_str}{right}"));
        match right.find(words_str) {
            Some(next) => {
                let partial_left = right[..next].to_owned();
                let new_right = right[next + words_str.len()..].to_owned();
                left = format!("{left}{words_str}{partial_left}");
                right = new_right;
            }
            None => break,
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        PartialWord, combine_occurrences, compute_threshold, recognize_partial_words,
        threshold_index,
    };
    use crate::config::MAX_COMBINE;
    use crate::ngram::{NgramTables, fence};

    #[test]
    fn threshold_index_matches_python_negative_indexing() {
        // int(10*0.5)=5 → [-5] → index 5. int(1*0.5)=0 → [0].
        assert_eq!(threshold_index(10, 0.5), 5);
        assert_eq!(threshold_index(1, 0.5), 0);
        // int(10*0.6)=6 → [-6] → index 4.
        assert_eq!(threshold_index(10, 0.6), 4);
    }

    #[test]
    fn combine_single_occurrence() {
        // " a b " -> " ab " within " x a b y ".
        let merged = combine_occurrences(
            &fence(&["x", "a", "b", "y"]),
            &fence(&["a", "b"]),
            &fence(&["ab"]),
        );
        assert_eq!(merged, vec![fence(&["x", "ab", "y"])]);
    }

    #[test]
    fn combine_adjacent_pairs_collapse_shared_fence() {
        // " a b a b " has the pair twice but they share the middle fence,
        // so only the first merges (matches the Python partition walk).
        let merged = combine_occurrences(
            &fence(&["a", "b", "a", "b"]),
            &fence(&["a", "b"]),
            &fence(&["ab"]),
        );
        assert_eq!(merged, vec![fence(&["ab", "a", "b"])]);
    }

    #[test]
    fn combine_separated_pairs_each_merge() {
        // " a b c a b " -> two merges, one per occurrence.
        let merged = combine_occurrences(
            &fence(&["a", "b", "c", "a", "b"]),
            &fence(&["a", "b"]),
            &fence(&["ab"]),
        );
        assert_eq!(
            merged,
            vec![fence(&["ab", "c", "a", "b"]), fence(&["a", "b", "c", "ab"])]
        );
    }

    #[test]
    fn threshold_from_dictionary_unigrams() {
        let mut tables = NgramTables::new(MAX_COMBINE);
        // 甲×5, 乙×3, 丙×1 (丙 below WORD_MINIMUM_OCCURRENCE 3, dropped).
        for _ in 0..5 {
            tables.populate_document("1 甲\n").expect("count");
        }
        for _ in 0..3 {
            tables.populate_document("2 乙\n").expect("count");
        }
        tables.populate_document("3 丙\n").expect("count");
        let dict: BTreeSet<String> = ["甲", "乙", "丙"].iter().map(|s| (*s).to_owned()).collect();
        // freqs kept: [3 (乙), 5 (甲)] sorted; len 2, int(2*0.5)=1 → [-1] → index 1 → 5.
        assert_eq!(compute_threshold(&tables, &dict).expect("threshold"), 5);
    }

    #[test]
    fn discovers_a_partial_word_over_threshold() {
        let mut tables = NgramTables::new(MAX_COMBINE);
        // "甲 乙" appears 4 times (bigram freq 4); threshold 3.
        for _ in 0..4 {
            tables.populate_document("1 甲\n2 乙\n").expect("count");
        }
        tables.prune();
        let dict = BTreeSet::new(); // 甲乙 is not a known word.
        let partials = recognize_partial_words(&mut tables, &dict, 3);
        assert_eq!(
            partials,
            vec![PartialWord {
                merged: "甲乙".to_owned(),
                prefix: "甲".to_owned(),
                postfix: "乙".to_owned(),
                freq: 4,
            }]
        );
    }

    #[test]
    fn a_repeated_pair_in_one_row_merges_each_occurrence_once() {
        // "甲 乙 丙 甲 乙" ×12: the 5-gram holds the pair twice. Each occurrence
        // yields one merged 4-gram — never doubled by the second hit of the
        // same pair while walking the row's adjacent pairs.
        let mut tables = NgramTables::new(MAX_COMBINE);
        for _ in 0..12 {
            tables
                .populate_document("1 甲\n2 乙\n3 丙\n1 甲\n2 乙\n0 \n")
                .expect("count");
        }
        tables.prune();
        let dict = BTreeSet::new();
        let partials = recognize_partial_words(&mut tables, &dict, 20);
        assert_eq!(partials.len(), 1, "{partials:?}");
        assert_eq!(partials[0].merged, "甲乙");
        assert_eq!(tables.get(4, &fence(&["甲乙", "丙", "甲", "乙"])), Some(12));
        assert_eq!(tables.get(4, &fence(&["甲", "乙", "丙", "甲乙"])), Some(12));
    }

    #[test]
    fn known_dictionary_word_is_not_a_partial_word() {
        let mut tables = NgramTables::new(MAX_COMBINE);
        for _ in 0..4 {
            tables.populate_document("1 甲\n2 乙\n").expect("count");
        }
        tables.prune();
        let dict: BTreeSet<String> = ["甲乙"].iter().map(|s| (*s).to_owned()).collect();
        let partials = recognize_partial_words(&mut tables, &dict, 3);
        assert!(
            partials.is_empty(),
            "甲乙 is a known word, not a partial word"
        );
    }
}
