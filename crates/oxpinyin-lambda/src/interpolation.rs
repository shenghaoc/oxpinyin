//! Deleted-interpolation EM reproducing libpinyin `estimate_interpolation`
//! (`utils/training/estimate_interpolation.cpp:30-88` inner,
//! `:119-139` outer).
//!
//! This is the one place W9 computes in floating point: `parameter_t` is
//! `double` (`src/include/novel_types.h:130`), and λ is a probability, so
//! the EM ratios are doubles. Counts stay integer (see [`crate::held_out`]
//! and [`oxpinyin_counter::Counts`]); floats appear only in this file.
//!
//! # The estimate, term by term (`estimate_interpolation.cpp:53-79`)
//!
//! For each held-out context `prev` and each held-out pair `(prev → cur)`
//! with `deleted_count`, the EM interpolates the **system bigram**
//! probability against the **unigram** probability:
//!
//! - bigram term (`:56-62`): `elem_poss = bigram.get_freq(cur) /
//!   bigram.get_total_freq()` — the system `SingleGram` for `prev` — else
//!   `0` when `prev` has no system entry or `cur` is unseen there;
//!   `numerator = lambda * elem_poss`.
//! - unigram term (`:65-73`): `elem_poss = unigram_freq(cur) /
//!   get_phrase_index_total_freq()`; `part_of_denominator =
//!   (1 - lambda) * elem_poss`. `get_phrase_item` returns `ERROR_OK == 0`
//!   on success, so `if (!get_phrase_item(...))` runs the block **when the
//!   token exists** (`:68`) — a load-bearing idiom, reproduced here as
//!   `unigrams.get(&cur)` (`Some` ⇒ present).
//! - accumulate (`:76-79`): skip when `numerator + part_of_denominator == 0`;
//!   else `next_lambda += deleted_count * numerator / (numerator +
//!   part_of_denominator)`.
//!
//! After the item loop, `next_lambda /= table_num` where `table_num` is the
//! held-out context's total freq (`:81-82`), then iterate until
//! `|lambda - next_lambda| <= epsilon` with `epsilon = 0.001`, seed
//! `next_lambda = 0.6` (`:34-37`).
//!
//! The single system λ is the **arithmetic mean** of the per-context λ
//! values (`:116-139`, `lambda_sum / lambda_count`), not one global EM.

use std::collections::BTreeMap;

use oxpinyin_counter::Counts;

use crate::error::LambdaError;
use crate::held_out::DeletedCounts;

/// Seed for `next_lambda` (`estimate_interpolation.cpp:34`).
pub const SEED_LAMBDA: f64 = 0.6;
/// Convergence threshold ε (`estimate_interpolation.cpp:35`,
/// `while fabs(lambda - next_lambda) > epsilon`).
pub const EPSILON: f64 = 0.001;

/// The estimated interpolation weight and the per-context λ it averages.
#[derive(Clone, Debug, PartialEq)]
pub struct Lambda {
    /// The single system λ: arithmetic mean of [`Lambda::per_context`]
    /// (`estimate_interpolation.cpp:139`). This is the value that would
    /// replace the decode path's authored constant (see
    /// `docs/findings/lambda-port.md`).
    pub average: f64,
    /// `(prev, λ_prev)` ascending by `prev`, one deleted-interpolation EM
    /// result per held-out context (`estimate_interpolation.cpp:127-129`).
    pub per_context: Vec<(u32, f64)>,
}

impl Lambda {
    /// λ formatted as libpinyin's `table.conf` writes it — `%f`, six
    /// decimals (`estimate_interpolation.cpp:139` prints `%f`; `evaluate.py`
    /// feeds that string to `make modify LAMBDA_PARAMETER=`). This is the
    /// "form usable by oxpinyin": the shipped `lambda parameter:0.xxxxxx`.
    #[must_use]
    pub fn table_conf_value(&self) -> String {
        format!("{:.6}", self.average)
    }
}

/// System `SingleGram` for one `prev`: `cur → count` and the stored total.
///
/// `total` mirrors `SingleGram::get_total_freq` (`ngram.cpp:48`), which
/// `gen_ngram` keeps equal to the sum of item counts — so `total` is that
/// sum. The EM divides by it (`estimate_interpolation.cpp:60`,
/// `assert(0 != total_freq)`), reached only when an item is present, so
/// `total > 0` there.
struct SystemContext {
    freqs: BTreeMap<u32, u64>,
    total: u64,
}

/// Groups the system bigram counts by `prev` context.
fn system_contexts(system: &Counts) -> BTreeMap<u32, SystemContext> {
    let mut contexts: BTreeMap<u32, SystemContext> = BTreeMap::new();
    for (&(prev, cur), &count) in &system.bigrams {
        let entry = contexts.entry(prev).or_insert_with(|| SystemContext {
            freqs: BTreeMap::new(),
            total: 0,
        });
        entry.freqs.insert(cur, count);
        entry.total += count;
    }
    contexts
}

/// The per-context deleted-interpolation EM
/// (`estimate_interpolation.cpp:30-88`).
///
/// `deleted_ctx` is `cur → deleted_count` for one `prev` (ascending `cur`,
/// the `retrieve_all` order). `system_ctx` is that `prev`'s system
/// `SingleGram`, or `None` when `prev` is absent from the system bigram
/// (`bigram == NULL`, `:56`). `unigrams`/`uni_total` are the phrase index's
/// per-token frequency and its grand total (`:69-71`).
fn compute_interpolation(
    deleted_ctx: &BTreeMap<u32, u64>,
    system_ctx: Option<&SystemContext>,
    unigrams: &BTreeMap<u32, u64>,
    uni_total: u64,
) -> f64 {
    // `table_num` = deleted context total freq (`:81`), the sum of its
    // item counts (`gen_deleted_ngram` keeps the SingleGram total in step).
    let table_num: u64 = deleted_ctx.values().sum();

    // `parameter_t lambda = 0, next_lambda = 0.6; epsilon = 0.001;` (:34-35)
    let mut lambda = 0.0_f64;
    let mut next_lambda = SEED_LAMBDA;

    // `while ( fabs(lambda - next_lambda) > epsilon )` (:37)
    while (lambda - next_lambda).abs() > EPSILON {
        lambda = next_lambda; // (:38)
        next_lambda = 0.0; // (:39)

        // `for (i = 0; i < array->len; ++i)` over retrieve_all, ascending
        // `cur` — deterministic, matches BTreeMap iteration (:47).
        for (&cur, &deleted_count) in deleted_ctx {
            // Bigram block (:53-63).
            let elem_poss_bigram = match system_ctx.and_then(|ctx| ctx.freqs.get(&cur)) {
                // `bigram && bigram->get_freq(cur, freq)` hit: total_freq is
                // the context total, > 0 because this item is present.
                Some(&freq) => freq as f64 / system_ctx.map_or(0, |c| c.total) as f64,
                None => 0.0,
            };
            let numerator = lambda * elem_poss_bigram; // (:62)

            // Unigram block (:65-73). `!get_phrase_item` ⇒ token present.
            let elem_poss_unigram = match unigrams.get(&cur) {
                Some(&freq) => freq as f64 / uni_total as f64, // (:71)
                None => 0.0,
            };
            let part_of_denominator = (1.0 - lambda) * elem_poss_unigram; // (:73)

            // `if (0 == numerator + part_of_denominator) continue;` (:76-77)
            let sum = numerator + part_of_denominator;
            if sum == 0.0 {
                continue;
            }

            // `next_lambda += deleted_count * (numerator / sum);` (:79)
            next_lambda += deleted_count as f64 * (numerator / sum);
        }

        // `next_lambda /= table_num;` (:82)
        next_lambda /= table_num as f64;
    }

    // `lambda = next_lambda; return lambda;` (:86-87)
    next_lambda
}

/// Estimates the single system λ from the system counts and the held-out
/// (`DELETED_BIGRAM`) counts, reproducing `estimate_interpolation`'s
/// per-context EM and cross-context averaging.
///
/// `system` supplies both the system bigram (`SYSTEM_BIGRAM`) and the
/// phrase-index unigram frequencies + grand total (freq-1 floor included —
/// `training-algorithm.md` §6, and this is exactly [`oxpinyin_counter`]'s
/// full-corpus [`Counts`]). `deleted` is the held-out counting from
/// [`crate::held_out`].
///
/// # Errors
///
/// Returns [`LambdaError::EmptyDeleted`] when `deleted` has no contexts:
/// the outer mean would divide by zero (libpinyin prints `-nan`; a port
/// must not panic).
pub fn estimate_lambda(system: &Counts, deleted: &DeletedCounts) -> Result<Lambda, LambdaError> {
    // `get_phrase_index_total_freq()` = Σ unigram frequencies (`:70`,
    // phrase_index.h:615). guint32 in the pin; the sum is < 2^32 here and
    // exact as f64.
    let uni_total: u64 = system.unigrams.values().sum();
    let system_ctx = system_contexts(system);

    let mut per_context: Vec<(u32, f64)> = Vec::new();
    // `lambda_sum`, `lambda_count` (`:116-117`). Iterate contexts ascending
    // by `prev`; the pin uses tkrzw `get_all_items` order, which changes
    // only the outer sum's rounding (per-context λ is keyed by `prev`).
    let mut lambda_sum = 0.0_f64;
    let mut lambda_count: u32 = 0;
    for prev in deleted.contexts() {
        let deleted_ctx = deleted.context(prev);
        let lambda = compute_interpolation(
            &deleted_ctx,
            system_ctx.get(&prev),
            &system.unigrams,
            uni_total,
        );
        per_context.push((prev, lambda));
        lambda_sum += lambda; // (:131)
        lambda_count += 1; // (:132)
    }

    if lambda_count == 0 {
        return Err(LambdaError::EmptyDeleted);
    }

    Ok(Lambda {
        average: lambda_sum / f64::from(lambda_count), // (:139)
        per_context,
    })
}

#[cfg(test)]
mod tests {
    use super::{EPSILON, SEED_LAMBDA, compute_interpolation, estimate_lambda};
    use crate::error::LambdaError;
    use crate::held_out::DeletedCounts;
    use oxpinyin_counter::Counts;
    use std::collections::BTreeMap;

    /// One context whose held-out pair has full bigram support: λ climbs
    /// from 0.6 toward the bigram, converging within ε.
    #[test]
    fn single_context_converges() {
        let deleted_ctx = BTreeMap::from([(20_u32, 3_u64)]);
        let system_ctx = super::SystemContext {
            freqs: BTreeMap::from([(20_u32, 5_u64)]),
            total: 5,
        };
        let unigrams = BTreeMap::from([(20_u32, 4_u64)]);
        let uni_total = 100;
        let lambda = compute_interpolation(&deleted_ctx, Some(&system_ctx), &unigrams, uni_total);
        // Sole item ⇒ next_lambda = numerator/(numerator+denom); the ratio
        // is a fixed point once |Δ| ≤ ε. It must land in (0.6, 1).
        assert!(lambda > SEED_LAMBDA && lambda < 1.0, "lambda = {lambda}");
    }

    /// The bigram-miss path (`bigram == NULL` / unseen `cur`): numerator is
    /// 0 every iteration, so the per-context λ collapses to 0.
    #[test]
    fn bigram_miss_drives_lambda_to_zero() {
        let deleted_ctx = BTreeMap::from([(20_u32, 3_u64)]);
        // No system context for this prev at all.
        let unigrams = BTreeMap::from([(20_u32, 4_u64)]);
        let lambda = compute_interpolation(&deleted_ctx, None, &unigrams, 100);
        // numerator = lambda*0 = 0 each pass ⇒ next_lambda = 0; converges to 0.
        assert_eq!(lambda, 0.0);
    }

    /// A `cur` with neither bigram nor unigram support is skipped (the
    /// `numerator + part_of_denominator == 0` continue), contributing
    /// nothing — same λ as if it were absent.
    #[test]
    fn zero_support_item_is_skipped() {
        let with_ghost = BTreeMap::from([(20_u32, 3_u64), (99_u32, 7_u64)]);
        let without = BTreeMap::from([(20_u32, 3_u64)]);
        let system_ctx = super::SystemContext {
            freqs: BTreeMap::from([(20_u32, 5_u64)]),
            total: 5,
        };
        let unigrams = BTreeMap::from([(20_u32, 4_u64)]); // 99 absent everywhere
        // table_num differs (3 vs 10) so λ is not identical, but the ghost
        // adds 0 to next_lambda; check it never contributes a NaN/inf.
        let a = compute_interpolation(&with_ghost, Some(&system_ctx), &unigrams, 100);
        let b = compute_interpolation(&without, Some(&system_ctx), &unigrams, 100);
        assert!(a.is_finite() && b.is_finite());
        assert!(a > 0.0 && b > 0.0);
    }

    /// The system λ is the arithmetic mean of the per-context λ values.
    #[test]
    fn average_is_mean_of_per_context() {
        let mut system = Counts::default();
        system.unigrams.insert(10, 4);
        system.unigrams.insert(20, 6);
        system.unigrams.insert(30, 2);
        // two prev contexts: 10→20 and 20→30
        system.bigrams.insert((10, 20), 5);
        system.bigrams.insert((20, 30), 3);
        let deleted = DeletedCounts {
            bigrams: BTreeMap::from([((10, 20), 2), ((20, 30), 4)]),
        };
        let lambda = estimate_lambda(&system, &deleted).expect("lambda");
        assert_eq!(lambda.per_context.len(), 2);
        let mean = lambda.per_context.iter().map(|&(_, l)| l).sum::<f64>() / 2.0;
        assert_eq!(lambda.average, mean);
    }

    #[test]
    fn empty_deleted_is_error_not_panic() {
        let system = Counts::default();
        let deleted = DeletedCounts::default();
        assert!(matches!(
            estimate_lambda(&system, &deleted),
            Err(LambdaError::EmptyDeleted)
        ));
    }

    #[test]
    fn epsilon_and_seed_match_source() {
        assert_eq!(SEED_LAMBDA, 0.6);
        assert_eq!(EPSILON, 0.001);
    }

    #[test]
    fn table_conf_value_is_six_decimals() {
        let lambda = super::Lambda {
            average: 0.3126987654,
            per_context: vec![],
        };
        assert_eq!(lambda.table_conf_value(), "0.312699");
    }
}
