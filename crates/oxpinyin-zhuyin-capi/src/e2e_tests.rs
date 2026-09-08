//! End-to-end tests: the zhuyin training, import, and dictionary entry
//! points, driven through the C ABI functions themselves — the battery
//! mirroring `oxpinyin-capi`'s e2e suite (the review that found this
//! crate had only a header-identity check). The unit tests call the
//! `#[unsafe(no_mangle)]` symbols directly (they are ordinary crate
//! functions when compiled for testing) and read the counts back
//! through the instance's own store handle — the committed state,
//! exactly as an export would observe it. The system tables are the
//! committed mini fixture (`fixtures/w3`); no model bytes are added by
//! these tests.
//!
//! `su3cl3` is 你好 on the standard chewing keyboard (the `'`-joined
//! session buffer `ni3'hao3`), the same composition the cursor tests in
//! `lib.rs` pin.

use std::os::raw::c_char;
use std::ptr;

use oxpinyin_user::{SENTENCE_START, USER_DICTIONARY};

use crate::candidates::{zhuyin_choose_candidate, zhuyin_get_n_candidate, zhuyin_train};
use crate::context::{zhuyin_fini, zhuyin_init, zhuyin_save};
use crate::instance::{zhuyin_alloc_instance, zhuyin_free_instance, zhuyin_reset};
use crate::iterators::{
    zhuyin_begin_add_phrases, zhuyin_end_add_phrases, zhuyin_iterator_add_phrase,
};
use crate::parse::{zhuyin_get_parsed_input_length, zhuyin_parse_more_chewings};
use crate::phrase::{zhuyin_get_n_phrase, zhuyin_get_phrase_token, zhuyin_phrase_segment};
use crate::sentence::{
    zhuyin_get_sentence, zhuyin_guess_sentence, zhuyin_guess_sentence_with_prefix,
};
use crate::test_support::{
    TempUserDir, candidate, close, cstr, open, open_with_user, take_sentence, token_of, with_store,
};

/// The choose-then-train seed sequence through the wired zhuyin path:
/// the doubling counts (69, 138, 414, …) with `sentence_start` as the
/// first predecessor, and the fresh-composition reset contract.
#[test]
fn train_records_the_pinned_doubling_sequence() {
    let user_dir = TempUserDir::new("train-doubling");
    let (context, instance) = open_with_user(&user_dir.path);

    let first = candidate(instance, "su3cl3", 0);
    let t1 = token_of(instance, first);
    assert!(zhuyin_choose_candidate(instance, 0, first) > 0);

    // 69 on first selection; the predecessor is sentence_start.
    assert!(zhuyin_train(instance));
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 69);
        assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), 69);
        assert_eq!(store.unigram_delta(t1).unwrap(), 483); // 69 * 7
    });

    // 138 on reselection (count 207), then 414 (count 621).
    assert!(zhuyin_train(instance));
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 207);
    });
    assert!(zhuyin_train(instance));
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 621);
        assert_eq!(store.unigram_delta(t1).unwrap(), 483 + 966 + 2898);
    });

    // A new composition starts fresh at sentence_start: the explicit
    // reset (the frontend's reset-on-commit contract), then a different
    // row trains from 69 again with no cross-sentence bigram.
    assert!(zhuyin_reset(instance));
    let second = candidate(instance, "su3cl3", 1);
    let t2 = token_of(instance, second);
    assert_ne!(t1, t2, "distinct rows carry distinct tokens");
    assert!(zhuyin_choose_candidate(instance, 0, second) > 0);
    assert!(zhuyin_train(instance));
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t2).unwrap(), 69);
        assert_eq!(store.bigram_count(t1, t2).unwrap(), 0);
    });

    close(context, instance);
}

/// `zhuyin_train` refuses without a selection to train (nothing was
/// chosen) and without a user store (nowhere to write) — the two false
/// arms of the facade's train law.
#[test]
fn train_refuses_without_a_selection_or_a_user_store() {
    // A guess alone selects nothing: train must refuse.
    let user_dir = TempUserDir::new("train-noselect");
    let (context, instance) = open_with_user(&user_dir.path);
    let input = cstr("su3cl3");
    assert_eq!(zhuyin_parse_more_chewings(instance, input.as_ptr()), 6);
    assert!(zhuyin_guess_sentence(instance));
    assert!(!zhuyin_train(instance), "no choose happened yet");
    close(context, instance);

    // No user dir: the corpus driver's shape — training degrades to
    // refusing, never to a write.
    let (context, instance) = open();
    let chosen = candidate(instance, "su3cl3", 0);
    assert!(zhuyin_choose_candidate(instance, 0, chosen) > 0);
    assert!(!zhuyin_train(instance), "no user store to train into");
    close(context, instance);
}

/// `zhuyin_save` is the §4 gated save: false on a clean store, true
/// after a training write dirties it, false again once saved.
#[test]
fn save_is_the_gated_dirty_flag() {
    let user_dir = TempUserDir::new("save-gate");
    let (context, instance) = open_with_user(&user_dir.path);
    assert!(
        !zhuyin_save(context),
        "a freshly opened store is clean (§4)"
    );

    let chosen = candidate(instance, "su3cl3", 0);
    assert!(zhuyin_choose_candidate(instance, 0, chosen) > 0);
    assert!(zhuyin_train(instance));
    assert!(zhuyin_save(context), "the training write dirtied it");
    assert!(
        !zhuyin_save(context),
        "the save cleared the dirty flag (§4)"
    );

    close(context, instance);
}

/// The add-phrases trio writes the USER index per-phrase committed and
/// arms the dirty flag at end (upstream `pinyin.cpp:657-658`), so the
/// rows appear in the store's §9 export and the next save compacts.
#[test]
fn add_phrase_batch_writes_the_user_index() {
    let user_dir = TempUserDir::new("add-batch");
    let (context, instance) = open_with_user(&user_dir.path);

    let iter = zhuyin_begin_add_phrases(context, USER_DICTIONARY);
    assert!(!iter.is_null());
    let phrase = cstr("网词");
    let pinyin = cstr("wangci");
    assert!(zhuyin_iterator_add_phrase(
        iter,
        phrase.as_ptr(),
        pinyin.as_ptr(),
        5,
    ));
    // A pinyin that parses to no complete key path is refused, not
    // stored half-way.
    let bad = cstr("vvvvx");
    assert!(!zhuyin_iterator_add_phrase(
        iter,
        phrase.as_ptr(),
        bad.as_ptr(),
        1,
    ));
    zhuyin_end_add_phrases(iter);

    let rows = with_store(instance, |store| {
        store
            .export_phrases_in(USER_DICTIONARY)
            .expect("export the user index")
    });
    assert_eq!(
        rows,
        vec![oxpinyin_user::ExportedPhrase {
            text: "网词".to_owned(),
            pinyin: "wang'ci".to_owned(),
            count: 5,
        }]
    );
    assert!(zhuyin_save(context), "the batch armed the dirty flag");

    close(context, instance);
}

/// The sentence surface: `zhuyin_guess_sentence` then `get_sentence`
/// yields the decoded composition, and prefix seeding keeps it.
#[test]
fn sentence_row_and_prefix_seeding() {
    let (context, instance) = open();
    let input = cstr("su3cl3");
    assert_eq!(zhuyin_parse_more_chewings(instance, input.as_ptr()), 6);

    assert!(zhuyin_guess_sentence(instance));
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_sentence(instance, &raw mut sentence));
    assert_eq!(take_sentence(sentence), "你好");

    // Re-guess seeded with the first character as a prefix token: the
    // sentence keeps starting there.
    let prefix = cstr("你");
    assert!(zhuyin_guess_sentence_with_prefix(instance, prefix.as_ptr()));
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_sentence(instance, &raw mut sentence));
    assert_eq!(take_sentence(sentence), "你好");

    close(context, instance);
}

/// `zhuyin_phrase_segment` fills the phrase result the token getters
/// read back: one token for the fixture's 你好.
#[test]
fn phrase_segment_fills_the_token_result() {
    let (context, instance) = open();
    let sentence = cstr("你好");
    assert!(zhuyin_phrase_segment(instance, sentence.as_ptr()));

    let mut count = 0;
    assert!(zhuyin_get_n_phrase(instance, &raw mut count));
    // The mini fixture's merged table carries 你 and 好 individually, so
    // the best path over it segments the pair as two one-character
    // phrases — the result array is fully readable either way.
    assert_eq!(count, 2, "the fixture segments 你好 character-wise");
    let mut first = 0;
    assert!(zhuyin_get_phrase_token(instance, 0, &raw mut first));
    assert_ne!(first, 0, "a matched phrase carries a real token");
    let mut second = 0;
    assert!(zhuyin_get_phrase_token(instance, 1, &raw mut second));
    assert_ne!(second, 0, "every matched phrase carries a real token");

    // Invalid UTF-8 is refused, not stored (the pin's g_return_val_if_fail
    // gate, zhuyin.cpp:965).
    let invalid = std::ffi::CString::new([0xFFu8, 0xFE]).expect("no interior NUL");
    assert!(!zhuyin_phrase_segment(instance, invalid.as_ptr()));

    close(context, instance);
}

/// `zhuyin_reset` clears the composition: the parsed input length drops
/// to zero and the sentence getter has nothing to return.
#[test]
fn reset_clears_the_parse() {
    let (context, instance) = open();
    let input = cstr("su3cl3");
    assert_eq!(zhuyin_parse_more_chewings(instance, input.as_ptr()), 6);
    assert_eq!(zhuyin_get_parsed_input_length(instance), 6);

    assert!(zhuyin_reset(instance));
    assert_eq!(
        zhuyin_get_parsed_input_length(instance),
        0,
        "reset dropped the parsed keys"
    );
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(
        !zhuyin_get_sentence(instance, &raw mut sentence),
        "nothing is composed after the reset"
    );

    close(context, instance);
}

/// Null and empty inputs are refused, never dereferenced: the
/// availability-class law every entry point carries.
#[test]
fn null_and_empty_inputs_are_refused() {
    // Empty system dir: init's NULL.
    let empty = cstr("");
    assert!(zhuyin_init(empty.as_ptr(), empty.as_ptr()).is_null());

    // Null handles: every entry point answers its false shape.
    assert!(zhuyin_alloc_instance(ptr::null_mut()).is_null());
    assert!(!zhuyin_train(ptr::null_mut()));
    assert!(!zhuyin_guess_sentence(ptr::null_mut()));
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(!zhuyin_get_sentence(ptr::null_mut(), &raw mut sentence));
    let mut count = 0;
    assert!(!zhuyin_get_n_candidate(ptr::null_mut(), &raw mut count));
    assert_eq!(
        zhuyin_choose_candidate(ptr::null_mut(), 0, ptr::null_mut()),
        -1
    );
    assert!(!zhuyin_save(ptr::null_mut()));
    assert!(!zhuyin_reset(ptr::null_mut()));
    assert_eq!(
        zhuyin_parse_more_chewings(ptr::null_mut(), empty.as_ptr()),
        0
    );
    // The free paths are no-ops on null.
    zhuyin_free_instance(ptr::null_mut());
    zhuyin_fini(ptr::null_mut());
}
