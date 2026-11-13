//! A system library the session has unloaded has no sub-index.
//!
//! The pin's `unload` deletes it (`phrase_index.cpp:261-269`). A write that
//! names it answers an error before it moves anything
//! (`phrase_index.h:628-635`), `_add_phrase` finds no range to add at
//! (`pinyin.cpp:589-591`), and `compact` skips it (`phrase_index.cpp:651-652`).
//! The session's baseline for the library has given its mapping to a live
//! buffer, and the unload dropped that buffer: a write that made a buffer from
//! the baseline again would read arrays the baseline no longer holds.
//!
//! Each test unloads the library of a mapped chunk through the session's own
//! `reset_system_library` and calls the store as the C ABI does.

use oxpinyin_core::ChewingKey;

use super::provenance::{LIBRARY, Profile, header};
use crate::store::{SENTENCE_START, UserStore, UserStoreError};

/// A store whose library 1 holds the shipped chunk's four items, and which has
/// unloaded it.
fn unloaded(name: &str) -> (Profile, UserStore) {
    let profile = Profile::records(name, &[header(185, 185)]);
    let mut store = profile.open();
    store.reset_system_library(1);
    (profile, store)
}

fn keys(syllable: &str) -> Vec<crate::phrase::PinyinKey> {
    super::packed_to_pinyin_keys(&[ChewingKey::from_pinyin(syllable).expect("key").to_packed()])
        .expect("keys")
}

/// What an unloaded library must leave alone: the facade total's extra and
/// the library's own total.
fn totals(store: &UserStore) -> (u64, Option<u32>) {
    (
        store.unigram_total().expect("facade extra"),
        Some(crate::test_session::library_total(store, 1)),
    )
}

/// `add_unigram_frequency` answers `ERROR_NO_SUB_PHRASE_INDEX` before
/// `m_total_freq += delta` (`phrase_index.h:631-633`).
#[test]
fn an_increment_into_an_unloaded_library_moves_nothing() {
    let (_profile, mut store) = unloaded("increment");
    let before = totals(&store);
    assert!(
        !store
            .add_system_unigram_frequency(LIBRARY | 1, 3)
            .expect("answer")
    );
    assert_eq!(totals(&store), before);
    assert!(!store.system_provenance_recorded(1));
}

/// `_add_phrase` into a library without a sub-index: `get_range` fails, and the
/// call answers `false` with nothing added (`pinyin.cpp:589-591`).
#[test]
fn an_import_into_an_unloaded_library_is_refused() {
    let (_profile, mut store) = unloaded("import");
    let before = totals(&store);
    assert!(matches!(
        store.add_system_phrase_in(1, None, &"四".into(), &keys("si"), Some(5)),
        Err(UserStoreError::InvalidPhrase)
    ));
    assert_eq!(totals(&store), before);
    assert!(!store.system_provenance_recorded(1));
}

/// A reading added to an item the library held: the item is looked up first
/// and the unloaded library answers none, so the refusal never reaches a
/// buffer. A loaded library takes the same call.
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

/// `train_result3` trains the bigram, ignores the failed `get_phrase_item` and
/// the failed `add_unigram_frequency`, and has no item to raise the readings
/// of (`phonetic_lookup.h:868-929`). The bigram is the one write that stays.
#[test]
fn training_an_unloaded_library_keeps_the_bigram_and_nothing_else() {
    let (_profile, mut store) = unloaded("train");
    let before = totals(&store);
    store
        .observe_selection(SENTENCE_START, LIBRARY | 1)
        .expect("the bigram trains");
    store
        .train_pronunciations(
            LIBRARY | 1,
            std::iter::once(vec![ChewingKey::from_pinyin("ni").expect("key")]),
            69,
        )
        .expect("the readings are left alone");
    assert_eq!(
        store
            .bigram_count(SENTENCE_START, LIBRARY | 1)
            .expect("count"),
        69
    );
    assert_eq!(totals(&store), before);
    assert!(!store.system_provenance_recorded(1));
}

/// A prefix row of the prediction adds 483 to its unigram and returns before
/// any bigram (`pinyin.cpp:2607-2616`); the add the unloaded library cannot
/// take is not the overflow that answers `false`.
#[test]
fn a_predicted_prefix_in_an_unloaded_library_trains_nothing() {
    let (_profile, mut store) = unloaded("prefix");
    let before = totals(&store);
    store
        .observe_predicted_prefix(LIBRARY | 1)
        .expect("the add that finds no sub-index is not the overflow");
    assert_eq!(totals(&store), before);
}

/// `FacadePhraseIndex::compact` skips a missing sub-index
/// (`phrase_index.cpp:651-652`): the library keeps its total, and none of it
/// moves into the facade's.
#[test]
fn compacting_leaves_an_unloaded_librarys_total_alone() {
    let (_profile, mut store) = unloaded("compact");
    let before = totals(&store);
    store.compact_libraries();
    assert_eq!(totals(&store), before);
}
