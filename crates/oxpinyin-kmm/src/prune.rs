//! `prune_k_mixture_model` (`utils/training/prune_k_mixture_model.cpp` at
//! `074a2219`): drop the pairs whose survival probability is below the CDF.
//!
//! For each `(W1, W2)`, `remained = 1 − Σ_{k=0}^{K−1} Pr_G_3(k)`. A value
//! outside `[0, 1]` (NaN included) prints the pin's two diagnostics and
//! `exit(EDOM)`; a `remained < CDF` removes the pair and subtracts its
//! `m_WC` from the row header, the magic header's `m_WC` and
//! `m_total_freq`, and — in the pin's second pass — from W2's unigram
//! `m_freq`. Rows whose header ends fully zero are removed.
//!
//! The pin's progress line is `items->len / 20` per asterisk (`:141-146`);
//! with fewer than twenty rows that quotient is zero, so every row prints
//! one, and with no rows nothing does.

use std::ffi::CStr;

use oxpinyin_input::tool::{self, k_mixture_model as edge};

use crate::database::Database;
use crate::math::compute_pr_g_3_with_count;
use crate::records::{
    ArrayItemWithToken, KMixtureModelArrayItem, KMixtureModelMagicHeader,
};

/// Default `-k` (`prune_k_mixture_model.cpp:34`).
pub const DEFAULT_PRUNE_K: i32 = 3;
/// Default `--CDF` (`:35`).
pub const DEFAULT_CDF: f64 = 0.99;

/// `main`'s body (`:107-191`).
///
/// # Errors
///
/// The pin's ordinary exits: `ENODATA` for a missing magic header and
/// `EDOM` for an out-of-range probability. The function calls
/// [`tool::exit`] and does not return in those cases.
pub fn run(filename: Option<&CStr>, prune_k: i32, cdf: f64) {
    let db = Database::open_read_write(filename);
    let Some(mut magic_header) = db.get_magic_header() else {
        edge::report_no_magic();
        tool::exit(tool::ENODATA);
    };

    let items = db.get_all_items();

    let mut progress: usize = 0;
    let onestep = items.len() / 20;
    for &token in &items {
        if progress >= onestep {
            progress = 0;
            edge::print_progress_star();
        }
        progress += 1;

        let mut single_gram = db
            .load(token)
            .unwrap_or_else(|| std::process::abort());
        let mut removed_array = Vec::new();
        prune_k_mixture_model(
            &mut magic_header,
            &mut single_gram,
            &mut removed_array,
            prune_k,
            cdf,
        );
        db.store(token, &single_gram);

        for item in &removed_array {
            let mut array_header = db.array_header_checked(item.token);
            array_header.m_freq = array_header.m_freq.wrapping_sub(item.item.m_wc);
            // The pin's `assert(array_header.m_freq >= 0)` is vacuous for a
            // `guint32`; the wrapping subtraction above is what it observes.
            db.set_array_header_checked(item.token, &array_header);
        }
    }

    // The pin's trailing newline on stderr after the progress line.
    edge::print_progress_newline();

    db.set_magic_header(&magic_header);

    for &token in &items {
        let array_header = db.array_header_checked(token);
        if array_header.m_wc == 0 && array_header.m_freq == 0 {
            db.remove_checked(token);
        }
    }
}

/// `prune_k_mixture_model` over one single gram (`:45-105`).
fn prune_k_mixture_model(
    magic_header: &mut KMixtureModelMagicHeader,
    single_gram: &mut crate::records::KmmGram,
    removed_array: &mut Vec<ArrayItemWithToken<KMixtureModelArrayItem>>,
    prune_k: i32,
    cdf: f64,
) {
    let array = single_gram.items().to_vec();
    for item in &array {
        let token = item.token;
        let mut remained_poss = 1.0_f64;
        let mut errors = false;
        for k in 0..prune_k.max(0) {
            let one_poss = compute_pr_g_3_with_count(
                k as u32,
                magic_header.m_n,
                item.item.m_wc,
                magic_header.m_n.wrapping_sub(item.item.m_n_n_0),
                item.item.m_n_1,
            );
            if !(0.0..=1.0).contains(&one_poss) {
                errors = true;
            }
            remained_poss -= one_poss;
        }

        if remained_poss.abs() < f64::EPSILON {
            remained_poss = 0.0;
        }

        if errors || !(0.0..=1.0).contains(&remained_poss) {
            edge::report_wrong_possibility(remained_poss);
            edge::print_prune_counts(
                prune_k,
                magic_header.m_n as i32,
                item.item.m_wc as i32,
                magic_header.m_n.wrapping_sub(item.item.m_n_n_0) as i32,
                item.item.m_n_1 as i32,
            );
            tool::exit(tool::EDOM);
        }

        if remained_poss < cdf {
            let Some(removed_item) = single_gram.remove_array_item(token) else {
                std::process::abort();
            };
            removed_array.push(ArrayItemWithToken {
                token,
                item: removed_item,
            });

            let mut array_header = *single_gram.header();
            let removed_count = removed_item.m_wc;
            array_header.m_wc = array_header.m_wc.wrapping_sub(removed_count);
            single_gram.set_header(array_header);
            magic_header.m_wc = magic_header.m_wc.wrapping_sub(removed_count);
            magic_header.m_total_freq = magic_header
                .m_total_freq
                .wrapping_sub(removed_count);
        }
    }
}
