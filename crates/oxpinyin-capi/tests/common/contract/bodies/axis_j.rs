//! Axis J: an n-best choice and the multi-key chewing keyboards.

use crate::facade;
use crate::readers::{c, row_of};
use crate::support::Kit;

#[derive(Debug, PartialEq)]
pub struct NbestChoice {
    pub ranked: bool,
    pub rank: u8,
    pub chosen: i32,
}

/// Chooses the displayed n-best row after a sentence and candidate guess.
pub fn choose_nbest(k: &mut Kit) -> NbestChoice {
    let inst = k.alloc();
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    facade::guess_sentence(inst);
    facade::guess_candidates(inst, 0, 0);
    let candidate = row_of(inst, 1).expect("no displayed NBEST candidate");
    let mut rank = 255_u8;
    let ranked = facade::get_candidate_nbest_index(inst, candidate, &raw mut rank);
    let chosen = facade::choose_candidate(inst, 0, candidate);
    NbestChoice {
        ranked,
        rank,
        chosen,
    }
}

#[derive(Debug, PartialEq)]
pub struct MultiKey {
    pub scheme: i32,
    pub selected: bool,
    pub parsed: usize,
    pub length: usize,
}

/// Selects a chewing keyboard (the pinyin facade's `set_zhuyin_scheme`, the
/// zhuyin facade's `set_chewing_scheme`) and parses `abc`.
pub fn multi_chewing(k: &mut Kit, scheme: i32) -> MultiKey {
    let ctx = k.ctx();
    let selected = if facade::ZHUYIN {
        facade::set_chewing_scheme(ctx, scheme)
    } else {
        facade::set_zhuyin_scheme(ctx, scheme)
    };
    let inst = k.inst();
    let parsed = facade::parse_more_chewings(inst, c("abc").as_ptr());
    let length = facade::get_parsed_input_length(inst);
    MultiKey {
        scheme,
        selected,
        parsed,
        length,
    }
}
