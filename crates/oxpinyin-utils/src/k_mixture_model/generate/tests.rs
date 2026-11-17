use super::{GenerateParams, train_second_word};
use crate::k_mixture_model::database::Database;
use crate::k_mixture_model::database::tests::{
    SHORT, TOKEN, assert_short_single_gram_exit, child_dir, plant,
};
use oxpinyin_input::tool::DirectHashTable;

/// A first word whose stored row is shorter than the array header: the
/// pin's `load` succeeds (`:252`) and `train_single_gram` reads the
/// header outside the record (`:220`, class (b)), so the tool takes the
/// store-error exit.
#[test]
fn a_short_first_word_row_takes_the_store_error_exit() {
    if let Some(dir) = child_dir() {
        let model = plant(&dir, "model.db", &[(TOKEN, SHORT)]);
        let bigram = Database::open_read_write(Some(&model));
        let mut pairs = DirectHashTable::new();
        pairs.insert(TOKEN, 0);
        let mut second = DirectHashTable::new();
        second.insert(TOKEN + 1, 1);
        let mut unigram = DirectHashTable::new();
        unigram.insert(TOKEN + 1, 1);
        let _ = train_second_word(
            &mut unigram,
            &bigram,
            &pairs,
            &[second],
            TOKEN,
            GenerateParams::default(),
        );
        return;
    }
    assert_short_single_gram_exit(
        "k_mixture_model::generate::tests::a_short_first_word_row_takes_the_store_error_exit",
    );
}
