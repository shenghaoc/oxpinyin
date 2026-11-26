//! A system library the session has unloaded has no sub-index.
//!
//! Basis: `074a2219 phrase_index.cpp:261-269,651-652`, `phrase_index.h:628-635`, `pinyin.cpp:589-591`.

use super::mapped_replay::{LIBRARY, Profile, header};
use crate::parser::ChewingKey;
use crate::user::store::{UserStore, UserStoreError};

/// A store whose library 1, a mapped chunk of four items, is unloaded.
fn unloaded(name: &str) -> (Profile, UserStore) {
    let profile = Profile::records(name, &[header(185, 185)]);
    let mut store = profile.open();
    store.reset_system_library(1);
    (profile, store)
}

fn keys(syllable: &str) -> Vec<crate::user::PinyinKey> {
    super::super::packed_to_pinyin_keys(&[ChewingKey::from_pinyin(syllable)
        .expect("key")
        .to_packed()])
    .expect("keys")
}

/// The facade total's extra and library 1's total.
fn totals(store: &UserStore) -> (u64, Option<u32>) {
    (
        store.unigram_total(),
        Some(crate::user::test_session::library_total(store, 1)),
    )
}

/// Basis: `pinyin.cpp:589-591` (`get_range` fails; nothing is added).
#[test]
fn an_import_into_an_unloaded_library_is_refused() {
    let (_profile, mut store) = unloaded("import");
    let before = totals(&store);
    assert!(matches!(
        store.add_system_phrase_in(1, None, &"四".into(), &keys("si"), Some(5)),
        Err(UserStoreError::InvalidPhrase)
    ));
    assert_eq!(totals(&store), before);
}

/// Basis: `phrase_index.h:628-635` (no sub-index, no item; a loaded library takes the reading).
#[test]
fn a_new_reading_for_an_item_of_an_unloaded_library_is_refused() {
    let (profile, mut store) = unloaded("merge");
    assert!(matches!(
        store.add_system_phrase_in(1, Some(LIBRARY | 1), &"你".into(), &keys("li"), Some(5)),
        Err(UserStoreError::InvalidPhrase)
    ));

    let mut loaded = profile.open();
    assert_eq!(
        loaded
            .add_system_phrase_in(1, Some(LIBRARY | 1), &"你".into(), &keys("li"), Some(5))
            .expect("a loaded library takes the reading"),
        LIBRARY | 1
    );
}

/// Basis: `pinyin.cpp:2607-2616` (the refused add is not the overflow that answers false).
#[test]
fn a_predicted_prefix_in_an_unloaded_library_trains_nothing() {
    let (_profile, mut store) = unloaded("prefix");
    let before = totals(&store);
    store
        .observe_predicted_prefix(LIBRARY | 1)
        .expect("the add that finds no sub-index is not the overflow");
    assert_eq!(totals(&store), before);
}

/// Basis: `phrase_index.cpp:651-652` (`compact` skips a missing sub-index).
#[test]
fn compacting_leaves_an_unloaded_librarys_total_alone() {
    let (_profile, mut store) = unloaded("compact");
    let before = totals(&store);
    store.compact_libraries();
    assert_eq!(totals(&store), before);
}
