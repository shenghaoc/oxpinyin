//! Sentence guesses, `pinyin_get_sentence`, sentence prefixes, n-best rows
//! and the sentence step prices.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// Each prefix occurrence seeds a node (074a2219 pinyin.cpp:1415-1421,
/// phonetic_lookup.h:244-265). Two strong 甲 paths occupy two result rows;
/// with one seed, the weaker 乙 path occupies row 1 instead. The score
/// gaps exceed one, so the pin's integer tail comparator orders them.
#[test]
fn repeated_prefix_tokens_keep_duplicate_sentence_rows() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::items_system(
        &system_dir(),
        scratch.path(),
        &[
            (&[0x7532], &[0x1081], 1),
            (&[0x4e59], &[0x1081], 1),
            (&[0x4e19], &[0x1084], 999_998),
        ],
        1_000_000,
    );
    let config = system.join("table.conf");
    let config_text = std::fs::read_to_string(&config).expect("configuration");
    std::fs::remove_file(&config).expect("unlink fixture configuration");
    std::fs::write(
        &config,
        config_text
            .lines()
            .map(|line| {
                if line.starts_with("lambda parameter:") {
                    "lambda parameter:0.5"
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("configuration");
    craft_hash(
        &system.join("bigram.db"),
        &[
            (
                1_u32.to_le_bytes().to_vec(),
                gram_bytes(1000, &[(0x0100_0002, 10), (0x0100_0003, 990)]),
            ),
            (
                0x0100_0003_u32.to_le_bytes().to_vec(),
                gram_bytes(1000, &[(0x0100_0001, 1000)]),
            ),
        ],
    );
    let user = TempUserDir::new("repeated-prefix");
    // The system row supplies the first occurrence; this user row adds
    // the second without changing the item or its pronunciation.
    set_phrase_table_row(&user.path, "丙", &[0x0100_0003]);
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert_eq!(parse(instance, "ba1"), 3);
    assert!(unsafe {
        crate::sentence::pinyin_guess_sentence_with_prefix(instance, c"丙".as_ptr())
    });
    for (index, text) in [(0, "甲"), (1, "甲"), (2, "乙")] {
        assert_eq!(sentence_out(instance, index), (true, Some(text.to_owned())));
    }
    close(context, instance);
}

/// The second import adds a reading but no index entry (074a2219
/// pinyin.cpp:563-582). Validation keeps that reading's constraint
/// (phonetic_lookup.cpp:141-165), and any SEARCH_OK lets ONESTEP step
/// its token without a range-membership test (phonetic_lookup.h:552-554,
/// 601-612,776-788). The guess returns true with 甲, not an empty result.
#[rstest::rstest]
#[case::system(1)]
#[case::user(6)]
fn an_unranged_onestep_token_still_produces_a_sentence(#[case] library: u8) {
    let user = TempUserDir::new("unranged-onestep");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        import_batch(
            context,
            library,
            &[("甲", "ba4", 3), ("甲", "da4", 9), ("乙", "da4", 2)],
        ),
        [true; 3]
    );
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    assert_eq!(parse(instance, "ba4"), 3);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    let chosen = row_with(instance, NORMAL, "甲");
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, chosen) }, 3);
    assert_eq!(parse(instance, "da4"), 3);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    let rows = capture_rows(instance);
    assert!(
        rows.iter()
            .any(|(kind, text, _)| *kind == NORMAL && text == "乙")
    );
    assert!(
        !rows
            .iter()
            .any(|(kind, text, _)| *kind == NORMAL && text == "甲")
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence_out(instance, 0), (true, Some("甲".to_owned())));
    close(context, instance);
}

/// The Luoma and secondary-zhuyin index parses run the same parser
/// (`FullPinyinParser2::set_scheme`, `pinyin_parser2.cpp:383-401`), so
/// `'ni` leaves column 0 empty too (`phonetic_key_matrix.cpp:52-79`): the
/// beam never leaves it (`phonetic_lookup.h:753-771`) and the guess has
/// no sentence.
#[rstest::rstest]
fn an_index_parse_leading_separator_guess_has_no_sentence(#[values(2, 3)] scheme: i32) {
    let user = TempUserDir::new("index-separator-sentence");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, SEPARATOR_OPTIONS) });
    assert!(unsafe { crate::config::pinyin_set_full_pinyin_scheme(context, scheme) });
    assert_eq!(parse(instance, "'ni"), 3);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence_out(instance, 0), (false, None));
    close(context, instance);
}

/// Every `search_bigram2` call loads the predecessor's system gram through
/// the attached handle, `sentence_start` included, and follows that read's
/// result (`phonetic_lookup.h:588-599`, `ngram_tkrzwdb.cpp:111-114` and its
/// BDB and KC peers at 074a2219; the sentence-start ruling of the
/// compatibility policy). Library 1 holds 甲 (ba1) at 1, 乙 (da1) at 1000
/// and 丙丁 (ba1 da1) at 1; the `sentence_start` gram gives 甲 and 丙丁 half
/// each. The step at 0 loads the gram for the span of 甲 and again for the
/// span of 丙丁. With both reads good, 丙丁 takes its bigram and is the
/// sentence. When the second read fails, 丙丁 has its unigram alone and
/// 甲乙 is the sentence.
#[test]
fn a_later_span_reads_the_sentence_start_gram_again() {
    crate::test_support::run_in_child(
        "boundary_tests::sentence::a_later_span_reads_the_sentence_start_gram_again",
        |user| {
            let scratch = tempfile::tempdir().expect("scratch");
            let system = crate::test_support::items_system(
                &system_dir(),
                scratch.path(),
                &[
                    (&[0x7532], &[0x1081], 1),
                    (&[0x4e59], &[0x1084], 1000),
                    (&[0x4e19, 0x4e01], &[0x1081, 0x1084], 1),
                ],
                2000,
            );
            let sentence_start = 1_u32.to_le_bytes();
            craft_hash(
                &system.join("bigram.db"),
                &[(
                    sentence_start.to_vec(),
                    gram_bytes(1000, &[(0x0100_0001, 500), (0x0100_0003, 500)]),
                )],
            );
            let context = init_at(&system, user);
            assert!(!context.is_null());
            let instance = alloc(context);
            assert!(unsafe { pinyin_set_options(context, 1 << 5) });
            assert_eq!(parse(instance, "ba1da1"), 6);
            assert!(unsafe { pinyin_guess_sentence(instance) });
            assert_eq!(sentence_out(instance, 0), (true, Some("丙丁".to_owned())));
            // The next guess reads the gram once for 甲's span; the read
            // for 丙丁's span fails.
            oxpinyin_store::read_faults::fail_point_read_of(&sentence_start, 1);
            assert!(unsafe { pinyin_guess_sentence(instance) });
            assert_eq!(sentence_out(instance, 0), (true, Some("甲乙".to_owned())));
            close(context, instance);
        },
    );
}

/// A step prices a reading with the query's tone (`phonetic_key_matrix.cpp:
/// 562,575`, `phrase_index.h:136-164` at 074a2219): a nonzero query tone
/// matches an equal or zero stored tone only (`pinyin_phrase3.h:96-105`).
/// The second import of 甲 adds a reading and no index row
/// (`pinyin.cpp:563-582`). Under `USE_TONE`, `ba4` prices 甲 at 2/100, so
/// 乙 (unigram 3) outranks 甲 (unigram 6).
#[test]
fn a_step_prices_readings_with_the_query_tone() {
    let user = TempUserDir::new("toned-step");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        crate::e2e_tests::import_batch(
            context,
            1,
            &[("甲", "ba4", 2), ("甲", "ba3", 98), ("乙", "ba4", 1)]
        ),
        [true; 3]
    );
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    assert_eq!(parse(instance, "ba4"), 3);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence_out(instance, 0), (true, Some("乙".to_owned())));
    close(context, instance);
}

/// A step prices a user item from its own readings
/// (`phonetic_lookup.h:646-656`, `phrase_index.h:136-164` at 074a2219).
/// Library 6's 甲 holds `ba` 3 and `da` 9, so `ba` prices it at 3/12 and 乙
/// (unigram 6, one reading) outranks 甲 (unigram 9). The second import adds
/// the reading and no index row (`pinyin.cpp:563-582`), so `da` lists no
/// 甲 row.
#[test]
fn a_step_prices_a_user_item_from_its_readings() {
    let user = TempUserDir::new("user-item-step");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        crate::e2e_tests::import_batch(
            context,
            6,
            &[("甲", "ba", 3), ("甲", "da", 9), ("乙", "ba", 2)]
        ),
        [true; 3]
    );
    assert_eq!(parse(instance, "ba"), 2);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence_out(instance, 0), (true, Some("乙".to_owned())));
    assert_eq!(parse(instance, "da"), 2);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    assert!(
        !capture_rows(instance)
            .iter()
            .any(|(kind, text, _)| *kind == NORMAL && text == "甲")
    );
    close(context, instance);
}
