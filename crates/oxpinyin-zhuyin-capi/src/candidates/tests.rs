#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::candidates::{
    zhuyin_choose_candidate, zhuyin_clear_constraint, zhuyin_get_candidate, zhuyin_get_n_candidate,
    zhuyin_train,
};

use crate::context::{zhuyin_fini, zhuyin_save};

use crate::dict::zhuyin_token_get_unigram_frequency;

use crate::instance::zhuyin_free_instance;

use crate::parse::zhuyin_parse_more_chewings;

use crate::sentence::{
    zhuyin_get_sentence, zhuyin_guess_candidates_before_cursor, zhuyin_guess_sentence,
};

use crate::state::instance_mut;

use crate::test_support::{
    LogCapture, TempUserDir, candidate, candidate_text, close, cstr, open, open_with_user,
    system_dir, take_sentence, tokens_of, with_store,
};

use crate::types::LookupCandidate;

use oxpinyin_user::SENTENCE_START;

use rstest::rstest;

use std::ffi::c_char;

use std::ptr;

/// `phonetic_lookup.h:904`: training stores into the failed Berkeley DB
/// user bigram container and the pin's `check_result` dies. Only
/// running out of memory makes that container, so the training answer
/// is handed to the refusal directly.
#[test]
fn a_store_into_the_failed_user_bigram_aborts_training() {
    let stderr = crate::test_support::assert_aborts(
        "candidates::tests::a_store_into_the_failed_user_bigram_aborts_training",
        || {
            super::train_answer(Err(oxpinyin_engine::EngineError::FailedUserBigramStore {
                token: 0x0100_0001,
            }));
        },
    );
    // No Rust panic text or other line: the pin writes none here but
    // glibc's assert line, the register's policy-accepted missing line.
    assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
}

/// The zhuyin twin of the pinyin crate's
/// `choosing_from_a_reanchored_window_uses_the_anchored_span`
/// (`oxpinyin-capi/src/candidates/tests.rs`): a
/// `zhuyin_guess_candidates_before_cursor` window is re-anchored, so
/// the following `zhuyin_choose_candidate` must resolve the row's
/// index against THAT window.
///
/// `su3cl3` is `ni3'hao3` in the session's `'`-joined buffer, and the
/// two lists provably differ at row 1: the composition-anchored cached
/// list offers 你 (the first key's span), the `before(6)` window offers
/// 好 (the second key's span, ending at the cursor). Resolving row 1
/// through the cached list committed 你 and answered cursor 3 — a row
/// the caller never displayed.
#[test]
fn choosing_from_a_before_cursor_window_uses_that_window() {
    let (context, instance) = open();
    let input = cstr("su3cl3");
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, input.as_ptr()) },
        "su3cl3".len(),
        "the whole keystroke run parses"
    );

    assert!(unsafe { zhuyin_guess_candidates_before_cursor(instance, 6) });
    let mut count = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    assert!(count > 1, "the before-cursor window carries several rows");

    // No sentence was guessed, so nothing is prepended
    // (`zhuyin.cpp:1277-1278`): every row is a phrase row of the spans
    // ending at the cursor (`:1594`), and row 1 is the second of them.
    let displayed = candidate_text(instance, 1);
    assert_eq!(displayed, "好", "the fixture's before(6) row 1");
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe { zhuyin_get_candidate(instance, 1, &raw mut cand) });
    assert!(!cand.is_null());

    // The chosen span is `[3, 6)` — the second key alone. Upstream
    // writes the constraint on `[m_begin, m_end)` and, for a
    // before-cursor row, answers `m_begin` as the new cursor
    // (`zhuyin.cpp:1656-1660` at the pin): 3, the span's start — not
    // the span's end. (The cached list's row 1 also ends at 3; the
    // committed text below is what tells the two rows apart.)
    assert_eq!(unsafe { zhuyin_choose_candidate(instance, 6, cand) }, 3);
    // The session's own composition offset advanced to the span's end,
    // which is the whole joined buffer here.
    // SAFETY: the instance is live and no other reference is held.
    let session = unsafe { &instance_mut(instance).core.session };
    assert_eq!(
        session.composition_offset(),
        session.raw_input().len(),
        "the composition consumed through the chosen span's end"
    );

    // The forcing is the row's own span `[3, 6)`, not `[0, 6)`: the
    // first key's cell is free, the second key's is forced.
    // (`clear_constraint` answers false for a free cell and true for a
    // hit anywhere inside a forced run; probing 0 first cannot disturb
    // the run at 3.)
    assert!(
        !unsafe { zhuyin_clear_constraint(instance, 0) },
        "no forcing covers the first key"
    );

    // The pin's constrain-and-re-decode: a re-guess keeps the leading
    // key's conversion and the forced 好 (register, measured
    // 2026-09-05 on the pin-built oracle: 你好 after the choose).
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(unsafe { zhuyin_get_sentence(instance, &raw mut sentence) });
    assert!(!sentence.is_null());
    // SAFETY: `sentence` is a NUL-terminated string this facade just
    // malloc'd for the caller; it is read, then released with the
    // allocator that produced it.
    let decoded = unsafe { std::ffi::CStr::from_ptr(sentence) }
        .to_str()
        .expect("UTF-8")
        .to_owned();
    // SAFETY: `sentence` is the returned GLib string, which `g_free`
    // releases, and is not used after this call.
    unsafe { glib_sys::g_free(sentence.cast()) };
    assert_eq!(decoded, "你好", "the re-decode keeps 你 and forces 好");
    assert!(
        unsafe { zhuyin_clear_constraint(instance, 3) },
        "the forcing starts where 好 starts"
    );
    unsafe { zhuyin_free_instance(instance) };
    unsafe { zhuyin_fini(context) };
}

// 074a2219 zhuyin.cpp:1058-1081 preserves the candidate;
// :1646-1666 chooses its own token/span and returns its original end.
#[rstest]
#[case::unchanged("su3cl3", false, 6, false)]
#[case::shorter("su3", false, 3, true)]
#[case::unconsumed_suffix("ni3!hao3", true, 3, true)]
#[case::zero_keys("''''''''", true, 8, true)]
fn reparse_keeps_the_reanchored_candidates_selection(
    #[case] reparse: &str,
    #[case] full_pinyin: bool,
    #[case] consumed: usize,
    #[case] refused: bool,
    #[values(false, true)] before: bool,
) {
    use crate::candidates::{
        zhuyin_choose_candidate, zhuyin_clear_constraint, zhuyin_get_candidate,
    };
    use crate::sentence::{
        zhuyin_guess_candidates_after_cursor, zhuyin_guess_candidates_before_cursor,
    };
    use oxpinyin_core::Dictionary;
    use std::ptr;

    let (context, instance) = open();
    let input = cstr("su3cl3");
    assert_eq!(
        unsafe { crate::parse::zhuyin_parse_more_chewings(instance, input.as_ptr()) },
        6
    );
    assert!(if before {
        unsafe { zhuyin_guess_candidates_before_cursor(instance, 6) }
    } else {
        unsafe { zhuyin_guess_candidates_after_cursor(instance, 3) }
    });
    let (index, token) = {
        // SAFETY: live instance immediately after the guess.
        let inst = unsafe { crate::state::instance_ref(instance) };
        let index = inst
            .state
            .candidates
            .iter()
            .position(|row| row.text_str() == Ok("好"))
            .expect("好 at offset 3");
        (index, inst.core.dict.tokens_for_text("好")[0])
    };
    let mut original = ptr::null_mut();
    assert!(unsafe {
        zhuyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut original,
        )
    });
    let reparsed = cstr(reparse);
    assert_eq!(
        if full_pinyin {
            unsafe { crate::parse::zhuyin_parse_more_full_pinyins(instance, reparsed.as_ptr()) }
        } else {
            unsafe { crate::parse::zhuyin_parse_more_chewings(instance, reparsed.as_ptr()) }
        },
        consumed
    );
    // No candidate guess: selection must still use 好 over the second key.
    // Unlike Pinyin, refusal still returns the original m_begin/m_end.
    let expected_tokens = [token];
    // The pin leaves the candidates intact on choose, so a second
    // choose still sees this same token and span (zhuyin.cpp:1646-1666).
    for _ in 0..2 {
        assert_eq!(
            unsafe { zhuyin_choose_candidate(instance, if before { 6 } else { 3 }, original) },
            if before { 3 } else { 6 }
        );
        assert_eq!(
            // SAFETY: the choose completed and the instance is still live.
            unsafe { crate::state::instance_ref(instance) }
                .core
                .session
                .selected_tokens(),
            if refused {
                &[][..]
            } else {
                &expected_tokens[..]
            }
        );
    }
    assert!(!unsafe { zhuyin_clear_constraint(instance, 0) });
    assert_eq!(unsafe { zhuyin_clear_constraint(instance, 3) }, !refused);
    close(context, instance);
}

/// The decoded sentence of n-best row 0.
fn sentence_of(instance: *mut crate::types::ZhuyinInstance) -> String {
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(unsafe { zhuyin_get_sentence(instance, &raw mut sentence) });
    take_sentence(sentence)
}

/// Each token's unigram, as `zhuyin_token_get_unigram_frequency` reads it.
fn unigrams_of(instance: *mut crate::types::ZhuyinInstance, tokens: &[u32]) -> Vec<u32> {
    tokens
        .iter()
        .map(|&token| {
            let mut freq = 0;
            assert!(unsafe { zhuyin_token_get_unigram_frequency(instance, token, &raw mut freq) });
            freq
        })
        .collect()
}

/// `zhuyin_train` after `zhuyin_guess_sentence` with no choose trains
/// nothing, answers true and arms the save; without a user directory it refuses.
///
/// At 074a2219, `zhuyin_train` (`zhuyin.cpp:1696-1716`) answers false only
/// without a user dir or without a sentence result; otherwise it sets
/// `m_modified` (`:1707`) before `train_result3`. That walk trains only at a
/// `CONSTRAINT_ONESTEP` position or right after one
/// (`lookup/phonetic_lookup.h:858-935`); with no choose there is none, so it
/// writes no bigram and no unigram and answers true. `zhuyin_save` answers
/// false unless `m_modified` (`zhuyin.cpp:551-552`), so the save writes the
/// user files and clears the flag (`:697`), and a second save answers false.
/// Each system library's journal is `SubPhraseIndex::diff`'s one
/// `LOG_MODIFY_HEADER` record of an unchanged total, with no item records
/// (`storage/phrase_index.cpp:392-437`, `phrase_index_logger.h:281-295`), and
/// the saved user bigram (`zhuyin.cpp:680-693`) holds no record.
#[test]
fn train_without_a_choose_trains_nothing_and_without_a_user_store_refuses() {
    let user_dir = TempUserDir::new("train-noselect");
    let (context, instance) = open_with_user(&user_dir.path);
    let logs = LogCapture::new();
    let input = cstr("su3cl3");
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, input.as_ptr()) },
        6
    );
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    let sentence = sentence_of(instance);
    // Every token of the sentence and of each of its characters: the
    // decoded row's tokens are among them, whatever its segmentation.
    let mut tokens = tokens_of(instance, &sentence);
    for ch in sentence.chars() {
        tokens.extend(tokens_of(instance, &ch.to_string()));
    }
    assert!(!tokens.is_empty(), "the sentence {sentence:?} has tokens");
    let unigrams = unigrams_of(instance, &tokens);

    assert!(
        unsafe { zhuyin_train(instance) },
        "the live sentence result admits the train"
    );
    assert_eq!(unigrams_of(instance, &tokens), unigrams, "no unigram moved");
    assert_eq!(sentence_of(instance), sentence, "the retained result");
    with_store(instance, |store| {
        assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), 0);
    });
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert_eq!(sentence_of(instance), sentence, "a fresh decode");

    assert!(unsafe { zhuyin_save(context) }, "the train armed the save");
    assert!(
        !unsafe { zhuyin_save(context) },
        "the save cleared the flag"
    );
    assert_eq!(logs.take(), Vec::new(), "no diagnostics");
    drop(logs);
    close(context, instance);

    let mut journals = 0;
    for entry in std::fs::read_dir(&user_dir.path).expect("the user dir") {
        let path = entry.expect("a user file").path();
        if path.extension().is_none_or(|extension| extension != "dbin") {
            continue;
        }
        journals += 1;
        let bytes = std::fs::read(&path).expect("the journal");
        // One record: type 4, null token, length 4, old total, new total.
        assert_eq!(bytes.len(), 8 + 18, "{}", path.display());
        let total = &bytes[18..22];
        let record = [&4u32.to_le_bytes()[..], &[0; 4], &[4, 0], total, total].concat();
        assert_eq!(
            bytes,
            oxpinyin_user::data::chunk_format::build_memory_chunk(&record).expect("frame"),
            "{}",
            path.display()
        );
    }
    assert!(journals > 0, "the save wrote the system journals");
    assert!(
        user_dir
            .path
            .join(oxpinyin_user::data::user_files::UserDbm::Bigram.file_name())
            .is_file()
    );
    let conf = oxpinyin_user::data::table_info::TableConf::load_from_dir(&system_dir())
        .expect("fixture table.conf");
    let marker =
        oxpinyin_user::data::user_files::UserTableInfo::load_path(&user_dir.path.join("user.conf"))
            .expect("saved user.conf");
    assert!(
        marker.is_conform(&oxpinyin_user::data::user_files::SystemVersions::from_conf(
            &conf
        ))
    );
    // The saved user bigram holds no record.
    let mut bigram_rows = 0;
    {
        use oxpinyin_store::RawReadStore as _;
        oxpinyin_store::DefaultStore::open_user_bigram(
            &user_dir
                .path
                .join(oxpinyin_user::data::user_files::UserDbm::Bigram.file_name()),
        )
        .expect("the saved user bigram")
        .copy_raw(|_, _| bigram_rows += 1)
        .expect("walk the user bigram");
    }
    assert_eq!(bigram_rows, 0, "no user bigram record");

    // No user dir: the corpus driver's shape — training degrades to
    // refusing, never to a write.
    let (context, instance) = open();
    let chosen = candidate(instance, "su3cl3", 0);
    assert!(unsafe { zhuyin_choose_candidate(instance, 0, chosen) } > 0);
    assert!(
        !unsafe { zhuyin_train(instance) },
        "NULL user directory refuses training"
    );
    close(context, instance);
}
