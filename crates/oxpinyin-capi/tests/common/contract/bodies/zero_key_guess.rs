//! The zero_key_guess family's case bodies: each answers the result document the old case returned.
//!
//! #902: separators consume bytes without keys (074a2219,
//! `storage/pinyin_parser2.cpp:238-250,360-378`). `fill_matrix` clears the
//! matrix before refusing zero keys (`storage/phonetic_key_matrix.cpp:34-38`).
//! Sentence lookup refuses before clearing results (`lookup/phonetic_lookup.h:743-748`);
//! candidate lookup clears candidates before refusing (`pinyin.cpp:2193-2196`;
//! `zhuyin.cpp:1468-1471,1548-1551`). Each export is observed both fresh and
//! after a successful guess.

use crate::facade;
use crate::readers::{c, owned_out, seeded};
use crate::support::{J, Kit, P, UNTOUCHED};

/// The selected guess export on `inst`.
fn operate(inst: P, operation: &str) -> bool {
    match operation {
        "guess_sentence" => facade::guess_sentence(inst),
        "guess_sentence_with_prefix" => facade::guess_sentence_with_prefix(inst, c("你").as_ptr()),
        "guess_candidates" => facade::guess_candidates(inst, 0, 0x1e),
        "guess_candidates_after_cursor" => facade::guess_candidates_after_cursor(inst, 0),
        "guess_candidates_before_cursor" => facade::guess_candidates_before_cursor(inst, 0),
        other => panic!("no operation {other}"),
    }
}

/// One condition's parse, guess, sentence and candidate count.
fn condition(k: &mut Kit, text: &[u8], operation: &str, seeded_first: bool) -> J {
    let inst = k.alloc();
    if seeded_first {
        facade::parse_more_full_pinyins(inst, c("ni3hao3").as_ptr());
        assert!(facade::guess_sentence(inst));
    }
    let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    let ret = operate(inst, operation);
    let mut sentence = seeded();
    let got_sentence = if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut sentence)
    } else {
        facade::get_sentence_at(inst, 0, &raw mut sentence)
    };
    let mut count = UNTOUCHED;
    let got_count = facade::get_n_candidate(inst, &raw mut count);
    obj! {
        "consumed" => parsed,
        "parsed_length" => facade::get_parsed_input_length(inst),
        "ret" => ret,
        "sentence" => (got_sentence, owned_out(sentence)),
        "candidates" => (got_count, count),
    }
}

/// `zero_key_guess_case(text, options, operation)`.
fn probe(k: &mut Kit, text: &[u8], options: Option<u32>, operation: &str) -> J {
    if let Some(options) = options {
        let ctx = k.ctx();
        assert!(facade::set_options(ctx, options));
    }
    let mut out = J::map();
    for seeded_first in [false, true] {
        let result = condition(k, text, operation, seeded_first);
        out.set(if seeded_first { "seeded" } else { "fresh" }, result);
    }
    out
}

/// `zero-key-guess/{mode}/{options}/{text}/{operation}`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let parts: Vec<&str> = id.split('/').collect();
    let ["zero-key-guess", mode, label, text, operation] = parts.as_slice() else {
        panic!("{id}: not a zero-key-guess case")
    };
    assert_eq!(
        *mode == "zhuyin",
        facade::ZHUYIN,
        "{id}: another facade's case"
    );
    let options = match *label {
        "default" => None,
        hex => Some(
            u32::from_str_radix(hex.strip_prefix("0x").expect("hex options"), 16)
                .expect("hex options"),
        ),
    };
    probe(k, text.as_bytes(), options, operation)
}
