//! Behavioural ports of the upstream user-state test suites.
//!
//! - `libpinyin/tests/storage/test_ngram.cpp`: `SingleGram` insert-or-set
//!   semantics, totals, persistence through a store roundtrip, `mask_out`,
//!   and the saved-snapshot read-back.
//! - `libchewing/tests/test-userphrase.c`: add → lookup → remove cycles,
//!   persistence across independent handles, same-reading/different-phrase
//!   coexistence, multi-reading phrases, the phrase-length ceiling, and
//!   removing an absent phrase.
//!
//! Divergences are asserted as the oxpinyin contract where the upstream
//! shape has no equivalent: `set_bigram_count` unifies upstream's
//! `insert_freq`/`set_freq` duality into one overwrite, totals are store-
//! maintained (upstream makes the caller maintain `total_freq`), and
//! `remove_user_phrase` drops a whole phrase token (upstream removes one
//! (phrase, bopomofo) pair). See
//! `docs/testing/upstream-test-coverage.md` for the ledger entries.

use std::path::PathBuf;

use oxpinyin_core::SyllableKey;
use oxpinyin_user::{FIRST_USER_TOKEN, SENTENCE_START, UserPhrase, UserStore, UserStoreError};

fn temp_path(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "oxpinyin-user-port-{tag}-{}.redb",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

fn key(text: &str) -> oxpinyin_user::PinyinKey {
    SyllableKey::from_text(text)
        .expect("frozen syllable")
        .index() as oxpinyin_user::PinyinKey
}

fn keys(text: &str) -> Vec<oxpinyin_user::PinyinKey> {
    text.split(',').map(key).collect()
}

// ── libpinyin test_ngram.cpp: bigram count semantics ─────────────────

#[test]
fn a_bigram_count_overwrites_and_the_total_tracks_it() {
    // Upstream: get_freq then set/insert; the last write for a pair wins
    // and `total_freq` reads back what the caller recorded.
    let mut store = UserStore::create_standalone(&temp_path("bigram-overwrite")).unwrap();

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
    let path = temp_path("bigram-roundtrip");
    let (a, b) = (FIRST_USER_TOKEN, FIRST_USER_TOKEN + 1);
    {
        let mut store = UserStore::create_standalone(&path).unwrap();
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
        store.save().unwrap();
    }

    // The reopen reads back the exact recorded numbers.
    let (seeded_count, seeded_total) = {
        let mut store = UserStore::create_standalone(&path).unwrap();
        let counts = (
            store.bigram_count(a, b).unwrap(),
            store.bigram_total(a).unwrap(),
        );
        store.save().unwrap();
        counts
    };
    let store = UserStore::create_standalone(&path).unwrap();
    assert_eq!(store.bigram_count(a, b).unwrap(), seeded_count);
    assert_eq!(store.bigram_total(a).unwrap(), seeded_total);
    assert!(seeded_count > 16);
}

#[test]
fn mask_out_zero_clears_every_recorded_item() {
    // Upstream: `bigram.mask_out(0x0, 0x0)` leaves no items behind.
    let mut store = UserStore::create_standalone(&temp_path("bigram-mask")).unwrap();
    let (a, b) = (FIRST_USER_TOKEN, FIRST_USER_TOKEN + 1);

    store.set_bigram_count(a, b, 4).unwrap();
    store.observe_selection(SENTENCE_START, a).unwrap();
    store.observe_selection(a, b).unwrap();
    assert!(store.bigram_total(a).unwrap() > 0);
    assert!(store.unigram_total().unwrap() > 0);

    store.mask_out(0, 0).unwrap();
    assert_eq!(store.bigram_count(a, b).unwrap(), 0);
    assert_eq!(store.bigram_total(a).unwrap(), 0);
    assert_eq!(store.unigram_total().unwrap(), 0);
}

#[test]
fn a_mask_selects_only_the_matching_tokens() {
    // Upstream mask_out is a predicate wipe `(token & mask) == value`; the
    // oxpinyin store applies it to bigram rows, unigram deltas, and phrase
    // texts alike. Wiping one side's token must leave the other rows.
    let mut store = UserStore::create_standalone(&temp_path("bigram-mask-one")).unwrap();
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

// ── libchewing test-userphrase.c: user phrase lifecycle ─────────────

fn user_phrase(phrase: &str, reading: &str) -> (String, Vec<oxpinyin_user::PinyinKey>) {
    (phrase.to_owned(), keys(reading))
}

#[test]
fn add_lookup_remove_roundtrip_and_persists_across_handles() {
    // Upstream: add → lookup 1; remove → lookup 0; a fresh context sees the
    // removal (persistence across contexts).
    let path = temp_path("phrase-roundtrip");
    let (text, reading) = user_phrase("测试", "ce,shi");

    let token = {
        let mut store = UserStore::create_standalone(&path).unwrap();
        assert!(store.token_for_phrase(&text).unwrap().is_none());
        let token = store.add_phrase(&text, &reading, None).unwrap();
        assert_eq!(store.token_for_phrase(&text).unwrap(), Some(token));
        token
    };

    {
        let mut store = UserStore::create_standalone(&path).unwrap();
        let phrase = store
            .phrase(token)
            .unwrap()
            .expect("persists across handles");
        assert_eq!(phrase.text(), "测试");
        assert_eq!(phrase.pronunciations().len(), 1);

        assert!(store.remove_user_phrase(token).unwrap());
        assert!(store.token_for_phrase(&text).unwrap().is_none());
    }

    let store = UserStore::create_standalone(&path).unwrap();
    assert!(
        store.token_for_phrase(&text).unwrap().is_none(),
        "a fresh handle must see the removal"
    );
}

#[test]
fn one_phrase_holds_several_readings_and_counts_accumulate_per_reading() {
    // Upstream: the same phrase under two bopomofo readings is stored
    // independently (remove-one-keeps-the-other), and re-adding the same
    // pair accumulates its count. oxpinyin keeps every reading of a phrase
    // token as its own pronunciation row with its own count; removal drops
    // the whole token (asserted as the oxpinyin contract below).
    let path = temp_path("phrase-readings");
    let mut store = UserStore::create_standalone(&path).unwrap();

    let text = "重";
    let zhong = keys("zhong");
    let chong = keys("chong");
    let token = store.add_phrase(text, &zhong, Some(7)).unwrap();
    store.add_phrase(text, &chong, Some(9)).unwrap();

    let phrase: UserPhrase = store.phrase(token).unwrap().expect("stored");
    assert_eq!(phrase.pronunciations().len(), 2, "both readings stored");

    store.add_phrase(text, &zhong, Some(3)).unwrap();
    let phrase = store.phrase(token).unwrap().expect("stored");
    let zhong_count = phrase
        .pronunciations()
        .iter()
        .find(|p| p.keys() == zhong.as_slice())
        .expect("zhong reading")
        .count();
    assert_eq!(zhong_count, 10, "same reading accumulates");

    // Removing the phrase takes every reading with it.
    assert!(store.remove_user_phrase(token).unwrap());
    assert!(store.phrase(token).unwrap().is_none());
}

#[test]
fn adding_an_existing_phrase_returns_the_same_token() {
    // Upstream chewing has no token exposure; the oxpinyin seam guarantees
    // one token per phrase text, so a re-add is an update, not a duplicate.
    let mut store = UserStore::create_standalone(&temp_path("phrase-same-token")).unwrap();
    let (text, reading) = user_phrase("你好", "ni,hao");
    let first = store.add_phrase(&text, &reading, None).unwrap();
    let second = store.add_phrase(&text, &reading, None).unwrap();
    assert_eq!(first, second);
    assert_eq!(store.phrases().unwrap().len(), 1);
}

#[test]
fn phrase_length_ceiling_rejects_sixteen_characters() {
    // Upstream rejects a 12-syllable phrase and accepts 11. The oxpinyin
    // ceiling is MAX_PHRASE_LENGTH (16): 15 characters are accepted, 16
    // are `InvalidPhrase` (the check is a strict `<`).
    let mut store = UserStore::create_standalone(&temp_path("phrase-length")).unwrap();

    let ok: String = "测".repeat(15);
    let token = store.add_phrase(&ok, &[key("ce"); 15], None).unwrap();
    assert!(store.phrase(token).unwrap().is_some());

    let too_long: String = "测".repeat(16);
    assert!(matches!(
        store.add_phrase(&too_long, &[key("ce"); 16], None),
        Err(UserStoreError::InvalidPhrase)
    ));
}

#[test]
fn key_count_must_match_the_phrase_length() {
    // Upstream rejects a phrase/bopomofo length mismatch for add and
    // remove alike; oxpinyin rejects it at add time with `InvalidPhrase`.
    let mut store = UserStore::create_standalone(&temp_path("phrase-keys")).unwrap();
    assert!(matches!(
        store.add_phrase("测试", &keys("ce"), None),
        Err(UserStoreError::InvalidPhrase)
    ));
    assert!(matches!(
        store.add_phrase("测试", &keys("ce,shi,de"), None),
        Err(UserStoreError::InvalidPhrase)
    ));
    assert!(matches!(
        store.add_phrase("", &[], None),
        Err(UserStoreError::InvalidPhrase)
    ));
}

#[test]
fn removing_an_absent_phrase_is_not_an_error() {
    // Upstream: removing an absent phrase returns 0. oxpinyin: Ok(false).
    let mut store = UserStore::create_standalone(&temp_path("phrase-absent")).unwrap();
    assert!(!store.remove_user_phrase(FIRST_USER_TOKEN).unwrap());
}

#[test]
fn learning_grows_unigrams_and_bigrams_differently() {
    // Upstream test_ngram asserts frequencies land in the gram; upstream
    // chewing asserts auto-learn persists after commit. The oxpinyin store
    // records a selection as a seeded bigram count plus a unigram delta;
    // without the explicit observe call nothing changes (learning is
    // caller-driven, `docs/findings/user-store.md` §2).
    let mut store = UserStore::create_standalone(&temp_path("observe")).unwrap();
    let (a, b) = (FIRST_USER_TOKEN, FIRST_USER_TOKEN + 1);

    let before = store.count_delta(Some(a), b).unwrap();
    assert_eq!(before, oxpinyin_core::UserCountDelta::ZERO);

    store.observe_selection(a, b).unwrap();

    let delta = store.count_delta(Some(a), b).unwrap();
    assert!(delta.bigram_count > 0, "the bigram row is seeded");
    assert_eq!(delta.bigram_total, store.bigram_total(a).unwrap());
    assert!(delta.unigram_delta > 0, "the unigram delta moves");
}
