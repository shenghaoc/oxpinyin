//! Behavioural ports of the upstream user-state test suites, over the
//! libpinyin session the shipped libraries open.
//!
//! - `libpinyin/tests/storage/test_ngram.cpp`: `SingleGram` insert-or-set
//!   semantics, totals, persistence through a save and reopen, `mask_out`,
//!   and the saved-snapshot read-back.
//! - `libchewing/tests/test-userphrase.c`: add → lookup → remove cycles,
//!   persistence across independent sessions, same-reading/different-phrase
//!   coexistence, multi-reading phrases, the phrase-length ceiling, and
//!   removing an absent phrase.
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

use oxpinyin_core::SyllableKey;
use oxpinyin_data::user_files::{SystemVersions, UserFileLayout};
use oxpinyin_user::{
    FIRST_USER_TOKEN, SENTENCE_START, SystemOriginals, Token, UserConfLaw, UserPhrase, UserStore,
    UserStoreError, toned_key,
};

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

/// A session over the user directory `dir`, for the tests that save.
fn session_in(dir: &Path) -> UserStore {
    UserStore::open_libpinyin(dir, no_originals(), versions(), UserConfLaw::Pinyin).unwrap()
}

/// A user item with a zero unigram and one reading of count zero: a token
/// the seed arithmetic is read on, bare.
fn bare_token(store: &mut UserStore, text: &str, reading: &str) -> Token {
    store
        .add_phrase(
            &oxpinyin_core::PhraseText::from(text),
            &keys(reading),
            Some(0),
        )
        .unwrap()
}

fn key(text: &str) -> oxpinyin_user::PinyinKey {
    oxpinyin_user::PinyinKey::try_from(
        SyllableKey::from_text(text)
            .expect("frozen syllable")
            .index(),
    )
    .expect("frozen syllable inventory fits u16")
}

fn keys(text: &str) -> Vec<oxpinyin_user::PinyinKey> {
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
    assert!(store.unigram_total().unwrap() > 0);

    store.mask_out(0, 0).unwrap();
    assert_eq!(store.bigram_count(a, b).unwrap(), 0);
    assert_eq!(store.bigram_total(a).unwrap(), 0);
    // `FacadePhraseIndex::mask_out` swaps the user sub-index's total out
    // of and back into the facade (`phrase_index.cpp:713-715`); with both
    // items gone the user side holds nothing.
    assert_eq!(store.unigram_total().unwrap(), 0);
}

#[test]
fn a_mask_selects_only_the_matching_tokens() {
    // Upstream mask_out is a predicate wipe `(token & mask) == value`; the
    // oxpinyin store applies it to bigram rows, unigram deltas, and phrase
    // texts alike. Wiping one side's token must leave the other rows.
    let mut store = session();
    let a = FIRST_USER_TOKEN;
    let b = FIRST_USER_TOKEN + 1;
    let c = FIRST_USER_TOKEN + 2;

    store.set_bigram_count(a, b, 4).unwrap();
    store.set_bigram_count(a, c, 8).unwrap();
    store.mask_out(!0, b).unwrap();
    assert_eq!(store.bigram_count(a, b).unwrap(), 0, "b's row is wiped");
    assert_eq!(store.bigram_count(a, c).unwrap(), 8, "c's row survives");
    assert_eq!(store.bigram_total(a).unwrap(), 8, "totals are rewritten");
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
    let system: Token = 0x0100_0001;
    let seed = |store: &mut UserStore| {
        store.set_bigram_count(user, system, 100).unwrap();
        store.set_bigram_count(system, user, 100).unwrap();
    };
    let rows = |store: &UserStore| {
        let mut rows: Vec<(Token, Token, u64)> = Vec::new();
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

fn user_phrase(phrase: &str, reading: &str) -> (String, Vec<oxpinyin_user::PinyinKey>) {
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
            .add_phrase(&oxpinyin_core::PhraseText::from(&text), &reading, None)
            .unwrap();
        assert_eq!(store.token_for_phrase(&text).unwrap(), Some(token));
        store.mark_modified();
        store.save_reporting().unwrap();
        token
    };

    {
        let mut store = session_in(dir.path());
        let phrase = store
            .phrase(token)
            .unwrap()
            .expect("persists across sessions");
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
    assert!(store.phrase(token).unwrap().is_none());
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
        .add_phrase(&oxpinyin_core::PhraseText::from(text), &zhong, Some(7))
        .unwrap();
    store
        .add_phrase(&oxpinyin_core::PhraseText::from(text), &chong, Some(9))
        .unwrap();

    let phrase: UserPhrase = store.phrase(token).unwrap().expect("stored");
    assert_eq!(phrase.pronunciations().len(), 2, "both readings stored");

    store
        .add_phrase(&oxpinyin_core::PhraseText::from(text), &zhong, Some(3))
        .unwrap();
    let phrase = store.phrase(token).unwrap().expect("stored");
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
    assert!(store.phrase(token).unwrap().is_some());
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
        .add_phrase(&oxpinyin_core::PhraseText::from("词"), &reading, Some(50))
        .unwrap();
    let phrase = store.phrase(token).unwrap().expect("stored");
    assert!(
        phrase.pronunciations()[0].indexed(),
        "the reading is indexed; the refusal is the double removal"
    );
    assert!(matches!(
        store.remove_user_phrase(token),
        Err(UserStoreError::UserReadingRemoval)
    ));
    assert!(store.phrase(token).unwrap().is_some());
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

    let phrase = store.phrase(token).unwrap().expect("stored");
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
    let phrase = store.phrase(token).unwrap().expect("stored");
    assert_eq!(phrase.pronunciations()[0].count(), u64::from(u32::MAX - 1));
    store.add_phrase(&"测试".into(), &reading, Some(1)).unwrap();
    let phrase = store.phrase(token).unwrap().expect("stored");
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

    let phrase_a = store.phrase(a).unwrap().expect("stored");
    let phrase_b = store.phrase(b).unwrap().expect("stored");
    assert_eq!(phrase_a.phrase_text().unwrap(), "测试");
    assert_eq!(phrase_b.phrase_text().unwrap(), "策士");
    assert_eq!(phrase_a.pronunciations().len(), 1);
    assert_eq!(phrase_b.pronunciations().len(), 1);
}

#[test]
fn adding_an_existing_phrase_returns_the_same_token() {
    // Upstream chewing has no token exposure; the oxpinyin seam guarantees
    // one token per phrase text, so a re-add is an update, not a duplicate.
    let mut store = session();
    let (text, reading) = user_phrase("你好", "ni,hao");
    let first = store
        .add_phrase(&oxpinyin_core::PhraseText::from(&text), &reading, None)
        .unwrap();
    let second = store
        .add_phrase(&oxpinyin_core::PhraseText::from(&text), &reading, None)
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(store.phrases().unwrap().len(), 1);
}

#[test]
fn phrase_length_ceiling_rejects_sixteen_characters() {
    // Upstream rejects a 12-syllable phrase and accepts 11. The oxpinyin
    // ceiling is MAX_PHRASE_LENGTH (16): 15 characters are accepted, 16
    // are `InvalidPhrase` (the check is a strict `<`).
    let mut store = session();

    let ok: String = "测".repeat(15);
    let token = store
        .add_phrase(
            &oxpinyin_core::PhraseText::from(&ok),
            &[key("ce"); 15],
            None,
        )
        .unwrap();
    assert!(store.phrase(token).unwrap().is_some());

    let too_long: String = "测".repeat(16);
    assert!(matches!(
        store.add_phrase(
            &oxpinyin_core::PhraseText::from(&too_long),
            &[key("ce"); 16],
            None
        ),
        Err(UserStoreError::InvalidPhrase)
    ));
}

#[test]
fn key_count_must_match_the_phrase_length() {
    // Upstream rejects a phrase/bopomofo length mismatch for add and
    // remove alike; oxpinyin rejects it at add time with `InvalidPhrase`.
    let mut store = session();
    assert!(matches!(
        store.add_phrase(&"测试".into(), &keys("ce"), None),
        Err(UserStoreError::InvalidPhrase)
    ));
    assert!(matches!(
        store.add_phrase(&"测试".into(), &keys("ce,shi,de"), None),
        Err(UserStoreError::InvalidPhrase)
    ));
    assert!(matches!(
        store.add_phrase(&"".into(), &[], None),
        Err(UserStoreError::InvalidPhrase)
    ));
}

#[test]
fn removing_an_absent_phrase_is_not_an_error() {
    // Upstream: removing an absent phrase returns 0. oxpinyin: Ok(false).
    let mut store = session();
    assert!(!store.remove_user_phrase(FIRST_USER_TOKEN).unwrap());
}

#[test]
fn learning_grows_unigrams_and_bigrams_differently() {
    // Upstream test_ngram asserts frequencies land in the gram; upstream
    // chewing asserts auto-learn persists after commit. The oxpinyin store
    // records a selection as a seeded bigram count plus a unigram delta on
    // the selected item; without the explicit observe call nothing changes
    // (learning is caller-driven, `docs/findings/user-store.md` §2).
    let mut store = session();
    let a = bare_token(&mut store, "甲", "jia");
    let b = bare_token(&mut store, "乙", "yi");

    let before = store.count_delta(Some(a), b).unwrap();
    assert_eq!(before, oxpinyin_core::UserCountDelta::ZERO);

    store.observe_selection(a, b).unwrap();

    let delta = store.count_delta(Some(a), b).unwrap();
    assert!(delta.bigram_count > 0, "the bigram row is seeded");
    assert_eq!(delta.bigram_total, store.bigram_total(a).unwrap());
    assert!(delta.unigram_delta > 0, "the unigram delta moves");
}
