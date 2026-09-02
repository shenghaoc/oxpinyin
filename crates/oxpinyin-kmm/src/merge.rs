//! `merge_k_mixture_model` — fold one model into another
//! (`utils/training/merge_k_mixture_model.cpp:38-238`).
//!
//! Per matching `(W1, W2)`: `m_WC`, `m_N_n_0`, `m_n_1` sum and `m_Mr` takes
//! the max (`merge_two_phrase_array`, `:59-74`); array and magic headers sum
//! field-wise, `m_N` included (`:96-129`). The token-ordered maps make the
//! merge-join trivial. Merging into a fresh model then folding the sorted
//! candidates reproduces `merge_k_mixture_model --result-file`.

use crate::error::KmmError;
use crate::model::{KMixtureModel, SingleGram};
use crate::text::merge_texts;

/// Merges `new_one` into `target` (`merge_two_k_mixture_model`, `:202-208`:
/// array items first, then the magic header).
///
/// # Errors
///
/// Returns [`KmmError::Invalid`] when the magic-header word count or total
/// freq would overflow `u32` (upstream `EOVERFLOW`).
pub fn merge_into(target: &mut KMixtureModel, new_one: &KMixtureModel) -> Result<(), KmmError> {
    // merge_magic_header (`:96-129`): overflow-guarded sums, computed before
    // any row is touched so an overflow leaves `target` unmodified (upstream
    // has already written the rows when it reports EOVERFLOW; a caller that
    // keeps the model after the error would otherwise hold a half-merge).
    let merged_wc = checked_sum(target.wc, new_one.wc, "magic word count")?;
    let merged_total_freq = checked_sum(target.total_freq, new_one.total_freq, "magic total freq")?;

    // merge_array_items (`:131-200`).
    for (&token1, new_gram) in &new_one.grams {
        match target.grams.get_mut(&token1) {
            Some(target_gram) => merge_single_gram(target_gram, new_gram),
            None => {
                target.grams.insert(token1, new_gram.clone());
            }
        }
    }

    target.wc = merged_wc;
    target.total_freq = merged_total_freq;
    target.n = target.n.wrapping_add(new_one.n);

    merge_texts(&mut target.texts, &new_one.texts);
    Ok(())
}

/// Merge one `W1` row: headers sum, items merge-join by `token2`.
fn merge_single_gram(target: &mut SingleGram, new: &SingleGram) {
    target.header_wc = target.header_wc.wrapping_add(new.header_wc);
    target.header_freq = target.header_freq.wrapping_add(new.header_freq);
    for (&token2, new_item) in &new.items {
        match target.items.get_mut(&token2) {
            Some(item) => {
                item.wc = item.wc.wrapping_add(new_item.wc);
                item.n_n_0 = item.n_n_0.wrapping_add(new_item.n_n_0);
                item.n_1 = item.n_1.wrapping_add(new_item.n_1);
                item.mr = item.mr.max(new_item.mr);
            }
            None => {
                target.items.insert(token2, *new_item);
            }
        }
    }
}

/// `a + b` with the `a + b < max(a, b)` overflow guard (`:108-118`).
fn checked_sum(a: u32, b: u32, what: &str) -> Result<u32, KmmError> {
    a.checked_add(b).ok_or_else(|| KmmError::Invalid {
        detail: format!("the {what} integer overflows"),
    })
}

#[cfg(test)]
mod tests {
    use super::merge_into;
    use crate::generate::GenerateParams;
    use crate::model::{ArrayItem, KMixtureModel};
    use crate::validate::validate;

    fn model_from(doc: &str) -> KMixtureModel {
        let mut model = KMixtureModel::new();
        model
            .add_document(doc, GenerateParams::default())
            .expect("count");
        model
    }

    #[test]
    fn merging_two_single_document_models_sums_and_maxes() {
        // Same document in two candidate models: merging equals training it
        // twice (two documents), so document frequency doubles and Mr maxes.
        // The document cycles 甲→乙→甲 so both tokens appear as a W1 (each has
        // a stored array header); a W2-only token would get no header under
        // the pin's Tkrzw backend and the model would then fail `validate`
        // (see `generate::tests::a_token2_only_token_gets_no_array_header`).
        let a = model_from("10 甲\n20 乙\n10 甲\n");
        let b = model_from("10 甲\n20 乙\n10 甲\n");
        let mut merged = a.clone();
        merge_into(&mut merged, &b).expect("merge");

        assert_eq!(merged.n, 2);
        assert_eq!(
            merged.grams[&10].items[&20],
            ArrayItem {
                wc: 2,
                n_n_0: 2,
                n_1: 2,
                mr: 1
            }
        );
        // Magic invariants hold after merge (a complete model validates).
        validate(&merged).expect("merged model validates");
    }

    #[test]
    fn merge_equals_single_run_over_both_documents() {
        // Merging per-document candidates must equal counting both docs in one
        // model (the crux of the candidate-merge stage). The invariant holds
        // when every token appears as a W1 in each document it occurs in — a
        // token that is W2-only in one candidate but W1 in another breaks it
        // (its unigram freq is stored in the combined run but not in the
        // per-candidate merge — exactly as the pin's Tkrzw gen behaves). The
        // cyclic documents below keep every token a W1, so the invariant holds.
        let da = "10 甲\n20 乙\n30 丙\n10 甲\n";
        let db = "20 乙\n30 丙\n10 甲\n20 乙\n";
        let a = model_from(da);
        let b = model_from(db);
        let mut merged = a.clone();
        merge_into(&mut merged, &b).expect("merge");

        let mut combined = KMixtureModel::new();
        combined
            .add_document(da, GenerateParams::default())
            .expect("d1");
        combined
            .add_document(db, GenerateParams::default())
            .expect("d2");

        assert_eq!(merged.grams, combined.grams);
        assert_eq!(merged.wc, combined.wc);
        assert_eq!(merged.n, combined.n);
        assert_eq!(merged.total_freq, combined.total_freq);
    }

    #[test]
    fn a_header_overflow_leaves_the_target_untouched() {
        let mut target = model_from("10 甲\n20 乙\n10 甲\n");
        target.wc = u32::MAX;
        let before = target.clone();
        let new_one = model_from("30 丙\n40 丁\n");
        let error = merge_into(&mut target, &new_one).expect_err("overflow");
        assert!(matches!(error, crate::error::KmmError::Invalid { .. }));
        assert_eq!(target, before, "no row is merged when the header overflows");
    }

    #[test]
    fn distinct_rows_are_unioned() {
        let a = model_from("10 甲\n20 乙\n");
        let b = model_from("30 丙\n40 丁\n");
        let mut merged = a.clone();
        merge_into(&mut merged, &b).expect("merge");
        assert!(merged.grams.contains_key(&10));
        assert!(merged.grams.contains_key(&30));
        assert_eq!(merged.text(40), Some("丁"));
    }
}
