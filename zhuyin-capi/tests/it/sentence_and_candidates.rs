//! Sentence and candidate guesses, choosing a candidate and the sentence
//! step prices.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::n_candidate;

use crate::support::{
    LogCapture, LookupCandidate, TempUserDir, assert_aborts, candidate, close, cstr, import,
    init_on, open, open_with_user, private_system, take_sentence, tokens_of, zhuyin_alloc_instance,
    zhuyin_choose_candidate, zhuyin_fini, zhuyin_free_instance, zhuyin_get_candidate,
    zhuyin_get_candidate_string, zhuyin_get_character_offset, zhuyin_get_n_candidate,
    zhuyin_get_parsed_input_length, zhuyin_get_sentence, zhuyin_guess_candidates_after_cursor,
    zhuyin_guess_sentence, zhuyin_load_phrase_library, zhuyin_parse_more_chewings,
    zhuyin_parse_more_full_pinyins, zhuyin_set_options, zhuyin_unload_phrase_library, *,
};

use std::os::raw::c_char;

use std::ptr;

/// The sentence getter resolves each kept token through the phrase index
/// when the caller reads it (`convert_to_utf8`, `lookup/lookup.cpp:37-48`
/// at 074a2219), and the candidate getter returns its saved string
/// (`zhuyin.cpp:1865-1870`). After a reload drops an unsaved phrase, its
/// token does not resolve: `get_phrase_item` fails and leaves the default
/// item of length 0 (`lookup.cpp:35,43`, `storage/phrase_index.h:81-84`),
/// so the token adds "". The next import reuses the token
/// (`zhuyin.cpp:474-478`, register row 104), and the kept sentence then
/// reads the new phrase with no new guess.
#[test]
fn a_kept_sentence_reads_its_tokens_through_the_live_phrase_index() {
    let user_dir = TempUserDir::new("sentence-token-reuse");
    let (context, spare) = open_with_user(&user_dir.path);
    // The count makes the 1-best row A's token alone, not the two system
    // phrases 好 and 你, which also read "好你". The read after the reload
    // tells the two rows apart.
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(unsafe { import(context, 7, "好你", "ㄏㄠˇ ㄋㄧˇ".as_bytes(), 10_000) });
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { chewing(context, "cl3su3") };
    let user_tokens = |text| {
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        (unsafe { tokens_of(instance, text) })
            .into_iter()
            .filter(|token| token >> 24 == 7)
            .collect::<Vec<_>>()
    };
    let [token] = user_tokens("好你")[..] else {
        panic!("one user token for 好你");
    };
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let candidate = unsafe { candidate_at(instance, 0) };
    let candidate_text = || {
        let mut text: *const c_char = ptr::null();
        assert!(unsafe { zhuyin_get_candidate_string(instance, candidate, &raw mut text) });
        // SAFETY: the getter answered the candidate's NUL-terminated text,
        // which the instance owns until its candidates are rebuilt.
        unsafe { std::ffi::CStr::from_ptr(text) }
            .to_str()
            .expect("UTF-8")
            .to_owned()
    };
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { token_phrase(instance, token) },
        (true, 2, Some("好你".into()))
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { sentence(instance) }, (true, Some("好你".into())));
    assert_eq!(candidate_text(), "好你");

    assert!(unsafe { zhuyin_unload_phrase_library(context, 7) });
    assert!(unsafe { zhuyin_load_phrase_library(context, 7) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { token_phrase(instance, token) },
        (false, UNTOUCHED, None)
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { sentence(instance) }, (true, Some(String::new())));

    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(unsafe { import(context, 7, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 50) });
    assert_eq!(user_tokens("你好"), [token]);
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { token_phrase(instance, token) },
        (true, 2, Some("你好".into()))
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { sentence(instance) }, (true, Some("你好".into())));
    assert_eq!(candidate_text(), "好你");
    unsafe { zhuyin_free_instance(instance) };
    // SAFETY: `context` was opened above and is not yet freed; `spare` was opened above and is not
    // yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, spare) };
}

/// A failed conversion for the row's only phrase leaves the sentence NULL
/// and still answers true (074a2219 `lookup/lookup.cpp:27-70`,
/// `zhuyin.cpp:983-999`).
#[test]
fn an_nbest_row_with_no_converted_phrase_answers_true_with_null() {
    use oxpinyin::data::phrase_library::{ChunkItem, build_chunk};
    use oxpinyin::data::row_format::pinyin_index::{encode_complete_key, encode_item};

    let scratch = tempfile::tempdir().expect("scratch");
    let system = private_system(scratch.path());
    let token = 0x0100_0001;
    let keys = [oxpinyin::ChewingKey::from_packed(0x1081)];
    let items = [(
        1,
        ChunkItem {
            phrase: vec![0x8000_0000],
            unigram: 100,
            prons: vec![(vec![keys[0].to_packed()], 10)],
        },
    )];
    let library = system.join("gb_char.bin");
    std::fs::remove_file(&library).expect("unlink gb_char.bin");
    std::fs::write(&library, build_chunk(&items).expect("one-item chunk"))
        .expect("write gb_char.bin");
    crate::support::craft_tree(
        &system.join("pinyin_index.bin"),
        &[(encode_complete_key(&keys), encode_item(token, &keys))],
    );

    let context = init_on(&system, None);
    assert!(!context.is_null());
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { full_pinyin(context, oxpinyin::OptionBits::USE_TONE.bits(), "ba1") };
    assert!(unsafe { zhuyin_guess_sentence(instance) });

    let mut out = SEEDED as *mut c_char;
    assert!(unsafe { zhuyin_get_sentence(instance, &raw mut out) });
    assert!(out.is_null());
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The zhuyin facade lists no longer phrase. `zhuyin_guess_candidates_after_cursor`
/// has the span loop and the sentence prepend only (`zhuyin.cpp:1460-1539`);
/// libpinyin's `_prepend_longer_candidates` has no counterpart, and
/// `zhuyin.h` has no such row type. After one syllable every row is
/// therefore one character, where the pinyin facade also lists a phrase that
/// starts with the syllable.
// tkrzw only: `candidate_rows` is.
#[cfg(feature = "tkrzw")]
#[test]
fn the_after_cursor_list_has_no_longer_row() {
    let (context, instance) = open();
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, c"su3".as_ptr()) },
        3
    );
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let rows = unsafe { crate::support::candidate_rows(instance) };
    assert!(rows.len() > 1, "the sentence row and a searched row");
    for (_, kind, text) in rows {
        assert!(
            matches!(
                kind,
                lookup_candidate_type_t::BEST_MATCH_CANDIDATE
                    | lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR
            ),
            "{kind:?}"
        );
        let text = String::from_utf8(text).expect("UTF-8");
        assert_eq!(text.chars().count(), 1, "{text}");
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// `zhuyin_train` on an instance with no result answers `false` before it
/// marks the context modified (`zhuyin.cpp:1704-1707`), so the save has
/// nothing to write and answers `false` (`:551-552`).
#[test]
fn a_fresh_instance_trains_nothing() {
    let user_dir = TempUserDir::new("train-fresh");
    let (context, instance) = open_with_user(&user_dir.path);
    assert!(!unsafe { crate::support::zhuyin_train(instance) });
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, c"su3cl3".as_ptr()) },
        6
    );
    assert!(!unsafe { crate::support::zhuyin_train(instance) });
    assert!(!unsafe { crate::support::zhuyin_save(context) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// `zhuyin_unload_phrase_library` refuses the TSI library, index 1, and
/// answers `true` for every other index below 16, with or without a loaded
/// library there (`zhuyin.cpp:379-390`; `TSI_DICTIONARY`,
/// `novel_types.h:155`).
#[test]
fn unload_refuses_only_the_tsi_library() {
    let (context, instance) = open();
    for index in 0..16_u8 {
        assert_eq!(
            unsafe { zhuyin_unload_phrase_library(context, index) },
            index != 1,
            "library {index}"
        );
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The candidate frequency uses the `table.conf` lambda as scanned, outside
/// `[0, 1]` too. The pin scans it with `%f` into a `gfloat`
/// (`table_info.cpp:220`) and computes
/// `(guint32)((lambda * bigram + (1 - lambda) * unigram / (gfloat) total) * 2^24)`
/// with no bigram here (`zhuyin.cpp:1264-1268`). With a facade total of
/// 2^26, 乙 (unigram 1) and 甲 (unigram 2) have the frequencies
/// `(guint32) 0.25` and `(guint32) 0.5` at lambda 0: a tie, which the
/// stable `g_array_sort` leaves in token order (`:1529`, `:1140-1143`). At
/// lambda -2 the factor is 3: `(guint32) 0.75` = 0 and `(guint32) 1.5` = 1,
/// so 甲 comes first. At lambda 1.5 the products are -0.125 and -0.25: both
/// truncate to 0, which a `guint32` holds, so the conversion is defined and
/// the tie keeps the token order. At lambda 6 the products are -1.25 and
/// -2.5: they truncate to -1 and -2, which no `guint32` holds, so the pin's
/// conversion is undefined (policy row 83, class (b)). oxpinyin stores 0 for
/// both, as Rust's `as u32` does for a negative value, and the tie keeps the
/// token order.
// tkrzw only: `candidate_rows` is.
#[cfg(feature = "tkrzw")]
#[test]
fn candidates_follow_a_lambda_outside_zero_to_one() {
    for (lambda, order) in [
        ("0", ["乙", "甲"]),
        ("-2", ["甲", "乙"]),
        ("1.5", ["乙", "甲"]),
        ("6", ["乙", "甲"]),
    ] {
        let scratch = tempfile::tempdir().expect("scratch");
        let system = crate::support::libraries_system(
            &crate::support::system_dir(),
            scratch.path(),
            &[
                (
                    1,
                    "gb_char.bin",
                    &[(&[0x4e59], &[0x1081], 1), (&[0x7532], &[0x1081], 2)],
                    1 << 26,
                ),
                (2, "gbk_char.bin", &[], 0),
                (3, "opengram.bin", &[], 0),
                (4, "merged.bin", &[], 0),
            ],
        );
        let conf = system.join("table.conf");
        let text = std::fs::read_to_string(&conf).expect("table.conf");
        assert!(text.contains("lambda parameter:0.312699"));
        std::fs::remove_file(&conf).expect("unlink the linked table.conf");
        std::fs::write(
            &conf,
            text.replace(
                "lambda parameter:0.312699",
                &format!("lambda parameter:{lambda}"),
            ),
        )
        .expect("write table.conf");
        let context = init_on(&system, None);
        assert!(!context.is_null(), "lambda {lambda}");
        let instance = unsafe { zhuyin_alloc_instance(context) };
        assert_eq!(
            unsafe { zhuyin_parse_more_full_pinyins(instance, c"ba1".as_ptr()) },
            3
        );
        assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let texts: Vec<String> = unsafe { crate::support::candidate_rows(instance) }
            .into_iter()
            .map(|(_, _, text)| String::from_utf8(text).expect("UTF-8"))
            .collect();
        assert_eq!(texts, order, "lambda {lambda}");
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
    }
}

/// A parse that places no key refuses the guess: an empty reparse, and a
/// lone separator, which keeps the earlier sentence and clears the list
/// (`phonetic_lookup.h:743-748`, `zhuyin.cpp:902-907`).
#[test]
fn a_keyless_parse_refuses_the_guess_and_keeps_the_sentence() {
    let (context, _) = open();
    assert!(unsafe { zhuyin_set_options(context, 1 << 5) });
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { chewing(context, "su3cl3") };
    assert_eq!(unsafe { zhuyin_get_parsed_input_length(instance) }, 6);
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, cstr("").as_ptr()) },
        0
    );
    assert!(!unsafe { zhuyin_guess_sentence(instance) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };

    let (context, instance) = open();
    unsafe { zhuyin_parse_more_full_pinyins(instance, cstr("ni3hao3").as_ptr()) };
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(instance, cstr("'").as_ptr()) },
        1
    );
    assert!(!unsafe { zhuyin_guess_sentence(instance) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { sentence(instance) },
        (true, Some("你好".to_owned()))
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { n_candidate(instance) }, (true, 0));
    unsafe { zhuyin_free_instance(instance) };

    let instance = unsafe { zhuyin_alloc_instance(context) };
    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(instance, cstr("'").as_ptr()) },
        1
    );
    assert!(!unsafe { zhuyin_guess_sentence(instance) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { sentence(instance) }, (false, None));
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A `BEST_MATCH` row chosen after a reparse that places no key answers the
/// empty matrix's `size() - 1`, -1 (`zhuyin.cpp:1643-1644`,
/// `phonetic_key_matrix.cpp:34-38` at 074a2219), not the consumed length.
#[test]
fn a_best_match_choose_after_a_keyless_reparse_answers_minus_one() {
    let (context, _) = open();
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { chewing(context, "su3cl3") };
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
    let mut row: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe { zhuyin_get_candidate(instance, 0, &raw mut row) });
    let mut kind = crate::support::lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR;
    assert!(unsafe { crate::support::zhuyin_get_candidate_type(instance, row, &raw mut kind) });
    assert_eq!(
        kind,
        crate::support::lookup_candidate_type_t::BEST_MATCH_CANDIDATE
    );
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, cstr("").as_ptr()) },
        0
    );
    assert_eq!(
        unsafe { crate::support::zhuyin_choose_candidate(instance, 0, row) },
        -1
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The after-cursor guess answers at the reserved slot and asserts one past
/// it (`zhuyin.cpp:1453`).
#[test]
fn after_cursor_guess_one_past_the_reserved_slot_aborts() {
    assert_aborts(
        "sentence_and_candidates::after_cursor_guess_one_past_the_reserved_slot_aborts",
        || {
            let (context, _) = open();
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            let instance = unsafe { chewing(context, "su3cl3") };
            assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 6) });
            unsafe { zhuyin_guess_candidates_after_cursor(instance, 7) };
        },
    );
}

/// The sentence getter answers from the n-best results alone: false with the
/// output untouched on a fresh and on a parsed instance (`zhuyin.cpp:988`).
#[test]
fn get_sentence_before_a_guess_leaves_the_output() {
    let (context, instance) = open();
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { sentence(instance) }, (false, None));
    unsafe { zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { sentence(instance) }, (false, None));
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A partial user gram refuses the candidate lookup with one warning.
#[test]
fn partial_candidate_gram_logs_once() {
    let (context, instance) = open();
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { tokens_of(instance, "你好") }[0];
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    let user = TempUserDir::new("partial-candidate-gram");
    let mut gram = 7_u32.to_le_bytes().to_vec();
    gram.extend_from_slice(&token.to_le_bytes());
    gram.extend_from_slice(&7_u32.to_le_bytes());
    gram.push(1);
    crate::support::plant_bigram(&user.path, vec![(token.to_le_bytes().to_vec(), gram)]);
    let (context, instance) = open_with_user(&user.path);
    assert!(unsafe {
        crate::support::zhuyin_set_options(context, oxpinyin::OptionBits::DYNAMIC_ADJUST.bits())
    });
    assert!(!unsafe {
        crate::support::zhuyin_guess_sentence_with_prefix(instance, c"你好".as_ptr())
    });
    assert_eq!(
        unsafe { crate::support::zhuyin_parse_more_full_pinyins(instance, c"hao".as_ptr()) },
        3
    );
    let logs = LogCapture::new();
    assert!(!unsafe { crate::support::zhuyin_guess_candidates_after_cursor(instance, 0) });
    assert_eq!(
        logs.take(),
        [(Some("libzhuyin".to_owned()), glib_sys::G_LOG_LEVEL_WARNING)]
    );
    let mut count = u32::MAX;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    assert_eq!(count, 0);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The zhuyin twin of issue #356: `zhuyin_get_character_offset` runs
/// the same phrase-table search and matrix walk (`zhuyin.cpp:2148`
/// at the pin). `su3cl3` is 你好 on the standard keyboard: the
/// keystroke string as the phrase answers `false`, the sentence
/// counts one character per key at or before the offset.
#[test]
fn character_offset_searches_the_phrase_and_walks_the_keys() {
    let (context, instance) = open();
    let input = cstr("su3cl3");
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, input.as_ptr()) },
        6
    );

    let phrase = cstr("su3cl3");
    for offset in [0, 3, 6] {
        let mut length = usize::MAX;
        assert!(!unsafe {
            zhuyin_get_character_offset(instance, phrase.as_ptr(), offset, &raw mut length)
        });
        assert_eq!(length, usize::MAX, "offset {offset}");
    }
    let phrase = cstr("你好");
    for (offset, expected) in [(0, 0), (3, 1)] {
        let mut length = usize::MAX;
        assert!(unsafe {
            zhuyin_get_character_offset(instance, phrase.as_ptr(), offset, &raw mut length)
        });
        assert_eq!(length, expected, "offset {offset}");
    }
    // At the reserved slot the pin's zhuyin walk has no zero-key skip: it
    // steps onto the last column, finds no reading and fails, storing the
    // length it reached (`zhuyin.cpp:2095-2140`, `:2192`).
    let mut length = usize::MAX;
    assert!(!unsafe { zhuyin_get_character_offset(instance, phrase.as_ptr(), 6, &raw mut length) });
    assert_eq!(length, 0);

    unsafe { zhuyin_free_instance(instance) };
    unsafe { zhuyin_fini(context) };
}

fn parse_for_character_offset(input: &str) -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    let (context, instance) = open();
    assert!(unsafe { zhuyin_set_options(context, oxpinyin::OptionBits::USE_TONE.bits()) });
    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(instance, cstr(input).as_ptr()) },
        input.len()
    );
    (context, instance)
}

/// The separator and the terminal key both test the next character's
/// pronunciation before comparing the raw end to the offset (zhuyin.cpp:2115-2135).
#[rstest::rstest]
#[case::before_separator("ni'hao", "你好", 0, (true, 0))]
#[case::at_separator("ni'hao", "你好", 2, (false, 0))]
#[case::past_separator("ni'hao", "你好", 4, (false, 0))]
#[case::at_end("ni'hao", "你好", 6, (false, 0))]
#[case::separator_matches("ni'hao", "你啊好", 4, (true, 2))]
#[case::terminal_matches("ni", "你啊", 2, (true, 1))]
fn character_offset_full_pinyin(
    #[case] input: &str,
    #[case] phrase: &str,
    #[case] offset: usize,
    #[case] expected: (bool, usize),
) {
    let (context, instance) = parse_for_character_offset(input);
    let mut length = SEEDED;
    let found = unsafe {
        zhuyin_get_character_offset(instance, cstr(phrase).as_ptr(), offset, &raw mut length)
    };
    assert_eq!((found, length), expected);
    unsafe { close(context, instance) };
}

/// The preceding column holds a lone zero key (zhuyin.cpp:1447-1454, 2159).
#[test]
fn character_offset_full_pinyin_immediately_after_separator_aborts() {
    assert_aborts_after_setup(
        "sentence_and_candidates::character_offset_full_pinyin_immediately_after_separator_aborts",
        || parse_for_character_offset("ni'hao"),
        |(_context, instance)| {
            let mut length = SEEDED;
            unsafe {
                zhuyin_get_character_offset(instance, c"你好".as_ptr(), 3, &raw mut length)
            };
        },
    );
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
    use crate::support::candidate_text;
    use crate::support::zhuyin_get_candidate;
    use crate::support::zhuyin_guess_candidates_after_cursor;

    let (context, instance) = open();
    let first = (0..8)
        .find(|index| {
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            let _ = unsafe { candidate(instance, "su3cl3", 0) };
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            (unsafe { candidate_text(instance, *index) }) == "你"
        })
        .expect("你 is offered at 0");
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let cand = unsafe {
        candidate(
            instance,
            "su3cl3",
            u32::try_from(first).expect("small index"),
        )
    };
    assert_eq!(unsafe { zhuyin_choose_candidate(instance, 0, cand) }, 3);
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 3) });
    let mut count = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    let other = (0..usize::try_from(count).expect("count fits"))
        .find(|index| {
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            let text = unsafe { candidate_text(instance, *index) };
            text.chars().count() == 1 && text != "好"
        })
        .expect("a one-character row other than 好 is offered at 3");
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let text = unsafe { candidate_text(instance, other) };
    let mut chosen: *mut crate::support::LookupCandidate = ptr::null_mut();
    assert!(unsafe {
        zhuyin_get_candidate(
            instance,
            u32::try_from(other).expect("small index"),
            &raw mut chosen,
        )
    });
    assert_eq!(unsafe { zhuyin_choose_candidate(instance, 3, chosen) }, 6);
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(unsafe { zhuyin_get_sentence(instance, &raw mut sentence) });
    assert!(
        !sentence.is_null(),
        "zhuyin_get_sentence answered true but wrote a NULL sentence"
    );
    assert_eq!(
        // SAFETY: `sentence` is not NULL (checked above), so it is the string that
        // `zhuyin_get_sentence` allocated for this caller, who owns it and frees it only here.
        unsafe { take_sentence(sentence) },
        format!("你{text}"),
        "the forcing at the cursor survived the re-guess"
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}
