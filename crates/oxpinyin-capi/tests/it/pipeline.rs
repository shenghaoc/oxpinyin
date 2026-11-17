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

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{TempUserDir, cstr, system_dir};

use pinyin_capi::{
    LookupCandidate, PinyinInstance, pinyin_alloc_instance, pinyin_choose_candidate,
    pinyin_free_instance, pinyin_get_candidate, pinyin_guess_candidates, pinyin_guess_sentence,
    pinyin_init, pinyin_mask_out, pinyin_parse_more_chewings, pinyin_parse_more_full_pinyins,
    pinyin_reset, pinyin_save, pinyin_train,
};

use std::os::raw::c_char;

fn open_fixture(user_dir: &str) -> (*mut pinyin_capi::PinyinContext, *mut PinyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir);
    // SAFETY: the returned handles are live until `pinyin_free_instance`/
    // `pinyin_fini` below; all calls are single-threaded within the test.
    let context = unsafe { pinyin_init(system.as_ptr(), user.as_ptr()) };
    assert!(!context.is_null(), "fixture init opens the mini tables");
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    (context, instance)
}

/// The `test_pinyin.cpp` loop: parse, then read the auxiliary text at
/// every offset `0..=len` — the driver walks them all, and every one must
/// answer.
#[test]
fn auxiliary_text_answers_at_every_offset_of_the_parsed_input() {
    let user_dir = TempUserDir::new("pipeline-aux-full");
    let (context, instance) = open_fixture(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("nihao");
    let len = unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) };
    assert_eq!(len, 5);

    for cursor in 0..=len {
        let mut aux: *mut c_char = std::ptr::null_mut();
        assert!(
            unsafe {
                pinyin_capi::pinyin_get_full_pinyin_auxiliary_text(instance, cursor, &raw mut aux)
            },
            "aux text must answer at offset {cursor}"
        );
        assert!(!aux.is_null(), "aux text at {cursor} is a real string");
    }

    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_capi::pinyin_fini(context) };
}

#[test]
fn the_chewing_pipeline_produces_auxiliary_text_too() {
    // `test_chewing.cpp`: the same walk over the zhuyin (chewing) parse
    // path and its own auxiliary text accessor.
    let user_dir = TempUserDir::new("pipeline-aux-chewing");
    let (context, instance) = open_fixture(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("vu0");
    let len = unsafe { pinyin_parse_more_chewings(instance, input.as_ptr()) };
    assert_eq!(len, 3, "the dachen spelling vu0 parses as one key");

    for cursor in 0..=len {
        let mut aux: *mut c_char = std::ptr::null_mut();
        assert!(
            unsafe {
                pinyin_capi::pinyin_get_chewing_auxiliary_text(instance, cursor, &raw mut aux)
            },
            "chewing aux text must answer at offset {cursor}"
        );
        assert!(!aux.is_null());
    }

    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_capi::pinyin_fini(context) };
}

#[test]
fn the_train_reset_save_cycle_runs_and_masks_out_clean() {
    // Every driver ends its loop iteration with train → reset → save and
    // closes with mask_out(0,0) → save. The cycle must run clean and
    // leave the instance usable.
    let user_dir = TempUserDir::new("pipeline-train-cycle");
    let (context, instance) = open_fixture(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("nihao");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        5
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    // The oxpinyin contract trains only an accepted selection (the ABI
    // e2e pins `train` refusing a selection-less instance), so choose a
    // candidate first — what a real session does before train.
    let mut cand: *mut LookupCandidate = std::ptr::null_mut();
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0x1e) });
    assert!(unsafe { pinyin_get_candidate(instance, 0, &raw mut cand) });
    assert!(unsafe { pinyin_choose_candidate(instance, 0, cand) } >= 0);
    assert!(unsafe { pinyin_train(instance, 0) });
    assert!(unsafe { pinyin_reset(instance) });
    // `pinyin_save` gates on the modified flag (the e2e suite pins the
    // gate): with training done the store is dirty, so this saves.
    assert!(unsafe { pinyin_save(context) });
    assert!(unsafe { pinyin_mask_out(context, 0, 0) });
    // mask_out over an emptied store leaves nothing dirty, so the closing
    // save may legally report no-op — run it as the drivers do.
    let _ = unsafe { pinyin_save(context) };

    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_capi::pinyin_fini(context) };
}
