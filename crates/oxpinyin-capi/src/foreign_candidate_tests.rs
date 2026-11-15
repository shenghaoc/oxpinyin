//! `pinyin_choose_candidate` with a candidate another live instance
//! published, through the C ABI only.
//!
//! The pin reads the row's type, token, n-best index and bounds and never
//! checks which instance published it (074a2219 `pinyin.cpp:2501-2588`);
//! the context, matrix, constraints, n-best results and sort option are the
//! receiving instance's. Each case runs with both instances on one context
//! and again on two contexts over the same system data.

use std::ffi::c_uint;

use crate::candidates::{
    pinyin_choose_candidate, pinyin_clear_constraint, pinyin_get_candidate,
    pinyin_get_candidate_string, pinyin_get_candidate_type, pinyin_get_n_candidate,
};
use crate::context::{pinyin_fini, pinyin_init};
use crate::dict::pinyin_token_get_unigram_frequency;
use crate::instance::{pinyin_alloc_instance, pinyin_free_instance};
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::{pinyin_guess_candidates, pinyin_guess_sentence};
use crate::test_support::{DEFAULT_SORT, TempUserDir, assert_aborts, cstr, lookup, open};
use crate::types::{LookupCandidate, PinyinContext, PinyinInstance, lookup_candidate_type_t};

/// ibus preset 1 (the GSettings default): LONGER rows on (bit 0x2 clear).
const IBUS_DEFAULT_SORT: c_uint = 0x1c;
/// `SORT_WITHOUT_SENTENCE_CANDIDATE` (`pinyin.h:56`).
const SORT_WITHOUT_SENTENCE: c_uint = 0x1;

/// A producing and a receiving instance, on one context or on two.
struct Pair {
    contexts: Vec<*mut PinyinContext>,
    producer: *mut PinyinInstance,
    receiver: *mut PinyinInstance,
    _dirs: Vec<TempUserDir>,
}

impl Pair {
    fn open(same_context: bool) -> Self {
        let first = TempUserDir::new("foreign-producer");
        let (context, producer) = open(first.path.to_str().expect("UTF-8 path"));
        if same_context {
            let receiver = pinyin_alloc_instance(context);
            assert!(!receiver.is_null());
            return Self {
                contexts: vec![context],
                producer,
                receiver,
                _dirs: vec![first],
            };
        }
        let second = TempUserDir::new("foreign-receiver");
        let (other, receiver) = open(second.path.to_str().expect("UTF-8 path"));
        Self {
            contexts: vec![context, other],
            producer,
            receiver,
            _dirs: vec![first, second],
        }
    }

    /// The receiving instance's context.
    fn receiver_context(&self) -> *mut PinyinContext {
        *self.contexts.last().expect("a context")
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        pinyin_free_instance(self.producer);
        pinyin_free_instance(self.receiver);
        for context in &self.contexts {
            pinyin_fini(*context);
        }
    }
}

fn parse(instance: *mut PinyinInstance, input: &str) {
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr(input).as_ptr()),
        input.len()
    );
}

fn row_type(instance: *mut PinyinInstance, row: *mut LookupCandidate) -> lookup_candidate_type_t {
    let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
    assert!(pinyin_get_candidate_type(instance, row, &raw mut kind));
    kind
}

fn row_text(instance: *mut PinyinInstance, row: *mut LookupCandidate) -> String {
    let mut text = std::ptr::null();
    assert!(pinyin_get_candidate_string(instance, row, &raw mut text));
    assert!(!text.is_null());
    // SAFETY: the getter answers the live row's NUL-terminated string.
    unsafe { std::ffi::CStr::from_ptr(text) }
        .to_str()
        .expect("UTF-8 candidate")
        .to_owned()
}

/// The first row of `instance`'s list that `wanted` accepts.
fn find_row(
    instance: *mut PinyinInstance,
    wanted: impl Fn(lookup_candidate_type_t, &str) -> bool,
) -> *mut LookupCandidate {
    let mut count = 0;
    assert!(pinyin_get_n_candidate(instance, &raw mut count));
    (0..count)
        .map(|index| {
            let mut row = std::ptr::null_mut();
            assert!(pinyin_get_candidate(instance, index, &raw mut row));
            row
        })
        .find(|&row| wanted(row_type(instance, row), &row_text(instance, row)))
        .expect("the guess publishes the wanted row")
}

fn unigram(instance: *mut PinyinInstance, token: u32) -> u32 {
    let mut freq = 0;
    pinyin_token_get_unigram_frequency(instance, token, &raw mut freq);
    freq
}

/// A NORMAL row forces its own span on the receiver, answers `offset +
/// (m_end - m_begin)`, and leaves the producer's constraints alone
/// (`pinyin.cpp:2577-2590`).
#[rstest::rstest]
fn a_normal_row_from_another_instance_forces_its_span_on_the_receiver(
    #[values(true, false)] same_context: bool,
) {
    let pair = Pair::open(same_context);
    parse(pair.producer, "nihao");
    parse(pair.receiver, "nihao");
    assert!(pinyin_guess_candidates(pair.producer, 0, DEFAULT_SORT));
    let row = find_row(pair.producer, |kind, text| {
        kind == lookup_candidate_type_t::NORMAL_CANDIDATE && text == "你好"
    });
    assert_eq!(pinyin_choose_candidate(pair.receiver, 0, row), 5);
    assert!(pinyin_clear_constraint(pair.receiver, 0));
    assert!(!pinyin_clear_constraint(pair.producer, 0));
}

/// The receiver's matrix decides: a span past it is refused, `offset + 0`,
/// with nothing forced (`phonetic_lookup.cpp:64-65`).
#[rstest::rstest]
fn a_span_past_the_receivers_matrix_is_refused(#[values(true, false)] same_context: bool) {
    let pair = Pair::open(same_context);
    parse(pair.producer, "nihao");
    parse(pair.receiver, "ni");
    assert!(pinyin_guess_candidates(pair.producer, 0, DEFAULT_SORT));
    let row = find_row(pair.producer, |kind, text| {
        kind == lookup_candidate_type_t::NORMAL_CANDIDATE && text == "你好"
    });
    assert_eq!(pinyin_choose_candidate(pair.receiver, 0, row), 0);
    assert!(!pinyin_clear_constraint(pair.receiver, 0));
}

/// A LONGER row trains the receiver's unigram by 483 and answers 1
/// (`pinyin.cpp:2521-2530`); a separate producer context is untouched.
#[rstest::rstest]
fn a_longer_row_trains_the_receivers_unigram(#[values(true, false)] same_context: bool) {
    let pair = Pair::open(same_context);
    parse(pair.producer, "ni");
    assert!(pinyin_guess_sentence(pair.producer));
    assert!(pinyin_guess_candidates(pair.producer, 0, IBUS_DEFAULT_SORT));
    let row = find_row(pair.producer, |kind, _| {
        kind == lookup_candidate_type_t::LONGER_CANDIDATE
    });
    let text = row_text(pair.producer, row);
    let token = *lookup(pair.receiver, &text)
        .first()
        .expect("the LONGER phrase");
    let (receiver_before, producer_before) =
        (unigram(pair.receiver, token), unigram(pair.producer, token));
    assert_eq!(pinyin_choose_candidate(pair.receiver, 0, row), 1);
    assert_eq!(unigram(pair.receiver, token), receiver_before + 483);
    if !same_context {
        assert_eq!(unigram(pair.producer, token), producer_before);
    }
    assert!(!pinyin_clear_constraint(pair.receiver, 0));
}

/// The receiver's own sort option decides the
/// `SORT_WITHOUT_SENTENCE_CANDIDATE` leg (`pinyin.cpp:2565-2576`).
#[rstest::rstest]
fn the_receivers_sort_option_selects_the_unigram_leg(#[values(true, false)] same_context: bool) {
    let pair = Pair::open(same_context);
    parse(pair.producer, "nihao");
    parse(pair.receiver, "nihao");
    assert!(pinyin_guess_candidates(pair.producer, 0, DEFAULT_SORT));
    assert!(pinyin_guess_candidates(
        pair.receiver,
        0,
        DEFAULT_SORT | SORT_WITHOUT_SENTENCE
    ));
    let row = find_row(pair.producer, |kind, text| {
        kind == lookup_candidate_type_t::NORMAL_CANDIDATE && text == "你好"
    });
    assert_eq!(pinyin_choose_candidate(pair.receiver, 0, row), 1);
    assert!(!pinyin_clear_constraint(pair.receiver, 0));
}

/// An NBEST row reads only its rank; the results and the answer, the
/// receiver's `matrix.size() - 1`, are the receiver's (`pinyin.cpp:2513-2519`).
/// Rank 1 forces the phrases where the receiver's result 1 differs from
/// its result 0 (`diff_result`, `phonetic_lookup.cpp:170-207`); rank 0
/// forces nothing.
#[rstest::rstest]
fn an_nbest_row_selects_from_the_receivers_results(#[values(true, false)] same_context: bool) {
    let forced = |instance| {
        (0..5)
            .filter(|&offset| pinyin_clear_constraint(instance, offset))
            .count()
    };
    for rank in [0_usize, 1] {
        let pair = Pair::open(same_context);
        parse(pair.producer, "nihao");
        assert!(pinyin_guess_sentence(pair.producer));
        assert!(pinyin_guess_candidates(pair.producer, 0, DEFAULT_SORT));
        let mut count = 0;
        assert!(pinyin_get_n_candidate(pair.producer, &raw mut count));
        let rows: Vec<_> = (0..count)
            .map(|index| {
                let mut row = std::ptr::null_mut();
                assert!(pinyin_get_candidate(pair.producer, index, &raw mut row));
                row
            })
            .filter(|&row| {
                row_type(pair.producer, row) == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE
            })
            .collect();
        assert!(rows.len() > 1, "two sentence rows for nihao");
        parse(pair.receiver, "nihao");
        assert!(pinyin_guess_sentence(pair.receiver));
        assert_eq!(pinyin_choose_candidate(pair.receiver, 0, rows[rank]), 5);
        assert!(!pinyin_clear_constraint(pair.producer, 0));
        if rank == 0 {
            assert_eq!(forced(pair.receiver), 0, "result 0 differs from nothing");
        } else {
            assert!(
                forced(pair.receiver) > 0,
                "result 1 forces where it differs"
            );
        }
    }
}

/// With no decoded results on the receiver, the NBEST choose's
/// `check_result(results.get_result(0, best))` asserts (`pinyin.cpp:2516`).
#[test]
fn an_nbest_row_on_a_receiver_without_results_aborts() {
    assert_aborts(
        "foreign_candidate_tests::an_nbest_row_on_a_receiver_without_results_aborts",
        || {
            let pair = Pair::open(true);
            parse(pair.producer, "ni");
            assert!(pinyin_guess_sentence(pair.producer));
            assert!(pinyin_guess_candidates(pair.producer, 0, DEFAULT_SORT));
            let row = find_row(pair.producer, |kind, _| {
                kind == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE
            });
            parse(pair.receiver, "nihao");
            pinyin_choose_candidate(pair.receiver, 0, row);
        },
    );
}

/// The library nibble of `token` (`PHRASE_INDEX_LIBRARY_INDEX`).
fn library(token: u32) -> u32 {
    (token >> 24) & 0x0f
}

/// An ADDON row promotes into the receiver's context and rewrites the
/// supplied row, which belongs to the producer (`pinyin.cpp:2532-2562`).
/// A later choose of that row takes the ordinary path with the promoted
/// token and promotes nothing again.
#[rstest::rstest]
fn an_addon_row_promotes_into_the_receivers_context(#[values(true, false)] same_context: bool) {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::union_e2e_tests::promotion_system(scratch.path());
    let system = cstr(system.to_str().expect("UTF-8 path"));
    let dirs = [
        TempUserDir::new("foreign-addon-producer"),
        TempUserDir::new("foreign-addon-receiver"),
    ];
    let init = |dir: &TempUserDir| {
        let context = pinyin_init(
            system.as_ptr(),
            cstr(dir.path.to_str().expect("UTF-8 path")).as_ptr(),
        );
        assert!(!context.is_null());
        assert!(crate::config::pinyin_load_addon_phrase_library(context, 4));
        context
    };
    let producer_context = init(&dirs[0]);
    let receiver_context = if same_context {
        producer_context
    } else {
        init(&dirs[1])
    };
    let producer = pinyin_alloc_instance(producer_context);
    let receiver = pinyin_alloc_instance(receiver_context);
    parse(producer, "erhuang");
    parse(receiver, "erhuang");
    assert!(pinyin_guess_candidates(producer, 0, DEFAULT_SORT));
    let row = find_row(producer, |kind, _| {
        kind == lookup_candidate_type_t::ADDON_CANDIDATE
    });
    let text = row_text(producer, row);
    let promoted = |instance| {
        lookup(instance, &text)
            .into_iter()
            .filter(|&token| library(token) == 5)
            .count()
    };
    assert_eq!(promoted(receiver), 0);

    assert_eq!(pinyin_choose_candidate(receiver, 0, row), 7);
    assert_eq!(
        row_type(producer, row),
        lookup_candidate_type_t::NORMAL_CANDIDATE
    );
    assert_eq!(row_text(producer, row), text, "the string is unchanged");
    assert_eq!(promoted(receiver), 1);
    assert_eq!(promoted(producer), usize::from(same_context));
    assert!(pinyin_clear_constraint(receiver, 0));

    // The promoted row again, now on the producer: no second promotion.
    assert_eq!(pinyin_choose_candidate(producer, 0, row), 7);
    assert_eq!(promoted(receiver), 1);
    assert_eq!(promoted(producer), usize::from(same_context));

    pinyin_free_instance(producer);
    pinyin_free_instance(receiver);
    pinyin_fini(producer_context);
    if !same_context {
        pinyin_fini(receiver_context);
    }
}

/// A full-pinyin 你好 row (`[0, 5)`) on a Microsoft double-pinyin receiver
/// parsing `nihk` (keys `[0, 2)` and `[2, 4)`, matrix size 5) ends on the
/// reserved last column. `add_constraint` accepts it (`5 > 5` is false,
/// `phonetic_lookup.cpp:64-65`) and clears the receiver's forcing at 0;
/// the second `validate_constraint` drops the too-long forcing
/// (`:120-168`). The answer is still `offset + 5` (`pinyin.cpp:2577-2590`).
#[test]
fn a_span_to_the_receivers_reserved_column_clears_and_forces_nothing() {
    let pair = Pair::open(false);
    assert!(crate::config::pinyin_set_double_pinyin_scheme(
        pair.receiver_context(),
        2
    ));
    assert_eq!(
        crate::parse::pinyin_parse_more_double_pinyins(pair.receiver, cstr("nihk").as_ptr()),
        4
    );
    assert!(pinyin_guess_candidates(pair.receiver, 0, DEFAULT_SORT));
    let own = find_row(pair.receiver, |kind, text| {
        kind == lookup_candidate_type_t::NORMAL_CANDIDATE && text == "你"
    });
    assert_eq!(pinyin_choose_candidate(pair.receiver, 0, own), 2);

    parse(pair.producer, "nihao");
    assert!(pinyin_guess_candidates(pair.producer, 0, DEFAULT_SORT));
    let row = find_row(pair.producer, |kind, text| {
        kind == lookup_candidate_type_t::NORMAL_CANDIDATE && text == "你好"
    });
    assert_eq!(pinyin_choose_candidate(pair.receiver, 0, row), 5);
    assert!(!pinyin_clear_constraint(pair.receiver, 0));
}

/// The pin's second `validate_constraint` after `add_constraint`
/// (`pinyin.cpp:2585`, `phonetic_lookup.cpp:120-168`) drops a forcing
/// whose token does not spell its span: 你 `[0, 2)` on a receiver parsing
/// `ma` answers `offset + 2` and forces nothing.
#[rstest::rstest]
fn a_forcing_the_receiver_cannot_spell_is_dropped(#[values(true, false)] same_context: bool) {
    let pair = Pair::open(same_context);
    parse(pair.producer, "ni");
    assert!(pinyin_guess_candidates(pair.producer, 0, DEFAULT_SORT));
    let row = find_row(pair.producer, |kind, text| {
        kind == lookup_candidate_type_t::NORMAL_CANDIDATE && text == "你"
    });
    parse(pair.receiver, "ma");
    assert_eq!(pinyin_choose_candidate(pair.receiver, 0, row), 2);
    assert!(!pinyin_clear_constraint(pair.receiver, 0));
}

/// An end strictly inside a receiver key: a double-pinyin 你好 row
/// (`[0, 4)`) on a full-pinyin receiver parsing `nihao` (keys `[0, 2)`,
/// `[2, 5)`). `add_constraint` clears the receiver's 你 forcing at 0 and
/// writes `[0, 4)`; nothing spells it, so the second `validate_constraint`
/// drops it (`phonetic_lookup.cpp:61-86`, `:120-168`). The answer is
/// `offset + 4`.
#[test]
fn a_span_ending_inside_a_receiver_key_clears_and_forces_nothing() {
    let pair = Pair::open(false);
    assert!(crate::config::pinyin_set_double_pinyin_scheme(
        pair.contexts[0],
        2
    ));
    assert_eq!(
        crate::parse::pinyin_parse_more_double_pinyins(pair.producer, cstr("nihk").as_ptr()),
        4
    );
    assert!(pinyin_guess_candidates(pair.producer, 0, DEFAULT_SORT));
    let row = find_row(pair.producer, |kind, text| {
        kind == lookup_candidate_type_t::NORMAL_CANDIDATE && text == "你好"
    });

    parse(pair.receiver, "nihao");
    assert!(pinyin_guess_candidates(pair.receiver, 0, DEFAULT_SORT));
    let own = find_row(pair.receiver, |kind, text| {
        kind == lookup_candidate_type_t::NORMAL_CANDIDATE && text == "你"
    });
    assert_eq!(pinyin_choose_candidate(pair.receiver, 0, own), 2);
    assert_eq!(pinyin_choose_candidate(pair.receiver, 0, row), 4);
    assert!(!pinyin_clear_constraint(pair.receiver, 0));
}

/// `USE_DIVIDED_TABLE` (`pinyin.h`, bit 7).
const DIVIDED: u32 = 1 << 7;

/// Opens a context over `system` whose full-pinyin scheme is `scheme`
/// with the divided table on, and an instance that parsed `input`.
fn divided(
    system: &std::path::Path,
    dir: &TempUserDir,
    scheme: std::ffi::c_int,
    input: &str,
) -> (*mut PinyinContext, *mut PinyinInstance) {
    let context = pinyin_init(
        cstr(system.to_str().expect("UTF-8 path")).as_ptr(),
        cstr(dir.path.to_str().expect("UTF-8 path")).as_ptr(),
    );
    assert!(!context.is_null());
    assert!(crate::config::pinyin_set_options(context, 0x8 | DIVIDED));
    assert!(crate::config::pinyin_set_full_pinyin_scheme(
        context, scheme
    ));
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    parse(instance, input);
    (context, instance)
}

/// The divided table gives Luoma `sian` (xian `[0, 4)`) the columns of
/// xi `[0, 2)` and an `[2, 4)` (`inner_split_step`,
/// `phonetic_key_matrix.cpp:170-200`). A 西 row `[0, 2)` forces its span
/// on the same instance, and so does the same row from a Hanyu `xian`
/// producer on that Luoma receiver.
#[rstest::rstest]
fn a_divided_column_row_forces_its_span(#[values(true, false)] same_instance: bool) {
    // A private library 1 holding 西 under `xi` and 安 under `an`.
    let scratch = tempfile::tempdir().expect("scratch");
    let xi = oxpinyin_core::ChewingKey::from_pinyin("xi")
        .expect("frozen syllable")
        .with_tone(1)
        .to_packed();
    let an = oxpinyin_core::ChewingKey::from_pinyin("an")
        .expect("frozen syllable")
        .with_tone(1)
        .to_packed();
    let system = crate::test_support::items_system(
        &crate::test_support::system_dir(),
        scratch.path(),
        &[(&[0x897f], &[xi], 100), (&[0x5b89], &[an], 100)],
        1000,
    );
    let luoma_dir = TempUserDir::new("divided-luoma");
    let (luoma_context, luoma) = divided(&system, &luoma_dir, 2, "sian");
    let hanyu_dir = TempUserDir::new("divided-hanyu");
    let (hanyu_context, hanyu) = divided(&system, &hanyu_dir, 1, "xian");
    let producer = if same_instance { luoma } else { hanyu };
    assert!(pinyin_guess_candidates(producer, 0, DEFAULT_SORT));
    let row = find_row(producer, |kind, text| {
        kind == lookup_candidate_type_t::NORMAL_CANDIDATE && text == "西"
    });
    assert_eq!(pinyin_choose_candidate(luoma, 0, row), 2);
    assert!(!pinyin_clear_constraint(luoma, 2));
    assert!(pinyin_clear_constraint(luoma, 0));
    // The Hanyu producer publishes the complementary divided [2, 4) span.
    assert!(pinyin_guess_candidates(hanyu, 2, DEFAULT_SORT));
    let row = find_row(hanyu, |kind, text| {
        kind == lookup_candidate_type_t::NORMAL_CANDIDATE && text == "安"
    });
    assert_eq!(pinyin_choose_candidate(luoma, 2, row), 4);
    assert!(!pinyin_clear_constraint(luoma, 0));
    assert!(pinyin_clear_constraint(luoma, 2));
    for (context, instance) in [(luoma_context, luoma), (hanyu_context, hanyu)] {
        pinyin_free_instance(instance);
        pinyin_fini(context);
    }
}
