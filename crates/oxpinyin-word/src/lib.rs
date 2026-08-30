//! Training-only word-recognition pipeline.
//!
//! Native Rust reproduction of the trainer's `prepare` → `populate` →
//! `partialword` → `newword` → `markpinyin` scripts
//! (`docs/findings/trainer-parity-audit.md` §8). It reads word-history
//! n-grams from the segmented corpus, discovers partial words by frequency
//! and new words by prefix/postfix entropy, and assigns each recognized
//! word its pinyin and frequency — emitting `recognized.txt`.
//!
//! The Python original's per-order SQLite databases and FTS3 phrase index
//! become the ordered maps of [`NgramTables`]; the crate is self-contained
//! (the segmented stream carries the word text; the dictionary word list
//! and pinyin list are supplied as files) and never ships with the engine.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod config;
mod error;
mod markpinyin;
mod newword;
mod ngram;
mod partial;

use std::collections::BTreeSet;

pub use config::{
    DEFAULT_PINYIN_TOTAL, MAX_COMBINE, MAXIMUM_ITERATION, MINIMUM_ENTROPY,
    MINIMUM_PINYIN_FREQUENCY, NEW_WORD_THRESHOLD, NGRAM_MINIMUM_OCCURRENCE, PARTIAL_WORD_THRESHOLD,
    PRUNE_MINIMUM_OCCURRENCE, WORD_MINIMUM_OCCURRENCE,
};
pub use error::WordError;
pub use markpinyin::{Mark, Marker, render_recognized};
pub use newword::{BigramIndex, recognize_new_words};
pub use ngram::{NgramTables, fence};
pub use partial::{PartialWord, compute_threshold, recognize_partial_words};

/// The complete word-recognition run, returning the `recognized.txt` text.
///
/// Chains all five stages: populate the n-gram tables from the segmented
/// documents and prune; derive the partial-word threshold; discover partial
/// words (mutating the tables by the cross-order merge); filter to new
/// words by entropy; and mark pinyins from the atomic (`oldwords`) and
/// merged (partial-word) decompositions.
///
/// `dict_words` is the dictionary word list (`words.txt`, one word per
/// line); `oldwords` is the atomic pinyin list (`oldwords.txt`,
/// `word pinyin freq`).
///
/// # Errors
///
/// Returns [`WordError`] when a document, a threshold, or a pinyin mark
/// cannot be produced.
pub fn recognize(
    documents: &[String],
    dict_words: &BTreeSet<String>,
    oldwords: &str,
) -> Result<String, WordError> {
    let mut tables = NgramTables::new(MAX_COMBINE);
    for document in documents {
        tables.populate_document(document)?;
    }
    tables.prune();

    let threshold = compute_threshold(&tables, dict_words)?;
    let partials = recognize_partial_words(&mut tables, dict_words, threshold);
    let new_words = recognize_new_words(&tables, dict_words, &partials)?;

    let mut marker = Marker::new(&partials);
    marker.load_atomic(oldwords)?;
    render_recognized(&marker, &new_words)
}

/// Loads a `words.txt` word list (one word per line) into a set.
#[must_use]
pub fn parse_word_list(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}
