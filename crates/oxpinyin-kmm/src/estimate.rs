//! `estimate_k_mixture_model` — the deleted-interpolation EM that scores a
//! candidate against a held-out model
//! (`utils/training/estimate_k_mixture_model.cpp:36-155`).
//!
//! For each `W1` present in the deleted model with non-zero header WC, run
//! the per-context EM (`compute_interpolation`, seed λ = 0.6, ε = 0.001):
//! the bigram term is `item.m_WC / header.m_WC` of the candidate, the
//! unigram term is `header.m_freq / magic.m_total_freq`, and the score is
//! the arithmetic mean of the per-context λ. `estimate.py` reads the
//! `average lambda:` line and sorts candidates by it, descending.

use crate::error::KmmError;
use crate::model::{KMixtureModel, Parameter, SingleGram};

/// Seed `next_lambda` (`estimate_k_mixture_model.cpp:40`).
pub const SEED_LAMBDA: Parameter = 0.6;
/// EM convergence threshold (`:41`).
pub const EPSILON: Parameter = 0.001;

/// Safety cap on EM iterations. The EM is a contraction and converges in a
/// few dozen steps for real data; the cap only prevents a pathological
/// hang and is never reached on well-formed models.
const MAX_ITERATIONS: usize = 100_000;

/// The estimation result: the score and the per-context λ values.
#[derive(Clone, Debug, PartialEq)]
pub struct Estimate {
    /// `average lambda` — the candidate's score.
    pub average: Parameter,
    /// `(W1, λ)` for each scored context, in token order.
    pub per_token: Vec<(u32, Parameter)>,
}

/// Scores `candidate` against the held-out `deleted` model.
///
/// # Errors
///
/// Returns [`KmmError::Invalid`] when the candidate's magic total freq is
/// zero (upstream `assert`), or when no context could be scored (the
/// `average lambda` would be `NaN`).
pub fn estimate(candidate: &KMixtureModel, deleted: &KMixtureModel) -> Result<Estimate, KmmError> {
    if candidate.total_freq == 0 {
        return Err(KmmError::Invalid {
            detail: "candidate magic total freq is zero".to_owned(),
        });
    }

    let mut lambda_sum: Parameter = 0.0;
    let mut per_token = Vec::new();
    for (&token1, deleted_gram) in &deleted.grams {
        if deleted_gram.header_wc == 0 {
            continue;
        }
        let candidate_gram = candidate.grams.get(&token1);
        let lambda = compute_interpolation(deleted_gram, candidate, candidate_gram);
        per_token.push((token1, lambda));
        lambda_sum += lambda;
    }

    if per_token.is_empty() {
        return Err(KmmError::Invalid {
            detail: "no scorable context in the deleted model".to_owned(),
        });
    }
    let average = lambda_sum / per_token.len() as Parameter;
    Ok(Estimate { average, per_token })
}

/// Per-context deleted-interpolation EM (`compute_interpolation`, `:36-96`).
fn compute_interpolation(
    deleted_gram: &SingleGram,
    candidate: &KMixtureModel,
    candidate_gram: Option<&SingleGram>,
) -> Parameter {
    let total_freq = Parameter::from(candidate.total_freq);
    let mut lambda: Parameter = 0.0;
    let mut next_lambda: Parameter = SEED_LAMBDA;

    let mut iterations = 0;
    while (lambda - next_lambda).abs() > EPSILON && iterations < MAX_ITERATIONS {
        iterations += 1;
        lambda = next_lambda;
        next_lambda = 0.0;

        for (&token2, item) in &deleted_gram.items {
            let deleted_count = Parameter::from(item.wc);

            // Bigram term: candidate item WC / candidate row header WC.
            let mut elem_poss = 0.0;
            if let Some(gram) = candidate_gram
                && let Some(candidate_item) = gram.items.get(&token2)
                && gram.header_wc != 0
            {
                elem_poss = Parameter::from(candidate_item.wc) / Parameter::from(gram.header_wc);
            }
            let numerator = lambda * elem_poss;

            // Unigram term: candidate row freq / candidate total freq.
            let mut unigram_poss = 0.0;
            if let Some(gram) = candidate.grams.get(&token2) {
                unigram_poss = Parameter::from(gram.header_freq) / total_freq;
            }
            let part_of_denominator = (1.0 - lambda) * unigram_poss;

            let denominator = numerator + part_of_denominator;
            if denominator == 0.0 {
                continue;
            }
            next_lambda += deleted_count * (numerator / denominator);
        }

        // header.m_WC != 0 is guaranteed by the caller's guard.
        next_lambda /= Parameter::from(deleted_gram.header_wc);
    }

    next_lambda
}

#[cfg(test)]
mod tests {
    use super::{SEED_LAMBDA, estimate};
    use crate::generate::GenerateParams;
    use crate::model::KMixtureModel;

    fn model_from(doc: &str) -> KMixtureModel {
        let mut model = KMixtureModel::new();
        model
            .add_document(doc, GenerateParams::default())
            .expect("count");
        model
    }

    #[test]
    fn average_lambda_is_in_the_unit_interval() {
        let candidate = model_from("10 甲\n20 乙\n10 甲\n20 乙\n");
        let deleted = model_from("10 甲\n20 乙\n");
        let result = estimate(&candidate, &deleted).expect("estimate");
        assert!(
            (0.0..=1.0).contains(&result.average),
            "average lambda {} out of range",
            result.average
        );
        assert!(!result.per_token.is_empty());
    }

    #[test]
    fn deterministic_across_runs() {
        let candidate = model_from("10 甲\n20 乙\n30 丙\n10 甲\n20 乙\n");
        let deleted = model_from("10 甲\n20 乙\n30 丙\n");
        let a = estimate(&candidate, &deleted).expect("a");
        let b = estimate(&candidate, &deleted).expect("b");
        assert_eq!(a, b, "the EM must be deterministic");
    }

    #[test]
    fn a_candidate_that_predicts_the_context_scores_high_bigram_weight() {
        // The candidate and the held-out share the bigram, so the bigram
        // term dominates and λ climbs above the seed toward 1.
        let candidate = model_from("10 甲\n20 乙\n10 甲\n20 乙\n10 甲\n20 乙\n");
        let deleted = model_from("10 甲\n20 乙\n");
        let result = estimate(&candidate, &deleted).expect("estimate");
        assert!(
            result.average > SEED_LAMBDA,
            "shared bigram should push λ above the seed, got {}",
            result.average
        );
    }

    #[test]
    fn zero_total_freq_is_an_error() {
        let candidate = KMixtureModel::new();
        let deleted = model_from("10 甲\n20 乙\n");
        assert!(estimate(&candidate, &deleted).is_err());
    }
}
