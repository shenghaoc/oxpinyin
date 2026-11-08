//! The row54_empty_forcing family's case bodies: each answers the result document the old case returned.
//!
//! Register row 54 (#695, #709): a parse replaces the matrix even for empty
//! input, every candidate guess searches the libraries loaded now without a
//! reparse, and a non-first NORMAL choice survives an empty parse until a
//! sentence guess validates the constraints against it.

use std::ffi::c_char;

use crate::facade;
use crate::readers::{c, owned_out, seeded};
use crate::support::{J, Kit, P, UNTOUCHED, borrowed_text};

/// A parse entry point.
type Parse = fn(P, *const c_char) -> usize;

/// `row54_empty`: three parse/guess pairs on one instance, the middle one empty.
fn empty(k: &mut Kit, parse: Parse, text: &str) -> J {
    facade::set_options(k.ctx(), 1 << 5);
    let inst = k.inst();
    let mut out = J::map();
    out.set("first_parse", parse(inst, c(text).as_ptr()));
    out.set("first_guess", facade::guess_sentence(inst));
    out.set("empty_parse", parse(inst, c("").as_ptr()));
    out.set("empty_guess", facade::guess_sentence(inst));
    out.set("reparse", parse(inst, c(text).as_ptr()));
    out.set("recovered_guess", facade::guess_sentence(inst));
    out
}

/// `row54_unload`'s `rows`: a guess and the list's count and strings.
fn unload_rows(inst: P, before: bool) -> J {
    let ret = if !facade::ZHUYIN {
        facade::guess_candidates(inst, 0, 0x1e)
    } else if before {
        facade::guess_candidates_before_cursor(inst, 5)
    } else {
        facade::guess_candidates_after_cursor(inst, 0)
    };
    let mut count = UNTOUCHED;
    let counted = facade::get_n_candidate(inst, &raw mut count);
    let strings: Vec<String> = (0..count)
        .map(|index| {
            let mut cand: P = std::ptr::null_mut();
            assert!(facade::get_candidate(inst, index, &raw mut cand));
            let mut text: *const c_char = std::ptr::null();
            assert!(facade::get_candidate_string(inst, cand, &raw mut text));
            borrowed_text(text).expect("candidate text")
        })
        .collect();
    J::from((ret, counted, count, strings))
}

/// `row54_unload`: candidate lists around unloading library 2.
fn unload(k: &mut Kit, before: bool, reparse: bool) -> J {
    facade::set_options(k.ctx(), 1 << 5);
    let inst = k.inst();
    let mut out = obj! { "parse" => facade::parse_more_full_pinyins(inst, c("nihao").as_ptr()) };
    out.set("before", unload_rows(inst, before));
    out.set("unload", facade::unload_phrase_library(k.ctx(), 2));
    if reparse {
        out.set(
            "reparse",
            facade::parse_more_full_pinyins(inst, c("nihao").as_ptr()),
        );
    }
    out.set("after", unload_rows(inst, before));
    out.set("repeated", unload_rows(inst, before));
    out
}

/// The first sentence and its flag, the untouched output kept.
fn sentence(inst: P) -> J {
    let mut text = seeded();
    let ret = if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut text)
    } else {
        facade::get_sentence_at(inst, 0, &raw mut text)
    };
    J::from((ret, owned_out(text)))
}

/// Chooses the first non-first NORMAL 泥 row, recording its index and text.
fn choose_ni(inst: P, out: &mut J) -> P {
    let mut count = 0_u32;
    assert!(facade::get_n_candidate(inst, &raw mut count));
    for index in 0..count {
        let mut cand: P = std::ptr::null_mut();
        assert!(facade::get_candidate(inst, index, &raw mut cand));
        let mut text: *const c_char = std::ptr::null();
        assert!(facade::get_candidate_string(inst, cand, &raw mut text));
        let mut kind = 0_i32;
        assert!(facade::get_candidate_type(inst, cand, &raw mut kind));
        let text = borrowed_text(text);
        if kind == 2 && text.as_deref() == Some("泥") {
            assert!(index > 0, "the chosen NORMAL candidate must not be first");
            out.set("chosen_index", index);
            out.set("chosen_text", text);
            return cand;
        }
    }
    panic!("missing non-first NORMAL candidate 泥");
}

/// `row54_forcing`: a non-first NORMAL 泥 choice, then the empty-parse `sequence`.
fn forcing(k: &mut Kit, sequence: &str) -> J {
    facade::set_options(k.ctx(), 1 << 5);
    let inst = k.inst();
    let mut out = obj! { "parse" => facade::parse_more_full_pinyins(inst, c("nihao").as_ptr()) };
    out.set("initial_guess", facade::guess_sentence(inst));
    out.set("initial_sentence", sentence(inst));
    out.set(
        "candidates",
        if facade::ZHUYIN {
            facade::guess_candidates_after_cursor(inst, 0)
        } else {
            facade::guess_candidates(inst, 0, 0x1e)
        },
    );
    let chosen = choose_ni(inst, &mut out);
    out.set("choose", facade::choose_candidate(inst, 0, chosen));
    if sequence != "no-empty" {
        out.set(
            "empty_parse",
            facade::parse_more_full_pinyins(inst, c("").as_ptr()),
        );
        if sequence == "empty-guess" {
            out.set("empty_guess", facade::guess_sentence(inst));
        }
    }
    out.set(
        "restored_parse",
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr()),
    );
    out.set("restored_guess", facade::guess_sentence(inst));
    out.set("restored_sentence", sentence(inst));
    out
}

/// Runs the case `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let mode = if facade::ZHUYIN { "zhuyin" } else { "pinyin" };
    let rest = id
        .strip_prefix("row54-")
        .and_then(|rest| {
            rest.split_once('-')
                .filter(|(_, tail)| tail.starts_with(mode))
                .map(|(kind, tail)| (kind, &tail[mode.len()..]))
        })
        .unwrap_or_else(|| panic!("no case {id}"));
    match rest {
        ("empty", "-full_pinyins") => empty(k, facade::parse_more_full_pinyins, "nihao"),
        ("empty", "-chewings") => empty(k, facade::parse_more_chewings, "su3cl3"),
        ("empty", "-double") => empty(k, facade::parse_more_double_pinyins, "nihk"),
        ("unload", "") => unload(k, false, false),
        ("unload", "-reparse") => unload(k, false, true),
        ("unload", "-before") => unload(k, true, false),
        ("forcing", sequence) => forcing(
            k,
            sequence
                .strip_prefix('-')
                .unwrap_or_else(|| panic!("no case {id}")),
        ),
        _ => panic!("no case {id}"),
    }
}
