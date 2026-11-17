#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::test_support::{alloc, open_at, parse, wrap_facade_total_to_zero};

use super::*;

use crate::candidates::pinyin_get_n_candidate;

use crate::context::pinyin_fini;

use crate::instance::pinyin_free_instance;

use crate::parse::pinyin_parse_more_full_pinyins;

use crate::sentence::{
    pinyin_get_character_offset, pinyin_guess_candidates, pinyin_guess_sentence,
};

use crate::test_support::{TempUserDir, assert_aborts, cstr, open};

use crate::types::{PinyinContext, PinyinInstance};

use oxpinyin_core::{Dictionary, OptionBits, PhraseText};

/// The value a test seeds an out-parameter with, to see it left alone.
const UNTOUCHED: u32 = 0x00AB_CDEF;

fn n_candidate(instance: *mut PinyinInstance) -> u32 {
    let mut count = UNTOUCHED;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut count) });
    count
}

/// With the facade total wrapped to zero, the candidate guess aborts in
/// the scorer (register row 61).
#[test]
fn guess_candidates_aborts_on_a_zero_facade_total() {
    assert_aborts(
        "sentence::tests::guess_candidates_aborts_on_a_zero_facade_total",
        || {
            let user = TempUserDir::new("facade-total-zero");
            let (_context, instance) = open_at(&user.path);
            wrap_facade_total_to_zero(instance);
            assert_eq!(parse(instance, "nihao"), 5);
            unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) };
        },
    );
}

/// A guess that ranks no word row never reaches the scorer: true with no
/// row on a zero facade total, mid-key (`sentence_rows_only`) and at the
/// reserved end slot, where the search loop does not run
/// (`pinyin.cpp:2230`).
#[test]
fn a_guess_with_no_word_row_ranks_nothing_on_a_zero_total() {
    let user = TempUserDir::new("facade-total-zero-no-rank");
    let (context, _) = open_at(&user.path);
    let logs = crate::test_support::LogCapture::new();
    for (offset, sort) in [(3, 3), (5, 2)] {
        let instance = alloc(context);
        assert_eq!(parse(instance, "nihao"), 5);
        wrap_facade_total_to_zero(instance);
        assert!(
            unsafe { pinyin_guess_candidates(instance, offset, sort) },
            "{offset}"
        );
        assert_eq!(n_candidate(instance), 0, "{offset}");
        unsafe { pinyin_free_instance(instance) };
    }
    assert!(logs.take().is_empty());
    unsafe { pinyin_fini(context) };
}

/// A context, its instance and the user directory they sit in, released in
/// that order when the fixture drops. A death-test child aborts without
/// dropping it; its scratch root removes the directory.
struct Fixture {
    context: *mut PinyinContext,
    instance: *mut PinyinInstance,
    _user_dir: TempUserDir,
}

impl Fixture {
    /// A fresh instance with nothing parsed.
    fn new(tag: &str) -> Self {
        let user_dir = TempUserDir::new(tag);
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        Self {
            context,
            instance,
            _user_dir: user_dir,
        }
    }

    /// An instance over `text`, which parses to `parsed` bytes.
    fn parsed(tag: &str, text: &str, parsed: usize) -> Self {
        let fixture = Self::new(tag);
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(fixture.instance, cstr(text).as_ptr()) },
            parsed
        );
        fixture
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { pinyin_free_instance(self.instance) };
        unsafe { pinyin_fini(self.context) };
    }
}

#[test]
fn the_character_offset_after_a_separator_aborts() {
    // The pin's _check_offset rejects a cursor immediately after a lone
    // zero key (074a2219 pinyin.cpp:3203-3204).
    //
    // The facade aborts without a message, and on the column, range and
    // token-count asserts too. The child therefore writes the walk's answer
    // for the same instance, phrase and offset to fd 2 before the call.
    let stderr = assert_aborts(
        "sentence::tests::the_character_offset_after_a_separator_aborts",
        || {
            use std::io::Write as _;
            let fixture = Fixture::parsed("char-after-separator", "ni'hao", 6);
            // SAFETY: the fixture's instance is live and this thread alone
            // uses it.
            let walk = unsafe { crate::state::instance_ref(fixture.instance) }
                .core
                .character_offset(&"你好".into(), 3);
            // Not `eprintln!`: libtest captures it and the abort drops it.
            writeln!(std::io::stderr(), "walk: {walk:?}").expect("write to stderr");
            let mut length = 0;
            unsafe {
                pinyin_get_character_offset(
                    fixture.instance,
                    cstr("你好").as_ptr(),
                    3,
                    &raw mut length,
                )
            };
        },
    );
    // The init's `open …/user.conf failed.` precedes it without a newline.
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(
        stderr.ends_with("walk: Err(ZeroKeyOffsetCheck { offset: 3 })\n"),
        "{stderr}"
    );
}

#[test]
fn a_read_failure_after_the_size_check_aborts() {
    // Kyoto Cabinet's `Bigram::load` asserts where the body read fails
    // after `check` found the record (`ngram_kyotodb.cpp:136-137`). No
    // differential can make a read fail after a successful check, so
    // this death test holds the abort.
    crate::test_support::assert_aborts(
        "sentence::tests::a_read_failure_after_the_size_check_aborts",
        || {
            let error = oxpinyin_engine::EngineError::ReadAfterCheck { token: 0x0100_0001 };
            crate::sentence::refuse_lookup_gram("pinyin_phrase_segment", &error);
        },
    );
}

/// `convert_to_utf8`'s chain (`lookup.cpp:37-67`): `g_strdup` of the
/// first phrase, `g_strconcat(prev, "", phrase, NULL)` for each later
/// one; a NULL phrase is left out, and no converted phrase leaves the
/// pin's NULL.
#[test]
fn the_chain_joins_each_phrase_and_no_phrase_is_null() {
    let phrases = [
        None,
        Some(PhraseText::from("你好")),
        Some(PhraseText::from("")),
        None,
        Some(PhraseText::from("世界")),
    ];
    assert_eq!(
        crate::ffi::take_owned_cstr(crate::ffi::sentence_text(phrases)),
        "你好世界"
    );
    assert!(crate::ffi::sentence_text([None::<PhraseText>]).is_null());
}

// 074a2219: pinyin.cpp:1711-1740,2200-2215 and
// zhuyin.cpp:1146-1175,1473-1489 choose the longest live prefix at zero.
// Prefixes are stored even when the sentence lookup refuses an empty matrix.
#[rstest::rstest]
fn dynamic_ranking_uses_the_longest_retained_prefix(#[values(false, true)] prediction: bool) {
    let user = TempUserDir::new("prefix-ranking");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let (short, long, hao, number) = {
        // SAFETY: the newly allocated instance is live; no mutation overlaps.
        let inst = unsafe { crate::state::instance_ref(instance) };
        let token = |text| inst.core.dict.tokens_for_text(text)[0].value();
        (token("好"), token("你好"), token("好"), token("号"))
    };
    {
        // SAFETY: the preceding reader ended; this test serializes all calls.
        let ctx = unsafe { crate::state::instance_context_mut(instance) };
        let store = ctx.core.user_store_mut();
        for (previous, next) in [
            (oxpinyin_user::SENTENCE_START, hao),
            (short, hao),
            (long, number),
        ] {
            store
                .set_bigram_count(previous, next, 1_000_000)
                .expect("seed real bigram row");
        }
    }
    assert!(unsafe {
        crate::config::pinyin_set_options(context, OptionBits::DYNAMIC_ADJUST.bits())
    });
    if prediction {
        assert!(unsafe {
            crate::predict::pinyin_guess_predicted_candidates(instance, cstr("你好").as_ptr())
        });
    } else {
        assert!(!unsafe { pinyin_guess_sentence_with_prefix(instance, cstr("你好").as_ptr()) });
    }
    assert_eq!(
        unsafe { crate::parse::pinyin_parse_more_full_pinyins(instance, cstr("hao").as_ptr()) },
        3
    );
    let rank = || {
        assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) });
        // SAFETY: the guess completed; its snapshot remains live for this read.
        let inst = unsafe { crate::state::instance_ref(instance) };
        let position = |text| {
            inst.state
                .candidates
                .iter()
                .position(|row| row.text_str() == Ok(text))
                .expect("candidate is present")
        };
        position("号") < position("好")
    };
    assert!(
        rank(),
        "the longer prefix's gram must outrank sentence_start and the shorter suffix"
    );
    assert!(unsafe { crate::config::pinyin_set_options(context, 0) });
    assert!(!rank(), "without DYNAMIC_ADJUST only unigram order remains");
    assert!(unsafe {
        crate::config::pinyin_set_options(context, OptionBits::DYNAMIC_ADJUST.bits())
    });
    assert!(rank(), "option changes preserve the prefix tokens");

    // A plain sentence guess clears prefixes even when its matrix is empty.
    assert_eq!(
        unsafe { crate::parse::pinyin_parse_more_full_pinyins(instance, cstr("").as_ptr()) },
        0
    );
    assert!(!unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(
        unsafe { crate::parse::pinyin_parse_more_full_pinyins(instance, cstr("hao").as_ptr()) },
        3
    );
    assert!(!rank(), "plain guess restores sentence_start");

    assert!(unsafe { pinyin_guess_sentence_with_prefix(instance, cstr("你好").as_ptr()) });
    assert!(unsafe { crate::instance::pinyin_reset(instance) });
    assert_eq!(
        unsafe { crate::parse::pinyin_parse_more_full_pinyins(instance, cstr("hao").as_ptr()) },
        3
    );
    assert!(!rank(), "full reset clears prefixes");
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// A lookup strictly inside a double-pinyin key answers the sentence rows
/// alone: the pin's matrix holds the key only on the column its key rest
/// begins on, so a mid-key `pinyin_guess_candidates` searches an empty
/// column (`pinyin.cpp:2224-2262`; the zhuyin facade's #577 law). Offset 0
/// and offset 2 are key starts and list phrases.
#[test]
fn a_mid_key_lookup_under_double_pinyin_lists_only_the_sentence_rows() {
    let user_dir = TempUserDir::new("double-mid-key");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(unsafe { crate::config::pinyin_set_double_pinyin_scheme(context, 2) });
    let keys = cstr("nihk");
    assert_eq!(
        unsafe { crate::parse::pinyin_parse_more_double_pinyins(instance, keys.as_ptr()) },
        4
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    let kinds = |instance: *mut PinyinInstance| -> Vec<crate::types::lookup_candidate_type_t> {
        // SAFETY: live instance immediately after a guess.
        let inst = unsafe { crate::state::instance_ref(instance) };
        inst.state
            .candidates
            .iter()
            .map(|cd| cd.candidate_type())
            .collect()
    };
    for offset in [0, 2] {
        assert!(unsafe {
            pinyin_guess_candidates(instance, offset, crate::test_support::DEFAULT_SORT)
        });
        assert!(
            kinds(instance).contains(&crate::types::lookup_candidate_type_t::NORMAL_CANDIDATE),
            "offset {offset} is a key start and lists phrases"
        );
    }
    for offset in [1, 3] {
        assert!(unsafe {
            pinyin_guess_candidates(instance, offset, crate::test_support::DEFAULT_SORT)
        });
        let kinds = kinds(instance);
        assert!(!kinds.is_empty(), "the sentence rows are listed");
        assert!(
            kinds
                .iter()
                .all(|kind| *kind == crate::types::lookup_candidate_type_t::NBEST_MATCH_CANDIDATE),
            "offset {offset} is mid-key: sentence rows only"
        );
    }
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}
