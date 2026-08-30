//! The phrase-result surface (`src/phrase.rs`): `pinyin_phrase_segment`
//! and its two getters, plus the plain predicted-candidate variant and
//! the prefix-seeded sentence guess — driven black-box through the
//! re-exported symbols against the `fixtures/w3` mini tables.

use std::ffi::CStr;
use std::os::raw::c_char;

use pinyin_capi::{
    LookupCandidate, PinyinContext, PinyinInstance, lookup_candidate_type_t,
    pinyin_choose_candidate, pinyin_fini, pinyin_free_instance, pinyin_get_candidate,
    pinyin_get_candidate_type, pinyin_get_n_candidate, pinyin_get_n_phrase,
    pinyin_get_phrase_token, pinyin_get_sentence, pinyin_guess_candidates,
    pinyin_guess_predicted_candidates, pinyin_guess_predicted_candidates_with_punctuations,
    pinyin_guess_sentence_with_prefix, pinyin_parse_more_full_pinyins, pinyin_phrase_segment,
    pinyin_reset, pinyin_train,
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
        pinyin_free_instance(self.instance);
        pinyin_fini(self.context);
    }
}

fn segment(fixture: &Fixture, sentence: &str) -> bool {
    pinyin_phrase_segment(fixture.instance, cstr(sentence).as_ptr())
}

fn n_phrase(fixture: &Fixture) -> u32 {
    let mut num: u32 = 0;
    assert!(pinyin_get_n_phrase(fixture.instance, &mut num));
    num
}

fn phrase_token(fixture: &Fixture, index: u32) -> Option<u32> {
    let mut token: pinyin_capi::PhraseTokenT = 0;
    if pinyin_get_phrase_token(fixture.instance, index, &mut token) {
        Some(token)
    } else {
        None
    }
}

/// `你好` and `中国` are adjacent stored phrases: the segmenter writes
/// each token at its span's start position with nulls between, and the
/// getters report the span shape.
#[test]
fn segment_two_adjacent_phrases() {
    let fixture = Fixture::new("phrase-two");

    // Derive the two token ids from single-phrase segments.
    assert!(segment(&fixture, "你好"));
    let nihao = phrase_token(&fixture, 0).expect("你好 token");
    assert!(segment(&fixture, "中国"));
    let zhongguo = phrase_token(&fixture, 0).expect("中国 token");

    assert!(segment(&fixture, "你好中国"));
    assert_eq!(n_phrase(&fixture), 4, "character-length result");
    let first = phrase_token(&fixture, 0).expect("a path covers char 0");
    assert_ne!(first, 0, "a real token, not the null filler");
    // 中国 is the only stored phrase over its span, so whichever split
    // the DP chose for 你好, its token sits at position 2. (Which split
    // wins is the pinned oracle differential's business, not a unit
    // test's.)
    assert_eq!(phrase_token(&fixture, 2), Some(zhongguo));
    let _ = nihao;

    // Out-of-range: false with the out-param zeroed.
    let mut token: pinyin_capi::PhraseTokenT = 0xBEEF;
    assert!(!pinyin_get_phrase_token(fixture.instance, 4, &mut token));
    assert_eq!(token, 0, "zeroed before the bounds check");
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
    assert!(!pinyin_phrase_segment(
        fixture.instance,
        invalid.as_ptr().cast::<c_char>()
    ));
}

/// A fresh instance reports zero phrases; `pinyin_reset` clears the
/// result after a successful segment.
#[test]
fn reset_clears_the_phrase_result() {
    let fixture = Fixture::new("phrase-reset");
    assert_eq!(n_phrase(&fixture), 0, "fresh instance");

    assert!(segment(&fixture, "你好"));
    assert_eq!(n_phrase(&fixture), 2);

    assert!(pinyin_reset(fixture.instance));
    assert_eq!(n_phrase(&fixture), 0, "reset clears the phrase result");
    assert_eq!(phrase_token(&fixture, 0), None, "false past the end");
}

/// The plain predicted-candidate variant, planted the union suite's way
/// (choose + train seeds 69, above the copied bigram filter of 10):
/// non-empty rows and never a punctuation row — and the retval
/// contrasts with `_with_punctuations` on a suffix-less prefix, whose
/// `false` the punctuated entry discards.
#[test]
fn plain_predicted_candidates_match_the_with_punctuations_body() {
    let fixture = Fixture::new("phrase-plain");
    let nihao = cstr("nihao");
    assert_eq!(
        pinyin_parse_more_full_pinyins(fixture.instance, nihao.as_ptr()),
        5
    );
    // The union flow: guess before choose — sort option 0x1e, the
    // pipeline tests' option.
    assert!(pinyin_guess_candidates(fixture.instance, 0, 0x1e));
    let mut candidate: *mut LookupCandidate = std::ptr::null_mut();
    assert!(pinyin_get_candidate(fixture.instance, 0, &mut candidate));
    assert!(pinyin_choose_candidate(fixture.instance, 0, candidate) > 0);
    assert!(pinyin_train(fixture.instance, 0));

    let mut num: u32 = 0;
    assert!(pinyin_guess_predicted_candidates(
        fixture.instance,
        cstr("你").as_ptr()
    ));
    assert!(pinyin_get_n_candidate(fixture.instance, &mut num));
    assert!(num > 0, "the planted bigram predicts rows");

    for index in 0..num {
        let mut row: *mut LookupCandidate = std::ptr::null_mut();
        assert!(pinyin_get_candidate(fixture.instance, index, &mut row));
        let mut kind = lookup_candidate_type_t::NBEST_MATCH_CANDIDATE;
        assert!(pinyin_get_candidate_type(fixture.instance, row, &mut kind));
        assert_ne!(
            kind,
            lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE,
            "the plain variant never prepends punctuation"
        );
    }

    // A prefix with no dictionary suffix: the plain variant answers
    // false; the punctuated entry discards that retval and answers true.
    assert!(!pinyin_guess_predicted_candidates(
        fixture.instance,
        cstr("abyss").as_ptr()
    ));
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        fixture.instance,
        cstr("abyss").as_ptr()
    ));
}

/// The prefix-seeded sentence guess: parses a composition first, then
/// the prefix tokens seed the decode; a prefix matching a stored phrase
/// yields that phrase in the sentence.
#[test]
fn prefix_seeded_sentence_guess() {
    let fixture = Fixture::new("phrase-prefix");
    let nihao = cstr("nihao");
    assert_eq!(
        pinyin_parse_more_full_pinyins(fixture.instance, nihao.as_ptr()),
        5
    );

    assert!(pinyin_guess_sentence_with_prefix(
        fixture.instance,
        cstr("你好").as_ptr()
    ));
    let mut sentence: *mut c_char = std::ptr::null_mut();
    assert!(pinyin_get_sentence(fixture.instance, 0, &mut sentence));
    let text = take_sentence(sentence);
    assert!(
        text.contains("你好"),
        "prefix-seeded sentence carries the prefix phrase: {text:?}"
    );

    // No parse yet: the fresh instance has an empty matrix, the decode
    // answers false.
    let fresh = Fixture::new("phrase-prefix-fresh");
    assert!(!pinyin_guess_sentence_with_prefix(
        fresh.instance,
        cstr("你好").as_ptr()
    ));
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
    // SAFETY: The buffer came from the capi's libc-malloc `owned_cstr`.
    unsafe {
        libc_free(sentence.cast());
    }
    text
}

unsafe extern "C" {
    #[link_name = "free"]
    fn libc_free(ptr: *mut core::ffi::c_void);
}
