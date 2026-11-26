//! `merge_k_mixture_model` (`utils/training/merge_k_mixture_model.cpp` at
//! `074a2219`): fold each source container into a result container.
//!
//! Per matching `(W1, W2)`, `merge_two_phrase_array` (`:38-94`) sums
//! `m_WC`, `m_N_n_0` and `m_n_1` and takes the larger `m_Mr`; the array
//! header and then the magic header sum field-wise. A token new to the
//! target gets the source's stored value unchanged, in the source file's
//! native token order (`merge_array_items`, `:131-200`). For an existing
//! predecessor, insertion sorts the emitted records by token and keeps the first
//! emitted record for each token (`:185-190`; `flexible_single_gram.h:151-181`).
//! The magic header's `m_WC` and `m_total_freq` sums are checked against the
//! pin's `sum < max(a, b)` overflow test; `EOVERFLOW` exits after the rows have
//! already been merged (`:96-129`, `:232-233`).

use std::ffi::CStr;

use oxpinyin_tool_support::input::{self as tool, k_mixture_model as edge};

use crate::k_mixture_model::database::{Database, Loaded, exit_on_short_single_gram};
use crate::k_mixture_model::records::{
    ArrayItemWithToken, KMixtureModelArrayHeader, KMixtureModelArrayItem, KMixtureModelMagicHeader,
    KmmGram,
};

/// `main`'s body: attach the result container, then fold each source in
/// `argv` order (`:210-237`).
///
/// # Errors
///
/// The pin's ordinary exit: `EOVERFLOW` when a magic-header sum is refused.
/// The function calls [`tool::exit`] and does not return in that case.
pub fn run(result_filename: Option<&CStr>, sources: &[&CStr]) {
    let target = Database::open_read_write(result_filename);
    for source in sources {
        let new_one = Database::open_read_only(Some(source));
        if !merge_two_k_mixture_model(&target, &new_one) {
            tool::exit(tool::EOVERFLOW);
        }
    }
}

/// `merge_two_k_mixture_model` (`:202-208`): array items first, then the
/// magic header. The pin's `merge_array_items` always answers true.
fn merge_two_k_mixture_model(target: &Database, new_one: &Database) -> bool {
    merge_array_items(target, new_one);
    merge_magic_header(target, new_one)
}

/// `merge_array_items` (`:131-200`). The pin's `load` succeeds on a row
/// shorter than its array header. A row the target lacks is copied unchanged, short or
/// not (`:142-146`). Otherwise the two `check_result(get_array_header(…))`
/// calls (`:155-156`) abort on a short row, as the pin's asserts do.
fn merge_array_items(target: &Database, new_one: &Database) {
    for token in new_one.get_all_items() {
        // `check_result(new_one->load(…))` (`:142`).
        let Some(new_single_gram) = new_one.load(token) else {
            std::process::abort()
        };
        let Some(target_single_gram) = target.load(token) else {
            target.copy_single_gram(token, new_one);
            continue;
        };

        let new_array_header = new_one.array_header_checked(token);
        let target_array_header = target.array_header_checked(token);
        // A short row aborted at `:155` or `:156`, so only a row that
        // changed since its load is short here. The pin reads it outside
        // its record (`retrieve_all`, `:171`, `:175`): class (b).
        let (Loaded::Gram(new_single_gram), Loaded::Gram(target_single_gram)) =
            (new_single_gram, target_single_gram)
        else {
            exit_on_short_single_gram()
        };
        let merged_array_header = KMixtureModelArrayHeader {
            m_wc: target_array_header.m_wc.wrapping_add(new_array_header.m_wc),
            m_freq: target_array_header
                .m_freq
                .wrapping_add(new_array_header.m_freq),
        };

        let mut merged_array =
            merge_two_phrase_array(target_single_gram.items(), new_single_gram.items());

        // The insert loop ignores duplicate refusals (`:185-190`;
        // `flexible_single_gram.h:151-181`): the first emitted record wins.
        merged_array.sort_by_key(|entry| entry.token);
        merged_array.dedup_by_key(|entry| entry.token);
        let merged_single_gram = KmmGram::from_parts(merged_array_header, merged_array);
        target.store(token, &merged_single_gram);
    }
}

/// `merge_two_phrase_array` (`:38-94`): a merge-join of two token-ascending
/// arrays, copying unmatched records and combining matching records. Stored
/// input order is preserved by the walk even when an input is unsorted. The
/// caller reproduces insertion's token order and first-emission duplicate rule.
#[must_use]
pub fn merge_two_phrase_array(
    first: &[ArrayItemWithToken<KMixtureModelArrayItem>],
    second: &[ArrayItemWithToken<KMixtureModelArrayItem>],
) -> Vec<ArrayItemWithToken<KMixtureModelArrayItem>> {
    let mut merged = Vec::with_capacity(first.len() + second.len());
    let (mut first_index, mut second_index) = (0_usize, 0_usize);
    while first_index < first.len() && second_index < second.len() {
        let first_item = &first[first_index];
        let second_item = &second[second_index];
        if first_item.token > second_item.token {
            merged.push(second_item.clone());
            second_index += 1;
        } else if first_item.token < second_item.token {
            merged.push(first_item.clone());
            first_index += 1;
        } else {
            merged.push(ArrayItemWithToken {
                token: first_item.token,
                item: KMixtureModelArrayItem {
                    m_wc: first_item.item.m_wc.wrapping_add(second_item.item.m_wc),
                    m_n_n_0: first_item
                        .item
                        .m_n_n_0
                        .wrapping_add(second_item.item.m_n_n_0),
                    m_n_1: first_item.item.m_n_1.wrapping_add(second_item.item.m_n_1),
                    m_mr: first_item.item.m_mr.max(second_item.item.m_mr),
                },
            });
            first_index += 1;
            second_index += 1;
        }
    }
    while first_index < first.len() {
        merged.push(first[first_index].clone());
        first_index += 1;
    }
    while second_index < second.len() {
        merged.push(second[second_index].clone());
        second_index += 1;
    }
    merged
}

/// `merge_magic_header` (`:96-129`).
fn merge_magic_header(target: &Database, new_one: &Database) -> bool {
    let target_magic_header = target.get_magic_header().unwrap_or_default();
    let new_magic_header = new_one.magic_header_checked();

    if target_magic_header.m_wc.wrapping_add(new_magic_header.m_wc)
        < target_magic_header.m_wc.max(new_magic_header.m_wc)
    {
        edge::report_magic_wc_overflow();
        return false;
    }
    if target_magic_header
        .m_total_freq
        .wrapping_add(new_magic_header.m_total_freq)
        < target_magic_header
            .m_total_freq
            .max(new_magic_header.m_total_freq)
    {
        edge::report_magic_total_freq_overflow();
        return false;
    }

    let merged_magic_header = KMixtureModelMagicHeader {
        m_wc: target_magic_header.m_wc.wrapping_add(new_magic_header.m_wc),
        m_n: target_magic_header.m_n.wrapping_add(new_magic_header.m_n),
        m_total_freq: target_magic_header
            .m_total_freq
            .wrapping_add(new_magic_header.m_total_freq),
    };
    target.set_magic_header_checked(&merged_magic_header);
    true
}

#[cfg(test)]
mod tests;
