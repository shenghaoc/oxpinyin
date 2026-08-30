//! `validate_k_mixture_model` — internal-consistency checks
//! (`utils/training/validate_k_mixture_model.cpp:28-139`).
//!
//! Returns `Ok(())` for a valid model and `Err(KmmError::Invalid)` for the
//! first violated invariant (upstream collects all violations on stderr but
//! the exit outcome is identical: pass or fail).

use crate::error::KmmError;
use crate::model::KMixtureModel;

/// Validates the magic-header/array-header consistency and the per-row item
/// sums.
///
/// # Errors
///
/// Returns [`KmmError::Invalid`] when any invariant fails.
pub fn validate(model: &KMixtureModel) -> Result<(), KmmError> {
    validate_unigram(model)?;
    validate_bigram(model)?;
    Ok(())
}

/// `validate_unigram` (`:28-80`).
fn validate_unigram(model: &KMixtureModel) -> Result<(), KmmError> {
    if model.wc == 0 {
        return Err(invalid("word count in magic header is unexpected zero"));
    }
    if model.total_freq == 0 {
        return Err(invalid("total freq in magic header is unexpected zero"));
    }
    if model.wc != model.total_freq {
        return Err(invalid("the word count doesn't match the total freq"));
    }

    let mut word_count: u64 = 0;
    let mut total_freq: u64 = 0;
    for gram in model.grams.values() {
        word_count += u64::from(gram.header_wc);
        total_freq += u64::from(gram.header_freq);
    }
    if word_count != u64::from(model.wc) {
        return Err(invalid(
            "sum of array-header word counts differs from magic word count",
        ));
    }
    if total_freq != u64::from(model.total_freq) {
        return Err(invalid(
            "sum of array-header freqs differs from magic total freq",
        ));
    }
    Ok(())
}

/// `validate_bigram` (`:82-139`).
fn validate_bigram(model: &KMixtureModel) -> Result<(), KmmError> {
    for (token, gram) in &model.grams {
        let expected = gram.header_wc;
        if expected == 0 {
            if !gram.items.is_empty() {
                return Err(invalid(&format!(
                    "token {token}: header word count is zero but it has array items"
                )));
            }
            if gram.header_freq == 0 {
                return Err(invalid(&format!(
                    "token {token}: both word count and freq are unexpected zero"
                )));
            }
            // freq-only header, no items: valid.
            continue;
        }
        let sum = gram.items_wc_sum();
        if sum != u64::from(expected) {
            return Err(invalid(&format!(
                "token {token}: sum of item word counts ({sum}) differs from header ({expected})"
            )));
        }
    }
    Ok(())
}

fn invalid(detail: &str) -> KmmError {
    KmmError::Invalid {
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::generate::GenerateParams;
    use crate::model::KMixtureModel;

    fn good_model() -> KMixtureModel {
        let mut model = KMixtureModel::new();
        model
            .add_document("10 甲\n20 乙\n10 甲\n20 乙\n", GenerateParams::default())
            .expect("count");
        model
    }

    #[test]
    fn a_generated_model_validates() {
        validate(&good_model()).expect("valid");
    }

    #[test]
    fn mismatched_magic_word_count_fails() {
        let mut model = good_model();
        model.wc += 1;
        assert!(validate(&model).is_err());
    }

    #[test]
    fn zero_headers_fail() {
        let model = KMixtureModel::new();
        assert!(validate(&model).is_err());
    }

    #[test]
    fn tampered_item_sum_fails() {
        let mut model = good_model();
        // Corrupt one item's wc without fixing the header.
        if let Some(gram) = model.grams.get_mut(&10)
            && let Some(item) = gram.items.get_mut(&20)
        {
            item.wc += 5;
        }
        assert!(validate(&model).is_err());
    }
}
