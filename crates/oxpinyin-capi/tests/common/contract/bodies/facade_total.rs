//! A facade total wrapped to zero (#540, register row 61).

use crate::facade;
use crate::readers::{c, n_candidate};
use crate::support::{Kit, P};

/// The loaded libraries' totals in the pinned data, summed.
const FACADE_TOTAL: u32 = 51_051_831;

/// Adds the delta that wraps the facade's `guint32` total to zero
/// (`phrase_index.h:628-634` raises it before the sub-index refuses).
fn wrap_total_to_zero(inst: P) -> bool {
    facade::token_add_unigram_frequency(inst, (1 << 24) | 1, FACADE_TOTAL.wrapping_neg())
}

#[derive(Debug, PartialEq)]
pub struct NoRank {
    pub ret: bool,
    pub rows: u32,
}

/// A guess whose window ranks no phrase row answers with no abort.
pub fn no_rank(k: &mut Kit, offset: usize, sort: u32) -> NoRank {
    let inst = k.alloc();
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    wrap_total_to_zero(inst);
    let ret = facade::guess_candidates(inst, offset, sort);
    NoRank {
        ret,
        rows: n_candidate(inst),
    }
}

/// Ranking a searched row with a zero total: `assert(0 < total_freq)`
/// (`pinyin.cpp:1859`).
pub fn zero_total(k: &mut Kit) {
    let inst = k.inst();
    wrap_total_to_zero(inst);
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    facade::guess_candidates(inst, 0, 0x1e);
}
