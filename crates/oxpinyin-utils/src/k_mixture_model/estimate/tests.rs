use crate::k_mixture_model::database::tests::{
    SHORT, TOKEN, WHOLE, assert_short_single_gram_exit, child_dir, plant,
};

/// A candidate row shorter than the array header for a token of the
/// deleted model: the pin's `load` succeeds (`:127`) and
/// `check_result(single_gram->get_array_header(…))` reads outside the
/// record (`:134`, class (b)), so the tool takes the store-error exit
/// instead of treating the row as missing.
#[test]
fn a_short_candidate_row_takes_the_store_error_exit() {
    if let Some(dir) = child_dir() {
        let bigram = plant(&dir, "bigram.db", &[(TOKEN, SHORT)]);
        let deleted = plant(&dir, "deleted.db", &[(TOKEN, WHOLE)]);
        super::run(Some(&bigram), Some(&deleted));
        return;
    }
    assert_short_single_gram_exit(
        "k_mixture_model::estimate::tests::a_short_candidate_row_takes_the_store_error_exit",
    );
}
