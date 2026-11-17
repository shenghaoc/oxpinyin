//! Sentence and candidate guesses, choosing a candidate and the sentence
//! step prices.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    LogCapture, LookupCandidate, TempUserDir, ZhuyinInstance, assert_aborts, candidate, close,
    cstr, import, init_on, open, open_with_user, private_system, take_sentence, tokens_of,
    zhuyin_alloc_instance, zhuyin_choose_candidate, zhuyin_fini, zhuyin_free_instance,
    zhuyin_get_candidate, zhuyin_get_candidate_string, zhuyin_get_character_offset,
    zhuyin_get_n_candidate, zhuyin_get_parsed_input_length, zhuyin_get_sentence,
    zhuyin_guess_candidates_after_cursor, zhuyin_guess_sentence, zhuyin_load_phrase_library,
    zhuyin_parse_more_chewings, zhuyin_parse_more_full_pinyins, zhuyin_set_options,
    zhuyin_unload_phrase_library, *,
};

use std::os::raw::c_char;

use std::ptr;

/// The candidate count of the current list.
fn n_candidate(instance: *mut ZhuyinInstance) -> (bool, u32) {
    let mut count = UNTOUCHED;
    let ret = unsafe { zhuyin_get_n_candidate(instance, &raw mut count) };
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
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
    let candidate = candidate_at(instance, 0);
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
        token_phrase(instance, token),
        (true, 2, Some("好你".into()))
    );
    assert_eq!(sentence(instance), (true, Some("好你".into())));
    assert_eq!(candidate_text(), "好你");

    assert!(unsafe { zhuyin_unload_phrase_library(context, 7) });
    assert!(unsafe { zhuyin_load_phrase_library(context, 7) });
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
    unsafe { zhuyin_free_instance(instance) };
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
    crate::support::craft_tree(
        &system.join("pinyin_index.bin"),
        &[(encode_complete_key(&keys), encode_item(token, &keys))],
    );

    let context = init_on(&system, None);
    assert!(!context.is_null());
    let instance = full_pinyin(context, oxpinyin_core::OptionBits::USE_TONE.bits(), "ba1");
    assert!(unsafe { zhuyin_guess_sentence(instance) });

    let mut out = SEEDED as *mut c_char;
    assert!(unsafe { zhuyin_get_sentence(instance, &raw mut out) });
    assert!(out.is_null());
    close(context, instance);
}

/// A parse that places no key refuses the guess: an empty reparse, and a
/// lone separator, which keeps the earlier sentence and clears the list
/// (`phonetic_lookup.h:743-748`, `zhuyin.cpp:902-907`).
#[test]
fn a_keyless_parse_refuses_the_guess_and_keeps_the_sentence() {
    let (context, _) = open();
    assert!(unsafe { zhuyin_set_options(context, 1 << 5) });
    let instance = chewing(context, "su3cl3");
    assert_eq!(unsafe { zhuyin_get_parsed_input_length(instance) }, 6);
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, cstr("").as_ptr()) },
        0
    );
    assert!(!unsafe { zhuyin_guess_sentence(instance) });
    close(context, instance);

    let (context, instance) = open();
    unsafe { zhuyin_parse_more_full_pinyins(instance, cstr("ni3hao3").as_ptr()) };
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(instance, cstr("'").as_ptr()) },
        1
    );
    assert!(!unsafe { zhuyin_guess_sentence(instance) });
    assert_eq!(sentence(instance), (true, Some("你好".to_owned())));
    assert_eq!(n_candidate(instance), (true, 0));
    unsafe { zhuyin_free_instance(instance) };

    let instance = unsafe { zhuyin_alloc_instance(context) };
    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(instance, cstr("'").as_ptr()) },
        1
    );
    assert!(!unsafe { zhuyin_guess_sentence(instance) });
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
    close(context, instance);
}

/// The after-cursor guess answers at the reserved slot and asserts one past
/// it (`zhuyin.cpp:1453`).
#[test]
fn after_cursor_guess_one_past_the_reserved_slot_aborts() {
    assert_aborts(
        "sentence_and_candidates::after_cursor_guess_one_past_the_reserved_slot_aborts",
        || {
            let (context, _) = open();
            let instance = chewing(context, "su3cl3");
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
    assert_eq!(sentence(instance), (false, None));
    unsafe { zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()) };
    assert_eq!(sentence(instance), (false, None));
    close(context, instance);
}

/// A partial user gram refuses the candidate lookup with one warning.
#[test]
fn partial_candidate_gram_logs_once() {
    let (context, instance) = open();
    let token = tokens_of(instance, "你好")[0];
    close(context, instance);
    let user = TempUserDir::new("partial-candidate-gram");
    let mut gram = 7_u32.to_le_bytes().to_vec();
    gram.extend_from_slice(&token.to_le_bytes());
    gram.extend_from_slice(&7_u32.to_le_bytes());
    gram.push(1);
    crate::support::plant_bigram(&user.path, vec![(token.to_le_bytes().to_vec(), gram)]);
    let (context, instance) = open_with_user(&user.path);
    assert!(unsafe {
        crate::support::zhuyin_set_options(
            context,
            oxpinyin_core::OptionBits::DYNAMIC_ADJUST.bits(),
        )
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
    close(context, instance);
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
            let _ = candidate(instance, "su3cl3", 0);
            candidate_text(instance, *index) == "你"
        })
        .expect("你 is offered at 0");
    let cand = candidate(
        instance,
        "su3cl3",
        u32::try_from(first).expect("small index"),
    );
    assert_eq!(unsafe { zhuyin_choose_candidate(instance, 0, cand) }, 3);
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 3) });
    let mut count = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    let other = (0..usize::try_from(count).expect("count fits"))
        .find(|index| {
            let text = candidate_text(instance, *index);
            text.chars().count() == 1 && text != "好"
        })
        .expect("a one-character row other than 好 is offered at 3");
    let text = candidate_text(instance, other);
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
    assert_eq!(
        take_sentence(sentence),
        format!("你{text}"),
        "the forcing at the cursor survived the re-guess"
    );
    close(context, instance);
}
