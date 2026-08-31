//! The native evaluation model: an interpolated bigram language model built
//! from a candidate `interpolation2.text` and an applied λ.
//!
//! This is the native replacement for `evaluate.py`'s `make`-rebuilt runtime
//! model. Its cost function is a line-for-line mirror of the shipping
//! decoder's LM (`oxpinyin_data::lm::BigramLanguageModel::model_cost`,
//! `crates/oxpinyin-data/src/lm/mod.rs:387-440`) — same interpolation
//! (`interpolate_ratio`, `λ·b/bt + (1−λ)·u/ut` over a common denominator),
//! same `UNIGRAM_TIEBREAK_SCALE`, same [`surprisal`]/[`reduce_ratio`], same
//! `UNKNOWN_COST` floors — so a decode against it ranks exactly as the real
//! engine would against the same counts and λ. The counts come from
//! `interpolation2.text` (`\1-gram` unigrams, `\2-gram` pair counts, bigram
//! total per prev = Σ its successor counts, as `import_interpolation` /
//! `oxpinyin-datagen` store them); λ is applied as the exact
//! [`oxpinyin_data::Lambda`] rational, matching `make modify
//! LAMBDA_PARAMETER=λ` writing `{:.6}` into `table.conf` and the decoder
//! reading it back.

use std::collections::BTreeMap;

use oxpinyin_core::cost::{UNKNOWN_COST, reduce_ratio, surprisal};
use oxpinyin_core::{Cost, LanguageModel, PhraseToken};
use oxpinyin_counter::Counts;
use oxpinyin_data::Lambda;

/// The empty-history unigram tie-break divisor (`lm/mod.rs:79`,
/// `UNIGRAM_TIEBREAK_SCALE`).
const UNIGRAM_TIEBREAK_SCALE: Cost = 16;

/// An interpolated bigram model built from `interpolation2.text` counts.
#[derive(Clone, Debug)]
pub struct EvalLanguageModel {
    unigrams: BTreeMap<u32, u64>,
    unigram_total: u64,
    bigrams: BTreeMap<(u32, u32), u64>,
    bigram_totals: BTreeMap<u32, u64>,
    lambda: Lambda,
}

impl EvalLanguageModel {
    /// Builds the model from the `interpolation2.text` counts and the applied
    /// λ. The bigram total per prev is Σ of its successor counts, exactly as
    /// the compiled bigram table stores it.
    #[must_use]
    pub fn from_counts(counts: &Counts, lambda: Lambda) -> Self {
        let unigram_total = counts.unigrams.values().sum();
        let mut bigram_totals: BTreeMap<u32, u64> = BTreeMap::new();
        for (&(prev, _next), &count) in &counts.bigrams {
            *bigram_totals.entry(prev).or_default() += count;
        }
        Self {
            unigrams: counts.unigrams.clone(),
            unigram_total,
            bigrams: counts.bigrams.clone(),
            bigram_totals,
            lambda,
        }
    }

    /// The λ in force.
    #[must_use]
    pub const fn lambda(&self) -> Lambda {
        self.lambda
    }

    /// Whether the model carries a unigram frequency table (`has_unigrams`).
    #[must_use]
    pub fn has_unigrams(&self) -> bool {
        !self.unigrams.is_empty() && self.unigram_total != 0
    }

    /// The `prev → next` transition `(count, total)`, or `None` when the pair
    /// is absent (`transition`/`merged_transition`, `lm/mod.rs`).
    fn transition(&self, prev: u32, next: u32) -> Option<(u64, u64)> {
        let count = self.bigrams.get(&(prev, next)).copied()?;
        let total = self.bigram_totals.get(&prev).copied()?;
        (total != 0).then_some((count, total))
    }

    /// Interpolated model cost of `token` after `history`, without the edge
    /// cost (`model_cost`, `lm/mod.rs:387-440`).
    #[must_use]
    pub fn model_cost(&self, history: &[PhraseToken], token: &PhraseToken) -> Cost {
        if !self.has_unigrams() {
            // pure_bigram_cost (`lm/mod.rs:444-457`).
            let Some(prev) = history.last() else {
                return 0;
            };
            return match self.transition(prev.value(), token.value()) {
                Some((count, total)) => surprisal(count, total),
                None => UNKNOWN_COST,
            };
        }

        let unigram = self.unigrams.get(&token.value()).copied().unwrap_or(0);
        if unigram == 0 || self.unigram_total == 0 {
            return UNKNOWN_COST;
        }
        let unigram_cost = surprisal(unigram, self.unigram_total);

        let Some(prev) = history.last() else {
            return unigram_cost / UNIGRAM_TIEBREAK_SCALE;
        };

        match self.transition(prev.value(), token.value()) {
            Some((bigram_count, bigram_total)) => {
                match interpolate_ratio(
                    self.lambda.numerator(),
                    self.lambda.denominator(),
                    u128::from(bigram_count),
                    u128::from(bigram_total),
                    u128::from(unigram),
                    u128::from(self.unigram_total),
                ) {
                    Some((numerator, denominator)) => {
                        let (numerator, denominator) = reduce_ratio(numerator, denominator);
                        surprisal(numerator, denominator)
                    }
                    None => UNKNOWN_COST,
                }
            }
            None => UNKNOWN_COST,
        }
    }
}

impl LanguageModel for EvalLanguageModel {
    type Token = PhraseToken;
    type Error = std::convert::Infallible;

    fn score(
        &self,
        history: &[PhraseToken],
        token: &PhraseToken,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        Ok(edge_cost.saturating_add(self.model_cost(history, token)))
    }
}

/// `λ·b/bt + (1 − λ)·u/ut` over a common denominator, `λ = num/den`
/// (`interpolate_ratio`, `oxpinyin-data/src/lm/mod.rs:88-108`). `None` on
/// overflow (the caller floors at `UNKNOWN_COST`).
fn interpolate_ratio(
    lambda_num: u128,
    lambda_den: u128,
    bigram_count: u128,
    bigram_total: u128,
    unigram: u128,
    unigram_total: u128,
) -> Option<(u128, u128)> {
    let one_minus_lambda = lambda_den.saturating_sub(lambda_num);
    let bigram_term = lambda_num
        .checked_mul(bigram_count)?
        .checked_mul(unigram_total)?;
    let unigram_term = one_minus_lambda
        .checked_mul(unigram)?
        .checked_mul(bigram_total)?;
    let numerator = bigram_term.checked_add(unigram_term)?;
    let denominator = lambda_den
        .checked_mul(bigram_total)?
        .checked_mul(unigram_total)?;
    Some((numerator, denominator))
}

#[cfg(test)]
mod tests {
    use super::EvalLanguageModel;
    use oxpinyin_core::{LanguageModel, PhraseToken};
    use oxpinyin_counter::Counts;
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
    fn a_known_transition_is_far_below_unknown() {
        let model = model();
        let ten = PhraseToken::new(10);
        let twenty = PhraseToken::new(20);
        // 10 → 20 is a frequent, observed transition: its interpolated cost
        // is a small surprisal, far below the UNKNOWN_COST floor that an
        // unobserved transition (below) gets.
        let observed = model.model_cost(&[ten], &twenty);
        assert!(
            observed < oxpinyin_core::cost::UNKNOWN_COST,
            "observed transition {observed} must be below UNKNOWN"
        );
    }

    #[test]
    fn an_absent_pair_or_zero_unigram_floors_at_unknown() {
        let model = model();
        // 20 → 10 is absent → UNKNOWN.
        assert_eq!(
            model.model_cost(&[PhraseToken::new(20)], &PhraseToken::new(10)),
            oxpinyin_core::cost::UNKNOWN_COST
        );
        // token 999 has no unigram → UNKNOWN.
        assert_eq!(
            model.model_cost(&[PhraseToken::new(10)], &PhraseToken::new(999)),
            oxpinyin_core::cost::UNKNOWN_COST
        );
    }

    #[test]
    fn score_adds_the_edge_cost() {
        let model = model();
        let base = model
            .score(&[PhraseToken::new(1)], &PhraseToken::new(10), 0)
            .expect("infallible");
        assert_eq!(
            model
                .score(&[PhraseToken::new(1)], &PhraseToken::new(10), 250)
                .expect("infallible"),
            base + 250
        );
    }

    #[test]
    fn lambda_extremes_shift_the_blend() {
        // λ=0 is pure unigram; λ=1 is pure bigram. The blended cost of a
        // frequent transition should differ between them.
        let mut counts = Counts::default();
        counts.unigrams.insert(10, 6);
        counts.unigrams.insert(20, 2);
        counts.bigrams.insert((10, 20), 4);
        let all_bigram = EvalLanguageModel::from_counts(&counts, lambda("1.000000"));
        let all_unigram = EvalLanguageModel::from_counts(&counts, lambda("0.000000"));
        let history = [PhraseToken::new(10)];
        let twenty = PhraseToken::new(20);
        assert_ne!(
            all_bigram.model_cost(&history, &twenty),
            all_unigram.model_cost(&history, &twenty)
        );
    }

    fn lambda(value: &str) -> Lambda {
        oxpinyin_data::parse_table_conf_lambda(&format!("lambda parameter:{value}\n"))
            .expect("valid lambda")
    }
}
