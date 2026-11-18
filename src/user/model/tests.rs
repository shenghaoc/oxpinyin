use crate::PhraseToken;

use super::*;

fn token(value: u32) -> PhraseToken {
    value
}

/// The pin's NULL-user-directory session over the stock layout.
fn transient() -> UserStore {
    UserStore::open_transient(
        crate::user::persistence::SystemOriginals::new(
            Default::default(),
            crate::data::user_files::UserFileLayout::stock(),
        ),
        crate::data::user_files::SystemVersions::for_this_build(7, 14),
        crate::user::persistence::UserConfLaw::Pinyin,
    )
    .unwrap()
}

#[test]
fn observe_uses_last_of_history_as_predecessor() {
    let mut store = transient();

    // history [5, 7] -> predecessor is 7.
    UserModel::observe(&mut store, &[token(5), token(7)], &token(42)).unwrap();
    assert_eq!(store.bigram_count(7, 42).unwrap(), 69);
    assert_eq!(store.bigram_count(5, 42).unwrap(), 0);

    // empty history -> sentence_start.
    UserModel::observe(&mut store, &[], &token(9)).unwrap();
    assert_eq!(store.bigram_count(SENTENCE_START, 9).unwrap(), 69);
}
