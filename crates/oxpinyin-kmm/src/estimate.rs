//! `estimate_k_mixture_model` (`utils/training/estimate_k_mixture_model.cpp`
//! at `074a2219`): the deleted-interpolation EM that scores a candidate
//! against a held-out model.
//!
//! For each W1 present in the deleted model with a nonzero header `m_WC`,
//! `compute_interpolation` (`:36-96`) runs the per-context EM: seed
//! λ = 0.6, ε = 0.001, one pass per iteration over the deleted single gram,
//! with the bigram term `array_item.m_WC / header.m_WC` of the candidate and
//! the unigram term `header.m_freq / magic.m_total_freq` of the candidate.
//! The average is `lambda_sum / lambda_count` with no zero guard: an empty
//! deleted model divides zero by zero and prints the C library's `-nan`.
//!
//! The pin's `assert(0 != magic.m_total_freq)`, `assert(0 != header.m_WC)`
//! and `check_result(get_magic_header(…))` abort; they are reached only when
//! the candidate's own header is inconsistent with the deleted model.

use std::ffi::CStr;

use oxpinyin_input::tool::k_mixture_model as edge;

use crate::database::Database;
use crate::records::KmmGram;

/// Seed `next_lambda` (`estimate_k_mixture_model.cpp:40`).
pub const SEED_LAMBDA: f64 = 0.6;
/// EM convergence threshold (`:41`).
pub const EPSILON: f64 = 0.001;
/// Default `--bigram-file` (`:25`).
pub const DEFAULT_BIGRAM_FILE: &str = "k_mixture_model_ngram.db";
/// Default `--deleted-bigram-file` (`:26`).
pub const DEFAULT_DELETED_BIGRAM_FILE: &str = "k_mixture_model_deleted_ngram.db";

/// `main`'s body (`:98-155`).
pub fn run(bigram_filename: Option<&CStr>, deleted_bigram_filename: Option<&CStr>) {
    let bigram = Database::open_read_only(bigram_filename);
    let deleted_bigram = Database::open_read_only(deleted_bigram_filename);

    let mut lambda_sum = 0.0_f64;
    let mut lambda_count = 0_i32;

    for &token in &deleted_bigram.get_all_items() {
        let single_gram = bigram.load(token);
        let deleted_single_gram = deleted_bigram.load_checked(token);
        let deleted_array_header = *deleted_single_gram.header();

        if deleted_array_header.m_wc != 0 {
            let lambda = compute_interpolation(&deleted_single_gram, &bigram, single_gram.as_ref());
            edge::print_token_lambda(token as i32, lambda);
            lambda_sum += lambda;
            lambda_count += 1;
        }
    }

    edge::print_average_lambda(lambda_sum / f64::from(lambda_count));
}

/// `compute_interpolation` (`:36-96`).
fn compute_interpolation(
    deleted_bigram: &KmmGram,
    unigram: &Database,
    bigram: Option<&KmmGram>,
) -> f64 {
    let magic_header = unigram.magic_header_checked();
    if magic_header.m_total_freq == 0 {
        std::process::abort();
    }

    let mut lambda = 0.0_f64;
    let mut next_lambda = SEED_LAMBDA;
    while (lambda - next_lambda).abs() > EPSILON {
        lambda = next_lambda;
        next_lambda = 0.0;
        for item in deleted_bigram.items() {
            let token = item.token;
            let deleted_count = f64::from(item.item.m_wc);

            let numerator = {
                let mut elem_poss = 0.0_f64;
                if let Some(gram) = bigram
                    && let Some(array_item) = gram.get_array_item(token)
                {
                    let array_header = *gram.header();
                    if array_header.m_wc == 0 {
                        std::process::abort();
                    }
                    elem_poss = f64::from(array_item.m_wc) / f64::from(array_header.m_wc);
                }
                lambda * elem_poss
            };

            let part_of_denominator = {
                let mut elem_poss = 0.0_f64;
                if let Some(array_header) = unigram.get_array_header(token) {
                    elem_poss =
                        f64::from(array_header.m_freq) / f64::from(magic_header.m_total_freq);
                }
                (1.0 - lambda) * elem_poss
            };

            if numerator + part_of_denominator == 0.0 {
                continue;
            }
            next_lambda += deleted_count * (numerator / (numerator + part_of_denominator));
        }

        if deleted_bigram.header().m_wc == 0 {
            std::process::abort();
        }
        next_lambda /= f64::from(deleted_bigram.header().m_wc);
    }
    next_lambda
}
