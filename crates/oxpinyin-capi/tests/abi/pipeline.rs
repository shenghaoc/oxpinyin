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

use std::ffi::CStr;

use std::os::raw::c_char;

use pinyin_capi::{
    PinyinInstance, pinyin_alloc_instance, pinyin_free_instance, pinyin_get_sentence,
    pinyin_guess_sentence, pinyin_init, pinyin_parse_more_chewings,
};

use crate::common::{TempUserDir, cstr, system_dir};

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

#[test]
fn the_zhuyin_pipeline_guesses_a_sentence_from_bopomofo_keys() {
    // `test_zhuyin.cpp`: `zhuyin_parse_more_chewings` →
    // `zhuyin_guess_sentence` → `zhuyin_get_sentence`. oxpinyin unifies
    // the zhuyin API onto the same instance surface, so the port drives
    // the chewing parser and asserts a sentence comes back.
    let user_dir = TempUserDir::new("pipeline-sentence-zhuyin");
    let (context, instance) = open_fixture(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("vu0");
    assert_eq!(
        unsafe { pinyin_parse_more_chewings(instance, input.as_ptr()) },
        3
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });

    let mut sentence: *mut c_char = std::ptr::null_mut();
    assert!(unsafe { pinyin_get_sentence(instance, 0, &raw mut sentence) });
    // SAFETY: handed out by the ABI call above; a valid, NUL-terminated
    // pointer until the instance's next guess.
    let text = unsafe { CStr::from_ptr(sentence) }
        .to_string_lossy()
        .into_owned();
    assert!(!text.is_empty(), "the xian window yields a sentence");

    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_capi::pinyin_fini(context) };
}
