//! Sentence and candidate guesses, choosing a candidate and the sentence
//! step prices.

use super::*;

/// The candidate count of the current list.
fn n_candidate(instance: *mut ZhuyinInstance) -> (bool, u32) {
    let mut count = UNTOUCHED;
    let ret = zhuyin_get_n_candidate(instance, &raw mut count);
    (ret, count)
}

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
    assert!(import(context, 7, "好你", "ㄏㄠˇ ㄋㄧˇ".as_bytes(), 10_000));
    let instance = chewing(context, "cl3su3");
    let user_tokens = |text| {
        tokens_of(instance, text)
            .into_iter()
            .filter(|token| token >> 24 == 7)
            .collect::<Vec<_>>()
    };
    let [token] = user_tokens("好你")[..] else {
        panic!("one user token for 好你");
    };
    assert!(zhuyin_guess_sentence(instance));
    assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
    let candidate = candidate_at(instance, 0);
    let candidate_text = || {
        let mut text: *const c_char = ptr::null();
        assert!(zhuyin_get_candidate_string(
            instance,
            candidate,
            &raw mut text
        ));
        // SAFETY: the getter answered the candidate's NUL-terminated text,
        // which the instance owns until its candidates are rebuilt.
        unsafe { std::ffi::CStr::from_ptr(text) }
            .to_str()
            .expect("UTF-8")
            .to_owned()
    };
    assert_eq!(
        token_phrase(instance, token),
        (true, 2, Some("好你".into()))
    );
    assert_eq!(sentence(instance), (true, Some("好你".into())));
    assert_eq!(candidate_text(), "好你");

    assert!(zhuyin_unload_phrase_library(context, 7));
    assert!(zhuyin_load_phrase_library(context, 7));
    assert_eq!(token_phrase(instance, token), (false, UNTOUCHED, None));
    assert_eq!(sentence(instance), (true, Some(String::new())));

    assert!(import(context, 7, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 50));
    assert_eq!(user_tokens("你好"), [token]);
    assert_eq!(
        token_phrase(instance, token),
        (true, 2, Some("你好".into()))
    );
    assert_eq!(sentence(instance), (true, Some("你好".into())));
    assert_eq!(candidate_text(), "好你");
    zhuyin_free_instance(instance);
    close(context, spare);
}

/// A failed conversion for the row's only phrase leaves the sentence NULL
/// and still answers true (074a2219 `lookup/lookup.cpp:27-70`,
/// `zhuyin.cpp:983-999`).
#[test]
fn an_nbest_row_with_no_converted_phrase_answers_true_with_null() {
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};
    use oxpinyin_data::row_format::pinyin_index::{encode_complete_key, encode_item};

    let scratch = tempfile::tempdir().expect("scratch");
    let system = private_system(scratch.path());
    let token = 0x0100_0001;
    let keys = [oxpinyin_core::ChewingKey::from_packed(0x1081)];
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
    crate::test_support::craft_tree(
        &system.join("pinyin_index.bin"),
        &[(encode_complete_key(&keys), encode_item(token, &keys))],
    );

    let context = init_on(&system, None);
    assert!(!context.is_null());
    let instance = full_pinyin(context, oxpinyin_core::OptionBits::USE_TONE.bits(), "ba1");
    assert!(zhuyin_guess_sentence(instance));

    let mut out = SEEDED as *mut c_char;
    assert!(zhuyin_get_sentence(instance, &raw mut out));
    assert!(out.is_null());
    close(context, instance);
}

/// A parse that places no key refuses the guess: an empty reparse, and a
/// lone separator, which keeps the earlier sentence and clears the list
/// (`phonetic_lookup.h:743-748`, `zhuyin.cpp:902-907`).
#[test]
fn a_keyless_parse_refuses_the_guess_and_keeps_the_sentence() {
    let (context, _) = open();
    assert!(zhuyin_set_options(context, 1 << 5));
    let instance = chewing(context, "su3cl3");
    assert_eq!(zhuyin_get_parsed_input_length(instance), 6);
    assert!(zhuyin_guess_sentence(instance));
    assert_eq!(zhuyin_parse_more_chewings(instance, cstr("").as_ptr()), 0);
    assert!(!zhuyin_guess_sentence(instance));
    close(context, instance);

    let (context, instance) = open();
    zhuyin_parse_more_full_pinyins(instance, cstr("ni3hao3").as_ptr());
    assert!(zhuyin_guess_sentence(instance));
    assert_eq!(
        zhuyin_parse_more_full_pinyins(instance, cstr("'").as_ptr()),
        1
    );
    assert!(!zhuyin_guess_sentence(instance));
    assert_eq!(sentence(instance), (true, Some("你好".to_owned())));
    assert_eq!(n_candidate(instance), (true, 0));
    zhuyin_free_instance(instance);

    let instance = zhuyin_alloc_instance(context);
    assert_eq!(
        zhuyin_parse_more_full_pinyins(instance, cstr("'").as_ptr()),
        1
    );
    assert!(!zhuyin_guess_sentence(instance));
    assert_eq!(sentence(instance), (false, None));
    close(context, instance);
}

/// A `BEST_MATCH` row chosen after a reparse that places no key answers the
/// empty matrix's `size() - 1`, -1 (`zhuyin.cpp:1643-1644`,
/// `phonetic_key_matrix.cpp:34-38` at 074a2219), not the consumed length.
#[test]
fn a_best_match_choose_after_a_keyless_reparse_answers_minus_one() {
    let (context, _) = open();
    let instance = chewing(context, "su3cl3");
    assert!(zhuyin_guess_sentence(instance));
    assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
    let mut row: *mut LookupCandidate = ptr::null_mut();
    assert!(zhuyin_get_candidate(instance, 0, &raw mut row));
    let mut kind = crate::types::lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR;
    assert!(crate::candidates::zhuyin_get_candidate_type(
        instance,
        row,
        &raw mut kind
    ));
    assert_eq!(
        kind,
        crate::types::lookup_candidate_type_t::BEST_MATCH_CANDIDATE
    );
    assert_eq!(zhuyin_parse_more_chewings(instance, cstr("").as_ptr()), 0);
    assert_eq!(
        crate::candidates::zhuyin_choose_candidate(instance, 0, row),
        -1
    );
    close(context, instance);
}

/// The after-cursor guess answers at the reserved slot and asserts one past
/// it (`zhuyin.cpp:1453`).
#[test]
fn after_cursor_guess_one_past_the_reserved_slot_aborts() {
    assert_aborts(
        "boundary_tests::sentence_and_candidates::after_cursor_guess_one_past_the_reserved_slot_aborts",
        || {
            let (context, _) = open();
            let instance = chewing(context, "su3cl3");
            assert!(zhuyin_guess_candidates_after_cursor(instance, 6));
            zhuyin_guess_candidates_after_cursor(instance, 7);
        },
    );
}

/// su3cl3 with the facade's unigram total wrapped to zero through
/// `zhuyin_token_add_unigram_frequency`.
fn zero_total(context: *mut ZhuyinContext) -> *mut ZhuyinInstance {
    let instance = chewing(context, "su3cl3");
    // SAFETY: `instance` is live; the reference ends in this block.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    let total = u32::try_from(total).expect("a guint32 total");
    assert!(zhuyin_token_add_unigram_frequency(
        instance,
        0x0100_0001,
        total.wrapping_neg()
    ));
    instance
}

/// A zero facade total asserts while the after-cursor guess ranks a row
/// (`zhuyin.cpp:1261`, register rows 61 and 66).
#[test]
fn a_zero_facade_total_aborts_the_after_cursor_ranking() {
    assert_aborts(
        "boundary_tests::sentence_and_candidates::a_zero_facade_total_aborts_the_after_cursor_ranking",
        || {
            let (context, _) = open();
            zhuyin_guess_candidates_after_cursor(zero_total(context), 0);
        },
    );
}

/// Before offset 0 the before-cursor guess searches no span, so a zero
/// total ranks nothing: true with no row.
#[test]
fn a_zero_facade_total_before_offset_zero_ranks_nothing() {
    let (context, _) = open();
    let instance = zero_total(context);
    assert!(zhuyin_guess_candidates_before_cursor(instance, 0));
    assert_eq!(n_candidate(instance), (true, 0));
    close(context, instance);
}

/// The sentence getter answers from the n-best results alone: false with the
/// output untouched on a fresh and on a parsed instance (`zhuyin.cpp:988`).
#[test]
fn get_sentence_before_a_guess_leaves_the_output() {
    let (context, instance) = open();
    assert_eq!(sentence(instance), (false, None));
    zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr());
    assert_eq!(sentence(instance), (false, None));
    close(context, instance);
}

/// A step prices a reading with the query's tone (`phonetic_key_matrix.cpp:
/// 562,575`, `phrase_index.h:136-164` at 074a2219): a nonzero query tone
/// matches an equal or zero stored tone only (`pinyin_phrase3.h:96-105`).
/// The second import of 甲 adds a reading and no index row
/// (`pinyin.cpp:563-582`). Under the default `USE_TONE` (`zhuyin.cpp:273`),
/// ㄅㄚˋ prices 甲 at 2/100, so 乙 (unigram 3) outranks 甲 (unigram 6).
#[test]
fn a_step_prices_readings_with_the_query_tone() {
    let user = TempUserDir::new("toned-step");
    let (context, instance) = open_with_user(&user.path);
    for (text, reading, count) in [("甲", "ㄅㄚˋ", 2), ("甲", "ㄅㄚˇ", 98), ("乙", "ㄅㄚˋ", 1)]
    {
        assert!(import(context, 1, text, reading.as_bytes(), count));
    }
    zhuyin_parse_more_chewings(instance, cstr("184").as_ptr());
    assert!(zhuyin_guess_sentence(instance));
    assert_eq!(sentence(instance), (true, Some("乙".to_owned())));
    close(context, instance);
}

/// A step prices a user item from its own readings
/// (`phonetic_lookup.h:646-656`, `phrase_index.h:136-164` at 074a2219).
/// Library 6's 甲 holds ㄅㄚˋ 3 and ㄉㄚˋ 9, so ㄅㄚˋ prices it at 3/12 and
/// 乙 (unigram 6, one reading) outranks 甲 (unigram 9).
#[test]
fn a_step_prices_a_user_item_from_its_readings() {
    let user = TempUserDir::new("user-item-step");
    let (context, instance) = open_with_user(&user.path);
    for (text, reading, count) in [("甲", "ㄅㄚˋ", 3), ("甲", "ㄉㄚˋ", 9), ("乙", "ㄅㄚˋ", 2)]
    {
        assert!(import(context, 6, text, reading.as_bytes(), count));
    }
    zhuyin_parse_more_chewings(instance, cstr("184").as_ptr());
    assert!(zhuyin_guess_sentence(instance));
    assert_eq!(sentence(instance), (true, Some("乙".to_owned())));
    close(context, instance);
}
