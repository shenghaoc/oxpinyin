//! The phrase-result surface (`src/phrase.rs`): `pinyin_phrase_segment`
//! and its two getters, plus the plain predicted-candidate variant and
//! the prefix-seeded sentence guess — driven black-box through the
//! re-exported symbols against the `fixtures/w3` mini tables.

use std::ffi::CStr;
use std::os::raw::c_char;

use pinyin_capi::{
    GArray, LookupCandidate, PinyinContext, PinyinInstance, lookup_candidate_type_t,
    pinyin_choose_candidate, pinyin_fini, pinyin_free_instance, pinyin_get_candidate,
    pinyin_get_candidate_type, pinyin_get_n_candidate, pinyin_get_n_phrase,
    pinyin_get_phrase_token, pinyin_get_sentence, pinyin_guess_candidates,
    pinyin_guess_predicted_candidates, pinyin_guess_predicted_candidates_with_punctuations,
    pinyin_guess_sentence_with_prefix, pinyin_load_phrase_library, pinyin_lookup_tokens,
    pinyin_parse_more_full_pinyins, pinyin_phrase_segment, pinyin_reset,
    pinyin_token_add_unigram_frequency, pinyin_token_get_n_pronunciation,
    pinyin_token_get_nth_pronunciation, pinyin_token_get_phrase,
    pinyin_token_get_unigram_frequency, pinyin_train, pinyin_unload_phrase_library,
};

use crate::common::{TempUserDir, cstr, open};

/// Reads a caller-owned string (`g_free`-releasable) and frees it.
fn take(rendered: *mut pinyin_capi::GChar) -> Option<String> {
    if rendered.is_null() {
        return None;
    }
    // SAFETY: The getters return a NUL-terminated UTF-8 buffer or null.
    let text = Some(
        unsafe { CStr::from_ptr(rendered) }
            .to_str()
            .expect("UTF-8 render")
            .to_owned(),
    );
    // SAFETY: The buffer came from the capi's libc-malloc `owned_cstr`.
    unsafe {
        libc_free(rendered.cast());
    }
    text
}

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

// The `GArray`-taking library entry points call glib's own
// `g_array_append_vals`, which dereferences the array's private
// `_GRealArray` fields — an inline `GArray { data, len }` view has
// none of those and would crash on the first append. So the tests
// hold a real glib array from `g_array_new`, freed with
// `g_array_free`. Linked through `libpinyin_capi.so`'s `libglib-2.0`
// NEEDED entry.
unsafe extern "C" {
    fn g_array_new(
        zero_terminated: core::ffi::c_int,
        clear: core::ffi::c_int,
        element_size: core::ffi::c_uint,
    ) -> *mut GArray;
    fn g_array_free(
        array: *mut GArray,
        free_segment: core::ffi::c_int,
    ) -> *mut core::ffi::c_char;
}

// ── Tier C: dictionary introspection ─────────────────────────────────

/// A real glib `GArray` of `u32` tokens: the library appends into it
/// through `g_array_append_vals` and truncates it through
/// `g_array_set_size`; the test reads the elements back out through the
/// array's documented public fields.
struct TokenArray {
    array: *mut GArray,
}

impl TokenArray {
    fn new() -> Self {
        // SAFETY: glib always returns a valid array pointer (or aborts
        // on OOM, matching upstream libpinyin's own behaviour).
        let array = unsafe {
            g_array_new(0, 0, u32::try_from(size_of::<u32>()).expect("u32 fits guint"))
        };
        assert!(!array.is_null(), "g_array_new returned null");
        Self { array }
    }

    fn array_ptr(&mut self) -> *mut GArray {
        self.array
    }

    fn elements(&self) -> Vec<u32> {
        // SAFETY: the array pointer stays live for the fixture's
        // lifetime; `data`/`len` are the glib GArray's documented
        // public fields.
        let view = unsafe { &*self.array };
        if view.data.is_null() || view.len == 0 {
            return Vec::new();
        }
        // SAFETY: data points at len consecutive u32 elements written
        // by the library through glib.
        unsafe {
            std::slice::from_raw_parts(view.data.cast::<u32>(), view.len as usize).to_vec()
        }
    }
}

impl Drop for TokenArray {
    fn drop(&mut self) {
        if !self.array.is_null() {
            // SAFETY: `array` came from `g_array_new`; `free_segment=1`
            // frees the underlying buffer glib grew.
            unsafe {
                g_array_free(self.array, 1);
            }
        }
    }
}

/// `pinyin_lookup_tokens`: 你好 resolves to exactly one token; an
/// unknown phrase resolves to none; the array is cleared per call.
#[test]
fn lookup_tokens_resolves_stored_phrases() {
    let fixture = Fixture::new("dict-lookup");

    let mut array = TokenArray::new();
    assert!(pinyin_lookup_tokens(
        fixture.instance,
        cstr("你好").as_ptr(),
        array.array_ptr()
    ));
    let tokens = array.elements();
    assert_eq!(tokens.len(), 1, "你好 resolves to one token");

    // Reuse the same array: it clears per call. ASCII matches no
    // phrase — and the retval is `SEARCH_OK & retval`, false for a
    // no-hit span (the array still cleared).
    assert!(!pinyin_lookup_tokens(
        fixture.instance,
        cstr("abyss").as_ptr(),
        array.array_ptr()
    ));
    assert!(array.elements().is_empty());

    // Null tokenarray: refused (the pin dereferences it unguarded).
    assert!(!pinyin_lookup_tokens(
        fixture.instance,
        cstr("你好").as_ptr(),
        std::ptr::null_mut()
    ));
}

/// `pinyin_token_get_phrase` round-trips the lookup: the token found by
/// text renders back to the same text; an unknown token answers false
/// with a NULL out-param.
#[test]
fn token_get_phrase_round_trips() {
    let fixture = Fixture::new("dict-phrase");

    let mut array = TokenArray::new();
    assert!(pinyin_lookup_tokens(
        fixture.instance,
        cstr("你好").as_ptr(),
        array.array_ptr()
    ));
    let token = array.elements()[0];

    let mut len: u32 = 0;
    let mut rendered: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(pinyin_token_get_phrase(
        fixture.instance,
        token,
        &mut len,
        &mut rendered
    ));
    assert_eq!(len as usize, 2, "len is the character count");
    assert_eq!(
        take(rendered).as_deref(),
        Some("你好"),
        "the text round-trips"
    );
    assert!(pinyin_token_get_phrase(
        fixture.instance,
        token,
        std::ptr::null_mut(),
        &mut rendered
    ));

    let unknown = 0x09FF_FFFF;
    assert!(!pinyin_token_get_phrase(
        fixture.instance,
        unknown,
        &mut len,
        &mut rendered
    ));
    assert!(rendered.is_null());
}

/// `pinyin_token_get_n_pronunciation` and `_get_nth_pronunciation`:
/// 你好 carries at least one pronunciation whose keys spell the phrase;
/// an out-of-range nth answers false (the pin appends garbage there —
/// the no-abort policy refuses instead).
#[test]
fn token_pronunciation_surface() {
    let fixture = Fixture::new("dict-pron");

    let mut array = TokenArray::new();
    assert!(pinyin_lookup_tokens(
        fixture.instance,
        cstr("你好").as_ptr(),
        array.array_ptr()
    ));
    let token = array.elements()[0];

    let mut num: u32 = 0;
    assert!(pinyin_token_get_n_pronunciation(
        fixture.instance,
        token,
        &mut num
    ));
    assert!(num >= 1);

    // A real glib GArray of `u16` chewing-key words. The library
    // appends into it via `g_array_append_vals`, which needs the
    // private `_GRealArray` metadata glib itself sets up.
    // SAFETY: g_array_new either returns a valid array or aborts (glib
    // policy, inherited by upstream libpinyin).
    let keys = unsafe {
        g_array_new(0, 0, u32::try_from(size_of::<u16>()).expect("u16 fits guint"))
    };
    assert!(!keys.is_null(), "g_array_new returned null");
    assert!(pinyin_token_get_nth_pronunciation(
        fixture.instance,
        token,
        0,
        keys,
    ));
    // SAFETY: keys is live; `len` is glib's public field.
    let keys_len = unsafe { (*keys).len };
    assert!(keys_len >= 2, "你+好: two chewing keys");

    // An out-of-range nth answers false. The array is not appended to.
    assert!(!pinyin_token_get_nth_pronunciation(
        fixture.instance,
        token,
        9,
        keys,
    ));

    // SAFETY: keys came from g_array_new; frees the buffer glib grew.
    unsafe {
        g_array_free(keys, 1);
    }
}

/// `pinyin_token_get_unigram_frequency` / `_add_unigram_frequency`:
/// read → add → shifted read; an absent-token add answers false.
#[test]
fn token_unigram_read_and_overlay_write() {
    let fixture = Fixture::new("dict-unigram");

    let mut array = TokenArray::new();
    assert!(pinyin_lookup_tokens(
        fixture.instance,
        cstr("你好").as_ptr(),
        array.array_ptr()
    ));
    let token = array.elements()[0];

    let mut freq: u32 = 0;
    assert!(pinyin_token_get_unigram_frequency(
        fixture.instance,
        token,
        &mut freq
    ));
    let before = freq;

    assert!(pinyin_token_add_unigram_frequency(
        fixture.instance,
        token,
        7
    ));
    assert!(pinyin_token_get_unigram_frequency(
        fixture.instance,
        token,
        &mut freq
    ));
    assert_eq!(freq, before + 7, "the overlay delta lands");

    // Absent token: false, and the read stays absent.
    let absent = 0x09FF_FFFF;
    assert!(!pinyin_token_get_unigram_frequency(
        fixture.instance,
        absent,
        &mut freq
    ));
    assert!(!pinyin_token_add_unigram_frequency(
        fixture.instance,
        absent,
        3
    ));
}

/// The load/unload contract: fresh GBK is already loaded (`load` false),
/// the first unload answers true, the second false (the sub-index is
/// gone), and the reload answers true. Non-GBK indexes are refused.
#[test]
fn phrase_library_load_unload_contract() {
    let fixture = Fixture::new("dict-lib");

    assert!(
        !pinyin_load_phrase_library(fixture.context, 2),
        "already loaded"
    );
    assert!(pinyin_unload_phrase_library(fixture.context, 2));
    assert!(
        !pinyin_unload_phrase_library(fixture.context, 2),
        "already gone"
    );
    assert!(pinyin_load_phrase_library(fixture.context, 2), "reload");
    assert!(
        !pinyin_load_phrase_library(fixture.context, 2),
        "loaded again"
    );

    // Content survives the cycle: 你好 still resolves.
    let mut array = TokenArray::new();
    assert!(pinyin_lookup_tokens(
        fixture.instance,
        cstr("你好").as_ptr(),
        array.array_ptr()
    ));
    assert_eq!(array.elements().len(), 1);
}
