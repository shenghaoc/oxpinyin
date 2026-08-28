//! Behavioural port of the top-level libpinyin interactive drivers.
//!
//! `libpinyin/tests/test_pinyin.cpp`, `test_chewing.cpp`, and
//! `test_zhuyin.cpp` are stdin drivers with no assertions: their value is
//! the pipeline they exercise — parse → guess → candidates/sentence →
//! auxiliary text at *every* offset → train → reset → save, ending in the
//! mask-out wipe. This module ports that pipeline over the same C ABI
//! symbols against the `fixtures/w3` mini tables, replacing the drivers'
//! printouts with assertions.
//!
//! The drivers' auxiliary text is read back and leaked here: the returned
//! string is a glib allocation whose free lives with the caller's event
//! loop, and the test process exits immediately after.
//!
//! `test_phrase.cpp`'s `pinyin_phrase_segment` path has no oxpinyin
//! counterpart (raw-Chinese-text segmentation is not an oxpinyin surface;
//! the equivalent coverage is the oracle phrase differentials) — see the
//! coverage ledger.

use std::ffi::CStr;
use std::os::raw::{c_char, c_uint};

use pinyin_capi::{
    LookupCandidate, PinyinInstance, oxpinyin_init_for_fixtures, pinyin_alloc_instance,
    pinyin_choose_candidate, pinyin_free_instance, pinyin_get_candidate,
    pinyin_get_candidate_string, pinyin_get_n_candidate, pinyin_get_sentence,
    pinyin_guess_candidates, pinyin_guess_sentence, pinyin_mask_out, pinyin_parse_more_chewings,
    pinyin_parse_more_full_pinyins, pinyin_reset, pinyin_save, pinyin_train,
};

use crate::common::{TempUserDir, cstr, system_dir};

fn open_fixture(user_dir: &str) -> (*mut pinyin_capi::PinyinContext, *mut PinyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir);
    // SAFETY: the returned handles are live until `pinyin_free_instance`/
    // `pinyin_fini` below; all calls are single-threaded within the test.
    let context = oxpinyin_init_for_fixtures(system.as_ptr(), user.as_ptr());
    assert!(!context.is_null(), "fixture init opens the mini tables");
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    (context, instance)
}

fn candidate_texts(instance: *mut PinyinInstance) -> Vec<String> {
    assert!(pinyin_guess_candidates(instance, 0, 0x1e));
    let mut count: c_uint = 0;
    assert!(pinyin_get_n_candidate(instance, &mut count));
    let mut out = Vec::new();
    for index in 0..count {
        let mut cand: *mut LookupCandidate = std::ptr::null_mut();
        assert!(pinyin_get_candidate(instance, index, &mut cand));
        let mut text: *const c_char = std::ptr::null();
        assert!(pinyin_get_candidate_string(instance, cand, &mut text));
        // Sentence rows can carry no display string; the upstream driver
        // prints `(null)` for those, here they are skipped.
        if text.is_null() {
            continue;
        }
        out.push(
            // SAFETY: the ABI hands back a NUL-terminated string valid
            // until the next guess on this instance.
            unsafe { CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned(),
        );
    }
    out
}

/// The `test_pinyin.cpp` loop: parse, then read the auxiliary text at
/// every offset `0..=len` — the driver walks them all, and every one must
/// answer.
#[test]
fn auxiliary_text_answers_at_every_offset_of_the_parsed_input() {
    let user_dir = TempUserDir::new("pipeline-aux-full");
    let (context, instance) = open_fixture(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("nihao");
    let len = pinyin_parse_more_full_pinyins(instance, input.as_ptr());
    assert_eq!(len, 5);

    for cursor in 0..=len {
        let mut aux: *mut c_char = std::ptr::null_mut();
        assert!(
            pinyin_capi::pinyin_get_full_pinyin_auxiliary_text(instance, cursor, &mut aux),
            "aux text must answer at offset {cursor}"
        );
        assert!(!aux.is_null(), "aux text at {cursor} is a real string");
    }

    pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

#[test]
fn the_chewing_pipeline_produces_auxiliary_text_too() {
    // `test_chewing.cpp`: the same walk over the zhuyin (chewing) parse
    // path and its own auxiliary text accessor.
    let user_dir = TempUserDir::new("pipeline-aux-chewing");
    let (context, instance) = open_fixture(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("vu0");
    let len = pinyin_parse_more_chewings(instance, input.as_ptr());
    assert_eq!(len, 3, "the dachen spelling vu0 parses as one key");

    for cursor in 0..=len {
        let mut aux: *mut c_char = std::ptr::null_mut();
        assert!(
            pinyin_capi::pinyin_get_chewing_auxiliary_text(instance, cursor, &mut aux),
            "chewing aux text must answer at offset {cursor}"
        );
        assert!(!aux.is_null());
    }

    pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

#[test]
fn the_pinyin_pipeline_guesses_the_sentence_and_candidates() {
    // `test_pinyin.cpp`: guess_sentence_with_prefix + guess_candidates
    // reproduce the pin's sentence and candidate list. The mini tables
    // pin `nihao` -> 你好 exactly (same expectation as the runtime
    // assembly tests).
    let user_dir = TempUserDir::new("pipeline-sentence-full");
    let (context, instance) = open_fixture(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("nihao");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);

    // Candidates come from `pinyin_guess_candidates` over the live
    // composition; reading them first, then guessing the sentence, is the
    // order in which both surfaces answer.
    let candidates = candidate_texts(instance);
    assert!(
        candidates.iter().any(|c| c == "你好"),
        "ni'hao must surface 你好 among the candidates: {candidates:?}"
    );

    // The sentence itself comes from the guess step.
    assert!(pinyin_guess_sentence(instance));
    let mut sentence: *mut c_char = std::ptr::null_mut();
    assert!(pinyin_get_sentence(instance, 0, &mut sentence));
    // SAFETY: NUL-terminated string valid until freed.
    let text = unsafe { CStr::from_ptr(sentence) }
        .to_string_lossy()
        .into_owned();
    assert_eq!(text, "你好");

    pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

#[test]
fn the_zhuyin_pipeline_guesses_a_sentence_from_bopomofo_keys() {
    // `test_zhuyin.cpp`: `zhuyin_parse_more_chewings` →
    // `zhuyin_guess_sentence` → `zhuyin_get_sentence`. oxpinyin unifies
    // the zhuyin API onto the same instance surface, so the port drives
    // the chewing parser and asserts a sentence comes back.
    let user_dir = TempUserDir::new("pipeline-sentence-zhuyin");
    let (context, instance) = open_fixture(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("vu0");
    assert_eq!(pinyin_parse_more_chewings(instance, input.as_ptr()), 3);
    assert!(pinyin_guess_sentence(instance));

    let mut sentence: *mut c_char = std::ptr::null_mut();
    assert!(pinyin_get_sentence(instance, 0, &mut sentence));
    let text = unsafe { CStr::from_ptr(sentence) }
        .to_string_lossy()
        .into_owned();
    assert!(!text.is_empty(), "the xian window yields a sentence");

    pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

#[test]
fn the_train_reset_save_cycle_runs_and_masks_out_clean() {
    // Every driver ends its loop iteration with train → reset → save and
    // closes with mask_out(0,0) → save. The cycle must run clean and
    // leave the instance usable.
    let user_dir = TempUserDir::new("pipeline-train-cycle");
    let (context, instance) = open_fixture(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("nihao");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);
    assert!(pinyin_guess_sentence(instance));
    // The oxpinyin contract trains only an accepted selection (the ABI
    // e2e pins `train` refusing a selection-less instance), so choose a
    // candidate first — what a real session does before train.
    let mut cand: *mut LookupCandidate = std::ptr::null_mut();
    assert!(pinyin_guess_candidates(instance, 0, 0x1e));
    assert!(pinyin_get_candidate(instance, 0, &mut cand));
    assert!(pinyin_choose_candidate(instance, 0, cand) >= 0);
    assert!(pinyin_train(instance, 0));
    assert!(pinyin_reset(instance));
    // `pinyin_save` gates on the modified flag (the e2e suite pins the
    // gate): with training done the store is dirty, so this saves.
    assert!(pinyin_save(context));
    assert!(pinyin_mask_out(context, 0, 0));
    // mask_out over an emptied store leaves nothing dirty, so the closing
    // save may legally report no-op — run it as the drivers do.
    let _ = pinyin_save(context);

    pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}
