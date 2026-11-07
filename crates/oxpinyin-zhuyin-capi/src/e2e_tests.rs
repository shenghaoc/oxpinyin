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

#[cfg(windows)]
#[test]
fn utf8_c_api_paths_open_cjk_directories_and_files() {
    let root = tempfile::tempdir().unwrap();
    let system = root.path().join("系统");
    let user = root.path().join("用户");
    std::fs::create_dir(&system).unwrap();
    std::fs::create_dir(&user).unwrap();
    for entry in std::fs::read_dir(crate::test_support::system_dir()).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), system.join(entry.file_name())).unwrap();
    }
    std::fs::rename(system.join("gb_char.bin"), system.join("汉字.bin")).unwrap();
    let conf = std::fs::read_to_string(system.join("table.conf")).unwrap();
    std::fs::write(
        system.join("table.conf"),
        conf.replace("gb_char.bin", "汉字.bin"),
    )
    .unwrap();
    let system = crate::test_support::cstr(system.to_str().unwrap());
    let user_name = crate::test_support::cstr(user.to_str().unwrap());
    let context = crate::context::zhuyin_init(system.as_ptr(), user_name.as_ptr());
    assert!(!context.is_null());
    // End arms the writer without changing the phrase data.
    zhuyin_end_add_phrases(zhuyin_begin_add_phrases(context, USER_DICTIONARY));
    assert!(zhuyin_save(context));
    crate::context::zhuyin_fini(context);
    assert!(user.join("user.conf").is_file());
}

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
    TempUserDir, candidate, close, cstr, open, open_with_user, system_dir, take_sentence, token_of,
    with_store,
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
    assert!(
        !zhuyin_train(instance),
        "a choose without decoded results refuses"
    );
    // The system item's live count, as the chunk shipped it.
    let shipped = with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 0);
        store.system_unigram(t1).expect("item")
    });

    // 69 on first selection; the predecessor is sentence_start.
    assert!(zhuyin_guess_sentence(instance));
    assert!(zhuyin_train(instance));
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 69);
        assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), 69);
        assert_eq!(store.system_unigram(t1), Some(shipped + 483)); // 69 * 7
    });

    // 138 on reselection (count 207), then 414 (count 621).
    assert!(zhuyin_guess_sentence(instance));
    assert!(zhuyin_train(instance));
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 207);
    });
    assert!(zhuyin_guess_sentence(instance));
    assert!(zhuyin_train(instance));
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 621);
        assert_eq!(store.system_unigram(t1), Some(shipped + 483 + 966 + 2898));
    });

    // A new composition starts fresh at sentence_start: the explicit
    // reset (the frontend's reset-on-commit contract), then a different
    // row trains from 69 again with no cross-sentence bigram.
    assert!(zhuyin_reset(instance));
    let second = candidate(instance, "su3cl3", 1);
    let t2 = token_of(instance, second);
    assert_ne!(t1, t2, "distinct rows carry distinct tokens");
    assert!(zhuyin_choose_candidate(instance, 0, second) > 0);
    assert!(zhuyin_guess_sentence(instance));
    assert!(zhuyin_train(instance));
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t2).unwrap(), 69);
        assert_eq!(store.bigram_count(t1, t2).unwrap(), 0);
    });

    close(context, instance);
}

/// `zhuyin_train` refuses without a sentence result (no guess ran) and
/// without a user store (nowhere to write) — the two false arms of the
/// facade's train law, `zhuyin_train`'s own gates
/// (`zhuyin.cpp:1696-1706`: user dir, then `results.size()`).
#[test]
fn train_refuses_without_a_selection_or_a_user_store() {
    // A guess alone runs the train and writes nothing: the pin's
    // `train_result3` walks a constraint-free result and answers true
    // (`phonetic_lookup.h:844-936`), so `zhuyin_train` answers true
    // after `zhuyin_guess_sentence` with no choose behind it.
    let user_dir = TempUserDir::new("train-noselect");
    let (context, instance) = open_with_user(&user_dir.path);
    let input = cstr("su3cl3");
    assert_eq!(zhuyin_parse_more_chewings(instance, input.as_ptr()), 6);
    assert!(zhuyin_guess_sentence(instance));
    assert!(
        zhuyin_train(instance),
        "the live sentence result admits the train"
    );
    with_store(instance, |store| {
        // The constraint-free walk writes nothing: no bigram anywhere.
        assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), 0);
    });
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
    assert!(zhuyin_guess_sentence(instance));
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
    // The reading is bopomofo, parsed by `ZhuyinDirectParser2` under
    // `USE_TONE | FORCE_TONE` (`zhuyin.cpp:515-523`): the tones ride on
    // the stored keys.
    let reading = cstr("ㄨㄤˇ ㄘˊ");
    assert!(zhuyin_iterator_add_phrase(
        iter,
        phrase.as_ptr(),
        reading.as_ptr(),
        5,
    ));
    // A romanized reading parses no bopomofo key and is refused, not
    // stored half-way.
    let romanized = cstr("wangci");
    assert!(!zhuyin_iterator_add_phrase(
        iter,
        phrase.as_ptr(),
        romanized.as_ptr(),
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
            text: "网词".chars().map(u32::from).collect(),
            pinyin: "wang3'ci2".to_owned(),
            count: 5,
        }]
    );
    assert!(zhuyin_save(context), "the batch armed the dirty flag");

    close(context, instance);
}

#[test]
fn token_unigram_distinguishes_zero_user_item_from_absent() {
    use crate::dict::{zhuyin_token_add_unigram_frequency, zhuyin_token_get_unigram_frequency};

    let user_dir = TempUserDir::new("dict-unigram-zero");
    let (context, instance) = open_with_user(&user_dir.path);
    let iter = zhuyin_begin_add_phrases(context, USER_DICTIONARY);
    assert!(!iter.is_null());
    assert!(zhuyin_iterator_add_phrase(
        iter,
        cstr("网词").as_ptr(),
        cstr("ㄨㄤˇ ㄘˊ").as_ptr(),
        0,
    ));
    zhuyin_end_add_phrases(iter);
    let token = with_store(instance, |store| store.phrases().unwrap()[0].token());
    assert_eq!(token >> 24, u32::from(USER_DICTIONARY));
    let mut freq = u32::MAX;
    assert!(zhuyin_token_get_unigram_frequency(
        instance,
        token,
        &raw mut freq
    ));
    assert_eq!(freq, 0);
    freq = u32::MAX;
    assert!(!zhuyin_token_get_unigram_frequency(
        instance,
        token + 1,
        &raw mut freq
    ));
    assert_eq!(freq, 0);
    assert!(zhuyin_token_add_unigram_frequency(instance, token, 7));
    assert!(zhuyin_token_get_unigram_frequency(
        instance,
        token,
        &raw mut freq
    ));
    assert_eq!(freq, 7);
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

/// #523's libzhuyin half: `zhuyin_init` only reads `user.conf` and
/// `zhuyin_fini` writes nothing (`zhuyin.cpp:126-162`, `:741-757`); the
/// one write is `zhuyin_save`'s `mark_version`, whose fresh
/// `UserTableInfo` carries a counter of 0 (`:164-176`, `:695`). No run of
/// launches can raise the counter toward the limit, so none can wipe.
#[test]
fn zhuyin_launches_never_move_the_open_counter() {
    let user_dir = TempUserDir::new("zhuyin-open-counter");
    let marker = user_dir.path.join("user.conf");

    let (context, instance) = open_with_user(&user_dir.path);
    assert!(!marker.exists(), "zhuyin_init wrote user.conf");
    // End arms m_modified with or without an add (zhuyin.cpp:543).
    zhuyin_end_add_phrases(zhuyin_begin_add_phrases(context, USER_DICTIONARY));
    assert!(zhuyin_save(context));
    let saved = std::fs::read(&marker).expect("zhuyin_save writes user.conf");
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    assert!(
        String::from_utf8_lossy(&saved).ends_with(&format!("open counter:0{newline}")),
        "zhuyin_save's marker carries a fresh counter"
    );
    close(context, instance);
    assert_eq!(
        std::fs::read(&marker).expect("marker"),
        saved,
        "zhuyin_fini wrote user.conf"
    );

    for _ in 0..10 {
        let (context, instance) = open_with_user(&user_dir.path);
        close(context, instance);
    }
    assert_eq!(std::fs::read(&marker).expect("marker"), saved);
}

/// Issue #602: a choose at the cursor after an earlier choose writes its
/// forcing on the matrix column the whole-buffer re-guess walks, so the
/// forcing survives `zhuyin_guess_sentence` and the sentence keeps the
/// chosen row — the pin's `add_constraint(m_begin, m_end, token)` at
/// `m_begin = offset` (`zhuyin.cpp:1649-1654`, `:1460`), kept by
/// `validate_constraint` (`phonetic_lookup.cpp:120-167`). The row chosen
/// is one the unconstrained decode would not pick, so a dropped forcing
/// shows in the sentence.
#[test]
fn a_choose_at_the_cursor_survives_the_next_guess() {
    use crate::candidates::zhuyin_get_candidate;
    use crate::sentence::zhuyin_guess_candidates_after_cursor;
    use crate::test_support::candidate_text;

    let (context, instance) = open();
    let first = (0..8)
        .find(|index| {
            let _ = candidate(instance, "su3cl3", 0);
            candidate_text(instance, *index) == "你"
        })
        .expect("你 is offered at 0");
    let cand = candidate(
        instance,
        "su3cl3",
        u32::try_from(first).expect("small index"),
    );
    assert_eq!(zhuyin_choose_candidate(instance, 0, cand), 3);
    assert!(zhuyin_guess_sentence(instance));
    assert!(zhuyin_guess_candidates_after_cursor(instance, 3));
    let mut count = 0;
    assert!(zhuyin_get_n_candidate(instance, &raw mut count));
    let other = (0..usize::try_from(count).expect("count fits"))
        .find(|index| {
            let text = candidate_text(instance, *index);
            text.chars().count() == 1 && text != "好"
        })
        .expect("a one-character row other than 好 is offered at 3");
    let text = candidate_text(instance, other);
    let mut chosen: *mut crate::types::LookupCandidate = ptr::null_mut();
    assert!(zhuyin_get_candidate(
        instance,
        u32::try_from(other).expect("small index"),
        &raw mut chosen
    ));
    assert_eq!(zhuyin_choose_candidate(instance, 3, chosen), 6);
    assert!(zhuyin_guess_sentence(instance));
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_sentence(instance, &raw mut sentence));
    assert_eq!(
        take_sentence(sentence),
        format!("你{text}"),
        "the forcing at the cursor survived the re-guess"
    );
    close(context, instance);
}

/// The same law after a MULTI-character choice: a choose of a phrase, a
/// later choose at the cursor the phrase left, and a whole-sentence
/// re-guess keep both forcings (`zhuyin.cpp:1649-1654`, `:1460`;
/// `phonetic_lookup.cpp:120-167`). The second row is one the
/// unconstrained decode of the tail would not pick, so a dropped later
/// forcing shows in the sentence.
#[test]
fn a_choose_at_the_cursor_survives_the_next_guess_after_a_phrase() {
    use crate::candidates::zhuyin_get_candidate;
    use crate::sentence::zhuyin_guess_candidates_after_cursor;
    use crate::test_support::candidate_text;

    let keys = "su3cl3su3";
    let (context, instance) = open();
    let phrase = (0..16)
        .find(|index| {
            let _ = candidate(instance, keys, 0);
            candidate_text(instance, *index) == "你好"
        })
        .expect("the two-character phrase 你好 is offered at 0");
    let cand = candidate(instance, keys, u32::try_from(phrase).expect("small index"));
    assert_eq!(zhuyin_choose_candidate(instance, 0, cand), 6);
    assert!(zhuyin_guess_sentence(instance));
    let mut unconstrained: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_sentence(instance, &raw mut unconstrained));
    let default_tail = take_sentence(unconstrained)
        .chars()
        .last()
        .expect("the re-guessed sentence is not empty")
        .to_string();

    assert!(zhuyin_guess_candidates_after_cursor(instance, 6));
    let mut count = 0;
    assert!(zhuyin_get_n_candidate(instance, &raw mut count));
    let other = (0..usize::try_from(count).expect("count fits"))
        .find(|index| {
            let text = candidate_text(instance, *index);
            text.chars().count() == 1 && text != default_tail
        })
        .expect("a one-character row other than the decode's pick is offered at 6");
    let text = candidate_text(instance, other);
    let mut chosen: *mut crate::types::LookupCandidate = ptr::null_mut();
    assert!(zhuyin_get_candidate(
        instance,
        u32::try_from(other).expect("small index"),
        &raw mut chosen
    ));
    assert_eq!(zhuyin_choose_candidate(instance, 6, chosen), 9);
    assert!(zhuyin_guess_sentence(instance));
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_sentence(instance, &raw mut sentence));
    assert_eq!(
        take_sentence(sentence),
        format!("你好{text}"),
        "both forcings survived the whole-sentence re-guess"
    );
    close(context, instance);
}

/// The `ZHUYIN_STANDARD_DVORAK` slot aborts the process, as the pin's
/// `set_scheme` falls through to `abort()` (`zhuyin_parser2.cpp:295`,
/// register row 5c).
#[test]
fn the_dvorak_chewing_scheme_aborts() {
    crate::test_support::assert_aborts("e2e_tests::the_dvorak_chewing_scheme_aborts", || {
        let (context, _instance) = open();
        crate::config::zhuyin_set_chewing_scheme(context, 7);
    });
}

/// An out-of-enum keyboard (`zhuyin.cpp:736`) and a full-pinyin scheme
/// outside 1-3 (`pinyin_parser2.cpp:398`) abort the process (register row
/// 5d).
#[test]
fn an_out_of_enum_chewing_scheme_aborts() {
    crate::test_support::assert_aborts("e2e_tests::an_out_of_enum_chewing_scheme_aborts", || {
        let (context, _instance) = open();
        crate::config::zhuyin_set_chewing_scheme(context, 10);
    });
}

#[test]
fn an_out_of_range_full_pinyin_scheme_aborts() {
    crate::test_support::assert_aborts(
        "e2e_tests::an_out_of_range_full_pinyin_scheme_aborts",
        || {
            let (context, _instance) = open();
            crate::config::zhuyin_set_full_pinyin_scheme(context, 4);
        },
    );
}

/// An apostrophe in the single-key parser's input is the shared
/// `parse_one_key` assert (`pinyin_parser2.cpp:170`, register row 21).
#[test]
fn a_single_key_apostrophe_aborts() {
    crate::test_support::assert_aborts("e2e_tests::a_single_key_apostrophe_aborts", || {
        let (_context, instance) = open();
        let text = cstr("n'i");
        let mut key = crate::types::ChewingKey::ZERO;
        crate::keys::zhuyin_parse_full_pinyin(instance, text.as_ptr(), &raw mut key);
    });
}

/// An unload index of 16 or more is the unload's assert (`zhuyin.cpp:381`,
/// register row 66).
#[test]
fn an_unload_index_past_the_sub_indices_aborts() {
    crate::test_support::assert_aborts(
        "e2e_tests::an_unload_index_past_the_sub_indices_aborts",
        || {
            let (context, _instance) = open();
            crate::config::zhuyin_unload_phrase_library(context, 16);
        },
    );
}

/// Register row 66: on a fresh instance `offset >= matrix.size() - 1` is
/// `offset >= SIZE_MAX` (`zhuyin.cpp:1892-1893`, `:1914-1915`), so
/// `SIZE_MAX` answers `false` with the out-param NULLed.
#[test]
fn the_key_accessors_answer_false_at_size_max_on_an_empty_matrix() {
    let (context, instance) = open();
    let mut key = std::ptr::NonNull::<crate::types::ChewingKey>::dangling().as_ptr();
    assert!(!crate::cursor::zhuyin_get_zhuyin_key(
        instance,
        usize::MAX,
        &raw mut key
    ));
    assert!(key.is_null());
    let mut rest = std::ptr::NonNull::<crate::types::ChewingKeyRest>::dangling().as_ptr();
    assert!(!crate::cursor::zhuyin_get_zhuyin_key_rest(
        instance,
        usize::MAX,
        &raw mut rest
    ));
    assert!(rest.is_null());
    close(context, instance);
}

/// Past byte 65535 the key-rest positions narrow into the pin's `guint16`
/// fields mod 2^16 (maintainer ruling 2026-11-01 UTC,
/// `docs/findings/1501-guint16-key-positions.md`). `su3` then `su`
/// repeated puts a key at 65535..65537, which crosses byte 65536, and one
/// at 65537..65539, wholly past it; both lengths stay 2.
#[test]
fn key_rest_positions_past_byte_65535_wrap() {
    let (context, instance) = open();
    assert!(crate::config::zhuyin_set_options(context, 1 << 5));
    assert!(crate::config::zhuyin_set_chewing_scheme(context, 1));
    let input = format!("su3{}", "su".repeat(32_769));
    assert_eq!(
        zhuyin_parse_more_chewings(instance, cstr(&input).as_ptr()),
        65_541
    );
    for (offset, begin, end) in [(65_535_usize, 65_535_u16, 1_u16), (65_537, 1, 3)] {
        let mut rest: *mut crate::types::ChewingKeyRest = ptr::null_mut();
        assert!(crate::cursor::zhuyin_get_zhuyin_key_rest(
            instance,
            offset,
            &raw mut rest
        ));
        let (mut b, mut e) = (0_u16, 0_u16);
        assert!(crate::cursor::zhuyin_get_zhuyin_key_rest_positions(
            instance, rest, &raw mut b, &raw mut e
        ));
        assert_eq!((b, e), (begin, end), "offset {offset}");
        let mut length = 0_u16;
        assert!(crate::cursor::zhuyin_get_zhuyin_key_rest_length(
            instance,
            rest,
            &raw mut length
        ));
        assert_eq!(length, 2, "offset {offset}");
    }
    close(context, instance);
}

/// Imports 你好 into libraries `1..=libraries` of the fixture, which loads
/// libraries 1 to 7, so the phrase-table search holds `libraries` tokens.
fn hold_nihao_in_libraries(context: *mut crate::types::ZhuyinContext, libraries: u8) {
    for library in 1..=libraries {
        let iter = zhuyin_begin_add_phrases(context, library);
        assert!(zhuyin_iterator_add_phrase(
            iter,
            cstr("你好").as_ptr(),
            cstr("ㄋㄧˇ ㄏㄠˇ").as_ptr(),
            1,
        ));
        zhuyin_end_add_phrases(iter);
    }
}

/// `reduce_tokens` clears and fills the caller's array before it validates
/// the count (`phrase_large_table3.h:79-95`). On a NULL array each GLib
/// call logs a critical: one for `g_array_set_size`, one
/// `g_array_append_vals` per loaded library (`phrase_index.h:762-772`).
/// The pin then dies of the assert on five tokens (`zhuyin.cpp:1690`), so
/// the criticals come first. GLib's default handler writes them to stderr.
#[test]
fn lookup_tokens_into_a_null_array_logs_the_criticals_before_it_aborts() {
    let user_dir = TempUserDir::new("lookup-null-five");
    let path = user_dir.path.clone();
    let stderr = crate::test_support::assert_aborts(
        "e2e_tests::lookup_tokens_into_a_null_array_logs_the_criticals_before_it_aborts",
        || {
            // SAFETY: GLib's own default handler, installed for every domain.
            unsafe {
                glib_sys::g_log_set_default_handler(
                    Some(glib_sys::g_log_default_handler),
                    ptr::null_mut(),
                );
            }
            let (context, instance) = open_with_user(&path);
            hold_nihao_in_libraries(context, 5);
            crate::dict::zhuyin_lookup_tokens(instance, cstr("你好").as_ptr(), ptr::null_mut());
        },
    );
    let (context, instance) = open_with_user(&path);
    // SAFETY: `instance` is live until `close` below, and the reference is
    // not used after that.
    let inst = unsafe { crate::state::instance_ref(instance) };
    let loaded = (0..16_u32)
        .filter(|library| inst.core.dict.phrase_table_token_enabled(library << 24))
        .count();
    close(context, instance);
    let stderr = String::from_utf8_lossy(&stderr);
    let criticals: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("CRITICAL"))
        .collect();
    assert!(loaded > 0);
    assert_eq!(criticals.len(), 1 + loaded, "{stderr}");
    assert!(criticals[0].contains("g_array_set_size"), "{stderr}");
    assert!(
        criticals[1..]
            .iter()
            .all(|line| line.contains("g_array_append_vals")),
        "{stderr}"
    );
}

/// Four tokens pass `reduce_tokens`' count, so the NULL-array lookup
/// answers instead of aborting.
#[test]
fn lookup_tokens_into_a_null_array_answers_with_four_tokens() {
    let user_dir = TempUserDir::new("lookup-null-four");
    let (context, instance) = open_with_user(&user_dir.path);
    hold_nihao_in_libraries(context, 4);
    assert!(crate::dict::zhuyin_lookup_tokens(
        instance,
        cstr("你好").as_ptr(),
        ptr::null_mut()
    ));
    close(context, instance);
}

/// Opens `user_dir` over the fixture, answering the raw handle.
fn init_at(user_dir: &std::path::Path) -> *mut crate::types::ZhuyinContext {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir.to_str().expect("UTF-8 path"));
    zhuyin_init(system.as_ptr(), user.as_ptr())
}

/// Register row 91: init's library loop merges `gb_char.dbin` and
/// `next_record` asserts on its non-null `MODIFY_HEADER` token
/// (`phrase_index_logger.h:202`).
#[test]
fn init_aborts_on_a_log_header_token() {
    crate::test_support::assert_aborts("e2e_tests::init_aborts_on_a_log_header_token", || {
        let user_dir = TempUserDir::new("init-header-token");
        crate::test_support::plant_log(
            &user_dir.path,
            "gb_char.dbin",
            &crate::test_support::logs::header_token(),
        );
        let _ = init_at(&user_dir.path);
    });
}

/// Register row 91 before row 76: library 1's merge asserts before the loop
/// reaches library 4 (`zhuyin.cpp:220` before `:200`), so a later chunk the
/// pin never opens cannot turn the abort into a NULL answer.
#[cfg(unix)]
#[test]
fn init_aborts_on_a_log_before_an_unmappable_library() {
    crate::test_support::assert_aborts(
        "e2e_tests::init_aborts_on_a_log_before_an_unmappable_library",
        || {
            let scratch = tempfile::tempdir().expect("scratch");
            let system = crate::test_support::private_system(scratch.path());
            std::fs::remove_file(system.join("merged.bin")).expect("unlink the library");
            std::fs::write(system.join("merged.bin"), b"abc").expect("short library");
            let user = TempUserDir::new("init-log-before-unmappable");
            crate::test_support::plant_log(
                &user.path,
                "gb_char.dbin",
                &crate::test_support::logs::header_token(),
            );
            let system_c = cstr(system.to_str().expect("UTF-8 path"));
            let user_c = cstr(user.path.to_str().expect("UTF-8 path"));
            let _ = zhuyin_init(system_c.as_ptr(), user_c.as_ptr());
        },
    );
}

/// Register row 108: init's merge stores a one-byte ADD payload and
/// `add_phrase_item` reads its unigram (`memory_chunk.h:390`).
#[test]
fn init_aborts_on_a_short_add_payload() {
    crate::test_support::assert_aborts("e2e_tests::init_aborts_on_a_short_add_payload", || {
        let user_dir = TempUserDir::new("init-short-add");
        crate::test_support::plant_log(
            &user_dir.path,
            "gb_char.dbin",
            &crate::test_support::logs::short_add(),
        );
        let _ = init_at(&user_dir.path);
    });
}

/// Register row 91's control: `merge` returns at a mismatched MODIFY before
/// `next_record` reads the bad header, and init completes.
#[test]
fn init_completes_when_the_merge_stops_before_a_bad_header() {
    let user_dir = TempUserDir::new("init-stop-before-header");
    let payload = [
        crate::test_support::logs::mismatched_modify(),
        crate::test_support::logs::header_token(),
    ]
    .concat();
    crate::test_support::plant_log(&user_dir.path, "gb_char.dbin", &payload);
    let context = init_at(&user_dir.path);
    assert!(!context.is_null());
    zhuyin_fini(context);
}

/// Register row 91 on the reload path: `zhuyin_load_phrase_library` of an
/// unloaded library merges its saved `.dbin` (`zhuyin.cpp:219`).
#[test]
fn reload_aborts_on_a_log_header_token() {
    crate::test_support::assert_aborts("e2e_tests::reload_aborts_on_a_log_header_token", || {
        let user_dir = TempUserDir::new("reload-header-token");
        crate::test_support::plant_log(&user_dir.path, "gbk_char.dbin", &[]);
        let context = init_at(&user_dir.path);
        assert!(!context.is_null());
        assert!(crate::config::zhuyin_unload_phrase_library(context, 2));
        crate::test_support::plant_log(
            &user_dir.path,
            "gbk_char.dbin",
            &crate::test_support::logs::header_token(),
        );
        let _ = crate::config::zhuyin_load_phrase_library(context, 2);
    });
}

/// Register row 97: `mask_out` reloads library 1 and `_mask_out_records`
/// appends a kept ADD with both chunk pointers (`phrase_index_logger.h:245`).
#[test]
fn mask_out_aborts_on_a_surviving_add() {
    crate::test_support::assert_aborts("e2e_tests::mask_out_aborts_on_a_surviving_add", || {
        let user_dir = TempUserDir::new("mask-surviving-add");
        crate::test_support::plant_log(&user_dir.path, "gb_char.dbin", &[]);
        let context = init_at(&user_dir.path);
        assert!(!context.is_null());
        let payload = [
            crate::test_support::logs::header(),
            crate::test_support::logs::add(),
        ]
        .concat();
        crate::test_support::plant_log(&user_dir.path, "gb_char.dbin", &payload);
        let _ = crate::config::zhuyin_mask_out(context, 0xffff_ffff, 0x0100_0001);
    });
}

/// Register row 85: `zhuyin_mask_out` loads token 1's gram, which the mask
/// leaves alone, and `SingleGram::mask_out` reads its total from three bytes
/// (`ngram.cpp:80`, `memory_chunk.h:390`).
#[test]
fn mask_out_aborts_on_a_short_bigram_value() {
    crate::test_support::assert_aborts(
        "e2e_tests::mask_out_aborts_on_a_short_bigram_value",
        || {
            let user_dir = TempUserDir::new("mask-short-gram");
            crate::test_support::plant_bigram(
                &user_dir.path,
                vec![(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
            );
            let context = init_at(&user_dir.path);
            assert!(!context.is_null());
            let _ = crate::config::zhuyin_mask_out(context, 0xffff_ffff, 0);
        },
    );
}

/// Row 85's control: a key the mask selects is removed without loading its
/// value (`ngram_bdb.cpp:231-238`), and the mask completes.
#[test]
fn mask_out_of_a_selected_short_key_completes() {
    let user_dir = TempUserDir::new("mask-short-gram-selected");
    crate::test_support::plant_bigram(
        &user_dir.path,
        vec![(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
    );
    let context = init_at(&user_dir.path);
    assert!(!context.is_null());
    assert!(crate::config::zhuyin_mask_out(context, 0xffff_ffff, 1));
    zhuyin_fini(context);
}

/// Whether a directory reads as a chunk on this host's temporary file
/// system (register row 96); where it does not, a directory library is row
/// 76's unmappable one (class (b)).
#[cfg(unix)]
fn directory_reads_as_a_chunk_here() -> bool {
    let probe = tempfile::tempdir().expect("probe dir");
    std::fs::write(probe.path().join("e".repeat(25)), b"x").expect("fill the probe");
    oxpinyin_data::phrase_library::directory_reads_as_a_chunk(probe.path())
}

/// One call over a private system directory whose library is a directory
/// (register row 96): init maps `merged.bin` (`zhuyin.cpp:199`), the save and
/// the mask-out map it again (`:589`, `:800`), and the reload of the
/// unloaded library 2 maps `gbk_char.bin` (`:200`).
#[cfg(unix)]
fn map_a_directory_library(site: &str) -> bool {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::private_system(scratch.path());
    let user = scratch.path().join("user");
    std::fs::create_dir_all(&user).expect("user dir");
    let system_c = cstr(system.to_str().expect("UTF-8 path"));
    let user_c = cstr(user.to_str().expect("UTF-8 path"));
    if site == "init" {
        crate::test_support::directory_library(&system, "merged.bin");
        return !zhuyin_init(system_c.as_ptr(), user_c.as_ptr()).is_null();
    }
    let context = zhuyin_init(system_c.as_ptr(), user_c.as_ptr());
    assert!(!context.is_null());
    match site {
        "save" => {
            // SAFETY: `context` is a live `zhuyin_init` handle, borrowed
            // uniquely for this statement.
            let ctx = unsafe { crate::state::context_mut(context) };
            ctx.core.user_store_mut().expect("store").mark_modified();
            crate::test_support::directory_library(&system, "merged.bin");
            zhuyin_save(context)
        }
        "mask" => {
            crate::test_support::directory_library(&system, "merged.bin");
            crate::config::zhuyin_mask_out(context, 0xff00_0000, 0x0400_0000)
        }
        _ => {
            assert!(crate::config::zhuyin_unload_phrase_library(context, 2));
            crate::test_support::directory_library(&system, "gbk_char.bin");
            crate::config::zhuyin_load_phrase_library(context, 2)
        }
    }
}

/// Register row 96 at init; row 76's NULL where a directory does not read
/// as a chunk.
#[cfg(unix)]
#[test]
fn init_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts("e2e_tests::init_aborts_on_a_directory_library", || {
            let _ = map_a_directory_library("init");
        });
    } else {
        assert!(!map_a_directory_library("init"));
    }
}

/// Register row 96 at save; row 76's completed save elsewhere.
#[cfg(unix)]
#[test]
fn save_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts("e2e_tests::save_aborts_on_a_directory_library", || {
            let _ = map_a_directory_library("save");
        });
    } else {
        assert!(map_a_directory_library("save"));
    }
}

/// Register row 96 at mask-out, after the user tables are masked.
#[cfg(unix)]
#[test]
fn mask_out_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts(
            "e2e_tests::mask_out_aborts_on_a_directory_library",
            || {
                let _ = map_a_directory_library("mask");
            },
        );
    } else {
        assert!(map_a_directory_library("mask"));
    }
}

/// Register row 96 at the reload of an unloaded library.
#[cfg(unix)]
#[test]
fn reload_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts(
            "e2e_tests::reload_aborts_on_a_directory_library",
            || {
                let _ = map_a_directory_library("reload");
            },
        );
    } else {
        assert!(map_a_directory_library("reload"));
    }
}

// ---------------------------------------------------------------------
// Ports of contract-diff cases whose pin side reads freed or
// heap-dependent memory (undefined behaviour), so no frozen pin
// reference can hold them. Each asserts oxpinyin's defined behaviour;
// the expectations come from the pin's source at 074a2219 and the
// project's rulings, not from running the pin.
// ---------------------------------------------------------------------

/// The `open <user dir>/user.conf failed.` line a fresh user directory
/// makes `zhuyin_init` write: `UserTableInfo::load` prints it when the
/// marker is missing, with no trailing newline (`table_info.cpp:332` at
/// 074a2219).
#[cfg(unix)]
fn fresh_user_conf_line(user_dir: &std::path::Path) -> String {
    format!("open {}/user.conf failed.", user_dir.display())
}

/// The zhuyin twin of audit D-06's row-42 witness
/// (`zhuyin-instance_outlives_context`): the pin's `zhuyin_guess_sentence`
/// hands the freed context to `get_nbest_match` (`zhuyin.cpp:895-909`) and
/// returns its result, so no pin answer is defined; the ruled safe answer
/// is `true`. A populated matrix reaches the lookup (`su3cl3` is 你好).
#[cfg(unix)]
#[test]
fn instance_outlives_context_guesses_true() {
    let run = crate::test_support::run_in_child(
        "e2e_tests::instance_outlives_context_guesses_true",
        |user| {
            let (context, instance) = open_with_user(user);
            assert_eq!(
                zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()),
                6
            );
            zhuyin_fini(context);
            assert!(zhuyin_guess_sentence(instance), "the ruled safe answer");
            zhuyin_free_instance(instance);
        },
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        fresh_user_conf_line(&run.user_dir)
    );
}

/// A second `zhuyin_fini` on one context (#573, register row 105): the pin
/// deletes the context twice (`zhuyin.cpp:741-757`) and dies of SIGSEGV;
/// the ruling is a silent return.
#[cfg(unix)]
#[test]
fn fini_twice_returns_silently() {
    let run = crate::test_support::run_in_child("e2e_tests::fini_twice_returns_silently", |user| {
        let (context, instance) = open_with_user(user);
        zhuyin_free_instance(instance);
        zhuyin_fini(context);
        zhuyin_fini(context);
    });
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        fresh_user_conf_line(&run.user_dir)
    );
}

/// GLib log records the child's default handler collected:
/// `(domain, level, message)`.
#[cfg(unix)]
static GLIB_LOG: std::sync::Mutex<Vec<(String, u32, String)>> = std::sync::Mutex::new(Vec::new());

/// Collects every GLib log record instead of printing it, so the child's
/// stderr holds only the library's raw `fprintf` lines.
#[cfg(unix)]
extern "C" fn collect_glib_log(
    domain: *const c_char,
    level: glib_sys::GLogLevelFlags,
    message: *const c_char,
    _data: glib_sys::gpointer,
) {
    let text = |ptr: *const c_char| {
        if ptr.is_null() {
            String::new()
        } else {
            // SAFETY: GLib hands NUL-terminated strings that outlive the call.
            unsafe { std::ffi::CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        }
    };
    GLIB_LOG
        .lock()
        .expect("log lock")
        .push((text(domain), level, text(message)));
}

/// `#919`, `table-conf-919-short-row-merged-zhuyin`: the five-word row
/// `default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin` plus
/// the next line's first word `SYSTEM_FILE` is one valid six-word row
/// (`fscanf("%255s ... %255s\n")` skips white space across newlines and
/// takes whole words, `table_info.cpp:254-258`), so GB_DICTIONARY is
/// re-pointed at the GBK files; the trailing `default RESERVED NULL NULL
/// NULL` is five words, `6 != num` drops it (`:260-261`). The library and
/// table then disagree and the pin reads a NULL item (SIGSEGV, class (b)),
/// so only oxpinyin's answer is defined. The session is the case's: look
/// up, learn a sentence, import a phrase, save, finalise.
///
/// libzhuyin writes and renames each library's log in turn
/// (`zhuyin.cpp:560-640`), so the second write over the shared
/// `gbk_char.dbin` finds its own tmp file and no `rename ... failed.`
/// line follows (the pinyin twin's save writes all logs first,
/// `pinyin.cpp:925-1090`); the only raw stderr line is the init's
/// missing-marker line (`table_info.cpp:332`).
#[cfg(unix)]
#[test]
fn table_conf_919_short_row_merged() {
    let run =
        crate::test_support::run_in_child("e2e_tests::table_conf_919_short_row_merged", |user| {
            let system = crate::test_support::private_system(user.parent().expect("scratch"));
            let conf = system.join("table.conf");
            let stock = std::fs::read_to_string(&conf).expect("stock table.conf");
            std::fs::remove_file(&conf).expect("unlink the link");
            std::fs::write(
                &conf,
                stock
                    + "default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin\n"
                    + "SYSTEM_FILE default RESERVED NULL NULL NULL\n",
            )
            .expect("crafted table.conf");
            // SAFETY: the handler is a plain function and `user_data` is unused.
            unsafe {
                glib_sys::g_log_set_default_handler(Some(collect_glib_log), ptr::null_mut());
            }
            let system_c = cstr(system.to_str().expect("UTF-8 path"));
            let user_c = cstr(user.to_str().expect("UTF-8 path"));
            let context = zhuyin_init(system_c.as_ptr(), user_c.as_ptr());
            assert!(!context.is_null());
            for text in ["ni3hao3", "yi4shu4"] {
                let inst = zhuyin_alloc_instance(context);
                crate::parse::zhuyin_parse_more_full_pinyins(inst, cstr(text).as_ptr());
                zhuyin_guess_sentence(inst);
                let mut sentence: *mut c_char = ptr::null_mut();
                zhuyin_get_sentence(inst, &raw mut sentence);
                if !sentence.is_null() {
                    take_sentence(sentence);
                }
                crate::sentence::zhuyin_guess_candidates_after_cursor(inst, 0);
            }
            let inst = zhuyin_alloc_instance(context);
            crate::parse::zhuyin_parse_more_full_pinyins(inst, cstr("ni3hao3").as_ptr());
            zhuyin_guess_sentence(inst);
            let train = zhuyin_train(inst);
            let it = zhuyin_begin_add_phrases(context, 7);
            let add = zhuyin_iterator_add_phrase(
                it,
                cstr("测试词").as_ptr(),
                cstr("ㄘㄜˋ ㄕˋ ㄘˊ").as_ptr(),
                1,
            );
            zhuyin_end_add_phrases(it);
            let save = zhuyin_save(context);
            zhuyin_fini(context);
            assert!(train && add && save, "train {train} add {add} save {save}");
            let mut sizes = std::collections::BTreeMap::new();
            let mut marker = String::new();
            for entry in std::fs::read_dir(user).expect("user dir") {
                let entry = entry.expect("entry");
                let name = entry.file_name().to_string_lossy().into_owned();
                let bytes = std::fs::read(entry.path()).expect("read");
                if name == "user.conf" {
                    marker = String::from_utf8_lossy(&bytes).into_owned();
                } else {
                    sizes.insert(name, bytes.len());
                }
            }
            let present: Vec<&str> = sizes.keys().map(String::as_str).collect();
            assert_eq!(
                present,
                [
                    "addon.bin",
                    "gbk_char.dbin",
                    "merged.dbin",
                    "network.bin",
                    "opengram.dbin",
                    "user.bin",
                    "user_bigram.db",
                    "user_phrase_index.bin",
                    "user_pinyin_index.bin",
                ]
            );
            for (name, size) in [
                ("addon.bin", 27),
                ("gbk_char.dbin", 26),
                ("merged.dbin", 26),
                ("network.bin", 27),
                ("opengram.dbin", 26),
                ("user.bin", 71),
            ] {
                assert_eq!(sizes[name], size, "{name}");
            }
            assert!(
                marker.starts_with("binary format version:7\nmodel data version:14\n")
                    && marker.ends_with("\nopen counter:0\n"),
                "{marker}"
            );
            let records = GLIB_LOG.lock().expect("log lock").clone();
            let warning = (
                "libzhuyin".to_owned(),
                glib_sys::G_LOG_LEVEL_WARNING,
                "zhuyin_guess_candidates: a candidate's phrase item cannot be read".to_owned(),
            );
            assert_eq!(records, [warning.clone(), warning]);
        });
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        fresh_user_conf_line(&run.user_dir)
    );
}

// ---------------------------------------------------------------------
// NULL arguments the pin never reads (row 41's non-crashing probes). Each
// test hands valid objects and the one NULL argument the pin ignores; the
// expectations are the pin's answer at 074a2219 (cited beside each), not a
// pin run.
// ---------------------------------------------------------------------

/// `zhuyin_get_candidate_type` never reads `instance` (`zhuyin.cpp:1858-1863`).
#[test]
fn null_instance_candidate_type_answers() {
    let (context, instance) = open();
    let cand = candidate(instance, "su3cl3", 0);
    let mut kind = crate::types::lookup_candidate_type_t::ZOMBIE_CANDIDATE;
    assert!(crate::candidates::zhuyin_get_candidate_type(
        ptr::null_mut(),
        cand,
        &raw mut kind
    ));
    assert_eq!(
        kind,
        crate::types::lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR
    );
    close(context, instance);
}

/// `zhuyin_get_candidate_string` never reads `instance` (`zhuyin.cpp:1865-1870`).
#[test]
fn null_instance_candidate_string_answers() {
    let (context, instance) = open();
    let cand = candidate(instance, "su3cl3", 0);
    let mut text = ptr::null();
    assert!(crate::candidates::zhuyin_get_candidate_string(
        ptr::null_mut(),
        cand,
        &raw mut text
    ));
    assert!(!text.is_null());
    // SAFETY: the candidate text is NUL-terminated and borrowed from the instance.
    let text = unsafe { std::ffi::CStr::from_ptr(text) };
    assert_eq!(text.to_str().expect("UTF-8"), "你好");
    close(context, instance);
}

/// `zhuyin_get_zhuyin_string` never reads `instance` (`zhuyin.cpp:1732-1741`).
#[test]
fn null_instance_zhuyin_string_answers() {
    let (context, instance) = open();
    let mut key = crate::types::ChewingKey::ZERO;
    assert!(crate::keys::zhuyin_parse_chewing(
        instance,
        cstr("su3").as_ptr(),
        &raw mut key
    ));
    let mut out = ptr::null_mut();
    assert!(crate::keys::zhuyin_get_zhuyin_string(
        ptr::null_mut(),
        &raw mut key,
        &raw mut out
    ));
    assert_eq!(take_sentence(out), "ㄋㄧˇ");
    close(context, instance);
}

/// `zhuyin_get_zhuyin_key_rest_positions` reads `key_rest` only to fill a
/// non-NULL `begin` or `end` (`zhuyin.cpp:1930-1940`).
#[test]
fn key_rest_positions_follow_the_pins_reads() {
    let (context, instance) = open();
    assert_eq!(
        zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()),
        6
    );
    let mut rest = ptr::null_mut();
    assert!(crate::cursor::zhuyin_get_zhuyin_key_rest(
        instance,
        0,
        &raw mut rest
    ));
    // NULL `key_rest` with both outs NULL: no read, `true` (`:1933-1938`).
    assert!(crate::cursor::zhuyin_get_zhuyin_key_rest_positions(
        instance,
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut()
    ));
    assert!(crate::cursor::zhuyin_get_zhuyin_key_rest_positions(
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut()
    ));
    // NULL `key_rest` with an out: the pin reads it (row 41), so this stays guarded.
    let mut begin = u16::MAX;
    assert!(!crate::cursor::zhuyin_get_zhuyin_key_rest_positions(
        instance,
        ptr::null_mut(),
        &raw mut begin,
        ptr::null_mut()
    ));
    assert_eq!(begin, u16::MAX);
    let mut end = u16::MAX;
    assert!(!crate::cursor::zhuyin_get_zhuyin_key_rest_positions(
        instance,
        ptr::null_mut(),
        ptr::null_mut(),
        &raw mut end
    ));
    // A real rest with a NULL instance fills both.
    let (mut begin, mut end) = (u16::MAX, u16::MAX);
    assert!(crate::cursor::zhuyin_get_zhuyin_key_rest_positions(
        ptr::null_mut(),
        rest,
        &raw mut begin,
        &raw mut end
    ));
    assert_eq!((begin, end), (0, 3));
    close(context, instance);
}

/// `zhuyin_begin_add_phrases` stores the context unread and returns the
/// iterator (`zhuyin.cpp:392-398`). `zhuyin_iterator_add_phrase` answers
/// `false` before it reads the context for a NULL phrase (`:509-510`), and
/// where it would read it (`_add_phrase`, `:412`) the pin crashes (row 41);
/// `zhuyin_end_add_phrases` reads it at `:542` (row 41), so the handle is freed.
#[test]
fn null_context_import_handle_is_returned_and_freed() {
    let iter = zhuyin_begin_add_phrases(ptr::null_mut(), 1);
    assert!(!iter.is_null());
    assert!(!zhuyin_iterator_add_phrase(
        iter,
        ptr::null(),
        cstr("ㄋㄧˇ ㄏㄠˇ").as_ptr(),
        1
    ));
    assert!(!zhuyin_iterator_add_phrase(
        iter,
        cstr("你好").as_ptr(),
        cstr("ㄋㄧˇ ㄏㄠˇ").as_ptr(),
        1
    ));
    zhuyin_end_add_phrases(iter);
}

/// The index assert (`zhuyin.cpp:381`) precedes any read of the context, so a
/// NULL context aborts for index 16; the `tsi.bin` refusal answers `false`
/// (`:385-386`) without reading it.
#[test]
fn null_context_unload_phrase_library_orders_assert_first() {
    assert!(!crate::config::zhuyin_unload_phrase_library(
        ptr::null_mut(),
        1 // TSI_DICTIONARY
    ));
    crate::test_support::assert_aborts(
        "e2e_tests::null_context_unload_phrase_library_orders_assert_first",
        || {
            crate::config::zhuyin_unload_phrase_library(ptr::null_mut(), 16);
        },
    );
}

/// D16: libzhuyin's import parses the reading with `ZhuyinDirectParser2` under
/// `USE_TONE | FORCE_TONE` (`zhuyin.cpp:515-523` at 074a2219). The parser splits on `' '` and
/// `'\''` and skips any run of them (`storage/zhuyin_parser2.cpp:914-960`), so an apostrophe and
/// a trailing space change nothing. A leading separator leaves an empty first key, which
/// `parse_one_key` refuses (`:866-867`), and an initial alone parses no key: the phrase's
/// length then differs from the key count, and the add answers false (`zhuyin.cpp:525`). Pin
/// capture of 2026-11-07, runs 1 and 2 identical, observations `zhuyin-import/run`,
/// `zhuyin-import/leading` and `zhuyin-import/initial` (the run's new library-7 token stored
/// `ㄋㄧˇ ㄏㄠˇ`; the others stored nothing).
#[test]
fn the_import_parser_skips_separator_runs_and_refuses_a_leading_one() {
    for (phrase, reading, added) in [
        ("你好", "ㄋㄧˇ' ㄏㄠˇ ", true),
        ("你好", " ㄋㄧˇ ㄏㄠˇ", false),
        ("你", "ㄋ", false),
    ] {
        let user_dir = TempUserDir::new("import-separators");
        let (context, instance) = open_with_user(&user_dir.path);
        let iter = zhuyin_begin_add_phrases(context, USER_DICTIONARY);
        assert!(!iter.is_null());
        assert_eq!(
            zhuyin_iterator_add_phrase(iter, cstr(phrase).as_ptr(), cstr(reading).as_ptr(), 5),
            added,
            "{reading:?}"
        );
        zhuyin_end_add_phrases(iter);
        let rows = with_store(instance, |store| {
            store
                .export_phrases_in(USER_DICTIONARY)
                .expect("export the user index")
        });
        let expected: Vec<oxpinyin_user::ExportedPhrase> = if added {
            vec![oxpinyin_user::ExportedPhrase {
                text: phrase.chars().map(u32::from).collect(),
                pinyin: "ni3'hao3".to_owned(),
                count: 5,
            }]
        } else {
            Vec::new()
        };
        assert_eq!(rows, expected, "{reading:?}");
        close(context, instance);
    }
}
