#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::test_support::{Fixture, n_candidate};
use oxpinyin::model::Dictionary as _;

use crate::test_support::{alloc, open_at, parse, wrap_facade_total_to_zero};

use super::*;

use crate::context::pinyin_fini;

use crate::instance::pinyin_free_instance;

use crate::sentence::{
    pinyin_get_character_offset, pinyin_guess_candidates, pinyin_guess_sentence,
};

use crate::test_support::{TempUserDir, assert_aborts, cstr, open};

use crate::types::PinyinInstance;

use oxpinyin::OptionBits;
use oxpinyin::vocab::PhraseText;

/// With the facade total wrapped to zero, the candidate guess aborts in
/// the scorer.
#[test]
fn guess_candidates_aborts_on_a_zero_facade_total() {
    assert_aborts(
        "sentence::tests::guess_candidates_aborts_on_a_zero_facade_total",
        || {
            let user = TempUserDir::new("facade-total-zero");
            let (_context, instance) = open_at(&user.path);
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { wrap_facade_total_to_zero(instance) };
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            assert_eq!(unsafe { parse(instance, "nihao") }, 5);
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
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { alloc(context) };
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { parse(instance, "nihao") }, 5);
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { wrap_facade_total_to_zero(instance) };
        assert!(
            unsafe { pinyin_guess_candidates(instance, offset, sort) },
            "{offset}"
        );
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { n_candidate(instance) }, 0, "{offset}");
        unsafe { pinyin_free_instance(instance) };
    }
    assert!(logs.take().is_empty());
    unsafe { pinyin_fini(context) };
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
            let walk = unsafe { crate::state::instance_ref(fixture.instance()) }
                .core
                .character_offset(&"你好".into(), 3, false);
            // Not `eprintln!`: libtest captures it and the abort drops it.
            writeln!(std::io::stderr(), "walk: {walk:?}").expect("write to stderr");
            let mut length = 0;
            unsafe {
                pinyin_get_character_offset(
                    fixture.instance(),
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
        stderr.ends_with("walk: Err(ZeroKeyOffsetCheck)\n"),
        "{stderr}"
    );
}

#[test]
fn a_read_failure_after_the_size_check_aborts() {
    // Kyoto Cabinet's `Bigram::load` asserts where the body read fails
    // after `check` found the record (`ngram_kyotodb.cpp:138-139`). No
    // differential can make a read fail after a successful check, so
    // this death test holds the abort.
    crate::test_support::assert_aborts(
        "sentence::tests::a_read_failure_after_the_size_check_aborts",
        || {
            let error = oxpinyin::engine::EngineError::ReadAfterCheck;
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
    use oxpinyin::marshal::SentencePhrase;
    // Item words `g_ucs4_to_utf8` refuses: a word at or above 0x8000_0000
    // before the first zero.
    let null = || SentencePhrase::Words([0x4f60, 0x8000_0000].into_iter().collect());
    let words = |text: &str| SentencePhrase::Words(text.chars().map(u32::from).collect());
    let phrases = [
        null(),
        words("你好"),
        SentencePhrase::Text(PhraseText::from("")),
        null(),
        SentencePhrase::Text(PhraseText::from("世界")),
        words("！"),
    ];
    assert_eq!(
        // SAFETY: `crate::ffi::sentence_text(phrases)` is the string the library just returned;
        // this code owns it and frees it only here.
        unsafe { crate::ffi::take_owned_cstr(crate::ffi::sentence_text(phrases)) },
        "你好世界！"
    );
    assert!(crate::ffi::sentence_text([null()]).is_null());
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
        let token = |text: &str| {
            inst.core
                .dict
                .tokens_for_ucs4(&text.chars().map(u32::from).collect::<Vec<_>>())[0]
        };
        (token("好"), token("你好"), token("好"), token("号"))
    };
    // SAFETY: both setup handles are live and are unused after closing them.
    unsafe { crate::test_support::close(context, instance) };
    crate::test_support::plant_bigram(
        &user.path,
        &[
            (oxpinyin::vocab::SENTENCE_START, hao),
            (short, hao),
            (long, number),
        ]
        .map(|(prev, next)| {
            (
                prev.to_le_bytes().to_vec(),
                crate::test_support::gram_bytes(1_000_000, &[(next, 1_000_000)]),
            )
        }),
    );
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
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

/// A private system whose library 1 holds 甲 (`0x01000001`) and 乙 under
/// `ba1` and `da1`, and whose `bigram.db` stores `value` under sentence
/// start, as the pin's `load` keeps any stored value (`ngram_tkrzwdb.cpp:102-121`).
fn corrupt_start_gram_system(scratch: &std::path::Path, value: Vec<u8>) -> std::path::PathBuf {
    use crate::test_support::{craft_hash, items_system, system_dir};
    let system = items_system(
        &system_dir(),
        scratch,
        &[(&[0x7532], &[0x1081], 1), (&[0x4e59], &[0x1084], 1000)],
        2000,
    );
    craft_hash(
        &system.join("bigram.db"),
        &[(
            oxpinyin::vocab::SENTENCE_START.to_le_bytes().to_vec(),
            value,
        )],
    );
    system
}

/// With a user gram present, the merge reads the system total first and
/// asserts on a value shorter than it (`check_result(system->get_total_freq(...))`,
/// `ngram.cpp:303`, through `:48-50` and `memory_chunk.h:390`), from the
/// phrase segmenter's `search_bigram2` (`phrase_lookup.cpp:269-272`).
#[test]
fn a_short_system_gram_merged_with_a_user_gram_aborts_the_segment() {
    assert_aborts(
        "sentence::tests::a_short_system_gram_merged_with_a_user_gram_aborts_the_segment",
        || {
            let scratch = tempfile::tempdir().expect("scratch");
            let system = corrupt_start_gram_system(scratch.path(), vec![0, 0]);
            let user = TempUserDir::new("short-system-gram-merge");
            crate::test_support::plant_bigram(
                &user.path,
                &[(
                    oxpinyin::vocab::SENTENCE_START.to_le_bytes().to_vec(),
                    crate::test_support::gram_bytes(1, &[(0x0100_0001, 1)]),
                )],
            );
            let context = crate::test_support::init_at(&system, &user.path);
            let instance = unsafe { alloc(context) };
            unsafe { crate::phrase::pinyin_phrase_segment(instance, cstr("甲").as_ptr()) };
        },
    );
}

/// With no user gram and a system value shorter than its total, the n-best
/// ranged step's `search` calls `lower_bound` on a range whose end comes
/// before its start (`ngram.cpp:155-161`), before `get_total_freq`
/// (`:163-165`): the call breaks `lower_bound`'s precondition, so the pin's
/// result is undefined. From `search_bigram2` (`phonetic_lookup.h:594-598`,
/// `:626`).
#[test]
fn a_short_system_gram_aborts_the_ranged_step() {
    assert_aborts(
        "sentence::tests::a_short_system_gram_aborts_the_ranged_step",
        || {
            let scratch = tempfile::tempdir().expect("scratch");
            let system = corrupt_start_gram_system(scratch.path(), vec![0, 0]);
            let user = TempUserDir::new("short-system-gram-ranged");
            let context = crate::test_support::init_at(&system, &user.path);
            assert!(unsafe {
                crate::config::pinyin_set_options(context, OptionBits::USE_TONE.bits())
            });
            let instance = unsafe { alloc(context) };
            assert_eq!(unsafe { parse(instance, "ba1") }, 3);
            unsafe { pinyin_guess_sentence(instance) };
        },
    );
}

/// A system value with a partial item tail and no whole item: the phrase
/// segmenter's `get_freq` walk never meets `end` and reads past the value
/// (`ngram.cpp:234-248`). The segment logs the
/// one warning and returns false.
#[test]
fn a_partial_system_gram_tail_refuses_the_segment_with_one_warning() {
    let scratch = tempfile::tempdir().expect("scratch");
    let mut value = crate::test_support::gram_bytes(10, &[]);
    value.extend_from_slice(&[0, 0, 0]);
    let system = corrupt_start_gram_system(scratch.path(), value);
    let user = TempUserDir::new("partial-system-gram");
    let context = crate::test_support::init_at(&system, &user.path);
    let instance = unsafe { alloc(context) };
    let logs = crate::test_support::LogCapture::new();
    assert!(!unsafe { crate::phrase::pinyin_phrase_segment(instance, cstr("甲").as_ptr()) });
    let messages: Vec<String> = logs
        .take_with_messages()
        .into_iter()
        .map(|(_, message)| message)
        .collect();
    assert_eq!(
        messages,
        [
            "pinyin_phrase_segment: a bigram value would be read past its end (upstream reads past it)"
        ]
    );
    drop(logs);
    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_fini(context) };
}
