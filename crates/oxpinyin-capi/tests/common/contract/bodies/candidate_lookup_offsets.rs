//! Candidate lookups at every byte offset (#696, register row 65).

use std::collections::BTreeMap;

use crate::facade;
pub use crate::readers::GuessRows;
use crate::readers::{c, guess_rows, zhuyin_guess_rows};
use crate::support::Kit;

const INPUTS: [&[u8]; 10] = [
    b"ni",
    b"n",
    b"nihao",
    b"nih",
    b"ni'hao",
    b"hao",
    b"women",
    b"zhongguo",
    b"woshi",
    b"xian",
];
const SORTS: [u32; 3] = [0, 0x1c, 0x1e];
const ZHUYIN_INPUTS: [&[u8]; 10] = [
    b"su3", b"su", b"su3cl3", b"su3cl", b"su3'cl3", b"cl3", b"ji3", b"ji3cl3", b"zhong1", b"xian1",
];

/// For each input, with and without a prior sentence guess, every sort word and
/// every offset from 0 through the reserved slot and one past it.
pub fn lookup_offset(k: &mut Kit) -> BTreeMap<String, GuessRows> {
    let mut out = BTreeMap::new();
    for text in INPUTS {
        for previous in [0, 1] {
            let inst = k.alloc();
            let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
            if previous == 1 {
                facade::guess_sentence(inst);
            }
            for sort in SORTS {
                for offset in 0..parsed + 2 {
                    let label = format!(
                        "{} prev={previous} sort={sort:#x} off={offset}",
                        String::from_utf8_lossy(text)
                    );
                    out.insert(label, guess_rows(inst, offset, sort));
                }
            }
        }
    }
    out
}

/// The zhuyin walk: offsets from 0 through the reserved final column.
pub fn lookup_offset_zhuyin(k: &mut Kit) -> BTreeMap<String, GuessRows> {
    let mut out = BTreeMap::new();
    for text in ZHUYIN_INPUTS {
        for previous in [0, 1] {
            let inst = k.alloc();
            let parsed = facade::parse_more_chewings(inst, c(text).as_ptr());
            if previous == 1 {
                facade::guess_sentence(inst);
            }
            for offset in 0..=parsed {
                let label = format!(
                    "{} prev={previous} off={offset}",
                    String::from_utf8_lossy(text)
                );
                out.insert(label, zhuyin_guess_rows(inst, offset));
            }
        }
    }
    out
}
