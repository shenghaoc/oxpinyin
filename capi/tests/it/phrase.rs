//! The phrase-result surface (`src/phrase.rs`): `pinyin_phrase_segment`
//! and its two getters, plus the plain predicted-candidate variant and
//! the prefix-seeded sentence guess — driven black-box through the
//! C functions against the `fixtures/w3` mini tables.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{LOGS, capture_segment_log};

use crate::support::take;

use crate::support::{PinyinInstance, TempUserDir, cstr, open};

use oxpinyin_capi::candidates::{
    pinyin_choose_candidate, pinyin_get_candidate, pinyin_get_candidate_type,
    pinyin_get_n_candidate, pinyin_train,
};
use oxpinyin_capi::config::{pinyin_load_phrase_library, pinyin_unload_phrase_library};
use oxpinyin_capi::context::pinyin_fini;
use oxpinyin_capi::dict::{
    pinyin_lookup_tokens, pinyin_token_add_unigram_frequency, pinyin_token_get_n_pronunciation,
    pinyin_token_get_nth_pronunciation, pinyin_token_get_phrase,
    pinyin_token_get_unigram_frequency,
};
use oxpinyin_capi::instance::pinyin_free_instance;
use oxpinyin_capi::parse::pinyin_parse_more_full_pinyins;
use oxpinyin_capi::phrase::{pinyin_get_n_phrase, pinyin_get_phrase_token, pinyin_phrase_segment};
use oxpinyin_capi::predict::pinyin_guess_predicted_candidates;
use oxpinyin_capi::sentence::{
    pinyin_guess_candidates, pinyin_guess_predicted_candidates_with_punctuations,
};
use oxpinyin_capi::types::{GArray, LookupCandidate, PinyinContext, lookup_candidate_type_t};

use std::ffi::CString;

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
    let mut token: oxpinyin_capi::types::PhraseTokenT = 0;
    if unsafe { pinyin_get_phrase_token(fixture.instance, index, &raw mut token) } {
        Some(token)
    } else {
        None
    }
}

// The `GArray`-taking library entry points call glib's own
// `g_array_append_vals`, which dereferences the array's private
// `_GRealArray` fields — an inline `GArray { data, len }` view has
// none of those and would crash on the first append. So the tests
// hold a real glib array from `g_array_new`, freed with
// `g_array_free`. Linked through `liboxpinyin_capi.so`'s `libglib-2.0`
// NEEDED entry.
use glib_sys::{g_array_free, g_array_new};

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
            g_array_new(
                0,
                0,
                u32::try_from(size_of::<u32>()).expect("u32 fits guint"),
            )
        };
        assert!(!array.is_null(), "g_array_new returned null");
        Self { array }
    }

    const fn array_ptr(&mut self) -> *mut GArray {
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
        unsafe { std::slice::from_raw_parts(view.data.cast::<u32>(), view.len as usize).to_vec() }
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
    assert_eq!(phrase_token(&fixture, 3), Some(0));
    let _ = nihao;

    // Out-of-range: false with the out-param zeroed.
    let mut token: oxpinyin_capi::types::PhraseTokenT = 0xBEEF;
    assert!(!unsafe { pinyin_get_phrase_token(fixture.instance, 4, &raw mut token) });
    assert_eq!(token, 0, "zeroed before the bounds check");
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
        unsafe { pinyin_parse_more_full_pinyins(fixture.instance, nihao.as_ptr()) },
        5
    );
    // The union flow: guess before choose — sort option 0x1e, the
    // pipeline tests' option.
    assert!(unsafe { pinyin_guess_candidates(fixture.instance, 0, 0x1e) });
    let mut candidate: *mut LookupCandidate = std::ptr::null_mut();
    assert!(unsafe { pinyin_get_candidate(fixture.instance, 0, &raw mut candidate) });
    assert!(unsafe { pinyin_choose_candidate(fixture.instance, 0, candidate) } > 0);
    assert!(unsafe { oxpinyin_capi::sentence::pinyin_guess_sentence(fixture.instance) });
    assert!(unsafe { pinyin_train(fixture.instance, 0) });

    let mut num: u32 = 0;
    assert!(unsafe { pinyin_guess_predicted_candidates(fixture.instance, cstr("你").as_ptr()) });
    assert!(unsafe { pinyin_get_n_candidate(fixture.instance, &raw mut num) });
    assert!(num > 0, "the planted bigram predicts rows");

    for index in 0..num {
        let mut row: *mut LookupCandidate = std::ptr::null_mut();
        assert!(unsafe { pinyin_get_candidate(fixture.instance, index, &raw mut row) });
        let mut kind = lookup_candidate_type_t::NBEST_MATCH_CANDIDATE;
        assert!(unsafe { pinyin_get_candidate_type(fixture.instance, row, &raw mut kind) });
        assert_ne!(
            kind,
            lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE,
            "the plain variant never prepends punctuation"
        );
    }

    // A prefix with no dictionary suffix: the plain variant answers
    // false; the punctuated entry discards that retval and answers true.
    assert!(!unsafe {
        pinyin_guess_predicted_candidates(fixture.instance, cstr("abyss").as_ptr())
    });
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(
            fixture.instance,
            cstr("abyss").as_ptr(),
        )
    });
}

/// `pinyin_token_get_phrase` round-trips the lookup: the token found by
/// text renders back to the same text; an unknown token answers false
/// and leaves both out-params untouched (`pinyin.cpp:1627`).
#[test]
fn token_get_phrase_round_trips() {
    let fixture = Fixture::new("dict-phrase");

    let mut array = TokenArray::new();
    assert!(unsafe {
        pinyin_lookup_tokens(fixture.instance, cstr("你好").as_ptr(), array.array_ptr())
    });
    let token = array.elements()[0];

    let mut len: u32 = 0;
    let mut rendered: *mut oxpinyin_capi::types::GChar = std::ptr::null_mut();
    assert!(unsafe {
        pinyin_token_get_phrase(fixture.instance, token, &raw mut len, &raw mut rendered)
    });
    assert_eq!(len as usize, 2, "len is the character count");
    assert_eq!(
        // SAFETY: `rendered` is the string the library just returned; this code owns it and frees
        // it only here.
        unsafe { take(rendered) }.as_deref(),
        Some("你好"),
        "the text round-trips"
    );
    assert!(unsafe {
        pinyin_token_get_phrase(
            fixture.instance,
            token,
            std::ptr::null_mut(),
            &raw mut rendered,
        )
    });

    let unknown = 0x09FF_FFFF;
    len = 0xBEEF;
    let sentinel = std::ptr::dangling_mut::<oxpinyin_capi::types::GChar>();
    rendered = sentinel;
    assert!(!unsafe {
        pinyin_token_get_phrase(fixture.instance, unknown, &raw mut len, &raw mut rendered)
    });
    assert_eq!(len, 0xBEEF, "len untouched");
    assert_eq!(rendered, sentinel, "the string out-param is untouched");
}

/// `pinyin_token_get_n_pronunciation` and `_get_nth_pronunciation`:
/// 你好 carries at least one pronunciation whose keys spell the phrase;
/// an out-of-range nth answers true and appends `phrase_length` keys (the
/// pin appends whatever its uninitialised stack held, `pinyin.cpp:2808-2817`;
/// here they are zeroed), and an unknown token empties the array first.
#[test]
fn token_pronunciation_surface() {
    let fixture = Fixture::new("dict-pron");

    let mut array = TokenArray::new();
    assert!(unsafe {
        pinyin_lookup_tokens(fixture.instance, cstr("你好").as_ptr(), array.array_ptr())
    });
    let token = array.elements()[0];

    let mut num: u32 = 0;
    assert!(unsafe { pinyin_token_get_n_pronunciation(fixture.instance, token, &raw mut num) });
    assert!(num >= 1);

    // A real glib GArray of `u16` chewing-key words. The library
    // appends into it via `g_array_append_vals`, which needs the
    // private `_GRealArray` metadata glib itself sets up.
    // SAFETY: g_array_new either returns a valid array or aborts (glib
    // policy, inherited by upstream libpinyin).
    let keys = unsafe {
        g_array_new(
            0,
            0,
            u32::try_from(size_of::<u16>()).expect("u16 fits guint"),
        )
    };
    assert!(!keys.is_null(), "g_array_new returned null");
    assert!(unsafe { pinyin_token_get_nth_pronunciation(fixture.instance, token, 0, keys,) });
    // SAFETY: keys is live; `len` is glib's public field.
    let keys_len = unsafe { (*keys).len };
    assert!(keys_len >= 2, "你+好: two chewing keys");

    // An out-of-range nth answers true with `phrase_length` zeroed keys:
    // the array is emptied first, then appended to.
    assert!(unsafe { pinyin_token_get_nth_pronunciation(fixture.instance, token, 9, keys,) });
    // SAFETY: keys is live; `len` and `data` are glib's public fields.
    let (zeroed_len, zeroed) = unsafe {
        let view = &*keys;
        (
            view.len,
            std::slice::from_raw_parts(view.data.cast::<u16>(), view.len as usize).to_vec(),
        )
    };
    assert_eq!(zeroed_len as usize, 2, "你好 is two characters");
    assert_eq!(zeroed, vec![0, 0], "past the last reading: zeroed keys");

    // An unknown token answers false and leaves the array empty.
    assert!(!unsafe { pinyin_token_get_nth_pronunciation(fixture.instance, 0x09FF_FFFF, 0, keys) });
    // SAFETY: keys is live; `len` is glib's public field.
    assert_eq!(unsafe { (*keys).len }, 0);

    // SAFETY: keys came from g_array_new; frees the buffer glib grew.
    unsafe {
        g_array_free(keys, 1);
    }
}

/// `pinyin_token_get_unigram_frequency` / `_add_unigram_frequency`:
/// read → add → shifted read; an absent-token add answers false.
#[test]
fn token_unigram_read_and_live_item_write() {
    let fixture = Fixture::new("dict-unigram");

    let mut array = TokenArray::new();
    assert!(unsafe {
        pinyin_lookup_tokens(fixture.instance, cstr("你好").as_ptr(), array.array_ptr())
    });
    let token = array.elements()[0];

    let mut freq: u32 = 0;
    assert!(unsafe { pinyin_token_get_unigram_frequency(fixture.instance, token, &raw mut freq) });
    let before = freq;

    assert!(unsafe { pinyin_token_add_unigram_frequency(fixture.instance, token, 7) });
    assert!(unsafe { pinyin_token_get_unigram_frequency(fixture.instance, token, &raw mut freq) });
    assert_eq!(freq, before + 7, "the live item count increases");

    // Absent token: false, and the read stays absent.
    let absent = 0x09FF_FFFF;
    assert!(!unsafe {
        pinyin_token_get_unigram_frequency(fixture.instance, absent, &raw mut freq)
    });
    assert!(!unsafe { pinyin_token_add_unigram_frequency(fixture.instance, absent, 3) });
}

#[test]
fn token_unigram_distinguishes_zero_user_item_from_absent() {
    use oxpinyin_capi::iterators::{
        pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase,
    };

    let fixture = Fixture::new("dict-unigram-zero");
    let iter = unsafe { pinyin_begin_add_phrases(fixture.context, 7) };
    assert!(!iter.is_null());
    assert!(unsafe {
        pinyin_iterator_add_phrase(iter, cstr("网词").as_ptr(), cstr("wang3'ci2").as_ptr(), 0)
    });
    unsafe { pinyin_end_add_phrases(iter) };
    let mut tokens = TokenArray::new();
    assert!(unsafe {
        pinyin_lookup_tokens(fixture.instance, cstr("网词").as_ptr(), tokens.array_ptr())
    });
    let token = tokens.elements()[0];
    assert_eq!(token >> 24, 7);
    let mut freq = u32::MAX;
    assert!(unsafe { pinyin_token_get_unigram_frequency(fixture.instance, token, &raw mut freq) });
    assert_eq!(freq, 0);
    freq = u32::MAX;
    assert!(!unsafe {
        pinyin_token_get_unigram_frequency(fixture.instance, token + 1, &raw mut freq)
    });
    assert_eq!(freq, 0);
    assert!(unsafe { pinyin_token_add_unigram_frequency(fixture.instance, token, 7) });
    assert!(unsafe { pinyin_token_get_unigram_frequency(fixture.instance, token, &raw mut freq) });
    assert_eq!(freq, 7);
}

/// The load/unload contract: fresh GBK is already loaded (`load` false),
/// the first unload answers true, the second false (the sub-index is
/// gone), and the reload answers true. Non-GBK indexes are refused.
#[test]
fn phrase_library_load_unload_contract() {
    let fixture = Fixture::new("dict-lib");

    assert!(
        !unsafe { pinyin_load_phrase_library(fixture.context, 2) },
        "already loaded"
    );
    assert!(unsafe { pinyin_unload_phrase_library(fixture.context, 2) });
    assert!(
        unsafe { pinyin_unload_phrase_library(fixture.context, 2) },
        "the pin ignores what `unload` returns: a repeat answers true too"
    );
    assert!(
        unsafe { pinyin_load_phrase_library(fixture.context, 2) },
        "reload"
    );
    assert!(
        !unsafe { pinyin_load_phrase_library(fixture.context, 2) },
        "loaded again"
    );

    // Content survives the cycle: 你好 still resolves.
    let mut array = TokenArray::new();
    assert!(unsafe {
        pinyin_lookup_tokens(fixture.instance, cstr("你好").as_ptr(), array.array_ptr())
    });
    assert_eq!(array.elements().len(), 1);
}

struct LogHandler {
    default: u32,
    facade: u32,
}

impl LogHandler {
    fn new() -> Self {
        LOGS.with(|logs| *logs.borrow_mut() = Some(Vec::new()));
        // SAFETY: The callback lives for the process, has no borrowed
        // user data, and is removed by Drop before the capture ends.
        let default = unsafe {
            glib_sys::g_log_set_handler(
                std::ptr::null(),
                glib_sys::G_LOG_LEVEL_MASK,
                Some(capture_segment_log),
                std::ptr::null_mut(),
            )
        };
        // SAFETY: Same callback and lifetime; the facade domain is the
        // one `ffi::log_warning` writes.
        let facade = unsafe {
            glib_sys::g_log_set_handler(
                c"libpinyin".as_ptr(),
                glib_sys::G_LOG_LEVEL_MASK,
                Some(capture_segment_log),
                std::ptr::null_mut(),
            )
        };
        Self { default, facade }
    }
}

impl Drop for LogHandler {
    fn drop(&mut self) {
        // SAFETY: These are the handler ids installed above.
        unsafe {
            glib_sys::g_log_remove_handler(std::ptr::null(), self.default);
            glib_sys::g_log_remove_handler(c"libpinyin".as_ptr(), self.facade);
        }
        LOGS.with(|logs| *logs.borrow_mut() = None);
    }
}

#[test]
fn surrogate_segment_matches_pin_length_and_no_critical() {
    let user = crate::support::TempUserDir::new("surrogate-segment");
    let (context, instance) = crate::support::open(user.path.to_str().expect("UTF-8 path"));
    let _logs = LogHandler::new();
    for bytes in [
        b"\xed\xa0\x80nihao".as_slice(),
        b"ni\xed\xa0\x80hao".as_slice(),
        b"nihao\xed\xa0\x80".as_slice(),
    ] {
        let sentence = CString::new(bytes).expect("no interior NUL");
        assert!(!unsafe { pinyin_phrase_segment(instance, sentence.as_ptr()) });
        let mut count = 0xABCDEF;
        assert!(unsafe { pinyin_get_n_phrase(instance, &raw mut count) });
        assert_eq!(count, 6);
        for index in 0..count {
            let mut token = 0xABCDEF;
            assert!(unsafe { pinyin_get_phrase_token(instance, index, &raw mut token) });
            assert_eq!(token, 0);
        }
        let mut token = 0xABCDEF;
        assert!(!unsafe { pinyin_get_phrase_token(instance, count, &raw mut token) });
        assert_eq!(token, 0);
        LOGS.with(|logs| assert!(logs.borrow().as_ref().expect("active capture").is_empty()));

        // Recover through normal lookup, then verify the invalid-byte
        // refusal keeps the prior result and logs nothing (#1297).
        assert!(unsafe { pinyin_phrase_segment(instance, c"你好".as_ptr()) });
        assert!(!unsafe { pinyin_phrase_segment(instance, c"\xff\xfe".as_ptr()) });
        assert!(unsafe { pinyin_get_n_phrase(instance, &raw mut count) });
        assert_eq!(count, 2);
        LOGS.with(|logs| assert!(logs.borrow().as_ref().expect("active capture").is_empty()));
    }
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}
