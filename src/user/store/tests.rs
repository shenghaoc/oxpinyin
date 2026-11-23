use super::*;

/// Whole fixture rows use the native DBM operation, with the session's
/// production bookkeeping so cached readers see the committed value.
pub(crate) fn seed_bigram(store: &mut UserStore, prev: u32, rows: &[(u32, u32)]) {
    let mut rows = rows.to_vec();
    rows.sort_unstable_by_key(|&(token, _)| token);
    let total = rows.iter().map(|&(_, count)| count).sum();
    store
        .container()
        .store(
            &prev.to_le_bytes(),
            &crate::single_gram::encode_single_gram(total, &rows),
        )
        .expect("store the fixture gram");
    store.mark_committed_write(true);
}

fn next_user_token(store: &UserStore) -> PhraseToken {
    let end = store
        .inner
        .libpinyin
        .phrases()
        .libraries
        .get(&USER_DICTIONARY)
        .map_or(1, crate::user::user_library::UserLibrary::token_end);
    crate::user::add_phrase_token(USER_DICTIONARY, end)
}
use std::collections::BTreeMap;

/// Basis: `074a2219 phrase_index.h:634` (facade before a refused add), `phrase_index.cpp:243-248` (reload).
#[test]
fn reload_preserves_the_delta_no_subindex_accepted() {
    use crate::user::{SystemLibrary, SystemOriginals, UserConfLaw};
    let originals = || {
        SystemOriginals::from(crate::user::LibraryTable::from([(
            1,
            SystemLibrary {
                mapped: None,
                total: u32::MAX,
                range_end: 2,
                items: BTreeMap::from([(
                    1,
                    crate::data::phrase_library::ChunkItem {
                        phrase: vec![0x4f60],
                        unigram: u32::MAX,
                        prons: Vec::new(),
                    },
                )]),
            },
        )]))
    };
    let mut store = UserStore::open_transient(
        originals(),
        crate::data::user_files::SystemVersions::for_this_build(7, 14),
        UserConfLaw::Pinyin,
    )
    .unwrap();
    assert_eq!(
        store.add_unigram_outcome(0x0100_0001, 7),
        UnigramAdd::Refused
    );
    assert_eq!(store.unigram_total(), 7);
    store.load_system_libraries(originals()).unwrap();
    assert_eq!(
        Some(crate::user::test_session::library_total(&store, 1)),
        Some(u32::MAX)
    );
    assert_eq!(store.unigram_total(), 7);
}

/// Basis: `phrase_index.cpp:56-91` (`add_pronunciation`: first match, running-total guard).
#[test]
fn system_import_changes_the_first_match_and_guards_its_prefix() {
    let mut item = crate::data::phrase_library::ChunkItem {
        phrase: vec![0x4f60],
        unigram: 0,
        prons: vec![(vec![1], u32::MAX - 10), (vec![2], 3), (vec![2], 5)],
    };
    let mut reading = SystemReading {
        packed: &[2],
        delta: 7,
    };
    let change = plan_system_reading(&item, &reading);
    validate_system_merge(&item, &reading, change).expect("valid item");
    apply_system_reading(&mut item, &reading, change);
    assert_eq!(item.prons[1].1, 10);
    assert_eq!(item.prons[2].1, 5);
    reading.delta = 1;
    let change = plan_system_reading(&item, &reading);
    apply_system_reading(&mut item, &reading, change);
    assert_eq!(item.prons[1].1, 10);
}

/// Sessions over `open_transient` (the pin's NULL user directory) or a user
/// directory. A unigram needs an item (`phrase_index.cpp:150-161`).
mod session {
    use super::super::*;
    use super::next_user_token;
    use crate::data::phrase_library::{ChunkItem, encode_phrase_item};
    use crate::data::user_files::{SystemVersions, UserFileLayout};
    use crate::user::FIRST_USER_TOKEN;
    use crate::user::{SystemLibrary, SystemOriginals, UserConfLaw};
    use crate::vocab::{
        ADDON_DICTIONARY, NETWORK_DICTIONARY, PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK,
        SENTENCE_START, SyllableKey, USER_DICTIONARY, phrase_index_make_token,
    };
    use std::collections::BTreeMap;

    fn versions() -> SystemVersions {
        SystemVersions::for_this_build(7, 14)
    }

    /// The stock layout with no system library.
    fn no_originals() -> SystemOriginals {
        SystemOriginals::new(Default::default(), UserFileLayout::stock())
    }

    fn transient() -> UserStore {
        UserStore::open_transient(no_originals(), versions(), UserConfLaw::Pinyin).unwrap()
    }

    /// A session over a user directory, for the tests that save.
    fn in_dir(originals: SystemOriginals) -> (tempfile::TempDir, UserStore) {
        let dir = tempfile::tempdir().unwrap();
        let store =
            crate::user::test_session::open(dir.path(), originals, versions(), UserConfLaw::Pinyin)
                .unwrap();
        (dir, store)
    }

    /// A user item with a zero unigram and one reading of count zero.
    fn bare_token(store: &mut UserStore, text: &str, keys: &[PinyinKey]) -> PhraseToken {
        store.add_phrase(&text.into(), keys, Some(0)).unwrap()
    }

    fn key(text: &str) -> PinyinKey {
        toned(text, 0)
    }

    fn toned(text: &str, tone: u8) -> PinyinKey {
        crate::user::toned_key(
            SyllableKey::from_text(text)
                .expect("frozen syllable")
                .index(),
            tone,
        )
        .expect("frozen syllable inventory fits u16")
    }

    /// One item at `slot` of `library`'s arena.
    fn plant_item(store: &UserStore, library: u8, slot: u32) {
        let bytes = encode_phrase_item(&ChunkItem {
            phrase: vec![0x7532],
            unigram: 0,
            prons: Vec::new(),
        })
        .unwrap();
        store
            .session_target()
            .phrases_mut()
            .libraries
            .get_or_default(library)
            .unwrap()
            .replace(slot, &bytes)
            .unwrap();
    }

    /// Basis: Rust contract of `count_delta`; seeds from `phonetic_lookup.h:892-894`.
    #[test]
    fn count_delta_reads_committed_writes() {
        let mut store = transient();
        let cur = bare_token(&mut store, "你", &[key("ni")]);
        let present = UserCountDelta {
            unigram_live: Some(0),
            ..UserCountDelta::ZERO
        };
        assert_eq!(store.count_delta(Some(1), cur).unwrap(), present);

        store.observe_selection(1, cur).unwrap();
        assert!(store.has_user_data());
        assert_eq!(
            store.count_delta(Some(1), cur).unwrap(),
            UserCountDelta {
                bigram_count: 69,
                bigram_total: 69,
                unigram_delta: 483,
                unigram_total_delta: 483,
                unigram_live: Some(483),
            }
        );
    }

    /// Basis: `074a2219 ngram_tkrzwdb.cpp:195-198` (a gram is stored back only when an item went).
    #[test]
    fn mask_out_leaves_an_itemless_gram_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let user = phrase_index_make_token(USER_DICTIONARY, 1);
        crate::user::native_fixture::craft_user_bigram(
            &dir.path().join("user_bigram.db"),
            &[
                (1_u32.to_le_bytes().to_vec(), 17_u32.to_le_bytes().to_vec()),
                (2_u32.to_le_bytes().to_vec(), 0_u32.to_le_bytes().to_vec()),
                (
                    3_u32.to_le_bytes().to_vec(),
                    [9_u32.to_le_bytes(), user.to_le_bytes(), 9_u32.to_le_bytes()].concat(),
                ),
            ],
        );
        assert!(
            crate::data::user_files::UserTableInfo::conform_to(&versions())
                .save(&dir.path().join("user.conf"))
        );
        let mut store = crate::user::test_session::open(
            dir.path(),
            no_originals(),
            versions(),
            UserConfLaw::Pinyin,
        )
        .unwrap();

        store
            .mask_out(
                PHRASE_INDEX_LIBRARY_MASK,
                phrase_index_make_token(USER_DICTIONARY, 0),
            )
            .unwrap();

        assert_eq!(
            store.bigram_row(1).unwrap(),
            Some(17_u32.to_le_bytes().to_vec())
        );
        assert_eq!(
            store.bigram_row(2).unwrap(),
            Some(0_u32.to_le_bytes().to_vec())
        );
        assert_eq!(store.bigram_row(3).unwrap(), None);
    }

    /// Basis: Rust contract (a save keeps the counts a cached read returned).
    #[test]
    fn save_compacts_after_a_cached_read() {
        let (_dir, mut store) = in_dir(no_originals());
        store.observe_selection(1, 100).unwrap();
        assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
        store.save_reporting().unwrap();
        assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
    }

    /// Basis: `user-store.md` §2 (seed added to the count and total, seed * 7 to the unigram).
    #[test]
    fn observe_applies_pinned_seed_sequence() {
        let mut store = transient();
        let cur = bare_token(&mut store, "你", &[key("ni")]);

        assert_eq!(store.observe_selection(1, cur).unwrap(), 69);
        assert_eq!(store.bigram_count(1, cur).unwrap(), 69);
        assert_eq!(store.bigram_total(1).unwrap(), 69);
        assert_eq!(store.unigram_delta(cur), 483);
        assert_eq!(store.unigram_total(), 483);

        assert_eq!(store.observe_selection(1, cur).unwrap(), 138);
        assert_eq!(store.bigram_count(1, cur).unwrap(), 207);
        assert_eq!(store.bigram_total(1).unwrap(), 207);
        assert_eq!(store.unigram_delta(cur), 483 + 966);
        assert_eq!(store.unigram_total(), 483 + 966);
        assert_eq!(
            store.count_delta(Some(1), cur).unwrap(),
            UserCountDelta {
                bigram_count: 207,
                bigram_total: 207,
                unigram_delta: 483 + 966,
                unigram_total_delta: 483 + 966,
                unigram_live: Some(483 + 966),
            }
        );
        assert_eq!(
            store.count_delta(None, cur).unwrap(),
            UserCountDelta {
                bigram_count: 0,
                bigram_total: 0,
                unigram_delta: 483 + 966,
                unigram_total_delta: 483 + 966,
                unigram_live: Some(483 + 966),
            }
        );
    }

    /// Basis: `user-store.md` §2 (`phonetic_lookup.h:884-903`: count and total per predecessor).
    #[test]
    fn totals_accumulate_per_predecessor() {
        let mut store = transient();
        assert_eq!(store.observe_selection(5, 10).unwrap(), 69);
        assert_eq!(store.observe_selection(5, 11).unwrap(), 69);
        assert_eq!(store.bigram_count(5, 10).unwrap(), 69);
        assert_eq!(store.bigram_count(5, 11).unwrap(), 69);
        assert_eq!(store.bigram_total(5).unwrap(), 138);
    }

    /// Basis: `user-store.md` §2 (`pinyin.cpp:2609-2639`: the predicted seed is always 69).
    #[test]
    fn predicted_path_is_flat_69() {
        let mut store = transient();
        let cur = bare_token(&mut store, "你", &[key("ni")]);
        assert_eq!(store.observe_predicted(1, cur).unwrap(), 69);
        assert_eq!(store.observe_predicted(1, cur).unwrap(), 69);
        assert_eq!(store.bigram_count(1, cur).unwrap(), 138);
        assert_eq!(store.bigram_total(1).unwrap(), 138);
        assert_eq!(store.unigram_delta(cur), 483 * 2);
    }

    /// Basis: `pinyin.cpp:2607-2616` (a prefix row adds 483 and returns before any bigram).
    #[test]
    fn predicted_prefix_invalidates_counts_without_training_a_bigram() {
        let mut store = transient();
        let cur = bare_token(&mut store, "你", &[key("ni")]);
        crate::user::seed_bigram(&mut store, 1, &[(cur, 10)]);
        assert_eq!(store.count_delta(Some(1), cur).unwrap().unigram_delta, 0);
        let generation = store.generation();
        for _ in 0..2 {
            assert_eq!(store.observe_predicted_prefix(cur).unwrap(), 69);
        }
        assert!(store.generation() > generation);
        assert_eq!(store.count_delta(Some(1), cur).unwrap().unigram_delta, 966);
        assert_eq!(store.unigram_total(), 966);
        assert_eq!(store.bigram_count(1, cur).unwrap(), 10);
        assert_eq!(store.bigram_total(1).unwrap(), 10);
        assert!(!store.is_modified());
    }

    /// Basis: `phrase_index.h:631-633` (the facade adds before the sub-index refuses).
    #[test]
    fn predicted_prefix_overflow_keeps_only_the_facade_total_increment() {
        let mut store = transient();
        let token = store
            .add_phrase(
                &"你好".into(),
                &[key("ni"), key("hao")],
                Some(1_431_655_631),
            )
            .unwrap();
        let before = store.unigram_delta(token);
        let total = store.unigram_total();
        let error = store.observe_predicted_prefix(token).unwrap_err();
        assert!(matches!(error, UserStoreError::UnigramTotalOverflow));
        assert_eq!(store.unigram_delta(token), before);
        assert_eq!(store.unigram_total(), total + 483);
        assert!(store.bigram_predecessors().unwrap().is_empty());
    }

    /// Basis: `user-store.md` §4 (`m_modified` is set only by `pinyin_train` and `pinyin_end_add_phrases`).
    #[test]
    fn dirty_gate_matches_m_modified_semantics() {
        let (_dir, mut store) = in_dir(no_originals());

        assert!(!store.is_modified());
        store.save_reporting().unwrap();
        store.save_reporting().unwrap();

        store.observe_selection(SENTENCE_START, 10).unwrap();
        assert!(store.is_modified());
        store.save_reporting().unwrap();
        assert!(!store.is_modified());
        store.save_reporting().unwrap();

        store.observe_predicted(10, 20).unwrap();
        assert!(!store.is_modified());
        store.save_reporting().unwrap();
        store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        assert!(!store.is_modified());
        store.save_reporting().unwrap();

        store.observe_selection(SENTENCE_START, 10).unwrap();
        assert!(store.is_modified());
        store.save_reporting().unwrap();
        assert!(!store.is_modified());
    }

    /// Basis: `phrase_index.cpp:640-644` (an empty sub-index ends at 1).
    #[test]
    fn first_allocation_is_first_user_token() {
        let mut store = transient();
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);
        let token = store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        assert_eq!(token, 0x0700_0001);
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN + 1);
    }

    /// Basis: `pinyin.cpp:587-601` (`_add_phrase` allocates in the target sub-index).
    #[test]
    fn network_and_user_can_share_phrase_text() {
        let mut store = transient();
        let user = store
            .add_phrase_in(USER_DICTIONARY, &"词".into(), &[key("ci")], Some(5))
            .unwrap();
        let net = store
            .add_phrase_in(NETWORK_DICTIONARY, &"词".into(), &[key("ci")], Some(5))
            .unwrap();
        assert_ne!(user, net);
        assert_eq!(crate::vocab::phrase_index_library_index(user), 7);
        assert_eq!(crate::vocab::phrase_index_library_index(net), 6);
        assert_eq!(
            crate::user::test_session::stored_phrase_rows(&store, USER_DICTIONARY).len(),
            1
        );
        assert_eq!(
            crate::user::test_session::stored_phrase_rows(&store, 6).len(),
            1
        );
        assert_eq!(next_user_token(&store), user + 1);
    }

    /// Basis: `pinyin.cpp:521-522` (default count 5, `guint32` `count * 3`).
    #[rstest::rstest]
    #[case::default(None, 15)]
    #[case::explicit(Some(10), 30)]
    #[case::wrapped_past_u32(Some(1_431_655_766), 2)]
    fn add_phrase_seeds_unigram_with_count_times_three(
        #[case] count: Option<u64>,
        #[case] unigram: u64,
    ) {
        let mut store = transient();
        let token = store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], count)
            .unwrap();
        assert_eq!(store.unigram_delta(token), unigram);
        assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 0);
    }

    /// Basis: `phrase_index.cpp:169-171` (overflow refused), `pinyin.cpp:2609-2612` (predicted path fails first).
    #[test]
    fn unigram_adds_stop_at_the_library_total() {
        let mut store = transient();
        let token = store
            .add_phrase(
                &"你好".into(),
                &[key("ni"), key("hao")],
                Some(1_431_655_631),
            )
            .unwrap();
        let full = store.unigram_delta(token);
        assert_eq!(full, 4_294_966_893);

        let err = store.observe_predicted(SENTENCE_START, token).unwrap_err();
        assert!(matches!(err, UserStoreError::UnigramTotalOverflow));
        assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 0);
        assert_eq!(store.unigram_delta(token), full);

        store.observe_selection(SENTENCE_START, token).unwrap();
        assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 69);
        assert_eq!(store.unigram_delta(token), full);
    }

    /// Basis: `pinyin.cpp:514-519` (`_add_phrase` takes one `ChewingKey` per character).
    #[test]
    fn invalid_phrase_is_rejected_without_allocation() {
        let mut store = transient();
        assert!(matches!(
            store.add_phrase(&"".into(), &[], None),
            Err(UserStoreError::InvalidPhrase)
        ));
        assert!(matches!(
            store.add_phrase(&"你好".into(), &[key("ni")], None),
            Err(UserStoreError::InvalidPhrase)
        ));
        assert!(SyllableKey::from_index(usize::from(crate::user::KEY_SYLLABLE_MASK)).is_none());
        assert!(matches!(
            store.add_phrase(
                &"你好".into(),
                &[key("ni"), crate::user::KEY_SYLLABLE_MASK],
                None
            ),
            Err(UserStoreError::InvalidPhrase)
        ));
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);
        assert!(store.token_for_phrase("你好").unwrap().is_none());
    }

    /// Basis: `phrase_index.cpp:180-206` (no item, no phrase).
    #[test]
    fn lookup_of_unknown_token_is_none() {
        let store = transient();
        assert!(store.phrase(FIRST_USER_TOKEN).is_none());
        assert!(store.phrase(0x0100_0001).is_none());
    }

    const SYSTEM_A: PhraseToken = 0x0100_0001;
    const SYSTEM_B: PhraseToken = 0x0200_0001;

    /// Libraries 1 and 2, each with the one item the mixed store trains.
    fn two_system_libraries() -> SystemOriginals {
        let library = |text: u32| SystemLibrary {
            total: 100,
            range_end: 2,
            mapped: None,
            items: BTreeMap::from([(
                1,
                ChunkItem {
                    phrase: vec![text],
                    unigram: 100,
                    prons: Vec::new(),
                },
            )]),
        };
        SystemOriginals::new(
            crate::user::LibraryTable::from([(1, library(0x4f60)), (2, library(0x597d))]),
            UserFileLayout::stock(),
        )
    }

    /// Two user phrases and four selections over them and two system
    /// items, in a session over a user directory.
    fn mixed_store() -> (tempfile::TempDir, UserStore, PhraseToken) {
        let (dir, mut store) = in_dir(two_system_libraries());
        let user_a = store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        let user_b = store
            .add_phrase(&"世界".into(), &[key("shi"), key("jie")], None)
            .unwrap();
        store.observe_selection(SYSTEM_A, SYSTEM_B).unwrap();
        store.observe_selection(SYSTEM_A, user_a).unwrap();
        store.observe_selection(user_a, SYSTEM_B).unwrap();
        store.observe_selection(user_a, user_b).unwrap();
        (dir, store, user_a)
    }

    /// Basis: `phrase_index.cpp:677-715` (facade mask), `ngram_bdb.cpp:218-253` (bigram mask).
    #[test]
    fn mask_out_user_clear_deletes_user_entries_and_keeps_system() {
        let (_dir, mut store, user_a) = mixed_store();
        assert!(store.is_modified());
        store.save_reporting().unwrap();

        store
            .mask_out(
                PHRASE_INDEX_LIBRARY_MASK,
                phrase_index_make_token(USER_DICTIONARY, 0),
            )
            .unwrap();

        assert!(store.phrase(user_a).is_none());
        assert!(store.token_for_phrase("你好").unwrap().is_none());
        assert_eq!(store.bigram_count(SYSTEM_A, SYSTEM_B).unwrap(), 69);
        assert_eq!(store.bigram_total(SYSTEM_A).unwrap(), 69);
        assert_eq!(store.bigram_count(user_a, SYSTEM_B).unwrap(), 0);
        assert_eq!(store.bigram_count(SYSTEM_A, user_a).unwrap(), 0);
        assert_eq!(store.bigram_count(user_a, user_a + 1).unwrap(), 0);
        // `phrase_index.h:628-635`: a system item's training lands in the item.
        assert_eq!(store.unigram_delta(SYSTEM_B), 0);
        assert_eq!(store.system_unigram(SYSTEM_B), Some(100 + 966));
        assert_eq!(store.unigram_delta(user_a), 0);
        assert_eq!(store.unigram_total(), 0);
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);
        assert!(!store.is_modified());
    }

    /// Basis: `pinyin.cpp:587-595`, `phrase_index.cpp:624-646` (the range ends past the highest live slot).
    #[test]
    fn next_user_token_follows_the_highest_live_token() {
        let mut store = transient();
        let a = store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        let b = store
            .add_phrase(&"世界".into(), &[key("shi"), key("jie")], None)
            .unwrap();
        let c = store
            .add_phrase(&"再见".into(), &[key("zai"), key("jian")], None)
            .unwrap();
        assert_eq!((b, c), (a + 1, a + 2));

        store.mask_out(0xFFFF_FFFF, b).unwrap();
        assert_eq!(next_user_token(&store), c + 1);

        store.mask_out(0xFFFF_FFFF, c).unwrap();
        assert_eq!(next_user_token(&store), a + 1);
        assert_eq!(
            store
                .add_phrase(&"好的".into(), &[key("hao"), key("de")], None)
                .unwrap(),
            a + 1
        );

        assert!(store.remove_user_phrase(a + 1).unwrap());
        assert_eq!(next_user_token(&store), a + 1);
        assert_eq!(
            store
                .add_phrase(&"再来".into(), &[key("zai"), key("lai")], None)
                .unwrap(),
            a + 1
        );
    }

    /// Basis: `pinyin.cpp:3726-3768` (`pinyin_remove_user_candidate`, no `m_modified`), `phrase_index.h:682-692`.
    #[test]
    fn remove_user_phrase_deletes_everywhere_and_rejects_others() {
        let (_dir, mut store, user_a) = mixed_store();
        store.save_reporting().unwrap();

        assert!(store.remove_user_phrase(user_a).unwrap());
        assert!(store.phrase(user_a).is_none());
        assert!(store.token_for_phrase("你好").unwrap().is_none());
        assert_eq!(store.bigram_count(user_a, SYSTEM_B).unwrap(), 0);
        assert_eq!(store.bigram_count(SYSTEM_A, user_a).unwrap(), 0);
        assert_eq!(store.bigram_count(user_a, user_a + 1).unwrap(), 0);
        assert_eq!(store.bigram_count(SYSTEM_A, SYSTEM_B).unwrap(), 69);
        assert_eq!(store.bigram_total(SYSTEM_A).unwrap(), 69);
        assert_eq!(store.bigram_total(user_a).unwrap(), 0);
        assert_eq!(store.unigram_delta(user_a), 0);
        assert_eq!(store.unigram_total(), 15 + 483);
        assert!(!store.is_modified());
        assert!(!store.remove_user_phrase(user_a).unwrap());
        assert!(!store.remove_user_phrase(SYSTEM_A).unwrap());
    }

    /// Basis: `ngram.cpp:86-99` (`SingleGram::mask_out`, `guint32` total), `ngram_bdb.cpp:229-246`.
    #[rstest::rstest]
    #[case::mask_subtracts(false, 17, [2, 3], 4, 13)]
    #[case::mask_reaches_zero(false, 1, [u32::MAX, 1], 1, 0)]
    #[case::mask_wraps(false, 0, [2, 3], 1, u32::MAX)]
    #[case::removal_wraps(true, 0, [2, 3], 1, u32::MAX)]
    fn bigram_removals_subtract_stored_totals_with_u32_wrap(
        #[case] remove_phrase: bool,
        #[case] total: u32,
        #[case] counts: [u32; 2],
        #[case] removed: u32,
        #[case] expected: u32,
    ) {
        let mut store = transient();
        let token = store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        let prev = SYSTEM_A;
        let kept = [0x0100_0002, 0x0100_0003];
        let gram = |total: u32, items: &[(PhraseToken, u32)]| {
            let mut bytes = total.to_le_bytes().to_vec();
            for (cur, count) in items {
                bytes.extend_from_slice(&cur.to_le_bytes());
                bytes.extend_from_slice(&count.to_le_bytes());
            }
            bytes
        };
        let planted = [
            (
                prev,
                gram(
                    total,
                    &[(kept[0], counts[0]), (kept[1], counts[1]), (token, removed)],
                ),
            ),
            (2, gram(17, &[(kept[0], 2)])),
            (token, gram(4, &[(kept[0], 4)])),
            (token | 0x8000_0000, gram(4, &[(kept[0], 4)])),
            (3, gram(0, &[(token, 0)])),
        ];
        for (p, value) in &planted {
            use crate::store::UserBigramDb as _;
            store
                .container()
                .store(&p.to_le_bytes(), value)
                .expect("store native fixture gram");
        }
        store.mark_committed_write(true);
        if remove_phrase {
            assert!(store.remove_user_phrase(token).unwrap());
        } else {
            store
                .mask_out(PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK, token)
                .unwrap();
        }
        assert_eq!(store.bigram_total(prev).unwrap(), u64::from(expected));
        assert_eq!(
            store.bigram_successors(prev).unwrap(),
            kept.into_iter()
                .zip(counts.map(u64::from))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            store.bigram_row(prev).unwrap(),
            Some(gram(
                expected,
                &[(kept[0], counts[0]), (kept[1], counts[1])]
            ))
        );
        assert_eq!(store.bigram_total(2).unwrap(), 17);
        for removed_prev in [token, token | 0x8000_0000, 3] {
            assert!(store.bigram_row(removed_prev).unwrap().is_none());
        }
    }

    /// Basis: `074a2219 pinyin.cpp:589-594`, `phrase_index.cpp:208-217,620` (no overflow check; wrap to slot 1).
    #[test]
    fn add_phrase_fills_the_last_slot_then_wraps_to_slot_1() {
        let mut store = transient();
        plant_item(&store, USER_DICTIONARY, PHRASE_MASK - 1);
        let last = phrase_index_make_token(USER_DICTIONARY, PHRASE_MASK);
        assert_eq!(next_user_token(&store), last);

        let token = store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        assert_eq!(token, last);
        assert_eq!(store.phrases().len(), 2);
        assert_eq!(
            store.user_library_token_end(USER_DICTIONARY).unwrap(),
            phrase_index_make_token(USER_DICTIONARY, 0)
        );
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);

        let wrapped = store
            .add_phrase(&"世界".into(), &[key("shi"), key("jie")], None)
            .unwrap();
        assert_eq!(wrapped, FIRST_USER_TOKEN);
        assert_eq!(store.phrases().len(), 3);
        assert_eq!(
            crate::user::test_session::library_total(&store, USER_DICTIONARY),
            30
        );

        let again = store
            .add_phrase(&"再见".into(), &[key("zai"), key("jian")], None)
            .unwrap();
        assert_eq!(again, FIRST_USER_TOKEN);
        assert_eq!(store.phrases().len(), 3);
        assert_eq!(
            store
                .phrase(FIRST_USER_TOKEN)
                .unwrap()
                .phrase_text()
                .unwrap(),
            "再见"
        );
        assert_eq!(store.unigram_delta(FIRST_USER_TOKEN), 15);
        assert_eq!(
            crate::user::test_session::library_total(&store, USER_DICTIONARY),
            45
        );
        assert_eq!(store.unigram_total(), 45);
    }

    /// Basis: `pinyin.cpp:2541-2545`, `phrase_index.cpp:208-217,620` (masked end, no bump; wrap to slot 0).
    #[test]
    fn promote_addon_phrase_fills_the_last_slot_then_wraps_to_slot_0() {
        let mut store = transient();
        plant_item(&store, ADDON_DICTIONARY, PHRASE_MASK - 1);
        let readings = [(vec![key("er"), key("huang")], 100)];
        let last = crate::user::test_session::promote(&mut store, "二簧", &readings, 100).unwrap();
        assert_eq!(last, phrase_index_make_token(ADDON_DICTIONARY, PHRASE_MASK));
        assert_eq!(
            store.user_library_token_end(ADDON_DICTIONARY).unwrap(),
            phrase_index_make_token(ADDON_DICTIONARY, 0)
        );

        let wrapped =
            crate::user::test_session::promote(&mut store, "二簧", &readings, 100).unwrap();
        assert_eq!(wrapped, phrase_index_make_token(ADDON_DICTIONARY, 0));
        assert_eq!(store.unigram_delta(wrapped), 100);
        assert_eq!(
            crate::user::test_session::library_total(&store, ADDON_DICTIONARY),
            200
        );

        let again = crate::user::test_session::promote(&mut store, "二簧", &readings, 100).unwrap();
        assert_eq!(again, wrapped);
        assert_eq!(store.unigram_delta(again), 100);
        assert_eq!(
            crate::user::test_session::library_total(&store, ADDON_DICTIONARY),
            300
        );
        assert_eq!(store.unigram_total(), 300);
    }

    /// Basis: `pinyin.cpp:2541-2558` (range end of library 5, item copied, no search).
    #[test]
    fn promote_addon_phrase_allocates_nibble_5_and_copies_frequency() {
        let mut store = transient();
        let keys = [toned("er", 2), toned("huang", 2)];

        let token =
            crate::user::test_session::promote(&mut store, "二簧", &[(keys.to_vec(), 100)], 100)
                .unwrap();
        assert_eq!(token, phrase_index_make_token(ADDON_DICTIONARY, 1));
        assert_eq!(store.unigram_delta(token), 100);
        assert_eq!(store.unigram_total(), 100);
        assert_eq!(
            store.token_for_phrase_in(ADDON_DICTIONARY, "二簧").unwrap(),
            Some(token)
        );
        let phrase = store.phrase(token).unwrap();
        assert_eq!(phrase.phrase_text().unwrap(), "二簧");
        assert_eq!(phrase.pronunciations().len(), 1);
        assert_eq!(phrase.pronunciations()[0].keys(), keys);
        assert_eq!(phrase.pronunciations()[0].count(), 100);

        let again =
            crate::user::test_session::promote(&mut store, "二簧", &[(keys.to_vec(), 100)], 100)
                .unwrap();
        assert_eq!(again, token + 1);
        assert_eq!(store.unigram_delta(token), 100);
        assert_eq!(store.unigram_delta(again), 100);
        assert_eq!(store.unigram_total(), 200);
        assert_eq!(
            store.phrase(token).unwrap().pronunciations()[0].count(),
            100
        );
        assert_eq!(
            store.phrase(again).unwrap().pronunciations()[0].count(),
            100
        );
    }

    /// The item bytes the store holds under `token`.
    fn stored_item(store: &UserStore, token: PhraseToken) -> Vec<u8> {
        store
            .with_user_item(token, |item| item.as_bytes().to_vec())
            .expect("promoted item")
    }

    /// Basis: `pinyin.cpp:2541-2558` (item bytes copied, each reading indexed).
    #[test]
    fn promote_addon_phrase_copies_the_item_bytes_and_indexes_each_reading() {
        let mut store = transient();
        let first = vec![toned("er", 2), toned("huang", 2)];
        let second = vec![toned("er", 4), toned("huang", 1)];
        let item = crate::user::test_session::addon_item(
            "二簧",
            &[(first.clone(), 7), (second.clone(), 3)],
            11,
        );

        let token = crate::user::test_session::promote_item(&mut store, &item).unwrap();
        assert_eq!(stored_item(&store, token), item);
        assert_eq!(store.unigram_delta(token), 11);
        let lookup = store.lookup();
        for keys in [&first, &second] {
            let syllables: Vec<SyllableKey> = keys
                .iter()
                .map(|key| SyllableKey::from_index(crate::user::key_syllable(*key)).unwrap())
                .collect();
            assert!(
                lookup
                    .lookup(&syllables)
                    .iter()
                    .any(|row| row.token() == token)
            );
        }
    }

    /// Basis: `pinyin.cpp:2541-2558` (the reading loop runs zero times).
    #[test]
    fn promote_addon_phrase_copies_an_item_with_no_reading() {
        let mut store = transient();
        let item = crate::user::test_session::addon_item("二簧", &[], 9);
        let token = crate::user::test_session::promote_item(&mut store, &item).unwrap();
        assert_eq!(stored_item(&store, token), item);
        assert_eq!(store.user_pronunciation_count(token), Some(0));
        assert_eq!(
            store.token_for_phrase_in(ADDON_DICTIONARY, "二簧").unwrap(),
            Some(token)
        );
        assert_eq!(store.unigram_delta(token), 9);
        assert_eq!(store.unigram_total(), 9);
    }

    /// Basis: `pinyin.cpp:2548` (words copied as `ucs4_t`, unvalidated).
    #[test]
    fn promote_addon_phrase_copies_a_word_that_is_not_a_scalar() {
        let mut store = transient();
        let item = crate::data::phrase_library::encode_phrase_item(
            &crate::data::phrase_library::ChunkItem {
                phrase: vec![0xD800, 0x4e8c],
                unigram: 4,
                prons: vec![(vec![0x0001, 0x0002], 4)],
            },
        )
        .unwrap();
        let token = crate::user::test_session::promote_item(&mut store, &item).unwrap();
        assert_eq!(stored_item(&store, token), item);
    }

    /// Basis: `pinyin.cpp:2548,2555` (arrays of `MAX_PHRASE_LENGTH`); 17 is undefined there and refused.
    #[test]
    fn promote_addon_phrase_takes_length_16_and_refuses_17() {
        let item = |length: usize| {
            crate::user::test_session::addon_item(
                &"啊".repeat(length),
                &[(vec![toned("a", 1); length], 5)],
                5,
            )
        };
        let mut store = transient();
        let item16 = item(16);
        let token = crate::user::test_session::promote_item(&mut store, &item16).unwrap();
        assert_eq!(stored_item(&store, token), item16);

        let mut store = transient();
        let err = crate::user::test_session::promote_item(&mut store, &item(17)).unwrap_err();
        assert!(matches!(err, UserStoreError::InvalidPhrase));
        assert!(!store.has_user_data());
    }

    /// Basis: Rust contract (a promotion reads a whole item or refuses).
    #[test]
    fn promote_addon_phrase_refuses_a_truncated_item() {
        let mut store = transient();
        let item =
            crate::user::test_session::addon_item("二簧", &[(vec![key("er"), key("huang")], 1)], 1);
        let err = store
            .promote_addon_phrase(
                store.addon_promotion_range_end().unwrap(),
                &item[..item.len() - 1],
            )
            .unwrap_err();
        assert!(matches!(err, UserStoreError::InvalidPhrase));
        assert!(!store.has_user_data());
    }
}

/// Basis: `phrase_index.cpp:169-175` (guarded sub-index total); the rejected saves keep it.
#[test]
fn reporting_save_guards_preserve_unigram_overflow() {
    use crate::data::user_files::SystemVersions;
    use crate::user::{SystemLibrary, SystemOriginals, UserConfLaw};
    fn originals() -> SystemOriginals {
        crate::user::LibraryTable::from([(
            1,
            SystemLibrary {
                total: 100,
                range_end: 2,
                mapped: None,
                items: BTreeMap::from([(
                    1,
                    crate::data::phrase_library::ChunkItem {
                        phrase: vec![0x4f60],
                        unigram: 100,
                        prons: Vec::new(),
                    },
                )]),
            },
        )])
        .into()
    }
    fn versions() -> SystemVersions {
        SystemVersions::for_this_build(7, 14)
    }
    for has_dir in [false, true] {
        for entry in 0..3 {
            let law = if (usize::from(has_dir) + entry) % 2 == 0 {
                UserConfLaw::Pinyin
            } else {
                UserConfLaw::Zhuyin
            };
            let scratch = tempfile::tempdir().expect("scratch");
            let dir = scratch.path();
            let mut store = if has_dir {
                crate::user::test_session::open(dir, originals(), versions(), law)
            } else {
                UserStore::open_transient(originals(), versions(), law)
            }
            .expect("open");
            store
                .inner
                .system_items
                .borrow_mut()
                .set_library_total(1, u32::MAX - 4);
            if !has_dir {
                store.mark_modified();
            }
            let token = 0x0100_0001;
            assert!(store.add_system_unigram_frequency(token, 2));
            let before = (
                store.system_unigram(token),
                store.unigram_total(),
                crate::user::test_session::library_total(&store, 1),
                store.is_modified(),
            );
            match entry {
                0 => store.save_reporting(),
                1 => store.save_reporting_compacted(&[]),
                _ => store.save_reporting_with_baseline(&[], |_, _| panic!("guarded baseline")),
            }
            .expect("guarded save");
            assert_eq!(
                (
                    store.system_unigram(token),
                    store.unigram_total(),
                    crate::user::test_session::library_total(&store, 1),
                    store.is_modified()
                ),
                before
            );
            assert!(!store.add_system_unigram_frequency(token, 4));
            assert_eq!(store.system_unigram(token), Some(102));
            assert_eq!(
                (store.unigram_total() as u32)
                    .wrapping_add(crate::user::test_session::library_total(&store, 1)),
                1
            );
            drop(store);
        }
    }
}

/// Basis: `phrase_index.cpp:150-176` (each add moves the item and its sub-index total).
#[test]
fn repeated_increments_move_the_live_unigram_and_total() {
    use crate::data::user_files::SystemVersions;
    use crate::user::{SystemLibrary, SystemOriginals, UserConfLaw};
    let item = crate::data::phrase_library::ChunkItem {
        phrase: vec![u32::from('你')],
        unigram: 0,
        prons: Vec::new(),
    };
    let originals = || -> SystemOriginals {
        crate::user::LibraryTable::from([(
            1,
            SystemLibrary {
                total: 0,
                range_end: 2,
                mapped: None,
                items: BTreeMap::from([(1, item.clone())]),
            },
        )])
        .into()
    };
    let mut store = UserStore::open_transient(
        originals(),
        SystemVersions::for_this_build(7, 14),
        UserConfLaw::Pinyin,
    )
    .unwrap();
    let token = 0x0100_0001;
    let mut last = 0;
    for _ in 0..2 {
        assert!(store.add_system_unigram_frequency(token, 1));
        let live = u64::from(
            store
                .count_delta(None, token)
                .unwrap()
                .unigram_live
                .unwrap(),
        );
        assert_eq!(live, last + 1);
        last = live;
    }
    assert_eq!(store.system_unigram(token), Some(2));
    assert_eq!(
        Some(crate::user::test_session::library_total(&store, 1)),
        Some(2)
    );
    assert_eq!(store.unigram_total(), 0);
    for _ in 0..2 {
        let counts = store.count_delta(None, token).unwrap();
        assert_eq!(counts.unigram_live, Some(2));
        assert_eq!(
            Some(crate::user::test_session::library_total(&store, 1)),
            Some(2)
        );
    }
}

mod backend_files {
    //! Ports of `libpinyin/tests/storage/test_ngram.cpp` and
    //! `libchewing/tests/test-userphrase.c`; see
    //! `docs/testing/upstream-test-coverage.md`.

    use std::path::Path;

    use crate::vocab::SyllableKey;

    use crate::data::user_files::{SystemVersions, UserFileLayout};

    use crate::user::{
        FIRST_USER_TOKEN, SystemOriginals, UserConfLaw, UserPhrase, UserStore, UserStoreError,
        toned_key,
    };
    use crate::vocab::{PhraseToken, SENTENCE_START};

    fn versions() -> SystemVersions {
        SystemVersions::for_this_build(7, 14)
    }

    fn no_originals() -> SystemOriginals {
        SystemOriginals::new(Default::default(), UserFileLayout::stock())
    }

    fn session() -> UserStore {
        UserStore::open_transient(no_originals(), versions(), UserConfLaw::Pinyin).unwrap()
    }

    /// A session over `dir` with the stock layout's three user libraries only.
    fn session_in(dir: &Path) -> UserStore {
        let system = crate::user::test_session::system_dir(&[]);
        std::fs::write(
            system.join("table.conf"),
            format!(
                "binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\
             source table format:pinyin\ndatabase format:{}\n\
             default ADDON_DICTIONARY NULL NULL addon.bin USER_FILE\n\
             default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE\n\
             default USER_DICTIONARY NULL NULL user.bin USER_FILE\n",
                versions().database_format
            ),
        )
        .unwrap();
        let conf = crate::data::TableConf::load_from_dir(&system).unwrap();
        crate::user::test_session::open_with_conf(dir, &system, &conf, UserConfLaw::Pinyin)
    }

    /// A user item with a zero unigram and one reading of count zero.
    fn bare_token(store: &mut UserStore, text: &str, reading: &str) -> PhraseToken {
        store
            .add_phrase(
                &crate::vocab::PhraseText::from(text),
                &keys(reading),
                Some(0),
            )
            .unwrap()
    }

    fn key(text: &str) -> crate::user::PinyinKey {
        crate::user::PinyinKey::try_from(
            SyllableKey::from_text(text)
                .expect("frozen syllable")
                .index(),
        )
        .expect("frozen syllable inventory fits u16")
    }

    fn keys(text: &str) -> Vec<crate::user::PinyinKey> {
        text.split(',').map(key).collect()
    }

    /// Basis: `test_ngram.cpp` store and reopen; `pinyin.cpp:399-401` loads `user_bigram.db`.
    #[test]
    fn a_bigram_table_roundtrips_through_the_store_file() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (FIRST_USER_TOKEN, FIRST_USER_TOKEN + 1);
        {
            let mut store = session_in(dir.path());
            crate::user::seed_bigram(&mut store, a, &[(b, 16)]);
            let (count_before, total_before) = (
                store.bigram_count(a, b).unwrap(),
                store.bigram_total(a).unwrap(),
            );
            store.observe_selection(a, b).unwrap();
            assert!(store.bigram_count(a, b).unwrap() > count_before);
            assert_eq!(
                store.bigram_total(a).unwrap(),
                total_before + (store.bigram_count(a, b).unwrap() - count_before)
            );
            store.save_reporting().unwrap();
        }

        let (seeded_count, seeded_total) = {
            let mut store = session_in(dir.path());
            let counts = (
                store.bigram_count(a, b).unwrap(),
                store.bigram_total(a).unwrap(),
            );
            store.save_reporting().unwrap();
            counts
        };
        let store = session_in(dir.path());
        assert_eq!(store.bigram_count(a, b).unwrap(), seeded_count);
        assert_eq!(store.bigram_total(a).unwrap(), seeded_total);
        assert!(seeded_count > 16);
    }

    /// Basis: `test_ngram.cpp` `mask_out(0x0, 0x0)`; `phrase_index.cpp:713-715` (facade total).
    #[test]
    fn mask_out_zero_clears_every_recorded_item() {
        let mut store = session();
        let a = bare_token(&mut store, "甲", "jia");
        let b = bare_token(&mut store, "乙", "yi");

        crate::user::seed_bigram(&mut store, a, &[(b, 4)]);
        store.observe_selection(SENTENCE_START, a).unwrap();
        store.observe_selection(a, b).unwrap();
        assert!(store.bigram_total(a).unwrap() > 0);
        assert!(store.unigram_total() > 0);

        store.mask_out(0, 0).unwrap();
        assert_eq!(store.bigram_count(a, b).unwrap(), 0);
        assert_eq!(store.bigram_total(a).unwrap(), 0);
        assert_eq!(store.unigram_total(), 0);
    }

    fn user_phrase(phrase: &str, reading: &str) -> (String, Vec<crate::user::PinyinKey>) {
        (phrase.to_owned(), keys(reading))
    }

    /// Basis: `test-userphrase.c` add, remove and reopen; `pinyin.cpp:658` arms the save.
    #[test]
    fn add_lookup_remove_roundtrip_and_persists_across_handles() {
        let dir = tempfile::tempdir().unwrap();
        let (text, reading) = user_phrase("测试", "ce,shi");

        let token = {
            let mut store = session_in(dir.path());
            assert!(store.token_for_phrase(&text).unwrap().is_none());
            let token = store
                .add_phrase(&crate::vocab::PhraseText::from(&text), &reading, None)
                .unwrap();
            assert_eq!(store.token_for_phrase(&text).unwrap(), Some(token));
            store.mark_modified();
            store.save_reporting().unwrap();
            token
        };

        {
            let mut store = session_in(dir.path());
            let phrase = store.phrase(token).expect("persisted");
            assert_eq!(phrase.phrase_text().unwrap(), "测试");
            assert_eq!(phrase.pronunciations().len(), 1);

            assert!(store.remove_user_phrase(token).unwrap());
            assert!(store.token_for_phrase(&text).unwrap().is_none());
            store.mark_modified();
            store.save_reporting().unwrap();
        }

        let store = session_in(dir.path());
        assert!(store.token_for_phrase(&text).unwrap().is_none());
        assert!(store.phrase(token).is_none());
    }

    /// Basis: `phrase_index.cpp:56-91` (readings accumulate); `pinyin.cpp:569-582,3759` (a merged
    /// reading has no index row and the pin asserts on its removal; the store refuses).
    #[test]
    fn one_phrase_holds_several_readings_and_counts_accumulate_per_reading() {
        let mut store = session();

        let text = "重";
        let zhong = keys("zhong");
        let chong = keys("chong");
        let token = store
            .add_phrase(&crate::vocab::PhraseText::from(text), &zhong, Some(7))
            .unwrap();
        store
            .add_phrase(&crate::vocab::PhraseText::from(text), &chong, Some(9))
            .unwrap();

        let phrase: UserPhrase = store.phrase(token).expect("stored");
        assert_eq!(phrase.pronunciations().len(), 2);

        store
            .add_phrase(&crate::vocab::PhraseText::from(text), &zhong, Some(3))
            .unwrap();
        let phrase = store.phrase(token).expect("stored");
        let zhong_count = phrase
            .pronunciations()
            .iter()
            .find(|p| p.keys() == zhong.as_slice())
            .expect("zhong reading")
            .count();
        assert_eq!(zhong_count, 10);
        assert!(matches!(
            store.remove_user_phrase(token),
            Err(UserStoreError::UserReadingRemoval)
        ));
        assert!(store.phrase(token).is_some());
    }

    /// Basis: `chewing_large_table2.cpp:200-219` (one key in both spaces), `pinyin.cpp:3759` (the assert).
    #[test]
    fn an_initial_only_reading_answers_the_pins_reading_removal_assert() {
        let mut store = session();
        let reading = [key("d")];
        let token = store
            .add_phrase(&crate::vocab::PhraseText::from("词"), &reading, Some(50))
            .unwrap();
        let phrase = store.phrase(token).expect("stored");
        assert!(phrase.pronunciations()[0].indexed());
        assert!(matches!(
            store.remove_user_phrase(token),
            Err(UserStoreError::UserReadingRemoval)
        ));
        assert!(store.phrase(token).is_some());
    }

    /// Basis: `pinyin.cpp:566-607` (only the first reading is indexed; a merge moves no unigram),
    /// `phrase_index.cpp:86-91` (readings append).
    #[test]
    fn readings_keep_insertion_order_tones_and_only_the_first_is_indexed() {
        let mut store = session();
        let ce = SyllableKey::from_text("ce").unwrap().index();
        let yan = SyllableKey::from_text("yan").unwrap().index();
        let toneless = [toned_key(ce, 0).unwrap(), toned_key(yan, 0).unwrap()];
        let toned = [toned_key(ce, 4).unwrap(), toned_key(yan, 4).unwrap()];

        let token = store.add_phrase(&"测验".into(), &toned, Some(7)).unwrap();
        store
            .add_phrase(&"测验".into(), &toneless, Some(5))
            .unwrap();
        store.add_phrase(&"测验".into(), &toned, Some(1)).unwrap();

        let phrase = store.phrase(token).expect("stored");
        let rows: Vec<(Option<String>, u64, bool)> = phrase
            .pronunciations()
            .iter()
            .map(|p| (p.render_pinyin(), p.count(), p.indexed()))
            .collect();
        assert_eq!(
            rows,
            vec![
                (Some("ce4'yan4".to_owned()), 8, true),
                (Some("ce'yan".to_owned()), 5, false),
            ]
        );
        assert_eq!(store.unigram_delta(token), 21);
    }

    /// Basis: `phrase_index.cpp:76-79` (the running `guint32` total must not wrap).
    #[test]
    fn a_reading_add_that_would_overflow_the_running_total_changes_nothing() {
        let mut store = session();
        let reading = keys("ce,shi");
        let token = store
            .add_phrase(&"测试".into(), &reading, Some(u64::from(u32::MAX - 1)))
            .unwrap();
        store.add_phrase(&"测试".into(), &reading, Some(2)).unwrap();
        let phrase = store.phrase(token).expect("stored");
        assert_eq!(phrase.pronunciations()[0].count(), u64::from(u32::MAX - 1));
        store.add_phrase(&"测试".into(), &reading, Some(1)).unwrap();
        let phrase = store.phrase(token).expect("stored");
        assert_eq!(phrase.pronunciations()[0].count(), u64::from(u32::MAX));
    }

    /// Basis: Rust contract (`MAX_PHRASE_LENGTH` 16 with a strict `<`; upstream's ceiling is 11).
    #[test]
    fn phrase_length_ceiling_rejects_sixteen_characters() {
        let mut store = session();

        let ok: String = "测".repeat(15);
        let token = store
            .add_phrase(&crate::vocab::PhraseText::from(&ok), &[key("ce"); 15], None)
            .unwrap();
        assert!(store.phrase(token).is_some());

        let too_long: String = "测".repeat(16);
        assert!(matches!(
            store.add_phrase(
                &crate::vocab::PhraseText::from(&too_long),
                &[key("ce"); 16],
                None
            ),
            Err(UserStoreError::InvalidPhrase)
        ));
    }
}

/// Basis: `074a2219 pinyin.cpp:655-660`, `phrase_index.cpp:180-206,624-675` (compact copies `[1, end)`).
#[test]
fn import_end_compaction_drops_the_reserved_slot() {
    use crate::data::chunk_format::build_memory_chunk;
    use crate::data::phrase_library::{ChunkItem, build_chunk};
    use crate::user::native_fixture::{TempUserDir, system_dir, write_user_conf};

    let user = TempUserDir::new("user-file-slot-zero");
    write_user_conf(&user.path, &system_dir());
    let item = ChunkItem {
        phrase: vec![u32::from('你')],
        unigram: 17,
        prons: vec![(vec![0x302b], 5)],
    };
    let chunk = build_chunk(&[(1, &item)]).expect("one-item user chunk");
    let mut payload = chunk[8..].to_vec();
    let offsets_start = u32::from_le_bytes(payload[4..8].try_into().expect("offset header"));
    let slot_zero = offsets_start as usize;
    payload.copy_within(slot_zero + 4..slot_zero + 8, slot_zero);
    payload[slot_zero + 4..slot_zero + 8].fill(0);
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");

    let mut store = crate::user::test_session::open_w3(&user.path);
    assert_eq!(
        crate::user::test_session::native_unigram(&store, 0x0700_0000),
        Some(17)
    );
    store.compact_libraries();
    assert_eq!(
        crate::user::test_session::native_unigram(&store, 0x0700_0000),
        None
    );
}

/// Basis: `ngram_bdb.cpp:231-238` (a selected predecessor is removed without loading its value).
#[test]
fn mask_out_of_a_selected_short_key_completes() {
    let user = tempfile::tempdir().unwrap();
    crate::user::native_fixture::plant_bigram(
        user.path(),
        &[(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
    );
    let mut store = crate::user::test_session::open_w3(user.path());
    store.mask_out(0xffff_ffff, 1).unwrap();
}

/// Basis: `074a2219 novel_types.h:41-44`, `phrase_index.cpp:677-715` (library nibble and slot bits only).
#[test]
fn mask_out_ignores_the_bits_above_the_library_index() {
    let user = tempfile::tempdir().unwrap();
    let mut store = crate::user::test_session::open_w3(user.path());
    store
        .add_phrase_in(
            7,
            &"测测".into(),
            &crate::user::test_session::native_keys("ce4'ce4"),
            Some(5),
        )
        .unwrap();
    store.compact_libraries();
    assert!(crate::user::test_session::native_unigram(&store, 0x0700_0001).is_some());
    store.mask_out(0x1000_0000, 0x1000_0000).unwrap();
    assert_eq!(
        crate::user::test_session::native_unigram(&store, 0x0700_0001),
        None
    );
}

/// Basis: `chewing_large_table2_tkrzwdb.cpp:430,448-466` (a key with no whole `ChewingKey` aborts).
#[test]
fn mask_out_aborts_on_an_empty_index_key() {
    let user = tempfile::tempdir().unwrap();
    crate::user::native_fixture::write_user_conf(
        user.path(),
        &crate::user::native_fixture::system_dir(),
    );
    crate::user::native_fixture::craft_user_index(
        &user.path().join("user_pinyin_index.bin"),
        &[(Vec::new(), Vec::new()), (vec![0, 1], Vec::new())],
    );
    let mut store = crate::user::test_session::open_w3(user.path());
    assert!(matches!(
        store.check_mask_out(0xffff_ffff, 0),
        Err(UserStoreError::OverlongIndexKey)
    ));
    assert!(matches!(
        store.mask_out(0xffff_ffff, 0),
        Err(UserStoreError::OverlongIndexKey)
    ));
}

/// Basis: `074a2219 phrase_index.cpp:169-175,345` (the total is guarded; the item's `guint32` wraps).
#[test]
fn user_unigram_item_wraps_when_its_sub_index_total_accepts_the_add() {
    use crate::data::chunk_format::{CHUNK_HEADER_SIZE, build_memory_chunk};
    use crate::data::phrase_library::{ChunkItem, build_chunk};
    use crate::user::native_fixture::TempUserDir;

    let user = TempUserDir::new("user-unigram-wrap");
    drop(crate::user::test_session::open_w3(&user.path));
    let chunk = build_chunk(&[(
        1,
        &ChunkItem {
            phrase: vec![u32::from('词')],
            unigram: u32::MAX - 2,
            prons: vec![],
        },
    )])
    .unwrap();
    let mut payload = chunk[CHUNK_HEADER_SIZE..].to_vec();
    payload[..4].copy_from_slice(&3_u32.to_le_bytes());
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&payload).unwrap(),
    )
    .unwrap();

    let mut store = crate::user::test_session::open_w3(&user.path);
    let token = crate::user::FIRST_USER_TOKEN;
    assert_eq!(
        store.user_unigram_frequency(token),
        Some(u64::from(u32::MAX - 2))
    );
    assert!(store.add_unigram_outcome(token, 4).is_accepted());
    assert_eq!(store.user_unigram_frequency(token), Some(1));
}

/// Basis: `074a2219 pinyin.cpp:2608-2616`, `phrase_index.h:629` (the tagged token masks to its item).
#[test]
fn a_prefix_row_under_high_bits_is_listed_and_trains_its_item() {
    let user = tempfile::tempdir().unwrap();
    let mut store = crate::user::test_session::open_w3(user.path());
    store
        .add_phrase_in(
            7,
            &"龘".into(),
            &crate::user::test_session::native_keys("da2"),
            Some(1),
        )
        .unwrap();
    let token = store
        .add_phrase_in(
            7,
            &"龘龘".into(),
            &crate::user::test_session::native_keys("da2'da2"),
            Some(1),
        )
        .unwrap();
    store.compact_libraries();
    store.mark_modified();
    store.save_reporting().unwrap();
    drop(store);
    crate::user::native_fixture::set_phrase_table_row(user.path(), "龘龘", &[token | 0x8000_0000]);
    let mut store = crate::user::test_session::open_w3(user.path());
    let base = store.user_unigram_frequency(token).unwrap();
    store.observe_predicted_prefix(token | 0x8000_0000).unwrap();
    assert_eq!(store.user_unigram_frequency(token), Some(base + 483));
}

/// Basis: `pinyin.cpp:2609-2639`, `phrase_index.h:629` (the gram keeps the tagged token; the unigram masks it).
#[test]
fn a_bigram_row_under_high_bits_is_listed_and_trains_its_item() {
    let user = tempfile::tempdir().unwrap();
    let mut store = crate::user::test_session::open_w3(user.path());
    let token = store
        .add_phrase_in(
            7,
            &"龘龘".into(),
            &crate::user::test_session::native_keys("da2'da2"),
            Some(5),
        )
        .unwrap();
    store.compact_libraries();
    store.mark_modified();
    store.save_reporting().unwrap();
    drop(store);
    let prefixes =
        crate::user::test_session::native_tokens(&crate::user::native_fixture::system_dir(), "你");
    let gram = crate::user::native_fixture::gram_bytes(20, &[(token | 0x8000_0000, 20)]);
    let rows: Vec<_> = prefixes
        .iter()
        .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
        .collect();
    crate::user::native_fixture::craft_user_bigram(&user.path().join("user_bigram.db"), &rows);
    crate::user::native_fixture::write_user_conf(
        user.path(),
        &crate::user::native_fixture::system_dir(),
    );
    let mut store = crate::user::test_session::open_w3(user.path());
    let base = store.user_unigram_frequency(token).unwrap();
    store
        .observe_predicted(prefixes[0], token | 0x8000_0000)
        .unwrap();
    assert_eq!(store.user_unigram_frequency(token), Some(base + 483));
    let trained: Vec<_> = prefixes
        .iter()
        .map(|prefix| store.bigram_successors(*prefix).unwrap())
        .filter(|items| *items != [(token | 0x8000_0000, 20)])
        .collect();
    assert_eq!(trained, [[(token | 0x8000_0000, 89)]]);
}

/// Basis: `phonetic_lookup.h:884-904` (the gram total refuses the seed; the unigram takes seed * 7).
#[test]
fn training_past_the_gram_total_limit_keeps_the_gram() {
    const COUNT: u32 = u32::MAX - 68;
    let user = tempfile::tempdir().unwrap();
    let system = crate::user::native_fixture::system_dir();
    let [ni, hao, zhong] =
        ["你", "好", "中"].map(|text| crate::user::test_session::native_tokens(&system, text)[0]);
    let gram = crate::user::native_fixture::gram_bytes(COUNT, &[(hao, COUNT)]);
    crate::user::native_fixture::plant_bigram(
        user.path(),
        &[
            (ni.to_le_bytes().to_vec(), gram.clone()),
            (zhong.to_le_bytes().to_vec(), gram),
            (1_u32.to_le_bytes().to_vec(), vec![0; 4]),
        ],
    );
    let mut store = crate::user::test_session::open_w3(user.path());
    let before = store.system_unigram(hao).unwrap();
    let mut check = TrainingCheck::default();
    store.check_training_step_with(&mut check, ni, hao).unwrap();
    let cached = crate::user::native_fixture::gram_bytes(COUNT, &[(hao, COUNT)]);
    // A missing token would be inserted before the pin skips its store.
    // Repeated checked steps must retain the original row on that refusal.
    store
        .check_training_step_with(&mut check, ni, zhong)
        .unwrap();
    assert_eq!(check.grams.get(&ni), Some(&cached));
    store.check_training_step_with(&mut check, ni, hao).unwrap();
    assert_eq!(check.grams.get(&ni), Some(&cached));
    assert_eq!(check.fault(), None);
    store.observe_selection(1, ni).unwrap();
    store.observe_selection(ni, hao).unwrap();
    assert_eq!(store.system_unigram(hao), Some(before + 154_560));
    store.save_reporting().unwrap();
    drop(store);
    let stored = crate::user::native_fixture::store_rows(&user.path().join("user_bigram.db"), true)
        .into_iter()
        .find(|(key, _)| *key == ni.to_le_bytes())
        .unwrap()
        .1;
    assert_eq!(
        stored,
        crate::user::native_fixture::gram_bytes(COUNT, &[(hao, COUNT)])
    );
    let store = crate::user::test_session::open_w3(user.path());
    assert_eq!(store.system_unigram(hao), Some(before + 154_560));
}
