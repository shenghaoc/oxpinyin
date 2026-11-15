//! Sentence guesses, `pinyin_get_sentence`, sentence prefixes, n-best rows
//! and the sentence step prices.

use super::*;

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
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "ba4"), 3);
    assert!(pinyin_guess_candidates(instance, 0, 0));
    let chosen = row_with(instance, NORMAL, "甲");
    assert_eq!(pinyin_choose_candidate(instance, 0, chosen), 3);
    assert_eq!(parse(instance, "da4"), 3);
    assert!(pinyin_guess_candidates(instance, 0, 0));
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
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence_out(instance, 0), (true, Some("甲".to_owned())));
    close(context, instance);
}

/// A toned initial aborts the sentence guess.
#[test]
fn guess_sentence_aborts_on_a_toned_initial() {
    assert_aborts(
        "boundary_tests::sentence::guess_sentence_aborts_on_a_toned_initial",
        || {
            let user = TempUserDir::new("toned-sentence");
            let (context, instance) = open_at(&user.path);
            assert!(pinyin_set_options(context, 0x28));
            assert_eq!(parse(instance, "n4"), 2);
            pinyin_guess_sentence(instance);
        },
    );
}

/// `get_sentence` asserts on an index past the n-best rows.
#[test]
fn get_sentence_aborts_past_the_rows() {
    assert_aborts(
        "boundary_tests::sentence::get_sentence_aborts_past_the_rows",
        || {
            let user = TempUserDir::new("sentence-past-rows");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "nihao"), 5);
            assert!(pinyin_guess_sentence(instance));
            let mut out: *mut c_char = ptr::null_mut();
            pinyin_get_sentence(instance, 5, &raw mut out);
        },
    );
}

/// Before a guess, and after a reset, `get_sentence` answers false and
/// leaves the out-pointer alone.
#[test]
fn get_sentence_before_a_guess_leaves_the_out_pointer() {
    let user = TempUserDir::new("sentence-before-guess");
    let (context, instance) = open_at(&user.path);
    for index in [0, 255] {
        assert_eq!(
            sentence_out(instance, index),
            (false, None),
            "fresh {index}"
        );
    }
    assert_eq!(parse(instance, "nihao"), 5);
    for index in [0, 255] {
        assert_eq!(
            sentence_out(instance, index),
            (false, None),
            "parsed {index}"
        );
    }
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence_out(instance, 0), (true, Some("你好".to_owned())));
    assert!(pinyin_reset(instance));
    assert_eq!(sentence_out(instance, 0), (false, None));
    close(context, instance);
}

/// A guess over a leading separator answers true with no n-best row, so
/// `get_sentence(0)` answers false and leaves the out-pointer alone.
#[test]
fn a_leading_separator_guess_has_no_sentence() {
    let user = TempUserDir::new("separator-sentence");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, SEPARATOR_OPTIONS));
    assert_eq!(parse(instance, "'ni'hao"), 7);
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence_out(instance, 0), (false, None));
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
    assert!(pinyin_set_options(context, SEPARATOR_OPTIONS));
    assert!(crate::config::pinyin_set_full_pinyin_scheme(
        context, scheme
    ));
    assert_eq!(parse(instance, "'ni"), 3);
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence_out(instance, 0), (false, None));
    close(context, instance);
}

/// A keyless parse (one separator) refuses both sentence guesses before
/// they clear the old result: the old sentence stays, the candidate list
/// is empty, and a fresh instance has no sentence (:183).
#[test]
fn keyless_sentence_guesses_keep_the_old_sentence() {
    let user = TempUserDir::new("zero-key-guess");
    let (context, _) = open_at(&user.path);
    let guesses: [fn(*mut PinyinInstance) -> bool; 2] = [
        |instance| pinyin_guess_sentence(instance),
        |instance| crate::sentence::pinyin_guess_sentence_with_prefix(instance, c"你".as_ptr()),
    ];
    for guess in guesses {
        let instance = alloc(context);
        assert_eq!(parse(instance, "ni3hao3"), 7);
        assert!(pinyin_guess_sentence(instance));
        let sentence = sentence_out(instance, 0);
        assert!(sentence.0);
        assert_eq!(parse(instance, "'"), 1);
        assert!(!guess(instance));
        assert_eq!(sentence_out(instance, 0), sentence);
        assert_eq!(n_candidate(instance), 0);
        pinyin_free_instance(instance);

        let fresh = alloc(context);
        assert_eq!(parse(fresh, "'"), 1);
        assert!(!guess(fresh));
        assert_eq!(sentence_out(fresh, 0), (false, None));
        pinyin_free_instance(fresh);
    }
    pinyin_fini(context);
}

/// GLib's historical UTF-8 form of `words`, up to six bytes per word.
fn glib_utf8(words: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    for &word in words {
        let (first, length): (u32, usize) = match word {
            0..0x80 => (0, 1),
            0x80..0x800 => (0xc0, 2),
            0x800..0x1_0000 => (0xe0, 3),
            0x1_0000..0x20_0000 => (0xf0, 4),
            0x20_0000..0x400_0000 => (0xf8, 5),
            _ => (0xfc, 6),
        };
        let mut rest = word;
        let mut tail = Vec::new();
        for _ in 1..length {
            tail.push(0x80 | u8::try_from(rest & 0x3f).expect("six bits"));
            rest >>= 6;
        }
        out.push(u8::try_from(first | rest).expect("a lead byte"));
        out.extend(tail.iter().rev());
    }
    out
}

/// A NULL prefix to `guess_sentence_with_prefix` is no prefix: true, and the
/// sentence is the unigram choice 丙 (docs/findings/upstream-divergences.md,
/// "NULL prediction prefix").
#[test]
fn a_null_sentence_prefix_is_no_prefix() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = prefix_system(scratch.path(), 0x4e01);
    let user = TempUserDir::new("prefix-null");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "ba1"), 3);
    let valid = std::ffi::CString::new(glib_utf8(&[0x7532, 0x4e01, 0x4e59])).expect("no NUL");
    assert!(crate::sentence::pinyin_guess_sentence_with_prefix(
        instance,
        valid.as_ptr()
    ));
    assert!(crate::sentence::pinyin_guess_sentence_with_prefix(
        instance,
        ptr::null()
    ));
    assert_eq!(sentence_out(instance, 0), (true, Some("丙".to_owned())));
    close(context, instance);
}

/// The sentence prefix is converted with `prefix_text`, which accepts a
/// word above Unicode: the prefix 甲·乙 finds its bigram and the sentence
/// is 戌.
#[test]
fn a_sentence_prefix_above_unicode_is_converted() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = prefix_system(scratch.path(), 0x11_0000);
    let user = TempUserDir::new("prefix-above-unicode");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "ba1"), 3);
    assert!(pinyin_guess_sentence(instance));
    let prefix = std::ffi::CString::new(glib_utf8(&[0x7532, 0x11_0000, 0x4e59])).expect("no NUL");
    assert!(crate::sentence::pinyin_guess_sentence_with_prefix(
        instance,
        prefix.as_ptr()
    ));
    assert_eq!(sentence_out(instance, 0), (true, Some("戌".to_owned())));
    close(context, instance);
}

/// An n-best row whose phrases all start with a zero word has the string
/// `""` at the pin (`lookup/lookup.cpp:39-68`): `pinyin_get_sentence`
/// answers true with it, and the NBEST row outranks the NORMAL row of the
/// same string in the dedup (`pinyin.cpp:2116-2125`, `:2295-2296`).
#[test]
fn an_nbest_row_with_empty_text_is_a_sentence_and_outranks_its_phrase() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0, 0x4e19], &[0x1081; 2], 100)],
        2000,
    );
    let user = TempUserDir::new("nbest-empty-text");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "ba1ba1"), 6);
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence_out(instance, 0), (true, Some(String::new())));
    assert!(pinyin_guess_candidates(instance, 0, 0));
    assert_eq!(capture_rows(instance), [(NBEST, String::new(), Some(0))]);
    close(context, instance);
}

/// `convert_to_utf8` leaves out a phrase whose `g_ucs4_to_utf8` is NULL, a
/// word at or above 0x80000000 (`lookup/lookup.cpp:58-65`), so the row
/// [A, 丙] reads `丙` and `pinyin_get_sentence` answers true. A's own NORMAL
/// row has a NULL string, which the candidate dedup compares (rows 80 and
/// 106, class (b)), so the test lists no candidates.
#[test]
fn an_nbest_row_leaves_out_a_phrase_that_does_not_convert() {
    let scratch = tempfile::tempdir().expect("scratch");
    let an = oxpinyin_core::ChewingKey::from_pinyin("an")
        .expect("frozen syllable")
        .with_tone(1)
        .to_packed();
    let system = crate::test_support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x8000_0000], &[0x1081], 100), (&[0x4e19], &[an], 100)],
        2000,
    );
    let user = TempUserDir::new("nbest-unconvertible");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "ba1an1"), 6);
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence_out(instance, 0), (true, Some("丙".to_owned())));
    close(context, instance);
}

/// A failed conversion for the row's only phrase leaves the sentence NULL
/// and still answers true (074a2219 `lookup/lookup.cpp:27-70`,
/// `pinyin.cpp:1464-1482`).
#[test]
fn an_nbest_row_with_no_converted_phrase_answers_true_with_null() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x8000_0000], &[0x1081], 100)],
        2000,
    );
    let user = TempUserDir::new("nbest-null-text");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_set_options(
        context,
        oxpinyin_core::OptionBits::USE_TONE.bits()
    ));
    assert_eq!(parse(instance, "ba1"), 3);
    assert!(pinyin_guess_sentence(instance));

    let mut out = UNTOUCHED as usize as *mut c_char;
    assert!(pinyin_get_sentence(instance, 0, &raw mut out));
    assert!(out.is_null());
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
            assert!(pinyin_set_options(context, 1 << 5));
            assert_eq!(parse(instance, "ba1da1"), 6);
            assert!(pinyin_guess_sentence(instance));
            assert_eq!(sentence_out(instance, 0), (true, Some("丙丁".to_owned())));
            // The next guess reads the gram once for 甲's span; the read
            // for 丙丁's span fails.
            oxpinyin_store::read_faults::fail_point_read_of(&sentence_start, 1);
            assert!(pinyin_guess_sentence(instance));
            assert_eq!(sentence_out(instance, 0), (true, Some("甲乙".to_owned())));
            close(context, instance);
        },
    );
}

/// NBEST choose reaches diff_result without validation (pinyin.cpp:2515-2520).
/// An empty-matrix guess shrinks the store but keeps the decoded results
/// (phonetic_lookup.cpp:120-138; phonetic_lookup.h:746-754). The differing
/// write then returns zero and check_result aborts (phonetic_lookup.cpp:203).
#[test]
fn a_kept_differing_sentence_aborts_after_an_empty_matrix_guess() {
    assert_aborts(
        "boundary_tests::sentence::a_kept_differing_sentence_aborts_after_an_empty_matrix_guess",
        || {
            let user = TempUserDir::new("empty-matrix-diff-write");
            let (context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "nihao"), 5);
            assert!(pinyin_guess_sentence(instance));
            assert!(pinyin_guess_candidates(instance, 0, 0));
            let rows = capture_rows(instance);
            let index = rows
                .iter()
                .enumerate()
                .find_map(|(index, (kind, _, _))| {
                    if *kind != NBEST {
                        return None;
                    }
                    let row = candidate_at(instance, index);
                    let mut rank = 0;
                    assert!(pinyin_get_candidate_nbest_index(
                        instance,
                        row,
                        &raw mut rank
                    ));
                    (rank > 0).then_some(index)
                })
                .expect("a differing n-best row");
            let row = candidate_at(instance, index);
            assert_eq!(parse(instance, ""), 0);
            assert!(!pinyin_guess_sentence(instance));
            pinyin_choose_candidate(instance, 0, row);
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
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "ba4"), 3);
    assert!(pinyin_guess_sentence(instance));
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
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence_out(instance, 0), (true, Some("乙".to_owned())));
    assert_eq!(parse(instance, "da"), 2);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, 0));
    assert!(
        !capture_rows(instance)
            .iter()
            .any(|(kind, text, _)| *kind == NORMAL && text == "甲")
    );
    close(context, instance);
}
