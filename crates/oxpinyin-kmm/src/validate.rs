//! `validate_k_mixture_model` (`utils/training/validate_k_mixture_model.cpp`
//! at `074a2219`): the magic-header/array-header consistency checks and the
//! per-row item sums.
//!
//! [`validate`] answers the pin's pass/fail; [`run`] is `main`'s body, which
//! prints the pin's `k mixture model validation failed.` line and exits
//! `ENODATA` on the first failed half rows answer.

use oxpinyin_input::tool::{self, k_mixture_model as edge};

use crate::database::Database;

/// `validate_unigram` then `validate_bigram` (`main`, `:162-170`).
///
/// # Errors
///
/// The pin's ordinary exit: `ENODATA` when either half fails. The function
/// calls [`tool::exit`] and does not return in that case.
pub fn run(db: &Database) {
    if !validate_unigram(db) || !validate_bigram(db) {
        edge::report_validation_failed();
        tool::exit(tool::ENODATA);
    }
}

/// `validate_unigram` (`:28-80`).
#[must_use]
pub fn validate_unigram(db: &Database) -> bool {
    let Some(magic) = db.get_magic_header() else {
        edge::report_no_magic();
        return false;
    };
    let expected_word_count = magic.m_wc;
    if expected_word_count == 0 {
        edge::report_zero_word_count();
        return false;
    }
    let expected_total_freq = magic.m_total_freq;
    if expected_total_freq == 0 {
        edge::report_zero_total_freq();
        return false;
    }
    if expected_word_count != expected_total_freq {
        edge::report_word_count_mismatch();
        return false;
    }

    let mut word_count = 0_u32;
    let mut total_freq = 0_u32;
    for token in db.get_all_items() {
        let header = db.array_header_checked(token);
        word_count = word_count.wrapping_add(header.m_wc);
        total_freq = total_freq.wrapping_add(header.m_freq);
    }

    if word_count != expected_word_count {
        edge::print_expected_word_count(expected_word_count as i32);
        edge::print_header_word_count_sum(word_count as i32);
        edge::report_word_count_sum_differs();
        return false;
    }
    if total_freq != expected_total_freq {
        edge::print_expected_total_freq(expected_total_freq as i32);
        edge::print_header_freq_sum(total_freq as i32);
        edge::report_total_freq_sum_differs();
        return false;
    }
    true
}

/// `validate_bigram` (`:82-139`), which collects every row's diagnostics
/// before answering.
#[must_use]
pub fn validate_bigram(db: &Database) -> bool {
    let mut result = true;
    for token in db.get_all_items() {
        let gram = db.load_checked(token);
        let header = *gram.header();
        let expected_sum = header.m_wc;
        let freq = header.m_freq;
        if expected_sum == 0 {
            if !gram.items().is_empty() {
                edge::print_array_header_token(token as i32);
                edge::report_zero_count_with_items();
                result = false;
            }
            if freq != 0 {
                continue;
            }
            edge::print_array_header_token(token as i32);
            edge::report_zero_count_and_freq();
            result = false;
        }

        let mut sum = 0_u32;
        for item in gram.items() {
            sum = sum.wrapping_add(item.item.m_wc);
        }
        if sum != expected_sum {
            edge::print_array_header_count(expected_sum as i32);
            edge::print_item_count_sum(sum as i32);
            edge::report_word_count_sum_differs();
            result = false;
        }
    }
    result
}
