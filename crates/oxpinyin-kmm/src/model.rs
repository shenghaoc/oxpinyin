//! K-mixture-model data model and the three-parameter math.
//!
//! Reproduces `utils/training/k_mixture_model.h`. Counts are `u32`
//! (`corpus_count_t`/`guint32`); the model math is `f64`
//! (`parameter_t = double`). Upstream stores the model in a
//! `FlexibleBigram` DBM keyed by token; the Rust representation is ordered
//! maps keyed by token, which makes every walk (export, merge, prune,
//! validate) token-ascending and deterministic — matching upstream's
//! `get_all_items` / `retrieve_all` order without depending on hash
//! iteration.

use std::collections::BTreeMap;

/// `null_token` (`novel_types.h:121`).
pub const NULL_TOKEN: u32 = 0;
/// `sentence_start` (`novel_types.h:122`).
pub const SENTENCE_START: u32 = 1;
/// Phrase text `taglib_token_to_string` prints for `sentence_start`
/// (`src/storage/tag_utility.cpp`).
pub const SENTENCE_START_TEXT: &str = "<start>";

/// Per-`(W1, W2)` record (`KMixtureModelArrayItem`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ArrayItem {
    /// `m_WC`: total instances of the `(W1, W2)` pair (`m_T` ≡ `m_WC`).
    pub wc: u32,
    /// `m_N_n_0`: number of documents containing the pair
    /// (so `n_0 = m_N − m_N_n_0`).
    pub n_n_0: u32,
    /// `m_n_1`: documents with exactly one occurrence of the pair.
    pub n_1: u32,
    /// `m_Mr`: max instances of the pair in any single seen document.
    pub mr: u32,
}

/// One `W1` row: its array header plus its `token2 → item` map
/// (`KMixtureModelSingleGram`). The map is ordered, so `items()` is
/// token2-ascending, matching `retrieve_all`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SingleGram {
    /// `KMixtureModelArrayHeader::m_WC`: Σ instances of `W1`.
    pub header_wc: u32,
    /// `KMixtureModelArrayHeader::m_freq`: unigram frequency of `W1`.
    pub header_freq: u32,
    /// `token2 → item`, token2-ascending.
    pub items: BTreeMap<u32, ArrayItem>,
}

impl SingleGram {
    /// Σ of the array items' `m_WC` — what `validate_bigram` checks against
    /// `header_wc`.
    #[must_use]
    pub fn items_wc_sum(&self) -> u64 {
        self.items.values().map(|item| u64::from(item.wc)).sum()
    }
}

/// A whole K-mixture model (`KMixtureModelBigram` + its magic header),
/// plus the phrase-text column carried alongside so export needs no phrase
/// index (see the crate Cargo.toml note).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KMixtureModel {
    /// `KMixtureModelMagicHeader::m_WC`: Σ instances of all words.
    pub wc: u32,
    /// `KMixtureModelMagicHeader::m_N`: total documents.
    pub n: u32,
    /// `KMixtureModelMagicHeader::m_total_freq`: Σ unigram frequency.
    pub total_freq: u32,
    /// `token1 → single gram`, token1-ascending.
    pub grams: BTreeMap<u32, SingleGram>,
    /// `token → phrase text`, for the export/import phrase column.
    /// `sentence_start` is resolved to `<start>` at emit time, so it need
    /// not appear here.
    pub texts: BTreeMap<u32, String>,
}

impl KMixtureModel {
    /// An empty model (a fresh `--k-mixture-model-file`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The phrase text for `token`: `<start>` for `sentence_start`,
    /// otherwise the carried column (`None` if unknown — export then skips
    /// the record, matching `taglib_token_to_string` returning `NULL`).
    #[must_use]
    pub fn text(&self, token: u32) -> Option<&str> {
        if token == SENTENCE_START {
            Some(SENTENCE_START_TEXT)
        } else {
            self.texts.get(&token).map(String::as_str)
        }
    }

    /// Records a `token → text` mapping seen in a segmented line or a KMM
    /// text file. `sentence_start` is never stored (it resolves specially).
    pub fn record_text(&mut self, token: u32, text: &str) {
        if token == SENTENCE_START || token == NULL_TOKEN {
            return;
        }
        // Idempotent in practice: a token's phrase text is stable, so the
        // first recorded text wins.
        self.texts.entry(token).or_insert_with(|| text.to_owned());
    }
}

/// `parameter_t` — the model math is `double` throughout.
pub type Parameter = f64;

/// `α = 1 − n_0 / N` (`k_mixture_model.h:40-43`).
#[must_use]
pub fn compute_alpha(n: u32, n_0: u32) -> Parameter {
    1.0 - Parameter::from(n_0) / Parameter::from(n)
}

/// `γ = 1 − n_1 / (N − n_0)` (`k_mixture_model.h:45-50`).
///
/// `N − n_0` is `guint32` arithmetic upstream; `wrapping_sub` reproduces it
/// bit-for-bit (equal to the real difference for the `n_0 ≤ N` invariant,
/// and it can never panic on a malformed model).
#[must_use]
pub fn compute_gamma(n: u32, n_0: u32, n_1: u32) -> Parameter {
    1.0 - Parameter::from(n_1) / Parameter::from(n.wrapping_sub(n_0))
}

/// `B` (`k_mixture_model.h:52-65`): the special case `T − n_1 == 0 &&
/// N − n_0 − n_1 == 0` returns `2`; otherwise `(T − n_1)/(N − n_0 − n_1)`.
/// All differences are `guint32` (`wrapping_sub`), as in `compute_gamma`.
#[must_use]
pub fn compute_b(n: u32, t: u32, n_0: u32, n_1: u32) -> Parameter {
    let t_minus_n1 = t.wrapping_sub(n_1);
    let n_minus_n0_n1 = n.wrapping_sub(n_0).wrapping_sub(n_1);
    if t_minus_n1 == 0 && n_minus_n0_n1 == 0 {
        return 2.0;
    }
    Parameter::from(t_minus_n1) / Parameter::from(n_minus_n0_n1)
}

/// `Pr_G_3(k)` three-parameter mixture (`k_mixture_model.h:67-83`).
///
/// `k == 0 → 1 − α`; `k == 1 → α(1 − γ)`;
/// `k > 1 → (αγ/(B−1))·(1 − 1/(B−1))^{k−2}`.
#[must_use]
pub fn compute_pr_g_3(k: u32, alpha: Parameter, gamma: Parameter, b: Parameter) -> Parameter {
    if k == 0 {
        return 1.0 - alpha;
    }
    if k == 1 {
        return alpha * (1.0 - gamma);
    }
    // `pow((1 - 1/(B-1)), k-2)` — C's `pow(double, double)`.
    (alpha * gamma / (b - 1.0)) * (1.0 - 1.0 / (b - 1.0)).powf(Parameter::from(k - 2))
}

/// `compute_Pr_G_3_with_count` (`k_mixture_model.h:85-95`): derive
/// `(α, γ, B)` from the counts, then `Pr_G_3(k)`.
#[must_use]
pub fn compute_pr_g_3_with_count(k: u32, n: u32, t: u32, n_0: u32, n_1: u32) -> Parameter {
    let alpha = compute_alpha(n, n_0);
    let gamma = compute_gamma(n, n_0, n_1);
    let b = compute_b(n, t, n_0, n_1);
    compute_pr_g_3(k, alpha, gamma, b)
}

#[cfg(test)]
mod tests {
    use super::{KMixtureModel, SENTENCE_START_TEXT, compute_b, compute_pr_g_3_with_count};

    #[test]
    fn text_resolves_sentence_start_specially() {
        let mut model = KMixtureModel::new();
        model.record_text(10, "甲");
        model.record_text(1, "ignored"); // sentence_start not stored
        assert_eq!(model.text(1), Some(SENTENCE_START_TEXT));
        assert_eq!(model.text(10), Some("甲"));
        assert_eq!(model.text(99), None);
    }

    #[test]
    fn b_special_case_returns_two() {
        // T - n_1 == 0 and N - n_0 - n_1 == 0.
        assert_eq!(compute_b(2, 1, 1, 1), 2.0);
    }

    #[test]
    fn pr_g_3_partitions_to_one_over_all_k() {
        // For a well-formed (N,T,n_0,n_1), Σ_k Pr_G_3(k) → 1 as k grows.
        let (n, t, n_0, n_1) = (10_u32, 20_u32, 3_u32, 4_u32);
        let mut total = 0.0;
        for k in 0..2000 {
            total += compute_pr_g_3_with_count(k, n, t, n_0, n_1);
        }
        assert!((total - 1.0).abs() < 1e-9, "Σ Pr_G_3 = {total}");
    }
}
