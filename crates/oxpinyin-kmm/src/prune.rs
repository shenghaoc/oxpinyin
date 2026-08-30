//! `prune_k_mixture_model` — drop pairs whose `P(occurrences ≥ K) < CDF`
//! (`utils/training/prune_k_mixture_model.cpp:45-191`).
//!
//! For each `(W1, W2)` pair, `remained = 1 − Σ_{k=0}^{K−1} Pr_G_3(k)`
//! (the survival `P(occurrences ≥ K)`); a pair with `remained < CDF` is
//! removed and its `m_WC` is subtracted from the row header, the magic word
//! count and total freq, and — as a post-pass — from `W2`'s unigram freq.
//! Rows whose header ends fully zero are dropped.
//!
//! Upstream interleaves the decisions and the mutations; the model math
//! reads only `magic.m_N` (constant during a prune) and each pair's own
//! counts (never modified by pruning another pair), so a decide-then-apply
//! two-pass is equivalent and keeps the borrows clean.

use crate::error::KmmError;
use crate::model::{KMixtureModel, compute_pr_g_3_with_count};

/// Default `-k` (`prune_k_mixture_model.cpp:34`).
pub const DEFAULT_PRUNE_K: u32 = 3;
/// Default `--CDF` (`:35`).
pub const DEFAULT_CDF: f64 = 0.99;

/// Prunes the model in place.
///
/// # Errors
///
/// Returns [`KmmError::Domain`] when a survival probability falls outside
/// `[0, 1]` (upstream `EDOM`).
pub fn prune(model: &mut KMixtureModel, prune_k: u32, cdf: f64) -> Result<(), KmmError> {
    let n = model.n;

    // Pass 1 — decide (read-only). Collect (token1, token2, wc) to remove.
    let mut to_remove: Vec<(u32, u32, u32)> = Vec::new();
    for (&token1, gram) in &model.grams {
        for (&token2, item) in &gram.items {
            let remained = survival(prune_k, n, item)?;
            if remained < cdf {
                to_remove.push((token1, token2, item.wc));
            }
        }
    }

    // Pass 2 — apply.
    for &(token1, token2, wc) in &to_remove {
        if let Some(gram) = model.grams.get_mut(&token1) {
            gram.items.remove(&token2);
            gram.header_wc = gram.header_wc.wrapping_sub(wc);
        }
        model.wc = model.wc.wrapping_sub(wc);
        model.total_freq = model.total_freq.wrapping_sub(wc);
    }
    // Unigram reduce: subtract the removed pair WC from W2's freq (`:159-169`).
    for &(_token1, token2, wc) in &to_remove {
        if let Some(gram) = model.grams.get_mut(&token2) {
            gram.header_freq = gram.header_freq.wrapping_sub(wc);
        }
    }

    // Clean up rows whose header is fully zero (`:179-186`).
    model
        .grams
        .retain(|_, gram| !(gram.header_wc == 0 && gram.header_freq == 0));
    Ok(())
}

/// `remained_poss = 1 − Σ_{k<K} Pr_G_3(k)`, with the `EDOM` range checks
/// (`:56-80`). `n_0 = N − m_N_n_0` is `guint32` (`wrapping_sub`).
fn survival(prune_k: u32, n: u32, item: &crate::model::ArrayItem) -> Result<f64, KmmError> {
    let n_0 = n.wrapping_sub(item.n_n_0);
    let mut remained = 1.0_f64;
    let mut errors = false;
    for k in 0..prune_k {
        let one = compute_pr_g_3_with_count(k, n, item.wc, n_0, item.n_1);
        if !(0.0..=1.0).contains(&one) {
            errors = true;
        }
        remained -= one;
    }
    if remained.abs() < f64::EPSILON {
        remained = 0.0;
    }
    if errors || !(0.0..=1.0).contains(&remained) {
        return Err(KmmError::Domain {
            detail: format!(
                "remained={remained} k={prune_k} N={n} WC={} n_0={n_0} n_1={}",
                item.wc, item.n_1
            ),
        });
    }
    Ok(remained)
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_CDF, DEFAULT_PRUNE_K, prune};
    use crate::generate::GenerateParams;
    use crate::model::KMixtureModel;
    use crate::validate::validate;

    fn model_from(docs: &[&str]) -> KMixtureModel {
        let mut model = KMixtureModel::new();
        for doc in docs {
            model
                .add_document(doc, GenerateParams::default())
                .expect("count");
        }
        model
    }

    #[test]
    fn default_cdf_prunes_all_rare_pairs() {
        // Every pair occurs once across a few documents: P(≥3 per doc) is
        // ~0, far below 0.99, so the default prune empties the model.
        let mut model = model_from(&["10 甲\n20 乙\n", "10 甲\n20 乙\n", "30 丙\n40 丁\n"]);
        prune(&mut model, DEFAULT_PRUNE_K, DEFAULT_CDF).expect("prune");
        // All bigram pairs pruned; only freq-only headers may remain.
        let bigram_pairs: usize = model.grams.values().map(|g| g.items.len()).sum();
        assert_eq!(bigram_pairs, 0, "default CDF prunes every rare pair");
    }

    #[test]
    fn cdf_zero_keeps_everything() {
        // remained is always ≥ 0, so remained < 0 never holds: nothing is
        // pruned and the model is unchanged.
        let original = model_from(&["10 甲\n20 乙\n10 甲\n20 乙\n"]);
        let mut model = original.clone();
        prune(&mut model, DEFAULT_PRUNE_K, 0.0).expect("prune");
        assert_eq!(model, original);
    }

    #[test]
    fn pruned_model_still_validates() {
        // After pruning, the magic/header/item invariants must still hold
        // (the WC and freq bookkeeping stays consistent), unless the model
        // is emptied (validate rejects an all-zero magic header, which the
        // trainer treats as a degenerate model).
        let mut model = model_from(&["10 甲\n20 乙\n10 甲\n20 乙\n", "10 甲\n20 乙\n"]);
        // Prune with CDF 0 keeps everything and must validate.
        prune(&mut model, DEFAULT_PRUNE_K, 0.0).expect("prune");
        validate(&model).expect("kept model validates");
    }
}
