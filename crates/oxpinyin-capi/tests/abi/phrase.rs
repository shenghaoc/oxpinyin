//! The phrase-result surface (`src/phrase.rs`): `pinyin_phrase_segment`
//! and its two getters, plus the plain predicted-candidate variant and
//! the prefix-seeded sentence guess — driven black-box through the
//! re-exported symbols against the `fixtures/w3` mini tables.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use std::ffi::CStr;

use std::os::raw::c_char;

use pinyin_capi::{
    PinyinContext, PinyinInstance, pinyin_fini, pinyin_free_instance, pinyin_get_n_phrase,
    pinyin_get_phrase_token, pinyin_get_sentence, pinyin_guess_sentence_with_prefix,
    pinyin_parse_more_full_pinyins, pinyin_phrase_segment, pinyin_reset,
};

use crate::common::{TempUserDir, cstr, open};

struct Fixture {
    context: *mut PinyinContext,
    instance: *mut PinyinInstance,
    _user_dir: TempUserDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let user_dir = TempUserDir::new(tag);
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        Self {
            context,
            instance,
            _user_dir: user_dir,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { pinyin_free_instance(self.instance) };
        unsafe { pinyin_fini(self.context) };
    }
}

fn segment(fixture: &Fixture, sentence: &str) -> bool {
    unsafe { pinyin_phrase_segment(fixture.instance, cstr(sentence).as_ptr()) }
}

fn n_phrase(fixture: &Fixture) -> u32 {
    let mut num: u32 = 0;
    assert!(unsafe { pinyin_get_n_phrase(fixture.instance, &raw mut num) });
    num
}

fn phrase_token(fixture: &Fixture, index: u32) -> Option<u32> {
    let mut token: pinyin_capi::PhraseTokenT = 0;
    if unsafe { pinyin_get_phrase_token(fixture.instance, index, &raw mut token) } {
        Some(token)
    } else {
        None
    }
}

#[test]
fn empty_segment_succeeds_with_an_empty_result() {
    // The pin's final step matches the start node for zero input
    // (074a2219 phrase_lookup.cpp:36-53,382-428).
    let fixture = Fixture::new("phrase-empty");
    assert!(segment(&fixture, ""));
    assert_eq!(n_phrase(&fixture), 0);
    assert_eq!(phrase_token(&fixture, 0), None);
}

/// The failed-match shape: an ASCII sentence matches no phrase, the
/// retval is `false`, and the result stays character-length and
/// all-null — `get_n_phrase` then reports the character count.
#[test]
fn failed_match_keeps_the_sized_all_null_array() {
    let fixture = Fixture::new("phrase-fail");
    assert!(!segment(&fixture, "abcd"));
    assert_eq!(n_phrase(&fixture), 4);
    assert_eq!(phrase_token(&fixture, 0), Some(0));
    assert_eq!(phrase_token(&fixture, 3), Some(0));

    // Invalid UTF-8: upstream's g_return_val_if_fail gate answers false.
    let invalid = [0xFF, 0xFE, 0x00];
    assert!(!unsafe { pinyin_phrase_segment(fixture.instance, invalid.as_ptr().cast::<c_char>()) });
}

/// A fresh instance reports zero phrases; `pinyin_reset` clears the
/// result after a successful segment.
#[test]
fn reset_clears_the_phrase_result() {
    let fixture = Fixture::new("phrase-reset");
    assert_eq!(n_phrase(&fixture), 0, "fresh instance");

    assert!(segment(&fixture, "你好"));
    assert_eq!(n_phrase(&fixture), 2);

    assert!(unsafe { pinyin_reset(fixture.instance) });
    assert_eq!(n_phrase(&fixture), 0, "reset clears the phrase result");
    assert_eq!(phrase_token(&fixture, 0), None, "false past the end");
}

/// The prefix-seeded sentence guess: parses a composition first, then
/// the prefix tokens seed the decode; a prefix matching a stored phrase
/// yields that phrase in the sentence.
#[test]
fn prefix_seeded_sentence_guess() {
    let fixture = Fixture::new("phrase-prefix");
    let nihao = cstr("nihao");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(fixture.instance, nihao.as_ptr()) },
        5
    );

    assert!(unsafe {
        pinyin_guess_sentence_with_prefix(fixture.instance, cstr("你好").as_ptr())
    });
    let mut sentence: *mut c_char = std::ptr::null_mut();
    assert!(unsafe { pinyin_get_sentence(fixture.instance, 0, &raw mut sentence) });
    let text = take_sentence(sentence);
    assert!(
        text.contains("你好"),
        "prefix-seeded sentence carries the prefix phrase: {text:?}"
    );

    // No parse yet: the fresh instance has an empty matrix, the decode
    // answers false.
    let fresh = Fixture::new("phrase-prefix-fresh");
    assert!(!unsafe { pinyin_guess_sentence_with_prefix(fresh.instance, cstr("你好").as_ptr()) });
}

fn take_sentence(sentence: *mut c_char) -> String {
    if sentence.is_null() {
        return String::new();
    }
    // SAFETY: `pinyin_get_sentence` returns a NUL-terminated UTF-8
    // buffer or null.
    let text = unsafe { CStr::from_ptr(sentence) }
        .to_str()
        .expect("UTF-8 sentence")
        .to_owned();
    // SAFETY: The buffer is a returned string this test owns; GLib
    // allocates with malloc, which libc `free` releases.
    unsafe {
        libc_free(sentence.cast());
    }
    text
}

unsafe extern "C" {
    #[link_name = "free"]
    fn libc_free(ptr: *mut core::ffi::c_void);
}
