//! The native evaluation model: an interpolated bigram language model built
//! from a candidate `interpolation2.text` and an applied λ.
//!
//! This is the native replacement for `evaluate.py`'s `make`-rebuilt runtime
//! model — `gen_binary_files` + `import_interpolation` + `gen_unigram`, so
//! [`EvalLanguageModel::from_counts_with_lexicon`] floors every phrase-index
//! token at count + 1 and sums the total over the whole lexicon, exactly as
//! the pin's `get_phrase_index_total_freq` sees it after that `make`. The
//! evaluator's decode (`decode.rs`) reads the float probabilities
//! ([`EvalLanguageModel::unigram_poss`], [`EvalLanguageModel::bigram_poss`])
//! the pin's `PhoneticLookup` scores with. The counts come from
//! `interpolation2.text` (`\1-gram` unigrams, `\2-gram` pair counts, bigram
//! total per prev = Σ its successor counts, as `import_interpolation`
//! stores them).

use std::collections::BTreeMap;

use crate::eval::counts::Counts;
use oxpinyin_core::PhraseToken;
use oxpinyin_data::Lambda;

/// An interpolated bigram model built from `interpolation2.text` counts or
/// from the binary tables a data directory holds.
#[derive(Clone, Debug)]
pub struct EvalLanguageModel {
    unigrams: BTreeMap<u32, u64>,
    unigram_total: u64,
    bigrams: BTreeMap<(u32, u32), u64>,
    bigram_totals: BTreeMap<u32, u64>,
    lambda_f32: f32,
}

impl EvalLanguageModel {
    /// Builds the model from the `interpolation2.text` counts and the applied
    /// λ, taking the unigram table as the counts give it (no floor). The
    /// bigram total per prev is Σ of its successor counts, exactly as the
    /// compiled bigram table stores it.
    #[must_use]
    pub fn from_counts(counts: &Counts, lambda: Lambda) -> Self {
        let unigram_total = counts.unigrams.values().sum();
        Self {
            unigrams: counts.unigrams.clone(),
            unigram_total,
            bigrams: counts.bigrams.clone(),
            bigram_totals: bigram_totals(counts),
            lambda_f32: lambda.as_f64() as f32,
        }
    }

    /// Builds the model the pin's `make` builds: every token of the phrase
    /// index (`lexicon`) carries its `interpolation2.text` count plus the
    /// `gen_unigram` floor of 1, and the unigram total is the sum over that
    /// whole lexicon (`get_phrase_index_total_freq`). A count for a token
    /// outside the lexicon is dropped, as `import_interpolation` cannot
    /// store it.
    #[must_use]
    pub fn from_counts_with_lexicon(
        counts: &Counts,
        lambda: Lambda,
        lexicon: impl IntoIterator<Item = PhraseToken>,
    ) -> Self {
        let mut unigrams = BTreeMap::new();
        let mut unigram_total: u64 = 0;
        for token in lexicon {
            let token = token.value();
            let freq = counts
                .unigrams
                .get(&token)
                .copied()
                .unwrap_or(0)
                .saturating_add(1);
            unigrams.insert(token, freq);
            unigram_total = unigram_total.saturating_add(freq);
        }
        Self {
            unigrams,
            unigram_total,
            bigrams: counts.bigrams.clone(),
            bigram_totals: bigram_totals(counts),
            lambda_f32: lambda.as_f64() as f32,
        }
    }

    /// Builds the model from a data directory's own tables: the phrase
    /// index's stored unigram counts and facade total
    /// (`PhraseItem::get_unigram_frequency`,
    /// `FacadePhraseIndex::get_phrase_index_total_freq`) and the
    /// `bigram.db` rows' stored frequencies and totals
    /// (`SingleGram::get_freq`, `SingleGram::get_total_freq`).
    ///
    /// `lambda` is the `gfloat` the pin's `PhoneticLookup` is constructed
    /// with (`eval_correction_rate.cpp:159-163`); `bigram_totals` keeps
    /// every present row, including a zero total, because
    /// `merge_single_gram` succeeds for any stored row.
    #[must_use]
    pub fn from_binary(
        unigrams: BTreeMap<u32, u64>,
        unigram_total: u64,
        bigrams: BTreeMap<(u32, u32), u64>,
        bigram_totals: BTreeMap<u32, u64>,
        lambda: f32,
    ) -> Self {
        Self {
            unigrams,
            unigram_total,
            bigrams,
            bigram_totals,
            lambda_f32: lambda,
        }
    }

    /// The `table.conf` λ as the pin's `gfloat`.
    #[must_use]
    pub fn lambda_f32(&self) -> f32 {
        self.lambda_f32
    }

    /// `P_uni(token)`: `get_unigram_frequency() / (gdouble)
    /// get_phrase_index_total_freq()`, the C division — an empty table
    /// gives the host's NaN, as the pin's `0/0` does.
    #[must_use]
    pub fn unigram_poss(&self, token: u32) -> f64 {
        let freq = self.unigrams.get(&token).copied().unwrap_or(0);
        freq as f64 / self.unigram_total as f64
    }

    /// Whether `prev` has a bigram row (`Bigram::load` succeeds, so
    /// `merge_single_gram` does too).
    #[must_use]
    pub fn has_bigram_row(&self, prev: u32) -> bool {
        self.bigram_totals.contains_key(&prev)
    }

    /// `P_bi(token | prev)`: `freq / (gfloat) total_freq` of `prev`'s row,
    /// or `None` when the row lacks `token` (`get_freq` miss). The stored
    /// total is used as stored: a zero total is the C `freq/0` result.
    #[must_use]
    pub fn bigram_poss(&self, prev: u32, token: u32) -> Option<f32> {
        let total = self.bigram_totals.get(&prev).copied()?;
        let freq = self.bigrams.get(&(prev, token)).copied()?;
        Some(freq as f32 / total as f32)
    }
}

/// The bigram total per prev: Σ of its successor counts.
fn bigram_totals(counts: &Counts) -> BTreeMap<u32, u64> {
    let mut totals: BTreeMap<u32, u64> = BTreeMap::new();
    for (&(prev, _next), &count) in &counts.bigrams {
        *totals.entry(prev).or_default() += count;
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::EvalLanguageModel;
    use crate::eval::counts::Counts;
    use oxpinyin_data::Lambda;

    fn model() -> EvalLanguageModel {
        let mut counts = Counts::default();
        counts.unigrams.insert(10, 6);
        counts.unigrams.insert(20, 2);
        // <start>(1) → 10 twice; 10 → 20 four times; 10 → 10 once.
        counts.bigrams.insert((1, 10), 2);
        counts.bigrams.insert((10, 20), 4);
        counts.bigrams.insert((10, 10), 1);
        EvalLanguageModel::from_counts(&counts, Lambda::PINNED)
    }

    #[test]
    fn the_float_probabilities_are_the_stored_ratios() {
        let model = model();
        assert_eq!(model.unigram_poss(10).to_bits(), 0.75_f64.to_bits());
        assert_eq!(model.bigram_poss(10, 20), Some(0.8));
        assert_eq!(model.bigram_poss(20, 10), None);
        assert!(model.has_bigram_row(10));
        assert!(!model.has_bigram_row(20));
    }
}
