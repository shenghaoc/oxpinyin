//! The changed_parse_constraints family's case bodies: each answers the result document the old case returned.
//!
//! #710: parsing never touches `m_constraints`. At 074a2219,
//! `lookup/phonetic_lookup.cpp:120-160` first resizes to `matrix.size()`, then
//! clears only ONESTEP spans ending at or past the size or whose pronunciation
//! possibility is below `FLT_EPSILON`. A guess (or a choose) validates; a parse
//! alone, including an empty or temporarily incompatible parse, does not.

use crate::facade;
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, n_candidate, owned_out, seeded,
};
use crate::support::{J, Kit, P};

/// One case's inputs: the first parse, the chosen `(offset, text)` rows, the
/// replacement parse and the optional intermediate parse.
struct Spec {
    initial: &'static str,
    choices: Choices,
    replacement: &'static str,
    intermediate: Option<&'static str>,
}

/// The edit shapes: name, first parse, choices and replacement.
/// Chosen rows: each `(byte offset, NORMAL-candidate text)`.
type Choices = &'static [(usize, &'static str)];

const SHAPES: [(&str, &str, Choices, &str); 9] = [
    ("changed-suffix", "nihao", &[(0, "泥")], "nihai"),
    ("shorter", "nihao", &[(0, "泥")], "ni"),
    ("longer", "nihao", &[(0, "泥")], "nihaoma"),
    ("different-first", "nihao", &[(0, "泥")], "nahao"),
    ("span-past-end", "nihao", &[(2, "浩")], "ni"),
    ("whole-span-past-end", "nihaoshijie", &[(0, "你好")], "ni"),
    (
        "edit-within-span",
        "nihaoshijie",
        &[(0, "你好")],
        "nihaishijie",
    ),
    (
        "whole-span-kept",
        "nihaoshijie",
        &[(0, "你好")],
        "nihaoshijian",
    ),
    ("two-choices", "nihao", &[(0, "泥"), (2, "浩")], "nihai"),
];

/// The temporary shapes: name and intermediate parse.
const TEMPORARY: [(&str, &str); 2] = [("temporary-mismatch", "nahao"), ("temporary-shorter", "n")];

/// The parse sequences, longest first so a suffix match takes the whole name.
const SEQUENCES: [&str; 5] = [
    "intermediate-guess",
    "empty-guess",
    "direct",
    "guess",
    "empty",
];

/// The instance's first sentence, the sentinel kept and owned text freed.
fn sentence(inst: P) -> J {
    let mut text = seeded();
    let ret = if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut text)
    } else {
        facade::get_sentence_at(inst, 0, &raw mut text)
    };
    J::from((ret, owned_out(text)))
}

/// `choose`: chooses the first NORMAL row `wanted` at `offset`.
fn choose(inst: P, offset: usize, wanted: &str) -> J {
    if facade::ZHUYIN {
        assert!(facade::guess_candidates_after_cursor(inst, offset));
    } else {
        assert!(facade::guess_candidates(inst, offset, 0x1e));
    }
    for index in 0..n_candidate(inst) {
        let cand = candidate_at(inst, index).expect("get_candidate");
        let text = candidate_text(inst, cand).expect("get_candidate_string");
        if candidate_type(inst, cand) == 2 && text == wanted {
            return J::from((wanted, facade::choose_candidate(inst, offset, cand)));
        }
    }
    panic!("missing NORMAL candidate: {wanted}")
}

/// `changed_parse_constraints(...)`'s probe.
fn probe(k: &mut Kit, spec: &Spec, sequence: &str) -> J {
    let ctx = k.ctx();
    assert!(facade::set_options(ctx, 1 << 5));
    let inst = k.inst();
    let parse = |text: &str| facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    let guess = || facade::guess_sentence(inst);
    let mut out = J::map();
    out.set("parse", parse(spec.initial));
    out.set("initial_guess", guess());
    out.set("initial_sentence", sentence(inst));
    let choices: Vec<J> = spec
        .choices
        .iter()
        .map(|(offset, text)| choose(inst, *offset, text))
        .collect();
    out.set("choices", choices);
    if sequence == "guess" {
        out.set("chosen_guess", guess());
        out.set("chosen_sentence", sentence(inst));
    }
    if sequence == "empty" || sequence == "empty-guess" {
        out.set("empty_parse", parse(""));
        if sequence == "empty-guess" {
            out.set("empty_guess", guess());
        }
    }
    if let Some(intermediate) = spec.intermediate {
        out.set("intermediate_parse", parse(intermediate));
        if sequence == "intermediate-guess" {
            out.set("intermediate_guess", guess());
            out.set("intermediate_sentence", sentence(inst));
        }
    }
    out.set("replacement_parse", parse(spec.replacement));
    out.set("replacement_guess", guess());
    out.set("replacement_sentence", sentence(inst));
    let cleared: Vec<bool> = spec
        .choices
        .iter()
        .map(|(offset, _)| facade::clear_constraint(inst, *offset))
        .collect();
    out.set("cleared_constraints", cleared);
    out
}

/// The spec a shape name names.
fn spec_of(name: &str) -> Spec {
    if let Some((_, intermediate)) = TEMPORARY.iter().find(|(shape, _)| *shape == name) {
        return Spec {
            initial: "nihao",
            choices: &[(0, "泥")],
            replacement: "nihai",
            intermediate: Some(intermediate),
        };
    }
    let (_, initial, choices, replacement) = SHAPES
        .iter()
        .find(|(shape, ..)| *shape == name)
        .unwrap_or_else(|| panic!("no shape {name}"));
    Spec {
        initial,
        choices,
        replacement,
        intermediate: None,
    }
}

/// `parse-constraints-{mode}-{name}-{sequence}`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let mode = if facade::ZHUYIN { "zhuyin" } else { "pinyin" };
    let rest = id
        .strip_prefix(&format!("parse-constraints-{mode}-"))
        .unwrap_or_else(|| panic!("{id}: not this facade's case"));
    let (name, sequence) = SEQUENCES
        .iter()
        .find_map(|sequence| {
            rest.strip_suffix(sequence)
                .and_then(|name| name.strip_suffix('-'))
                .map(|name| (name, *sequence))
        })
        .unwrap_or_else(|| panic!("{id}: no sequence"));
    probe(k, &spec_of(name), sequence)
}
