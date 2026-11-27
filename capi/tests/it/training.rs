//! Training and remember: the grams, the counts and the user indexes they
//! write.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    TempUserDir, cstr, lookup, pinyin_guess_sentence, pinyin_parse_full_pinyin, pinyin_set_options,
    system_dir, *,
};

/// `remember_user_input` keeps the toned keys of the parse even after the
/// options that made them are cleared.
#[test]
fn remember_keeps_the_parse_time_tones() {
    let user = TempUserDir::new("remember-cleared-tone");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, 0x20) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "ni3hao3") }, 7);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_set_options(context, 0) });
    assert!(unsafe { crate::support::pinyin_remember_user_input(instance, c"你好".as_ptr(), 3) });
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { crate::support::export_of(context, 7) },
        [("你好".to_owned(), "ni3'hao3".to_owned(), 3)]
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// Remember stores the matrix's own key, with or without a syllable-table
/// row. Under `USE_TONE | PINYIN_AMB_EN_ENG`, `shun3` puts the final
/// alternate (sh, u, eng, tone 3) in the matrix with no table check
/// (`phonetic_key_matrix.cpp:285-306` at 074a2219). 甲's only reading is
/// that key, so the one matching path carries it to `_add_phrase`
/// (`pinyin.cpp:3594-3606`, `:3647-3663`), which answers true and stores
/// it. The nth-pronunciation getter returns the stored word
/// (`pinyin.cpp:2801-2818`). The key has no `content_table` row:
/// `get_pinyin_string` renders the empty row-zero spelling and the tone digit
/// (`chewing_key.cpp:47-57`, `pinyin_parser_table.h:3977`).
#[test]
fn remember_stores_a_fuzzy_final_alternate_with_no_table_row() {
    let key_user = TempUserDir::new("remember-alternate-keys");
    let (key_context, key_instance) = open_at(&key_user.path);
    let key = |text: &str| {
        let mut key = 0_u16;
        let text = cstr(text);
        assert!(unsafe {
            pinyin_parse_full_pinyin(key_instance, text.as_ptr(), (&raw mut key).cast())
        });
        key
    };
    // The final field is bits 7 to 11 of the packed key.
    let alternate = (key("shun3") & !0x0f80) | (key("sheng") & 0x0f80);
    // SAFETY: `key_context` was opened above and is not yet freed; `key_instance` was opened above
    // and is not yet freed; nothing uses either handle after this call; only this thread calls into
    // them.
    unsafe { close(key_context, key_instance) };

    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x7532], &[alternate], 100)],
        1000,
    );
    let user = TempUserDir::new("remember-alternate");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    assert!(unsafe {
        pinyin_set_options(
            context,
            (oxpinyin::OptionBits::USE_TONE | oxpinyin::OptionBits::PINYIN_AMB_EN_ENG).bits(),
        )
    });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "shun3") }, 5);
    let phrase = cstr("甲");
    assert!(unsafe { crate::support::pinyin_remember_user_input(instance, phrase.as_ptr(), 5) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { lookup(instance, "甲") }
        .into_iter()
        .find(|&token| oxpinyin::facade::is_user_token(token))
        .expect("the remembered library-7 token");
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { reading_bytes(instance, token, 0) },
        alternate.to_le_bytes()
    );
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { export_rows_of(context, 7, "甲") },
        [("3".to_owned(), 5)]
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// `pinyin_train` at a full library answers `true` and leaves the item's
/// unigram alone. `train_result3` ignores the answer of
/// `add_unigram_frequency` (`phonetic_lookup.h:928-929`), and the sub-index
/// refuses a delta that would wrap its `guint32` total
/// (`phrase_index.cpp:169-171`). Below the limit the same training adds
/// `initial_seed * unigram_factor` = 69 * 7 (`phonetic_lookup.h:847-849`).
#[test]
fn train_at_a_full_library_answers_true_and_adds_no_unigram() {
    for (total, trained) in [(1000, 100 + 69 * 7), (u32::MAX - 15, 100)] {
        let scratch = tempfile::tempdir().expect("scratch");
        let system = crate::support::items_system(
            &system_dir(),
            scratch.path(),
            &[(&[0x7532], &[0x1081], 100)],
            total,
        );
        let user = TempUserDir::new("train-full-library");
        let context = init_at(&system, &user.path);
        assert!(!context.is_null());
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { alloc(context) };
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { parse(instance, "ba1") }, 3);
        // SORT_WITHOUT_LONGER_CANDIDATE: with the sentence bit clear the choose
        // adds its constraint and answers the row's end (`pinyin.cpp:2565-2588`).
        assert!(unsafe { crate::support::pinyin_guess_candidates(instance, 0, 0x2) });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let row = unsafe { row_of(instance, NORMAL) };
        // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
        assert_eq!(
            unsafe { crate::support::pinyin_choose_candidate(instance, 0, row) },
            3
        );
        assert!(unsafe { pinyin_guess_sentence(instance) });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { unigram(instance, 0x0100_0001) }, (true, 100));
        assert!(unsafe { crate::support::pinyin_train(instance, 0) });
        assert_eq!(
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { unigram(instance, 0x0100_0001) },
            (true, trained),
            "total {total}"
        );
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
    }
}

/// `pinyin_train` on an instance with no result answers `false` for every
/// index, before it marks the context modified and before the index assert
/// (`pinyin.cpp:2678-2684`). The save then has nothing to write and answers
/// `false` (`:1136`).
#[test]
fn a_fresh_instance_trains_nothing() {
    let user = TempUserDir::new("train-fresh");
    let (context, instance) = open_at(&user.path);
    for index in [0, 1, 255] {
        assert!(
            !unsafe { crate::support::pinyin_train(instance, index) },
            "{index}"
        );
    }
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(!unsafe { crate::support::pinyin_train(instance, 0) });
    assert!(!unsafe { crate::support::pinyin_save(context) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A remember onto an imported toned phrase adds its count to the imported
/// reading. The walk hands `_add_phrase` the parsed toned keys
/// (`pinyin.cpp:3594-3606`); the phrase has a token in the user library, so
/// `_add_phrase` takes the item out, calls `add_pronunciation` and puts it
/// back (`:561-582`); `add_pronunciation` finds the same keys and adds the
/// count to that reading (`phrase_index.cpp:73-84`).
#[test]
fn a_remember_onto_an_imported_toned_phrase_adds_to_its_reading() {
    let user = TempUserDir::new("remember-imported-tones");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { import_batch(context, 7, &[("你好你好", "ni3'hao3'ni3'hao3", 100_000)]) },
        [true]
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "ni3hao3ni3hao3") }, 14);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe {
        crate::support::pinyin_remember_user_input(instance, c"你好你好".as_ptr(), 3)
    });
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { export_rows_of(context, 7, "你好你好") },
        [("ni3'hao3'ni3'hao3".to_owned(), 100_003)]
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The tokens of the records in a `.dbin` log, by the record layout of
/// `PhraseIndexLogger::next_record` (`phrase_index_logger.h:158-215`).
fn log_tokens(path: &std::path::Path) -> Vec<u32> {
    let Ok(file) = std::fs::read(path) else {
        return Vec::new();
    };
    let word = |at: usize| u32::from_le_bytes(file[at..at + 4].try_into().expect("a word"));
    let short = |at: usize| usize::from(u16::from_le_bytes([file[at], file[at + 1]]));
    let mut tokens = Vec::new();
    // The chunk's length word and checksum come first.
    let mut at = 8;
    while at < file.len() {
        tokens.push(word(at + 4));
        at += 8 + match word(at) {
            1 | 2 => 2 + short(at + 8),
            3 => 4 + short(at + 8) + short(at + 10),
            4 => 2 + 2 * short(at + 8),
            other => panic!("log record type {other}"),
        };
    }
    tokens
}

/// The two-library witness of unloaded-token training,
/// through the C entries. Library 1 holds 甲 and library 2 holds 乙, both
/// `ba1`. One instance chooses 乙 and guesses; library 2 is unloaded
/// (`pinyin.cpp:464-475`, `phrase_index.cpp:261-268`); a second instance of
/// the context guesses `ba1`, which fetches 甲. Training the first instance
/// answers `true` and marks the context modified, as at the pin
/// (`pinyin.cpp:2681`, `phonetic_lookup.h:935`). The pin then writes the
/// reading count of the cached item, 甲 (`phonetic_lookup.h:924-927`).
/// oxpinyin writes into no item: this is the approved difference. This test
/// holds the `true` answer; the store test
/// `unloaded_training_keeps_bigram_but_never_materializes_an_item` holds
/// that no item is written, and the log checks below only guard it.
#[test]
fn training_an_unloaded_token_answers_true() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::support::libraries_system(
        &system_dir(),
        scratch.path(),
        &[
            (1, "gb_char.bin", &[(&[0x7532], &[0x1081], 100)], 1000),
            (2, "gbk_char.bin", &[(&[0x4e59], &[0x1081], 100)], 1000),
        ],
    );
    let user = TempUserDir::new("train-unloaded-token");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let first = unsafe { alloc(context) };
    // SAFETY: `first` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(first, "ba1") }, 3);
    // SORT_WITHOUT_LONGER_CANDIDATE.
    assert!(unsafe { crate::support::pinyin_guess_candidates(first, 0, 0x2) });
    // SAFETY: `first` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { crate::support::row_with(first, NORMAL, "乙") };
    // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
    assert_eq!(
        unsafe { crate::support::pinyin_choose_candidate(first, 0, row) },
        3
    );
    assert!(unsafe { pinyin_guess_sentence(first) });
    assert!(unsafe { crate::support::pinyin_unload_phrase_library(context, 2) });
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let second = unsafe { alloc(context) };
    // SAFETY: `second` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(second, "ba1") }, 3);
    assert!(unsafe { pinyin_guess_sentence(second) });

    assert!(unsafe { crate::support::pinyin_train(first, 0) });
    // SAFETY: `first` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { unigram(first, 0x0100_0001) }, (true, 100));
    assert!(unsafe { crate::support::pinyin_save(context) });
    assert!(
        !log_tokens(&user.path.join("gb_char.dbin")).contains(&0x0100_0001),
        "no record of 甲"
    );
    // The save skips the unloaded library (`pinyin.cpp:929-932`), so 乙 gets
    // no record either.
    assert!(log_tokens(&user.path.join("gbk_char.dbin")).is_empty());
    unsafe { crate::support::pinyin_free_instance(second) };
    // SAFETY: `context` was opened above and is not yet freed; `first` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, first) };
}

/// A negative remember count is stored as its bit pattern: `i32::MIN`
/// exports as itself.
#[test]
fn a_negative_remember_count_is_stored_as_its_bit_pattern() {
    let user = TempUserDir::new("remember-count-min");
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(unsafe {
        crate::support::pinyin_remember_user_input(instance, c"你好".as_ptr(), i32::MIN)
    });
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { crate::support::export_of(context, 7) },
        [("你好".to_owned(), "ni'hao".to_owned(), i32::MIN)]
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}
