//! `estimate_interpolation` (`utils/training/estimate_interpolation.cpp` at
//! libpinyin 074a2219).
//!
//! For every previous token of `deleted_bigram.db` in the container's own
//! `get_all_items` order, [`compute_interpolation`] runs the pin's
//! fixed-point estimate of the interpolation weight λ between the system
//! bigram and the unigram table, prints `token:%d lambda:%f`, and finally
//! prints the arithmetic mean. The pin's divisions are C doubles: an empty
//! deleted bigram leaves `0.0/0` and the host's NaN spelling.

use std::ffi::OsString;
use std::path::Path;

use oxpinyin_data::table_info::FileType;
use oxpinyin_input::tool;

use crate::storage::dbm::Hash;
use crate::storage::libraries::Libraries;
use crate::training::{Gram, all_tokens, load_gram, load_table_conf};

/// `compute_interpolation` (`estimate_interpolation.cpp:30-88`).
///
/// The pin's bodies use C doubles throughout; the check
/// `if (!unigram->get_phrase_item(token, item))` is the success test of an
/// `int`-returning accessor (zero is `ERROR_OK`), so the unigram term is
/// the item's stored frequency over
/// `get_phrase_index_total_freq` wherever the item exists and `0` where it
/// does not.
fn compute_interpolation(deleted_bigram: &Gram, unigram: &Libraries, bigram: Option<&Gram>) -> f64 {
    let mut lambda = 0.0_f64;
    let mut next_lambda = 0.6_f64;
    let epsilon = 0.001_f64;

    while (lambda - next_lambda).abs() > epsilon {
        lambda = next_lambda;
        next_lambda = 0.0;
        let unigram_total = unigram.total_freq();

        for &(token, deleted_count) in &deleted_bigram.records {
            let mut elem_poss = 0.0_f64;
            if let Some(bigram) = bigram
                && let Some(freq) = bigram.get_freq(token)
            {
                let total_freq = bigram.total;
                // `assert(0 != total_freq)` (`:59`).
                if total_freq == 0 {
                    tool::abort();
                }
                elem_poss = f64::from(freq) / f64::from(total_freq);
            }
            let numerator = lambda * elem_poss;

            let mut unigram_poss = 0.0_f64;
            if let Some(item) = unigram.item(token) {
                unigram_poss = f64::from(item.unigram) / f64::from(unigram_total);
            }
            let part_of_denominator = (1.0 - lambda) * unigram_poss;

            // `if (0 == (numerator + part_of_denominator)) continue;`
            if (numerator + part_of_denominator) == 0.0 {
                continue;
            }

            next_lambda +=
                f64::from(deleted_count) * (numerator / (numerator + part_of_denominator));
        }

        // `check_result(deleted_bigram->get_total_freq(table_num));`
        next_lambda /= f64::from(deleted_bigram.total);
    }

    lambda = next_lambda;
    lambda
}

/// `main` (`estimate_interpolation.cpp:90-142`).
pub fn main(_args: impl IntoIterator<Item = OsString>) {
    let conf = load_table_conf();
    let unigram = Libraries::load_rows(&conf, false, &[FileType::SystemFile], tool::ENOENT);
    let bigram = Hash::open_read_only(Path::new("bigram.db")).ok();
    let deleted_bigram = Hash::open_read_only(Path::new("deleted_bigram.db")).ok();

    let mut lambda_sum = 0.0_f64;
    let mut lambda_count = 0_i32;
    for token in all_tokens(deleted_bigram.as_ref()) {
        let system = load_gram(bigram.as_ref(), token);
        let Some(deleted) = load_gram(deleted_bigram.as_ref(), token) else {
            // `get_all_items` produced the key, so its row exists; a
            // row that cannot be read is the pin's NULL dereference.
            tool::abort();
        };
        let lambda = compute_interpolation(&deleted, &unigram, system.as_ref());
        tool::training::print_token_lambda(token, lambda);
        lambda_sum += lambda;
        lambda_count += 1;
    }

    // `lambda_sum/lambda_count`, a C double division: `0.0/0` is the host's
    // NaN, which the formatted call prints.
    tool::training::print_average_lambda(lambda_sum / f64::from(lambda_count));
}
