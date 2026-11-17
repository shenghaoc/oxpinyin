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
