//! `merge_k_mixture_model` (`utils/training/merge_k_mixture_model.cpp` at
//! `074a2219`): fold each source container into a result container.
//!
//! Per matching `(W1, W2)`, `merge_two_phrase_array` (`:38-94`) sums
//! `m_WC`, `m_N_n_0` and `m_n_1` and takes the larger `m_Mr`; the array
//! header and then the magic header sum field-wise. A token new to the
//! target gets the source's stored value unchanged, in the source file's
//! native token order (`merge_array_items`, `:131-200`). The magic
//! header's `m_WC` and `m_total_freq` sums are checked against the pin's
//! `sum < max(a, b)` overflow test; `EOVERFLOW` exits after the rows have
//! already been merged (`:96-129`, `:232-233`).

use std::ffi::CStr;

use oxpinyin_input::tool::{self, k_mixture_model as edge};

use crate::database::{Database, Loaded, exit_on_short_single_gram};
use crate::records::{
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
/// magic header.
fn merge_two_k_mixture_model(target: &Database, new_one: &Database) -> bool {
    merge_array_items(target, new_one) && merge_magic_header(target, new_one)
}

/// `merge_array_items` (`:131-200`). The pin's `load` succeeds on a row
/// shorter than its array header. A row the target lacks is copied unchanged, short or
/// not (`:142-146`). Otherwise the two `check_result(get_array_header(…))`
/// calls (`:155-156`) abort on a short row, as the pin's asserts do.
fn merge_array_items(target: &Database, new_one: &Database) -> bool {
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

        let merged_array =
            merge_two_phrase_array(target_single_gram.items(), new_single_gram.items());

        let mut merged_single_gram = KmmGram::new();
        for item in &merged_array {
            let inserted = merged_single_gram.insert_array_item(item.token, item.item);
            if !inserted {
                std::process::abort();
            }
        }
        merged_single_gram.set_header(merged_array_header);
        target.store(token, &merged_single_gram);
    }
    true
}

/// `merge_two_phrase_array` (`:38-94`): a merge-join of two token-ascending
/// arrays, copying the token and item of every record.
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
mod tests {
    use super::merge_two_phrase_array;
    use crate::records::{ArrayItemWithToken, KMixtureModelArrayItem};

    fn entry(token: u32, wc: u32, mr: u32) -> ArrayItemWithToken<KMixtureModelArrayItem> {
        ArrayItemWithToken {
            token,
            item: KMixtureModelArrayItem {
                m_wc: wc,
                m_n_n_0: wc,
                m_n_1: 0,
                m_mr: mr,
            },
        }
    }

    #[test]
    fn matching_tokens_sum_and_max_and_others_keep_their_order() {
        let first = vec![entry(1, 1, 1), entry(3, 2, 2), entry(7, 4, 4)];
        let second = vec![entry(2, 5, 5), entry(3, 6, 9), entry(8, 1, 1)];
        let merged = merge_two_phrase_array(&first, &second);
        let tokens: Vec<u32> = merged.iter().map(|entry| entry.token).collect();
        assert_eq!(tokens, [1, 2, 3, 7, 8]);
        let three = &merged[2].item;
        assert_eq!(three.m_wc, 8);
        assert_eq!(three.m_n_n_0, 8);
        assert_eq!(three.m_n_1, 0);
        assert_eq!(three.m_mr, 9);
    }

    /// A short target row with a whole source row: the pin loads both
    /// (`:142-143`) and asserts at `check_result(target->get_array_header(…))`
    /// (`:156`).
    #[test]
    fn a_short_target_row_aborts_at_the_targets_header() {
        use crate::database::tests::{SHORT, TOKEN, WHOLE, assert_aborts, child_dir, plant};
        if let Some(dir) = child_dir() {
            let target = plant(&dir, "target.db", &[(TOKEN, SHORT)]);
            let source = plant(&dir, "source.db", &[(TOKEN, WHOLE)]);
            let _ = super::merge_array_items(
                &crate::database::Database::open_read_write(Some(&target)),
                &crate::database::Database::open_read_only(Some(&source)),
            );
            return;
        }
        assert_aborts("merge::tests::a_short_target_row_aborts_at_the_targets_header");
    }

    /// A short source row that the target lacks: the pin stores the gram it
    /// loaded, its bytes unchanged (`:142-146`), and the merge goes on.
    #[test]
    fn a_short_source_row_the_target_lacks_is_copied_unchanged() {
        use crate::database::tests::{SHORT, TOKEN, plant};
        use oxpinyin_store::{
            AttachMode, DefaultStore, HashAttach, MAGIC_KEY, RawReadStore as _, WriteStore as _,
        };
        let dir = tempfile::tempdir().expect("scratch directory");
        let target = plant(dir.path(), "target.db", &[]);
        let source = plant(dir.path(), "source.db", &[(TOKEN, SHORT)]);
        {
            let target = crate::database::Database::open_read_write(Some(&target));
            let source = crate::database::Database::open_read_only(Some(&source));
            assert!(super::merge_array_items(&target, &source));
        }
        let HashAttach::Opened(db) = DefaultStore::attach_hash(
            &dir.path().join("target.db"),
            AttachMode::ReadOnly,
            (&MAGIC_KEY, b"KMMP"),
        )
        .expect("reopen the target") else {
            panic!("the target opens");
        };
        let mut value = Vec::new();
        assert!(db.get_raw(&TOKEN.to_ne_bytes(), &mut value).expect("read"));
        assert_eq!(value, SHORT);
    }
}
