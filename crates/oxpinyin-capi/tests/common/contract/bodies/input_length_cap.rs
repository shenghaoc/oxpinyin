//! The full-pinyin input length cap (#540, #1501).

use std::collections::BTreeMap;

use crate::facade;
use crate::readers::{UNTOUCHED_TEXT, c};
use crate::support::{Kit, UNTOUCHED};

#[derive(Debug, PartialEq)]
pub struct LongInput {
    pub guess: bool,
    pub parsed: usize,
    /// The sentence accessor's answer and the sentence's length in characters.
    pub sentence: (bool, Option<usize>),
}

/// Parses 4,800 bytes of `ni` on a fresh instance, guesses, and reads sentence 0
/// as a length.
pub fn long_input_sentence(k: &mut Kit) -> LongInput {
    let inst = k.alloc();
    let text = b"ni".repeat(2400);
    let parsed = facade::parse_more_full_pinyins(inst, c(&text).as_ptr());
    let guess = facade::guess_sentence(inst);
    let mut out = UNTOUCHED as usize as *mut std::ffi::c_char;
    let ret = facade::get_sentence_at(inst, 0, &raw mut out);
    let length = if out as usize == UNTOUCHED as usize {
        Some(UNTOUCHED_TEXT.chars().count())
    } else {
        crate::support::take_text(out).map(|text| text.chars().count())
    };
    LongInput {
        guess,
        parsed,
        sentence: (ret, length),
    }
}

/// Parses `length` bytes of repeated `ni` on a fresh instance: the parse's answer
/// and the stored parsed length, under the length's decimal label.
pub fn boundary(k: &mut Kit, length: usize) -> BTreeMap<String, (usize, usize)> {
    let inst = k.alloc();
    let mut text = b"ni".repeat(length.div_ceil(2));
    text.truncate(length);
    let parsed = facade::parse_more_full_pinyins(inst, c(&text).as_ptr());
    BTreeMap::from([(
        length.to_string(),
        (parsed, facade::get_parsed_input_length(inst)),
    )])
}
