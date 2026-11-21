use super::*;

/// The exit status of a death-test child whose operation panicked. A
/// panic that reaches a non-unwinding `extern "C"` function aborts the
/// process, so without this status an unrelated panic passes as the
/// pin's abort.
const OPERATION_PANICKED: i32 = 86;

/// The variable that names the one test a child runs for real.
const DEATH_TEST: &str = "OXPINYIN_USER_DEATH_TEST";

/// Runs `setup` and then `operation` in a child copy of this test binary
/// that runs only the test `name`, and requires the child to abort.
/// `setup` may panic on purpose: the strict check is armed only after
/// it, and a panic in `operation` then fails with its own message.
fn assert_aborts_after_setup<T>(name: &str, setup: impl FnOnce() -> T, operation: impl FnOnce(T)) {
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let value = setup();
        // From here on, a panic writes its message to fd 2 (past
        // libtest's capture, which an abort discards) and exits with
        // `OPERATION_PANICKED`.
        std::panic::set_hook(Box::new(|info| {
            use std::io::Write as _;
            let _ = writeln!(std::io::stderr(), "death-test operation panicked: {info}");
            std::process::exit(OPERATION_PANICKED);
        }));
        operation(value);
        // Reached only when `operation` did not abort.
        std::process::exit(0);
    }

    let output = crate::user::test_child::run_exact(name, [(DEATH_TEST, name)]);
    assert_ne!(
        output.status.code(),
        Some(OPERATION_PANICKED),
        "{name}: the operation panicked: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(
            output.status.signal(),
            Some(6),
            "expected SIGABRT: {output:?}"
        );
    }
    #[cfg(windows)]
    assert_eq!(
        output.status.code(),
        Some(i32::from_ne_bytes(0xC000_0409_u32.to_ne_bytes())),
        "expected STATUS_STACK_BUFFER_OVERRUN: {output:?}"
    );
}

/// A panic that reaches this non-unwinding function aborts the process.
extern "C" fn panic_in_a_c_boundary() {
    panic!("an unrelated panic");
}

/// The witness: a death test whose operation panics at a C boundary
/// fails with its own message, not as the pin's abort.
#[test]
fn a_panic_at_the_c_boundary_is_not_the_pins_abort() {
    const NAME: &str = "user::store::tests::a_panic_at_the_c_boundary_is_not_the_pins_abort";
    let parent = std::panic::catch_unwind(|| {
        assert_aborts_after_setup(NAME, || (), |()| panic_in_a_c_boundary());
    });
    let message = parent.expect_err("a panic must not pass as an abort");
    let message = message
        .downcast_ref::<String>()
        .expect("the helper's assertion message");
    assert!(message.contains("the operation panicked"), "{message}");
    assert!(message.contains("an unrelated panic"), "{message}");
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

/// 074a2219 phrase_index.h:634 increments the facade before a refused
/// sub-index add; phrase_index.cpp:243-248 reload adjusts only sub-index totals.
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
    assert_eq!(
        item.prons[1].1, 10,
        "the prefix fits even though the whole total does not"
    );
    assert_eq!(item.prons[2].1, 5, "the later exact match stays untouched");
    reading.delta = 1;
    let change = plan_system_reading(&item, &reading);
    apply_system_reading(&mut item, &reading, change);
    assert_eq!(item.prons[1].1, 10, "overflow refuses the increment");
}

// ── The libpinyin session: the constructors the C libraries reach ──

/// The store the shipped libraries open: `open_transient` is the pin's
/// NULL-user-directory session, `test_session::open` a session over a user
/// directory. Every value operation runs on the arena, the pin's user
/// bigram container and the live system items; nothing here reads a
/// DBM row. A unigram belongs to an item, as the pin's does
/// (074a2219 `phrase_index.cpp:150-161`: `add_unigram_frequency` of a
/// token without an item is `ERROR_NO_ITEM`), so a test that trains a
/// count allocates its token first.
mod session {
    use super::super::*;
    use super::next_user_token;
    use crate::SENTENCE_START;
    use crate::SyllableKey;
    use crate::data::phrase_library::{ChunkItem, encode_phrase_item};
    use crate::data::user_files::{SystemVersions, UserFileLayout};
    use crate::user::FIRST_USER_TOKEN;
    use crate::user::{SystemLibrary, SystemOriginals, UserConfLaw};
    use crate::{
        ADDON_DICTIONARY, NETWORK_DICTIONARY, PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK,
        USER_DICTIONARY, phrase_index_make_token,
    };
    use std::collections::BTreeMap;

    fn versions() -> SystemVersions {
        SystemVersions::for_this_build(7, 14)
    }

    /// No system library: the stock layout names the user, network
    /// and addon `USER_FILE` libraries and nothing else.
    fn no_originals() -> SystemOriginals {
        SystemOriginals::new(Default::default(), UserFileLayout::stock())
    }

    /// The pin's NULL-user-directory session.
    fn transient() -> UserStore {
        UserStore::open_transient(no_originals(), versions(), UserConfLaw::Pinyin).unwrap()
    }

    /// A session over a user directory, for the tests that save: a
    /// transient session has nowhere to write, so its `save` answers
    /// `false` and leaves the dirty flag.
    fn in_dir(originals: SystemOriginals) -> (tempfile::TempDir, UserStore) {
        let dir = tempfile::tempdir().unwrap();
        let store =
            crate::user::test_session::open(dir.path(), originals, versions(), UserConfLaw::Pinyin)
                .unwrap();
        (dir, store)
    }

    /// A user item with a zero unigram and one reading of count zero:
    /// the token the pinned seed arithmetic is read on, bare.
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

    /// One item at `slot` of `library`'s arena, so its `get_range` end
    /// is one past it.
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

    #[test]
    fn count_delta_reads_committed_writes() {
        let mut store = transient();
        let cur = bare_token(&mut store, "你", &[key("ni")]);
        // The bare item is present with a zero unigram: the item read
        // succeeds, so the delta carries its field.
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

        store.observe_selection(1, cur).unwrap();
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
    }

    /// `Bigram::mask_out` (`ngram_tkrzwdb.cpp:195-198` at 074a2219)
    /// stores a gram back only when the mask removed an item. An
    /// item-less gram has nothing to remove, so it stays as it is,
    /// a zero total included.
    #[test]
    fn mask_out_leaves_an_itemless_gram_untouched() {
        let mut store = transient();
        let user = phrase_index_make_token(USER_DICTIONARY, 1);
        store.plant_raw_bigram(1, &17_u32.to_le_bytes());
        store.plant_raw_bigram(2, &0_u32.to_le_bytes());
        let emptied = [9_u32.to_le_bytes(), user.to_le_bytes(), 9_u32.to_le_bytes()].concat();
        store.plant_raw_bigram(3, &emptied);

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
        // A gram the mask empties goes.
        assert_eq!(store.bigram_row(3).unwrap(), None);
    }

    #[test]
    fn save_compacts_after_a_cached_read() {
        let (_dir, mut store) = in_dir(no_originals());
        store.observe_selection(1, 100).unwrap();
        assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
        store.save_reporting().unwrap();
        assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
    }

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

    #[test]
    fn totals_accumulate_per_predecessor() {
        let mut store = transient();
        assert_eq!(store.observe_selection(5, 10).unwrap(), 69);
        assert_eq!(store.observe_selection(5, 11).unwrap(), 69);
        assert_eq!(store.bigram_count(5, 10).unwrap(), 69);
        assert_eq!(store.bigram_count(5, 11).unwrap(), 69);
        assert_eq!(store.bigram_total(5).unwrap(), 138);
    }

    #[test]
    fn set_bigram_count_plants_filter_edge() {
        let mut store = transient();
        store.set_bigram_count(1, 10, 9).unwrap();
        store.set_bigram_count(1, 11, 10).unwrap();
        assert_eq!(store.bigram_count(1, 10).unwrap(), 9);
        assert_eq!(store.bigram_count(1, 11).unwrap(), 10);
        assert_eq!(store.bigram_total(1).unwrap(), 19);
    }

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

    #[test]
    fn predicted_prefix_invalidates_counts_without_training_a_bigram() {
        let mut store = transient();
        let cur = bare_token(&mut store, "你", &[key("ni")]);
        store.set_bigram_count(1, cur, 10).unwrap();
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

    #[test]
    fn first_allocation_is_first_user_token() {
        let mut store = transient();
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);
        let token = store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        assert_eq!(token, FIRST_USER_TOKEN);
        assert_eq!(token, 0x0700_0001);
        assert!(crate::user::is_user_token(token));
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN + 1);
    }

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
        assert_eq!(crate::phrase_index_library_index(user), 7);
        assert_eq!(crate::phrase_index_library_index(net), 6);
        assert_eq!(store.export_phrases_in(USER_DICTIONARY).len(), 1);
        assert_eq!(store.export_phrases_in(6).len(), 1);
        assert_eq!(next_user_token(&store), user + 1);
    }

    #[test]
    fn add_phrase_seeds_unigram_with_count_times_three() {
        let mut store = transient();
        let token = store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        assert_eq!(store.unigram_delta(token), 15);
        assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 0);

        let token2 = store
            .add_phrase(&"世界".into(), &[key("shi"), key("jie")], Some(10))
            .unwrap();
        assert_eq!(store.unigram_delta(token2), 30);
    }

    /// `_add_phrase` adds `count * 3` as a `guint32` product: past
    /// `u32::MAX` it wraps (1431655766 * 3 = 2^32 + 2).
    #[test]
    fn add_phrase_count_times_three_wraps_past_u32() {
        let mut store = transient();
        let token = store
            .add_phrase(
                &"你好".into(),
                &[key("ni"), key("hao")],
                Some(1_431_655_766),
            )
            .unwrap();
        assert_eq!(store.unigram_delta(token), 2);
    }

    /// The sub-index refuses a unigram add that would overflow its
    /// `guint32` total (`phrase_index.cpp:169-171`): training goes on
    /// without the unigram, and an accepted predicted candidate
    /// answers `ERROR_INTEGER_OVERFLOW` before its bigram
    /// (`pinyin.cpp:2609-2612`).
    #[test]
    fn unigram_adds_stop_at_the_library_total() {
        let mut store = transient();
        // 402 below `u32::MAX`: room for no 483 delta.
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

        // Training keeps its bigram and loses only the unigram.
        store.observe_selection(SENTENCE_START, token).unwrap();
        assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 69);
        assert_eq!(store.unigram_delta(token), full);
    }

    #[test]
    fn existing_phrase_merges_a_new_reading() {
        let mut store = transient();
        let first = store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        let again = store
            .add_phrase(&"你好".into(), &[toned("ni", 3), key("hao")], Some(8))
            .unwrap();
        assert_eq!(first, again);
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN + 1);
        // The merge is a remove and an add of the same item
        // (`pinyin.cpp:576-580`; `phrase_index.h:669`, `:691`): no
        // unigram moves.
        assert_eq!(store.unigram_delta(first), 15);

        let got = store.phrase(first).unwrap();
        assert_eq!(got.phrase_text().unwrap(), "你好");
        assert_eq!(got.pronunciations().len(), 2);
        assert_eq!(got.pronunciations()[0].keys(), &[key("ni"), key("hao")]);
        assert_eq!(got.pronunciations()[0].count(), 5);
        assert_eq!(
            got.pronunciations()[1].keys(),
            &[toned("ni", 3), key("hao")]
        );
        assert_eq!(got.pronunciations()[1].count(), 8);
    }

    #[test]
    fn same_reading_accumulates_pronunciation_count() {
        let mut store = transient();
        let token = store
            .add_phrase(&"词".into(), &[key("ci")], Some(5))
            .unwrap();
        let again = store
            .add_phrase(&"词".into(), &[key("ci")], Some(5))
            .unwrap();
        assert_eq!(token, again);
        assert_eq!(store.unigram_delta(token), 15);
        let got = store.phrase(token).unwrap();
        assert_eq!(got.pronunciations().len(), 1);
        assert_eq!(got.pronunciations()[0].count(), 10);
    }

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
        assert!(matches!(
            store.add_phrase(
                &crate::PhraseText::from(&"啊".repeat(16)),
                &[key("a"); 16],
                None
            ),
            Err(UserStoreError::InvalidPhrase)
        ));
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);
        assert!(store.token_for_phrase("你好").unwrap().is_none());
    }

    #[test]
    fn lookup_of_unknown_token_is_none() {
        let store = transient();
        assert!(store.phrase(FIRST_USER_TOKEN).is_none());
        assert!(store.phrase(0x0100_0001).is_none());
    }

    #[test]
    fn export_phrases_render_the_pinned_triples() {
        let mut store = transient();
        store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], Some(7))
            .unwrap();
        store
            .add_phrase(&"世界".into(), &[key("shi"), key("jie")], Some(3))
            .unwrap();

        assert_eq!(
            store.export_phrases_in(USER_DICTIONARY),
            vec![
                ExportedPhrase {
                    text: "你好".chars().map(u32::from).collect(),
                    pinyin: "ni'hao".to_owned(),
                    count: 12,
                },
                ExportedPhrase {
                    text: "世界".chars().map(u32::from).collect(),
                    pinyin: "shi'jie".to_owned(),
                    count: 3,
                },
            ]
        );
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
        // A system item's training lands in the item itself
        // (`phrase_index.h:628-635`, `phrase_index.cpp:150-176`): the
        // session keeps no delta for it, and the mask of the user
        // library leaves it.
        assert_eq!(store.unigram_delta(SYSTEM_B), 0);
        assert_eq!(store.system_unigram(SYSTEM_B), Some(100 + 966));
        assert_eq!(store.unigram_delta(user_a), 0);
        // `FacadePhraseIndex::mask_out` takes the user sub-index's old
        // total out and its new one in (`phrase_index.cpp:713-715`):
        // with every user item gone, nothing is left beyond the system
        // libraries' totals.
        assert_eq!(store.unigram_total(), 0);
        // The user sub-index is empty again, so `get_range` ends at 1
        // (`phrase_index.cpp:624-646`) and the next import reuses it.
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);
        assert!(!store.is_modified());
        store.save_reporting().unwrap();

        store.mask_out(0x0, 0x0).unwrap();
        assert_eq!(store.bigram_count(SYSTEM_A, SYSTEM_B).unwrap(), 0);
        assert_eq!(store.bigram_total(SYSTEM_A).unwrap(), 0);
        assert_eq!(store.unigram_total(), 0);
        assert!(!store.is_modified());
    }

    /// `_add_phrase` takes `range.m_range_end` (`pinyin.cpp:587-595`) and
    /// `get_range` drops trailing zero offsets (`phrase_index.cpp:624-646`):
    /// the next token is one past the highest live token.
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

        // A hole below the top is not reused.
        store.mask_out(0xFFFF_FFFF, b).unwrap();
        assert_eq!(next_user_token(&store), c + 1);

        // Masking the top frees it and every empty slot beneath it.
        store.mask_out(0xFFFF_FFFF, c).unwrap();
        assert_eq!(next_user_token(&store), a + 1);
        assert_eq!(
            store
                .add_phrase(&"好的".into(), &[key("hao"), key("de")], None)
                .unwrap(),
            a + 1
        );

        // A single removal frees the top the same way.
        assert!(store.remove_user_phrase(a + 1).unwrap());
        assert_eq!(next_user_token(&store), a + 1);
        assert_eq!(
            store
                .add_phrase(&"再来".into(), &[key("zai"), key("lai")], None)
                .unwrap(),
            a + 1
        );
    }

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
        // `remove_phrase_item` takes the removed item's unigram out of
        // the facade total (`phrase_index.h:682-692`); the other user
        // item keeps its `count * 3` and its one selection.
        assert_eq!(store.unigram_total(), 15 + 483);
        // `pinyin_remove_user_candidate` (`pinyin.cpp:3726-3768`) sets
        // no `m_modified`.
        assert!(!store.is_modified());
        assert!(!store.remove_user_phrase(user_a).unwrap());
        assert!(!store.remove_user_phrase(SYSTEM_A).unwrap());
    }

    #[test]
    fn bigram_removals_subtract_stored_totals_with_u32_wrap() {
        for remove_phrase in [false, true] {
            for (total, counts, removed, expected) in [
                (1_u32, [u32::MAX, 1], 1_u32, 0_u32),
                (0, [2, 3], 1, u32::MAX),
                (17, [2, 3], 4, 13),
                (17, [2, 3], 0, 17),
                (1, [0, 0], 1, 0),
                (1, [2, 3], u32::MAX, 2),
                (9, [2, 3], 4, 5),
            ] {
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
                    // An independent untouched total, plus a selected
                    // predecessor (also with reserved token bits) and
                    // a gram whose last zero-frequency item is removed.
                    (2, gram(17, &[(kept[0], 2)])),
                    (token, gram(4, &[(kept[0], 4)])),
                    (token | 0x8000_0000, gram(4, &[(kept[0], 4)])),
                    (3, gram(0, &[(token, 0)])),
                ];
                for (p, value) in &planted {
                    store.plant_raw_bigram(*p, value);
                }
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
                // The raw gram is the masked bytes, total wrapped.
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
        }
    }

    /// `_add_phrase` takes `get_range`'s end as the new token with no
    /// check (`pinyin.cpp:589-594` at 074a2219), so the last slot is
    /// allocated like any other. The end past it, `PHRASE_MASK + 1`, is
    /// masked into the nibble by `FacadePhraseIndex::get_range`
    /// (`phrase_index.cpp:620`, `novel_types.h:45-46`) and bumped past
    /// the reserved id (`pinyin.cpp:592-594`): every later add lands on
    /// slot 1, where `add_phrase_item` overwrites the offset word and
    /// adds the new unigram to a total that keeps the overwritten one
    /// (`phrase_index.cpp:208-217`). Register row 119.
    #[test]
    fn add_phrase_fills_the_last_slot_then_wraps_to_slot_1() {
        let mut store = transient();
        plant_item(&store, USER_DICTIONARY, PHRASE_MASK - 1);
        let last = phrase_index_make_token(USER_DICTIONARY, PHRASE_MASK);
        assert_eq!(next_user_token(&store), last);

        let token = store
            .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
            .unwrap();
        assert_eq!(token, last, "the last slot is allocated like any other");
        assert_eq!(store.phrases().len(), 2);
        assert_eq!(
            store.user_library_token_end(USER_DICTIONARY).unwrap(),
            phrase_index_make_token(USER_DICTIONARY, 0),
            "the facade range end is masked into the nibble"
        );
        assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);

        let wrapped = store
            .add_phrase(&"世界".into(), &[key("shi"), key("jie")], None)
            .unwrap();
        assert_eq!(
            wrapped, FIRST_USER_TOKEN,
            "the add past the last slot wraps"
        );
        assert_eq!(store.phrases().len(), 3);
        assert_eq!(
            crate::user::test_session::library_total(&store, USER_DICTIONARY),
            30
        );

        // The range end stays past the last slot, so the next add lands
        // on slot 1 again: the offset word moves to the new item and the
        // overwritten item's unigram stays in `m_total_freq`.
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

    /// The promotion takes `ADDON_DICTIONARY`'s `get_range` end as is,
    /// "assume not over flow here" (`pinyin.cpp:2541-2545`): the last
    /// slot is allocated, and the end past it is masked to slot 0
    /// (`phrase_index.cpp:620`) with no reserved-id bump, so every later
    /// promotion lands on slot 0 and `add_phrase_item` overwrites it
    /// (`phrase_index.cpp:208-217`). Register row 119.
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
        assert_eq!(
            wrapped,
            phrase_index_make_token(ADDON_DICTIONARY, 0),
            "no `token++` on this path"
        );
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
            300,
            "the overwritten unigram stays in the total"
        );
        assert_eq!(store.unigram_total(), 300);
    }

    #[test]
    fn promote_addon_phrase_allocates_nibble_5_and_copies_frequency() {
        let mut store = transient();
        let keys = [toned("er", 2), toned("huang", 2)];

        let token =
            crate::user::test_session::promote(&mut store, "二簧", &[(keys.to_vec(), 100)], 100)
                .unwrap();
        assert_eq!(
            phrase_index_library_index(token),
            ADDON_DICTIONARY,
            "promotion lands in default nibble 5"
        );
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

        // A second promotion searches nothing: it copies the item to
        // the range end again (`pinyin.cpp:2543-2558`), so the facade
        // total takes its unigram a second time and the first item
        // keeps its reading count.
        let again =
            crate::user::test_session::promote(&mut store, "二簧", &[(keys.to_vec(), 100)], 100)
                .unwrap();
        assert_eq!(again, token + 1, "re-promotion takes the next range end");
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
        assert_eq!(stored_item(&store, token), item, "the bytes as stored");
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
                    .any(|row| row.token() == token),
                "each stored reading is indexed"
            );
        }
    }

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

    /// The pin copies words and readings into arrays of
    /// `MAX_PHRASE_LENGTH` (16) (`pinyin.cpp:2548`, `:2555`): 16
    /// promotes, 17 is undefined there and is refused.
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

// ── Cross-backend equivalence (one peer per build, all peers in CI) ──

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
            for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
                let scratch = tempfile::tempdir().expect("scratch");
                let dir = scratch.path();
                let mut store = if has_dir {
                    crate::user::test_session::open(dir, originals(), versions(), law)
                } else {
                    UserStore::open_transient(originals(), versions(), law)
                }
                .expect("open");
                // A sub-index total of MAX-4, independent of its one
                // item's frequency 100. Compact must not erase the
                // difference on either rejected save path.
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
}

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
    for _ in 0..2000 {
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
    assert_eq!(store.system_unigram(token), Some(2000));
    assert_eq!(
        Some(crate::user::test_session::library_total(&store, 1)),
        Some(2000)
    );
    assert_eq!(store.unigram_total(), 0);
    // Repeated reads of the store see the same live state.
    for _ in 0..4 {
        let counts = store.count_delta(None, token).unwrap();
        assert_eq!(counts.unigram_live, Some(2000));
        assert_eq!(
            Some(crate::user::test_session::library_total(&store, 1)),
            Some(2000)
        );
    }
}

mod backend_files {
    //! Behavioural ports of the upstream user-state test suites, over the
    //! libpinyin session the shipped libraries open.
    //!
    //! - `libpinyin/tests/storage/test_ngram.cpp`: `SingleGram` insert-or-set
    //!   semantics, totals, persistence through a save and reopen, `mask_out`,
    //!   and the saved-snapshot read-back.
    //! - `libchewing/tests/test-userphrase.c`: add → lookup → remove cycles,
    //!   persistence across independent sessions, same-reading/different-phrase
    //!   coexistence, multi-reading phrases and the phrase-length ceiling.
    //!
    //! Divergences are asserted as the oxpinyin contract where the upstream
    //! shape has no equivalent: `set_bigram_count` unifies upstream's
    //! `insert_freq`/`set_freq` duality into one overwrite, and totals are
    //! store-maintained (upstream makes the caller maintain `total_freq`).
    //! `remove_user_phrase` models `pinyin_remove_user_candidate` and drops a
    //! whole phrase token (upstream removes one (phrase, bopomofo) pair); as
    //! the pin does, it refuses when the token's phrase-table row or any of
    //! its indexed readings is missing (`pinyin.cpp:3743`, `:3750`, `:3759`).
    //! A unigram belongs to an item (`phrase_index.cpp:150-161`), so a test
    //! that trains a count allocates its token first. Upstream line numbers
    //! are libpinyin 074a2219.
    //! See `docs/testing/upstream-test-coverage.md` for the ledger entries.

    use std::path::Path;

    use crate::SyllableKey;

    use crate::data::user_files::{SystemVersions, UserFileLayout};

    use crate::user::{
        FIRST_USER_TOKEN, SystemOriginals, UserConfLaw, UserPhrase, UserStore, UserStoreError,
        toned_key,
    };
    use crate::{PhraseToken, SENTENCE_START};

    fn versions() -> SystemVersions {
        SystemVersions::for_this_build(7, 14)
    }

    /// No system library: the stock layout's user and network libraries.
    fn no_originals() -> SystemOriginals {
        SystemOriginals::new(Default::default(), UserFileLayout::stock())
    }

    /// The pin's NULL-user-directory session.
    fn session() -> UserStore {
        UserStore::open_transient(no_originals(), versions(), UserConfLaw::Pinyin).unwrap()
    }

    /// A session over the user directory `dir`, for the tests that save: an
    /// init over a `table.conf` with the stock layout's three user libraries
    /// and no system library.
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

    /// A user item with a zero unigram and one reading of count zero: a token
    /// the seed arithmetic is read on, bare.
    fn bare_token(store: &mut UserStore, text: &str, reading: &str) -> PhraseToken {
        store
            .add_phrase(&crate::PhraseText::from(text), &keys(reading), Some(0))
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

    // ── libpinyin test_ngram.cpp: bigram count semantics ─────────────────

    #[test]
    fn a_bigram_count_overwrites_and_the_total_tracks_it() {
        // Upstream: get_freq then set/insert; the last write for a pair wins
        // and `total_freq` reads back what the caller recorded.
        let mut store = session();

        let (a, b, c) = (FIRST_USER_TOKEN, FIRST_USER_TOKEN + 1, FIRST_USER_TOKEN + 2);
        store.set_bigram_count(a, b, 4).unwrap();
        assert_eq!(store.bigram_count(a, b).unwrap(), 4);
        assert_eq!(store.bigram_total(a).unwrap(), 4);

        store.set_bigram_count(a, b, 32).unwrap();
        assert_eq!(store.bigram_count(a, b).unwrap(), 32, "last write wins");
        assert_eq!(store.bigram_total(a).unwrap(), 32, "no double count");

        store.set_bigram_count(a, c, 8).unwrap();
        assert_eq!(store.bigram_total(a).unwrap(), 40, "totals accumulate");
    }

    #[test]
    fn a_bigram_table_roundtrips_through_the_store_file() {
        // Upstream: `bigram.store` → reopen → `gram->search` reproduces the
        // items; the snapshot save/load roundtrip reads back what was written.
        // The session's save writes `user_bigram.db`, which the next open
        // loads (`pinyin.cpp:399-401`).
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (FIRST_USER_TOKEN, FIRST_USER_TOKEN + 1);
        {
            let mut store = session_in(dir.path());
            store.set_bigram_count(a, b, 16).unwrap();
            let (count_before, total_before) = (
                store.bigram_count(a, b).unwrap(),
                store.bigram_total(a).unwrap(),
            );
            store.observe_selection(a, b).unwrap();
            assert!(
                store.bigram_count(a, b).unwrap() > count_before,
                "the observed selection adds on top of the seeded count"
            );
            assert_eq!(
                store.bigram_total(a).unwrap(),
                total_before + (store.bigram_count(a, b).unwrap() - count_before),
                "the total absorbs exactly the count's delta"
            );
            store.save_reporting().unwrap();
        }

        // The reopen reads back the exact recorded numbers.
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

    #[test]
    fn mask_out_zero_clears_every_recorded_item() {
        // Upstream: `bigram.mask_out(0x0, 0x0)` leaves no items behind.
        let mut store = session();
        let a = bare_token(&mut store, "甲", "jia");
        let b = bare_token(&mut store, "乙", "yi");

        store.set_bigram_count(a, b, 4).unwrap();
        store.observe_selection(SENTENCE_START, a).unwrap();
        store.observe_selection(a, b).unwrap();
        assert!(store.bigram_total(a).unwrap() > 0);
        assert!(store.unigram_total() > 0);

        store.mask_out(0, 0).unwrap();
        assert_eq!(store.bigram_count(a, b).unwrap(), 0);
        assert_eq!(store.bigram_total(a).unwrap(), 0);
        // `FacadePhraseIndex::mask_out` swaps the user sub-index's total out
        // of and back into the facade (`phrase_index.cpp:713-715`); with both
        // items gone the user side holds nothing.
        assert_eq!(store.unigram_total(), 0);
    }

    #[test]
    fn a_mask_removes_the_rows_that_list_a_masked_token_as_successor() {
        // libpinyin's `Bigram::mask_out` (`ngram_bdb.cpp:218-253`, kept in step
        // with `ngram_kyotodb.cpp`) walks every predecessor: one whose token
        // matches the mask is removed whole (`:229-231`); every other gram drops
        // the items whose token matches (`SingleGram::mask_out`, `ngram.cpp:86-99`,
        // each item's count leaving the total, `:90`), and a gram left with no
        // item is removed (`:243-244`), else stored back (`:246`). The former
        // `issue762-token-reuse-stale-bigram` cases seeded a user token's row and
        // a system token's row that listed the user token, then masked library 7
        // (`0x0f00_0000`, `0x0700_0000`): both rows go.
        let user = FIRST_USER_TOKEN;
        let system: PhraseToken = 0x0100_0001;
        let seed = |store: &mut UserStore| {
            store.set_bigram_count(user, system, 100).unwrap();
            store.set_bigram_count(system, user, 100).unwrap();
        };
        let rows = |store: &UserStore| {
            let mut rows: Vec<(PhraseToken, PhraseToken, u64)> = Vec::new();
            for prev in store.bigram_predecessors().unwrap() {
                for (cur, count) in store.bigram_successors(prev).unwrap() {
                    rows.push((prev, cur, count));
                }
            }
            rows.sort_unstable();
            rows
        };

        let mut masked = session();
        seed(&mut masked);
        assert_eq!(
            rows(&masked),
            vec![(system, user, 100), (user, system, 100)]
        );
        masked.mask_out(0x0f00_0000, 0x0700_0000).unwrap();
        assert_eq!(
            rows(&masked),
            vec![],
            "the row under the user token and the row listing it are removed"
        );
        assert_eq!(masked.bigram_predecessors().unwrap(), vec![]);
        assert_eq!(masked.bigram_total(system).unwrap(), 0);

        // The no-mask control: a mask that selects another library leaves both rows.
        let mut control = session();
        seed(&mut control);
        control.mask_out(0x0f00_0000, 0x0200_0000).unwrap();
        assert_eq!(
            rows(&control),
            vec![(system, user, 100), (user, system, 100)]
        );
        assert_eq!(control.bigram_total(system).unwrap(), 100);
        assert_eq!(control.bigram_total(user).unwrap(), 100);
    }

    // ── libchewing test-userphrase.c: user phrase lifecycle ─────────────

    fn user_phrase(phrase: &str, reading: &str) -> (String, Vec<crate::user::PinyinKey>) {
        (phrase.to_owned(), keys(reading))
    }

    #[test]
    fn add_lookup_remove_roundtrip_and_persists_across_handles() {
        // Upstream: add → lookup 1; remove → lookup 0; a fresh context sees the
        // removal (persistence across contexts). The pin saves only a modified
        // context: `pinyin_end_add_phrases` sets `m_modified`
        // (`pinyin.cpp:658`), `pinyin_remove_user_candidate` does not
        // (`:3726-3768`), so each save here is armed the way the pin's
        // import and training arm it.
        let dir = tempfile::tempdir().unwrap();
        let (text, reading) = user_phrase("测试", "ce,shi");

        let token = {
            let mut store = session_in(dir.path());
            assert!(store.token_for_phrase(&text).unwrap().is_none());
            let token = store
                .add_phrase(&crate::PhraseText::from(&text), &reading, None)
                .unwrap();
            assert_eq!(store.token_for_phrase(&text).unwrap(), Some(token));
            store.mark_modified();
            store.save_reporting().unwrap();
            token
        };

        {
            let mut store = session_in(dir.path());
            let phrase = store.phrase(token).expect("persists across sessions");
            assert_eq!(phrase.phrase_text().unwrap(), "测试");
            assert_eq!(phrase.pronunciations().len(), 1);

            assert!(store.remove_user_phrase(token).unwrap());
            assert!(store.token_for_phrase(&text).unwrap().is_none());
            store.mark_modified();
            store.save_reporting().unwrap();
        }

        let store = session_in(dir.path());
        assert!(
            store.token_for_phrase(&text).unwrap().is_none(),
            "a fresh session must see the removal"
        );
        assert!(store.phrase(token).is_none());
    }

    /// The `user-store-ops` crash input `22d` (`crash-c249fd86…`): add `词` under
    /// `dian`, merge `hen` into the same token, then remove it. The merged
    /// reading has no pinyin-index row, so the pin's
    /// `pinyin_remove_user_candidate` dies at `pinyin.cpp:3759`; the store
    /// answers that assert as the typed refusal and keeps the phrase, which is
    /// what the fuzz target now accepts for exactly this condition.
    #[test]
    fn one_phrase_holds_several_readings_and_counts_accumulate_per_reading() {
        // Upstream: the same phrase under two bopomofo readings is stored
        // independently (remove-one-keeps-the-other), and re-adding the same
        // pair accumulates its count. oxpinyin keeps every reading of a phrase
        // token as its own pronunciation with its own count.
        let mut store = session();

        let text = "重";
        let zhong = keys("zhong");
        let chong = keys("chong");
        let token = store
            .add_phrase(&crate::PhraseText::from(text), &zhong, Some(7))
            .unwrap();
        store
            .add_phrase(&crate::PhraseText::from(text), &chong, Some(9))
            .unwrap();

        let phrase: UserPhrase = store.phrase(token).expect("stored");
        assert_eq!(phrase.pronunciations().len(), 2, "both readings stored");

        store
            .add_phrase(&crate::PhraseText::from(text), &zhong, Some(3))
            .unwrap();
        let phrase = store.phrase(token).expect("stored");
        let zhong_count = phrase
            .pronunciations()
            .iter()
            .find(|p| p.keys() == zhong.as_slice())
            .expect("zhong reading")
            .count();
        assert_eq!(zhong_count, 10, "same reading accumulates");

        // Only the reading a phrase was created with is indexed; the `chong`
        // merge never touched the pinyin table (`pinyin.cpp:569-582`).
        // `pinyin_remove_user_candidate` asserts `pinyin_table->remove_index`
        // for every reading (`:3759`), so the pin aborts on this phrase; the
        // session answers that assert as the typed refusal and keeps the
        // phrase and its readings.
        assert!(matches!(
            store.remove_user_phrase(token),
            Err(UserStoreError::UserReadingRemoval)
        ));
        assert!(store.phrase(token).is_some());
    }

    /// An initial-only reading names one key in both index spaces, so
    /// `ChewingLargeTable2::remove_index` removes it once and then answers
    /// `ERROR_REMOVE_ITEM_DONOT_EXISTS` (`chewing_large_table2.cpp:200-219`),
    /// which `pinyin_remove_user_candidate` asserts on (`pinyin.cpp:3759`).
    /// The store answers that assert as the typed refusal and keeps the phrase.
    /// The `user-store-ops` generator cannot build this shape, so only this
    /// test covers it.
    #[test]
    fn an_initial_only_reading_answers_the_pins_reading_removal_assert() {
        let mut store = session();
        // The initial-only row `d` packs to initial 4 with middle and final 0, so
        // both index spaces name the same key.
        let reading = [key("d")];
        let token = store
            .add_phrase(&crate::PhraseText::from("词"), &reading, Some(50))
            .unwrap();
        let phrase = store.phrase(token).expect("stored");
        assert!(
            phrase.pronunciations()[0].indexed(),
            "the reading is indexed; the refusal is the double removal"
        );
        assert!(matches!(
            store.remove_user_phrase(token),
            Err(UserStoreError::UserReadingRemoval)
        ));
        assert!(store.phrase(token).is_some());
    }

    #[test]
    fn readings_keep_insertion_order_tones_and_only_the_first_is_indexed() {
        // `_add_phrase` (pinyin.cpp:566-607): a new phrase is indexed under the
        // reading it is created with; a reading merged into it later is
        // appended (`add_pronunciation`, phrase_index.cpp:86-91) and never
        // indexed. The tone is part of the key (`pinyin_exact_compare2`).
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
    }

    #[test]
    fn a_reading_add_that_would_overflow_the_running_total_changes_nothing() {
        // `PhraseItem::add_pronunciation`'s guard (phrase_index.cpp:76-79): the
        // guint32 total of the readings up to the match must not wrap.
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

    #[test]
    fn the_same_reading_on_different_texts_yields_distinct_tokens() {
        // Upstream: 測試 and 側室 share identical bopomofo and coexist as
        // independent user phrases. oxpinyin allocates one token per phrase
        // text, so equal key sequences on two texts stay separate entries.
        let mut store = session();
        let reading = keys("ce,shi");
        let a = store.add_phrase(&"测试".into(), &reading, None).unwrap();
        let b = store.add_phrase(&"策士".into(), &reading, None).unwrap();
        assert_ne!(a, b, "distinct texts allocate distinct tokens");

        let phrase_a = store.phrase(a).expect("stored");
        let phrase_b = store.phrase(b).expect("stored");
        assert_eq!(phrase_a.phrase_text().unwrap(), "测试");
        assert_eq!(phrase_b.phrase_text().unwrap(), "策士");
        assert_eq!(phrase_a.pronunciations().len(), 1);
        assert_eq!(phrase_b.pronunciations().len(), 1);
    }

    #[test]
    fn phrase_length_ceiling_rejects_sixteen_characters() {
        // Upstream rejects a 12-syllable phrase and accepts 11. The oxpinyin
        // ceiling is MAX_PHRASE_LENGTH (16): 15 characters are accepted, 16
        // are `InvalidPhrase` (the check is a strict `<`).
        let mut store = session();

        let ok: String = "测".repeat(15);
        let token = store
            .add_phrase(&crate::PhraseText::from(&ok), &[key("ce"); 15], None)
            .unwrap();
        assert!(store.phrase(token).is_some());

        let too_long: String = "测".repeat(16);
        assert!(matches!(
            store.add_phrase(&crate::PhraseText::from(&too_long), &[key("ce"); 16], None),
            Err(UserStoreError::InvalidPhrase)
        ));
    }
}

/// The compaction at the end of an import copies the tokens of the range
/// `[1, end)` into a fresh sub-index, so an item at the reserved slot 0 is
/// dropped (`pinyin.cpp:655-660`, `phrase_index.cpp:624-675` at 074a2219).
/// Before the compaction the item is readable: `get_phrase_item` has no
/// slot-0 exclusion (`phrase_index.cpp:180-206`).
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
    let chunk = build_chunk(&[(1, item)]).expect("one-item user chunk");
    // Move the item's offset word from slot 1 to slot 0.
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

/// A selected short value is removed without loading it (ngram_bdb.cpp:231-238).
#[test]
fn mask_out_of_a_selected_short_key_completes() {
    let user = tempfile::tempdir().unwrap();
    crate::user::native_fixture::plant_bigram(
        user.path(),
        vec![(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
    );
    let mut store = crate::user::test_session::open_w3(user.path());
    store.mask_out(0xffff_ffff, 1).unwrap();
}

/// The mask uses the four-bit library index and low 24 slot bits
/// (novel_types.h:41-44, phrase_index.cpp:677-715 at 074a2219).
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

/// The C shared mask-abort path converts this owner error to abort.
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

/// A one-byte user index key holds no whole `ChewingKey`: the pin's word
/// count is `key.size() / sizeof(ChewingKey)` = 0, which its `switch` sends
/// to `default: abort()` like the empty key
/// (`chewing_large_table2_tkrzwdb.cpp:430`, `:448-466`; bdb `:529`, kc
/// `:499`). The C shared mask-abort path converts this owner error to abort.
#[test]
fn mask_out_aborts_on_a_one_byte_index_key() {
    let user = tempfile::tempdir().unwrap();
    crate::user::native_fixture::write_user_conf(
        user.path(),
        &crate::user::native_fixture::system_dir(),
    );
    crate::user::native_fixture::craft_user_index(
        &user.path().join("user_pinyin_index.bin"),
        &[(vec![7], Vec::new()), (vec![0, 1], Vec::new())],
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

/// Removing the item subtracts its whole live unigram (phrase_index.cpp:233,713-715).
#[test]
fn removing_a_user_item_takes_its_unigram_increment_out_of_the_total() {
    let user = tempfile::tempdir().unwrap();
    let mut store = crate::user::test_session::open_w3(user.path());
    let before = store.unigram_total();
    let token = store
        .add_phrase_in(
            7,
            &"龘龘".into(),
            &crate::user::test_session::native_keys("da2'da2"),
            Some(0),
        )
        .unwrap();
    store.compact_libraries();
    assert!(store.add_unigram_outcome(token, 9).is_accepted());
    assert_eq!(store.unigram_total(), before + 9);
    store.mask_out(u32::MAX, token).unwrap();
    assert_eq!(store.user_unigram_frequency(token), None);
    assert_eq!(store.unigram_total(), before);
}

/// U6: the persisted sub-index total is independent of item sums. The
/// pin guards that total and wraps the item's `guint32` (074a2219
/// `phrase_index.cpp:169-175,345`).
#[test]
fn user_unigram_item_wraps_when_its_sub_index_total_accepts_the_add() {
    use crate::data::chunk_format::{CHUNK_HEADER_SIZE, build_memory_chunk};
    use crate::data::phrase_library::{ChunkItem, build_chunk};
    use crate::user::native_fixture::TempUserDir;

    let user = TempUserDir::new("user-unigram-wrap");
    drop(crate::user::test_session::open_w3(&user.path));
    let chunk = build_chunk(&[(
        1,
        ChunkItem {
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

/// The predicted-prefix write masks the tagged token to its live item
/// (pinyin.cpp:2608-2616, phrase_index.h:629 at 074a2219).
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

/// The gram retains the whole tagged token while the unigram masks it.
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

/// The pinned training seeds are 69, 138, 414 (counts 69, 207, 621).
#[test]
fn train_records_the_pinned_doubling_sequence() {
    let user = tempfile::tempdir().unwrap();
    let system = crate::user::native_fixture::system_dir();
    let mut store = crate::user::test_session::open_native(
        user.path(),
        &system,
        crate::user::UserConfLaw::Zhuyin,
    );
    let token = crate::user::test_session::native_tokens(&system, "你好")[0];
    let shipped = store.system_unigram(token).unwrap();
    assert_eq!(store.bigram_count(1, token).unwrap(), 0);
    assert_eq!(store.observe_selection(1, token).unwrap(), 69);
    assert_eq!(store.bigram_count(1, token).unwrap(), 69);
    assert_eq!(store.bigram_total(1).unwrap(), 69);
    assert_eq!(store.system_unigram(token), Some(shipped + 483));
    assert_eq!(store.observe_selection(1, token).unwrap(), 138);
    assert_eq!(store.bigram_count(1, token).unwrap(), 207);
    assert_eq!(store.observe_selection(1, token).unwrap(), 414);
    assert_eq!(store.bigram_count(1, token).unwrap(), 621);
    assert_eq!(
        store.system_unigram(token),
        Some(shipped + 483 + 966 + 2898)
    );
}
