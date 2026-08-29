//! Scoring-model data the segmenter reads through the existing loaders.
//!
//! Bigrams come from [`oxpinyin_data::BigramLanguageModel`] (the
//! `bigram` table in the compiled-in backend's format, the verbatim
//! export of the pin's `SYSTEM_BIGRAM`). Unigrams come from
//! [`oxpinyin_data::parse_interpolation2`] plus the `gen_unigram` freq-1 floor
//! that the pin applies when it builds `phrase_index.bin`
//! (`utils/training/gen_unigram.cpp:45-68`, run from `data/Makefile.am:62`).

use std::collections::HashMap;
use std::path::Path;

use oxpinyin_data::{BigramLanguageModel, BigramRow, parse_interpolation2};

use crate::error::SegmentError;
use crate::lexicon::PhraseLexicon;

/// λ written in the pin's `table.conf` (`lambda parameter:0.312699`).
///
/// `SystemTableInfo2::load` scans that line with `fscanf(..., "%f")`
/// (`src/storage/table_info.cpp:220`). The nearest `f32` to the decimal is
/// the same value `ngseg` then passes to `PhraseLookup`.
pub const PINNED_LAMBDA: f32 = 0.312699;

/// `null_token` (`novel_types.h:121`).
pub const NULL_TOKEN: u32 = 0;

/// `sentence_start` (`novel_types.h:122`). The trellis's initial node.
pub const SENTENCE_START: u32 = 1;

/// Bigram + unigram counts used by `get_best_match`.
pub struct SegmentModel {
    unigram: HashMap<u32, u64>,
    unigram_total: u64,
    bigram: BigramStore,
}

enum BigramStore {
    Table(BigramLanguageModel),
    Memory(HashMap<u32, (u32, HashMap<u32, u32>)>),
}

impl SegmentModel {
    /// Loads the pinned bigram and interpolation2 unigrams, then applies
    /// the freq-1 floor over every token in `lexicon`.
    ///
    /// # Errors
    ///
    /// Returns [`SegmentError`] when a file cannot be read or parsed.
    pub fn open(
        bigram_path: &Path,
        interpolation2_path: &Path,
        lexicon: &PhraseLexicon,
    ) -> Result<Self, SegmentError> {
        let bigram = BigramLanguageModel::open(bigram_path)?;
        let table = parse_interpolation2(interpolation2_path)?;
        Ok(Self::with_counts(
            BigramStore::Table(bigram),
            |token| table.count(token),
            lexicon,
        ))
    }

    /// In-memory model for tests. `unigrams` is the raw interpolation2 map;
    /// the freq-1 floor is applied over `lexicon`.
    #[must_use]
    pub fn memory(
        unigrams: HashMap<u32, u64>,
        bigram: HashMap<u32, (u32, HashMap<u32, u32>)>,
        lexicon: &PhraseLexicon,
    ) -> Self {
        Self::with_counts(
            BigramStore::Memory(bigram),
            |token| unigrams.get(&token).copied(),
            lexicon,
        )
    }

    fn with_counts(
        bigram: BigramStore,
        count: impl Fn(u32) -> Option<u64>,
        lexicon: &PhraseLexicon,
    ) -> Self {
        let mut unigram = HashMap::new();
        let mut unigram_total = 0_u64;
        for token in lexicon.tokens() {
            // Loaded phrase-index item frequency after import_interpolation
            // + gen_unigram: missing interpolation2 rows start at 0 and still
            // receive the floor, matching add_unigram_frequency on a real item.
            let freq = count(token).unwrap_or(0).saturating_add(1);
            unigram.insert(token, freq);
            unigram_total = unigram_total.saturating_add(freq);
        }
        Self {
            unigram,
            unigram_total,
            bigram,
        }
    }

    /// Unigram count of `token` after the freq-1 floor, or `0` if the
    /// token is not in the loaded phrase index.
    #[must_use]
    pub fn unigram(&self, token: u32) -> u64 {
        self.unigram.get(&token).copied().unwrap_or(0)
    }

    /// Denominator of `P_unigram`: `get_phrase_index_total_freq()` over the
    /// loaded system libraries.
    #[must_use]
    pub const fn unigram_total(&self) -> u64 {
        self.unigram_total
    }

    /// System-bigram row for `prev`. `None` is a `load` miss.
    ///
    /// # Errors
    ///
    /// Returns [`SegmentError`] when the store backend fails.
    pub fn successors(&self, prev: u32) -> Result<Option<BigramRow>, SegmentError> {
        match &self.bigram {
            BigramStore::Table(table) => Ok(table.load_successors(prev)?),
            BigramStore::Memory(map) => Ok(map.get(&prev).map(|(total, records)| BigramRow {
                total: *total,
                records: records
                    .iter()
                    .map(|(&next, &count)| (next, count))
                    .collect(),
            })),
        }
    }
}

/// Reads `lambda parameter:` from a `table.conf` body.
///
/// Matches `fscanf(input, "lambda parameter:%f\n", &lambda)` on the pin
/// (`src/storage/table_info.cpp:220`).
#[must_use]
pub fn parse_table_conf_lambda(text: &str) -> Option<f32> {
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("lambda parameter:") else {
            continue;
        };
        return rest.trim().parse().ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{PINNED_LAMBDA, parse_table_conf_lambda};
    use crate::lexicon::PhraseLexicon;
    use crate::model::SegmentModel;
    use std::collections::HashMap;

    #[test]
    fn the_pinned_lambda_parses_the_table_conf_decimal() {
        assert_eq!(
            parse_table_conf_lambda("lambda parameter:0.312699\n"),
            Some(PINNED_LAMBDA)
        );
        assert_eq!(parse_table_conf_lambda("binary format version:7\n"), None);
    }

    #[test]
    fn the_freq_one_floor_covers_every_lexicon_token() {
        let lexicon = PhraseLexicon::from_pairs(vec![(10, "中".to_owned()), (11, "国".to_owned())]);
        let mut unigrams = HashMap::new();
        unigrams.insert(10, 5);
        let model = SegmentModel::memory(unigrams, HashMap::new(), &lexicon);
        assert_eq!(model.unigram(10), 6);
        assert_eq!(model.unigram(11), 1);
        assert_eq!(model.unigram(99), 0);
        assert_eq!(model.unigram_total(), 7);
    }
}
