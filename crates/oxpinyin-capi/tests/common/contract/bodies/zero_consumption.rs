//! The zero_consumption family's case bodies: each answers the result document the old case returned.
//!
//! #710, row 53 item 2: every zero-consumption parse leaves an empty matrix.
//! `get_nbest_match` returns false before clearing sentence rows (074a2219,
//! `phonetic_lookup.h:743-748`), including the prefix-seeded path.

use std::ffi::c_char;

use crate::facade;
use crate::readers::{c, owned_out, seeded};
use crate::support::{J, Kit, P};

/// The instance's first sentence (`sentence`): the return and the text, the
/// sentinel kept and owned text freed.
fn sentence(inst: P) -> J {
    let mut value = seeded();
    let ret = if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut value)
    } else {
        facade::get_sentence_at(inst, 0, &raw mut value)
    };
    J::from((ret, owned_out(value)))
}

/// The `parse_more_<parser>` export.
fn parser_of(parser: &str) -> fn(P, *const c_char) -> usize {
    match parser {
        "full_pinyins" => facade::parse_more_full_pinyins,
        "chewings" => facade::parse_more_chewings,
        "double_pinyins" => facade::parse_more_double_pinyins,
        other => panic!("no parser {other}"),
    }
}

/// `zero_consumption_sentence(parser, text, with_prefix)`: fresh instances and
/// ones holding a nihao sentence, each after a zero-consumption parse.
fn probe(k: &mut Kit, parser: &str, text: &[u8], with_prefix: bool) -> J {
    let ctx = k.ctx();
    assert!(facade::set_options(ctx, 1 << 5));
    let parse = parser_of(parser);
    let mut out = J::map();
    for stale in [false, true] {
        let inst = k.alloc();
        let mut result = J::map();
        if stale {
            result.set(
                "initial_parse",
                facade::parse_more_full_pinyins(inst, c("nihao").as_ptr()),
            );
            result.set("initial_guess", facade::guess_sentence(inst));
            result.set("initial_sentence", sentence(inst));
        }
        let parsed = parse(inst, c(text).as_ptr());
        assert_eq!(parsed, 0, "the case must leave an empty matrix");
        result.set("parse", parsed);
        let guess = if with_prefix {
            facade::guess_sentence_with_prefix(inst, c("你").as_ptr())
        } else {
            facade::guess_sentence(inst)
        };
        result.set("guess", guess);
        result.set("sentence", sentence(inst));
        out.set(if stale { "stale" } else { "fresh" }, result);
    }
    out
}

/// `zero-consumption-{mode}-{parser}-{label}[-prefix]`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let mode = if facade::ZHUYIN { "zhuyin" } else { "pinyin" };
    let rest = id
        .strip_prefix(&format!("zero-consumption-{mode}-"))
        .unwrap_or_else(|| panic!("{id}: not this facade's case"));
    let (rest, with_prefix) = match rest.strip_suffix("-prefix") {
        Some(rest) => (rest, true),
        None => (rest, false),
    };
    let (parser, label) = rest.rsplit_once('-').expect("parser and label");
    let text: &[u8] = match label {
        "empty" => b"",
        other => other.as_bytes(),
    };
    probe(k, parser, text, with_prefix)
}
