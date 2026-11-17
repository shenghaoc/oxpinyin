//! `zhuyin_choose_candidate` with a candidate another live instance
//! published, through the C ABI only.
//!
//! The pin reads the row's type, token and bounds and never checks which
//! instance published it (074a2219 `zhuyin.cpp:1635-1666`): `BEST_MATCH`
//! answers the receiver's `matrix.size() - 1`, and a normal row forces its
//! span on the receiver and answers its own `m_end` (after the cursor) or
//! `m_begin` (before it). Each case runs with both instances on one
//! context and again on two contexts over the same system data.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::candidates::{
    zhuyin_choose_candidate, zhuyin_clear_constraint, zhuyin_get_candidate,
    zhuyin_get_candidate_type, zhuyin_get_n_candidate,
};
use crate::context::zhuyin_fini;
use crate::instance::{zhuyin_alloc_instance, zhuyin_free_instance};
use crate::parse::zhuyin_parse_more_chewings;
use crate::sentence::{
    zhuyin_guess_candidates_after_cursor, zhuyin_guess_candidates_before_cursor,
    zhuyin_guess_sentence,
};
use crate::test_support::{cstr, open};
use crate::types::{LookupCandidate, ZhuyinContext, ZhuyinInstance, lookup_candidate_type_t};

/// A producing and a receiving instance, on one context or on two.
struct Pair {
    contexts: Vec<*mut ZhuyinContext>,
    producer: *mut ZhuyinInstance,
    receiver: *mut ZhuyinInstance,
}

impl Pair {
    fn open(same_context: bool) -> Self {
        let (context, producer) = open();
        if same_context {
            let receiver = unsafe { zhuyin_alloc_instance(context) };
            assert!(!receiver.is_null());
            return Self {
                contexts: vec![context],
                producer,
                receiver,
            };
        }
        let (other, receiver) = open();
        Self {
            contexts: vec![context, other],
            producer,
            receiver,
        }
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        unsafe { zhuyin_free_instance(self.producer) };
        unsafe { zhuyin_free_instance(self.receiver) };
        for context in &self.contexts {
            unsafe { zhuyin_fini(*context) };
        }
    }
}

fn parse(instance: *mut ZhuyinInstance, input: &str) {
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, cstr(input).as_ptr()) },
        input.len()
    );
}

/// Every row of `instance`'s list with its type.
fn rows(instance: *mut ZhuyinInstance) -> Vec<(*mut LookupCandidate, lookup_candidate_type_t)> {
    let mut count = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    (0..count)
        .map(|index| {
            let mut row = std::ptr::null_mut();
            assert!(unsafe { zhuyin_get_candidate(instance, index, &raw mut row) });
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(unsafe { zhuyin_get_candidate_type(instance, row, &raw mut kind) });
            (row, kind)
        })
        .collect()
}

/// A `BEST_MATCH` row answers the receiver's matrix end and forces nothing
/// (`zhuyin.cpp:1643-1644`).
#[rstest::rstest]
fn a_best_match_row_answers_the_receivers_matrix_end(#[values(true, false)] same_context: bool) {
    let pair = Pair::open(same_context);
    parse(pair.producer, "su3cl3");
    assert!(unsafe { zhuyin_guess_sentence(pair.producer) });
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(pair.producer, 0) });
    let (row, _) = rows(pair.producer)
        .into_iter()
        .find(|(_, kind)| *kind == lookup_candidate_type_t::BEST_MATCH_CANDIDATE)
        .expect("a sentence row");
    parse(pair.receiver, "su3");
    assert_eq!(unsafe { zhuyin_choose_candidate(pair.receiver, 0, row) }, 3);
    assert!(!unsafe { zhuyin_clear_constraint(pair.receiver, 0) });
}

/// An after-cursor row forces its span on the receiver and answers its own
/// `m_end`; the producer is untouched (`zhuyin.cpp:1649-1654`, `:1664-1666`).
/// On a receiver whose parse is shorter than the span, `add_constraint`
/// refuses it and the answer is still `m_end`. On a receiver whose keys the
/// row cannot spell, the pin's second `validate_constraint` after
/// `add_constraint` (`zhuyin.cpp:1662`, `phonetic_lookup.cpp:120-168`) drops
/// the forcing: a one-key row `[0, 3)` on a receiver parsing `a83` (ㄇㄚˇ)
/// answers its `m_end` and forces nothing.
#[rstest::rstest]
fn an_after_cursor_row_forces_the_receiver_and_answers_its_end(
    #[values(true, false)] same_context: bool,
) {
    let pair = Pair::open(same_context);
    parse(pair.producer, "su3cl3");
    parse(pair.receiver, "su3cl3");
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(pair.producer, 0) });
    let after: Vec<_> = rows(pair.producer)
        .into_iter()
        .filter(|(_, kind)| *kind == lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR)
        .map(|(row, _)| row)
        .collect();
    assert!(!after.is_empty());
    // The pin answers the row's own `m_end` whatever the caller's offset.
    let ends: Vec<_> = after
        .iter()
        .map(|&row| unsafe { zhuyin_choose_candidate(pair.receiver, 9, row) })
        .collect();
    assert!(ends.iter().all(|&end| end == 3 || end == 6), "{ends:?}");
    assert!(unsafe { zhuyin_clear_constraint(pair.receiver, 0) });
    assert!(!unsafe { zhuyin_clear_constraint(pair.producer, 0) });

    let whole = after[ends
        .iter()
        .position(|&end| end == 6)
        .expect("a two-key row")];
    let short = unsafe { zhuyin_alloc_instance(*pair.contexts.last().expect("a context")) };
    parse(short, "su3");
    assert_eq!(unsafe { zhuyin_choose_candidate(short, 0, whole) }, 6);
    assert!(!unsafe { zhuyin_clear_constraint(short, 0) });
    unsafe { zhuyin_free_instance(short) };

    let one_key = after[ends
        .iter()
        .position(|&end| end == 3)
        .expect("a one-key row")];
    let unspellable = unsafe { zhuyin_alloc_instance(*pair.contexts.last().expect("a context")) };
    parse(unspellable, "a83");
    assert_eq!(
        unsafe { zhuyin_choose_candidate(unspellable, 0, one_key) },
        3
    );
    assert!(!unsafe { zhuyin_clear_constraint(unspellable, 0) });
    unsafe { zhuyin_free_instance(unspellable) };
}

/// A before-cursor row answers its own `m_begin` on the receiver
/// (`zhuyin.cpp:1656-1661`).
#[rstest::rstest]
fn a_before_cursor_row_answers_its_begin(#[values(true, false)] same_context: bool) {
    let pair = Pair::open(same_context);
    parse(pair.producer, "su3cl3");
    parse(pair.receiver, "su3cl3");
    assert!(unsafe { zhuyin_guess_candidates_before_cursor(pair.producer, 6) });
    let begins: Vec<_> = rows(pair.producer)
        .into_iter()
        .filter(|(_, kind)| *kind == lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR)
        .map(|(row, _)| unsafe { zhuyin_choose_candidate(pair.receiver, 9, row) })
        .collect();
    assert!(!begins.is_empty());
    assert!(
        begins.iter().all(|&begin| begin == 0 || begin == 3),
        "{begins:?}"
    );
    assert!(!unsafe { zhuyin_clear_constraint(pair.producer, 0) });
    assert!(!unsafe { zhuyin_clear_constraint(pair.producer, 3) });
}
