//! Training and remember: the grams, the counts and the user indexes they
//! write.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// Training over an unpartitioned sentence-start gram whose items are not
/// sorted answers true where the pin aborts
/// (docs/findings/upstream-divergences.md#stored-gram-partitions-training).
#[test]
fn training_over_an_unsorted_start_gram_answers_true() {
    let user = TempUserDir::new("train-unsorted-gram");
    let (context, instance) = open_at(&user.path);
    let ni = lookup(instance, "你")[0];
    close(context, instance);
    let items = [
        (ni - 4, 1),
        (ni - 3, 1),
        (ni - 2, 1),
        (ni + 1, 1),
        (ni - 1, 1),
    ];
    plant_bigram(
        &user.path,
        vec![(1_u32.to_le_bytes().to_vec(), gram_bytes(5, &items))],
    );
    let (context, instance) = open_at(&user.path);
    let logs = crate::test_support::LogCapture::new();
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    for (offset, text) in [(0, "你"), (2, "好")] {
        assert!(unsafe {
            pinyin_guess_candidates(instance, offset, crate::test_support::DEFAULT_SORT)
        });
        assert!(
            unsafe { pinyin_choose_candidate(instance, offset, row_with(instance, NORMAL, text)) }
                > 0
        );
    }
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { crate::candidates::pinyin_train(instance, 0) });
    assert!(logs.take().is_empty());
    close(context, instance);
}

/// `remember_user_input` keeps the toned keys of the parse even after the
/// options that made them are cleared.
#[test]
fn remember_keeps_the_parse_time_tones() {
    let user = TempUserDir::new("remember-cleared-tone");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, 0x20) });
    assert_eq!(parse(instance, "ni3hao3"), 7);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_set_options(context, 0) });
    assert!(unsafe {
        crate::user_data::pinyin_remember_user_input(instance, c"你好".as_ptr(), 3)
    });
    assert_eq!(
        crate::e2e_tests::export_of(context, 7),
        [("你好".to_owned(), "ni3'hao3".to_owned(), 3)]
    );
    close(context, instance);
}

/// Hex of each `(key, value)` row.
fn hex_rows(rows: Vec<(Vec<u8>, Vec<u8>)>) -> Vec<(String, String)> {
    let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    rows.iter()
        .map(|(key, value)| (hex(key), hex(value)))
        .collect()
}

/// The saved user index trees hold the pin's records: each reading key
/// with its prefix marker, and each phrase key with its token.
#[test]
fn saved_user_indexes_hold_the_pins_records() {
    let user = TempUserDir::new("saved-index-format");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        import_batch(
            context,
            7,
            &[
                ("龘龘", "da2'da2", 100),
                ("龘龘", "da4'da4", 100),
                ("靐靐", "bing4'bing4", 100)
            ]
        ),
        [true, true, true]
    );
    assert!(unsafe { pinyin_save(context) });
    close(context, instance);
    let rows = |name: &str| crate::test_support::store_rows(&user.path.join(name), false);
    let pairs = |rows: &[(&str, &str)]| -> Vec<(String, String)> {
        rows.iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    };
    assert_eq!(
        hex_rows(rows("user_pinyin_index.bin")),
        pairs(&[
            ("0100", ""),
            ("01000100", "0200000781488148"),
            ("0400", ""),
            ("04000400", "0100000784208420"),
            ("8108", ""),
            ("81088108", "0200000781488148"),
            ("8400", ""),
            ("84008400", "0100000784208420"),
        ])
    );
    assert_eq!(
        hex_rows(rows("user_phrase_index.bin")),
        pairs(&[
            ("50970000", ""),
            ("5097000050970000", "02000007"),
            ("989f0000", ""),
            ("989f0000989f0000", "01000007"),
        ])
    );
    let (context, instance) = open_at(&user.path);
    assert_eq!(lookup(instance, "龘龘"), [0x0700_0001]);
    assert_eq!(lookup(instance, "靐靐"), [0x0700_0002]);
    close(context, instance);
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
    close(key_context, key_instance);

    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x7532], &[alternate], 100)],
        1000,
    );
    let user = TempUserDir::new("remember-alternate");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(unsafe {
        pinyin_set_options(
            context,
            (oxpinyin_core::OptionBits::USE_TONE | oxpinyin_core::OptionBits::PINYIN_AMB_EN_ENG)
                .bits(),
        )
    });
    assert_eq!(parse(instance, "shun3"), 5);
    let phrase = cstr("甲");
    assert!(unsafe { crate::user_data::pinyin_remember_user_input(instance, phrase.as_ptr(), 5) });
    let token = lookup(instance, "甲")
        .into_iter()
        .find(|&token| oxpinyin_user::is_user_token(token))
        .expect("the remembered library-7 token");
    assert_eq!(reading_bytes(instance, token, 0), alternate.to_le_bytes());
    assert_eq!(export_rows_of(context, 7, "甲"), [("3".to_owned(), 5)]);
    close(context, instance);
}

/// A negative remember count is stored as its bit pattern: `i32::MIN`
/// exports as itself.
#[test]
fn a_negative_remember_count_is_stored_as_its_bit_pattern() {
    let user = TempUserDir::new("remember-count-min");
    let (context, instance) = open_at(&user.path);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe {
        crate::user_data::pinyin_remember_user_input(instance, c"你好".as_ptr(), i32::MIN)
    });
    assert_eq!(
        crate::e2e_tests::export_of(context, 7),
        [("你好".to_owned(), "ni'hao".to_owned(), i32::MIN)]
    );
    close(context, instance);
}

/// Training over a user gram whose total is at the `guint32` limit keeps
/// the gram's counts and still raises the unigram by the trained delta.
#[test]
fn training_past_the_gram_total_limit_keeps_the_gram() {
    const COUNT: u32 = 0xFFFF_FFFF - 22_080 + 1;
    let user = TempUserDir::new("d18-train-total-overflow");
    let (context, instance) = open_at(&user.path);
    let [ni, hao, zhong] = ["你", "好", "中"].map(|text| lookup(instance, text)[0]);
    close(context, instance);
    let gram = gram_bytes(COUNT, &[(hao, COUNT)]);
    plant_bigram(
        &user.path,
        vec![
            (ni.to_le_bytes().to_vec(), gram.clone()),
            (zhong.to_le_bytes().to_vec(), gram),
            (1_u32.to_le_bytes().to_vec(), vec![0; 4]),
        ],
    );
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    let before = unigram(instance, hao);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    for (offset, text) in [(0, "你"), (2, "好")] {
        assert!(unsafe {
            pinyin_guess_candidates(instance, offset, crate::test_support::DEFAULT_SORT)
        });
        assert!(
            unsafe { pinyin_choose_candidate(instance, offset, row_with(instance, NORMAL, text)) }
                > 0
        );
    }
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { crate::candidates::pinyin_train(instance, 0) });
    let after = unigram(instance, hao);
    assert_eq!(after, (true, before.1 + 154_560));
    assert!(unsafe { pinyin_save(context) });
    close(context, instance);
    let stored = crate::test_support::store_rows(&user.path.join("user_bigram.db"), true)
        .into_iter()
        .find(|(key, _)| *key == ni.to_le_bytes())
        .expect("the 你 gram")
        .1;
    assert_eq!(stored, gram_bytes(COUNT, &[(hao, COUNT)]));
    let (context, instance) = open_at(&user.path);
    assert_eq!(unigram(instance, hao), after);
    close(context, instance);
}
