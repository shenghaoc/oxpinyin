//! The interpolated bigram model for the evaluator's binary tables.
//!
//! The decoder reads the phrase index's unigram counts and the stored
//! bigram counts and row totals. It scores with the float probabilities
//! used by the pin's `PhoneticLookup`.

use oxpinyin::__internal::data::PhraseLibraries;

use crate::storage::Hash;
use crate::training::{Gram, load_gram};

/// The evaluator's tables, borrowed without walking their contents.
pub struct EvalLanguageModel<'a> {
    libraries: &'a PhraseLibraries,
    bigram: Option<&'a Hash>,
    lambda_f32: f32,
}

impl<'a> EvalLanguageModel<'a> {
    /// Borrows the tables, as `PhoneticLookup` stores their pointers
    /// (`phonetic_lookup.h:707-728` at 074a2219).
    pub fn new(libraries: &'a PhraseLibraries, bigram: Option<&'a Hash>, lambda: f32) -> Self {
        Self {
            libraries,
            bigram,
            lambda_f32: lambda,
        }
    }

    /// The `table.conf` λ as the pin's `gfloat`.
    #[must_use]
    pub fn lambda_f32(&self) -> f32 {
        self.lambda_f32
    }

    /// Reads the requested item's stored unigram and the facade's stored
    /// total (`phonetic_lookup.h:646-651,674-679`). Zero over zero is NaN.
    #[must_use]
    pub fn unigram_poss(&self, token: u32) -> f64 {
        let freq = self
            .libraries
            .library(token)
            .and_then(|library| library.item(token))
            .map_or(0, |item| item.unigram());
        f64::from(freq) / f64::from(self.libraries.unigram_total())
    }

    /// Loads one row for one beam node and span. The pin reloads and deletes
    /// it on each expansion; its scratch merged row is not a row cache
    /// (`phonetic_lookup.h:588-636`, `ngram.cpp:277-299`).
    pub fn bigram_row(&self, prev: u32) -> Option<Gram> {
        load_gram(self.bigram, prev)
    }
}

#[cfg(test)]
mod tests;
