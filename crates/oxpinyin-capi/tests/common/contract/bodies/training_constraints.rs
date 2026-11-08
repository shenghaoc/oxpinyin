//! The training_constraints family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 40 (#1175, #525, #825, register rows 6 and 82): training after
//! a stale forcing, past the results or after a re-parse that shrank the matrix
//! (`phonetic_lookup.h:858-921`, `phonetic_key_matrix.cpp:661-664`), and forced
//! phrases over abbreviations whose completions pass the expansion limit
//! (`phonetic_lookup.cpp:120-165`).

use std::ffi::c_char;

use crate::facade;
use crate::readers::{c, full_inst, n_candidate, owned_out, seeded, tokens_of};
use crate::support::{J, Kit, P, borrowed_text};

/// `CONSTRAINT_OPTIONS`: incomplete pinyin.
const CONSTRAINT_OPTIONS: u32 = 0x18A;
/// `CONSTRAINT_SORT`.
const CONSTRAINT_SORT: u32 = 0x1E;
/// `ZHUYIN_REPARSE_OPTIONS`.
const ZHUYIN_REPARSE_OPTIONS: u32 = 0x18a;

/// One phrase import row: text, reading and count.
type Phrase = (&'static str, &'static str, i32);

const PHRASES: [Phrase; 2] = [
    ("中华人民", "zhong1'hua2'ren2'min2", 50),
    ("种花人民", "zhong3'hua1'ren2'min2", 1),
];
const PHRASES_LONG: [Phrase; 2] = [
    ("中华人民共和国", "zhong1'hua2'ren2'min2'gong4'he2'guo2", 50),
    ("中华人民公会国", "zhong1'hua2'ren2'min2'gong1'hui4'guo2", 1),
];
const PHRASES_V_U: [Phrase; 2] = [
    ("决中华人民", "jue2'zhong1'hua2'ren2'min2", 50),
    ("绝种花人民", "jue2'zhong3'hua1'ren2'min2", 1),
];

/// A candidate's pointer, type and borrowed text, accessor flags unchecked.
fn candidate(inst: P, index: u32) -> (P, i32, Option<String>) {
    let mut cand: P = std::ptr::null_mut();
    facade::get_candidate(inst, index, &raw mut cand);
    let mut kind = 0_i32;
    facade::get_candidate_type(inst, cand, &raw mut kind);
    let mut text: *const c_char = std::ptr::null();
    facade::get_candidate_string(inst, cand, &raw mut text);
    (cand, kind, borrowed_text(text))
}

/// `row_with_text`: the first candidate of `kind` reading `text`, or NULL.
fn row_with_text(inst: P, kind: i32, text: &str) -> P {
    (0..n_candidate(inst))
        .map(|index| candidate(inst, index))
        .find(|(_, found, string)| *found == kind && string.as_deref() == Some(text))
        .map_or(std::ptr::null_mut(), |(cand, _, _)| cand)
}

/// The first candidate reading `text`, of any kind, or NULL.
fn row_reading(inst: P, text: &str) -> P {
    (0..n_candidate(inst))
        .map(|index| candidate(inst, index))
        .find(|(_, _, string)| string.as_deref() == Some(text))
        .map_or(std::ptr::null_mut(), |(cand, _, _)| cand)
}

/// `sentence_out(k, inst, 0)`: the flag and the text or `"untouched"`.
fn sentence_out(inst: P) -> J {
    let mut out = seeded();
    let ret = facade::get_sentence_at(inst, 0, &raw mut out);
    J::from((ret, owned_out(out)))
}

/// `choose_text`: guesses at `offset` and chooses the first row reading `wanted`.
fn choose_text(inst: P, offset: usize, wanted: &str) -> Option<i32> {
    facade::guess_candidates(inst, offset, 0);
    let cand = row_reading(inst, wanted);
    (!cand.is_null()).then(|| facade::choose_candidate(inst, offset, cand))
}

/// `choose_normal`: guesses at `offset` and chooses the first NORMAL row reading `wanted`.
fn choose_normal(inst: P, offset: usize, wanted: &str) -> Option<i32> {
    if facade::ZHUYIN {
        facade::guess_candidates_after_cursor(inst, offset);
    } else {
        facade::guess_candidates(inst, offset, 0);
    }
    let cand = row_with_text(inst, 2, wanted);
    (!cand.is_null()).then(|| facade::choose_candidate(inst, offset, cand))
}

/// `forced_sentence`: `nihao` decoded, 泥 forced over `ni`, optionally looked up again.
fn forced_sentence(k: &mut Kit, relook: bool) -> (P, i32) {
    let inst = k.alloc();
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    facade::guess_sentence(inst);
    facade::guess_candidates(inst, 0, 0);
    let chosen = facade::choose_candidate(inst, 0, row_with_text(inst, 2, "泥"));
    if relook {
        facade::guess_sentence(inst);
    }
    (inst, chosen)
}

fn zhuyin_stale_forcing(k: &mut Kit) -> J {
    let inst = k.alloc();
    facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    facade::guess_sentence(inst);
    facade::guess_candidates_after_cursor(inst, 0);
    let chosen = row_reading(inst, "泥");
    facade::choose_candidate(inst, 0, chosen);
    obj! { "ret" => facade::train_top(inst) }
}

/// `candidate_heads`: the first six rows' kinds and texts, empty text as `None`.
fn candidate_heads(inst: P) -> Vec<(i32, Option<String>)> {
    (0..n_candidate(inst).min(6))
        .map(|index| {
            let (_, kind, text) = candidate(inst, index);
            (kind, text.filter(|text| !text.is_empty()))
        })
        .collect()
}

/// `constraint_instance`: imports, parses, guesses and chooses the second phrase.
fn constraint_instance(k: &mut Kit, phrases: &[Phrase; 2], parse: &str, options: u32) -> (P, J) {
    let mut out = obj! { "options" => facade::set_options(k.ctx(), options) };
    let it = facade::begin_add_phrases(k.ctx(), 7);
    assert!(!it.is_null());
    let added: Vec<bool> = phrases
        .iter()
        .map(|(text, reading, count)| {
            facade::iterator_add_phrase(it, c(text).as_ptr(), c(reading).as_ptr(), *count)
        })
        .collect();
    facade::end_add_phrases(it);
    out.set("import", added);
    let inst = k.alloc();
    out.set(
        "parse",
        facade::parse_more_full_pinyins(inst, c(parse).as_ptr()),
    );
    out.set("control guess", facade::guess_sentence(inst));
    out.set("control row 0", sentence_out(inst));
    out.set(
        "candidates",
        facade::guess_candidates(inst, 0, CONSTRAINT_SORT),
    );
    out.set("head", candidate_heads(inst));
    let wanted = phrases[1].0;
    let kind = if row_with_text(inst, 2, wanted).is_null() {
        1
    } else {
        2
    };
    out.set("chosen kind", kind);
    let row = row_with_text(inst, kind, wanted);
    assert!(!row.is_null(), "no row for {wanted}");
    out.set("choose", facade::choose_candidate(inst, 0, row));
    (inst, out)
}

fn constraint_survives(k: &mut Kit, parse: &str) -> J {
    let phrases = if parse == "zhhrmghg" {
        &PHRASES_LONG
    } else {
        &PHRASES
    };
    let (inst, mut out) = constraint_instance(k, phrases, parse, CONSTRAINT_OPTIONS);
    out.set("guess 5", facade::guess_sentence(inst));
    out.set("row 0 at 5", sentence_out(inst));
    out.set(
        "reparse",
        facade::parse_more_full_pinyins(inst, c(parse).as_ptr()),
    );
    out.set("guess 6", facade::guess_sentence(inst));
    out.set("row 0 at 6", sentence_out(inst));
    out
}

fn survives_reconcile(k: &mut Kit, parse: &str, replacement: &str) -> J {
    let (inst, mut out) = constraint_instance(k, &PHRASES_V_U, parse, CONSTRAINT_OPTIONS | 1 << 27);
    out.set(
        "reparse",
        facade::parse_more_full_pinyins(inst, c(replacement).as_ptr()),
    );
    out.set(
        "candidates after",
        facade::guess_candidates(inst, 0, CONSTRAINT_SORT),
    );
    out.set("head after", candidate_heads(inst));
    out.set("guess", facade::guess_sentence(inst));
    out.set("row 0", sentence_out(inst));
    out
}

fn stale_item(k: &mut Kit, first: &str, second: &str, parse: &str) -> J {
    let mut out = obj! { "options" => facade::set_options(k.ctx(), CONSTRAINT_OPTIONS) };
    let inst = k.alloc();
    out.set(
        "parse",
        facade::parse_more_full_pinyins(inst, c(parse).as_ptr()),
    );
    out.set("guess", facade::guess_sentence(inst));
    let chosen = choose_text(inst, 0, first);
    out.set("first", chosen);
    let offset = usize::try_from(chosen.expect("a first choice")).expect("an offset");
    out.set("second", choose_text(inst, offset, second));
    let tokens: Vec<String> = tokens_of(k, second)
        .iter()
        .map(|token| format!("{token:#x}"))
        .collect();
    out.set("tokens", tokens);
    out.set("guess loaded", facade::guess_sentence(inst));
    out.set("row 0 loaded", sentence_out(inst));
    out.set("unload", facade::unload_phrase_library(k.ctx(), 2));
    out.set("guess unloaded", facade::guess_sentence(inst));
    out.set("row 0 unloaded", sentence_out(inst));
    out
}

/// `choose_and_relook`: `nihao` parsed, candidate 2 chosen, the sentence guessed again.
fn choose_and_relook(inst: P) {
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    facade::guess_sentence(inst);
    facade::guess_candidates(inst, 0, 0);
    let mut chosen: P = std::ptr::null_mut();
    facade::get_candidate(inst, 2, &raw mut chosen);
    facade::choose_candidate(inst, 0, chosen);
    facade::guess_sentence(inst);
}

/// `reparse_after_choose`: [`choose_and_relook`], then a re-parse of `shrink`.
fn reparse_after_choose(k: &mut Kit, shrink: &str) -> P {
    let inst = k.alloc();
    choose_and_relook(inst);
    facade::parse_more_full_pinyins(inst, c(shrink).as_ptr());
    inst
}

fn choose_lookup_control(k: &mut Kit) -> J {
    let inst = k.alloc();
    choose_and_relook(inst);
    let mut out = obj! { "no-reparse" => facade::train_top(inst) };
    let inst = k.alloc();
    choose_and_relook(inst);
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    out.set("same-length-reparse", facade::train_top(inst));
    out
}

/// `train-after-reparse-and-choose-control` and its zhuyin twin.
fn reparse_and_choose(k: &mut Kit) -> J {
    if facade::ZHUYIN {
        facade::set_options(k.ctx(), ZHUYIN_REPARSE_OPTIONS);
    }
    let inst = k.alloc();
    let mut out = obj! { "parse" => facade::parse_more_full_pinyins(inst, c("nihaoma").as_ptr()) };
    out.set("first choose", choose_normal(inst, 2, "好"));
    out.set("guess", facade::guess_sentence(inst));
    if !facade::ZHUYIN {
        out.set("row 0", sentence_out(inst));
    }
    out.set(
        "reparse",
        facade::parse_more_full_pinyins(inst, c("niha").as_ptr()),
    );
    out.set("second choose", choose_normal(inst, 0, "你"));
    out.set("train", facade::train_top(inst));
    out
}

/// `zhuyin_reparse_after_choose`: 你 chosen at 0 through libzhuyin, then a re-parse.
fn zhuyin_reparse_after_choose(k: &mut Kit, shrink: &str) -> P {
    facade::set_options(k.ctx(), ZHUYIN_REPARSE_OPTIONS);
    let inst = k.alloc();
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    facade::guess_sentence(inst);
    facade::guess_candidates_after_cursor(inst, 0);
    let chosen = row_reading(inst, "你");
    assert!(!chosen.is_null(), "no 你 row after nihao");
    facade::choose_candidate(inst, 0, chosen);
    facade::guess_sentence(inst);
    facade::parse_more_full_pinyins(inst, c(shrink).as_ptr());
    inst
}

/// The re-parse input of a `*train-after-reparse-*` case.
fn shrink_of(id: &str) -> Option<&'static str> {
    let rest = id
        .strip_prefix("abort-zhuyin-")
        .or_else(|| id.strip_prefix("zhuyin-"))
        .or_else(|| id.strip_prefix("abort-"))
        .unwrap_or(id);
    Some(match rest {
        "train-after-reparse-past-matrix-end" => "ni",
        "train-after-reparse-empty-start-column" => "'ni",
        "train-after-reparse-internal-zero-key" => "ni'ha",
        "train-after-reparse-empty-end-column" => "ni'hao",
        _ => return None,
    })
}

/// Runs the case `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(parse) = id.strip_prefix("constraint-survives-reconcile-") {
        let replacement = parse.replacen("jue", "jve", 1);
        return survives_reconcile(k, parse, &replacement);
    }
    if let Some(parse) = id.strip_prefix("constraint-survives-") {
        return constraint_survives(k, parse);
    }
    if let Some(shrink) = shrink_of(id) {
        let inst = if facade::ZHUYIN {
            zhuyin_reparse_after_choose(k, shrink)
        } else {
            reparse_after_choose(k, shrink)
        };
        return obj! { "ret" => facade::train_top(inst) };
    }
    match id {
        "abort-zhuyin-train-stale-forcing" => zhuyin_stale_forcing(k),
        "abort-train-stale-forcing" => {
            let (inst, _) = forced_sentence(k, false);
            obj! { "ret" => facade::train_top(inst) }
        }
        "abort-train-index-past-results" => {
            let inst = full_inst(k, b"nihao");
            assert!(facade::guess_sentence(inst), "no sentence to train");
            obj! { "ret" => facade::train_at(inst, 255) }
        }
        "train-forcing-after-lookup" => {
            let (inst, chosen) = forced_sentence(k, true);
            let train = facade::train_top(inst);
            obj! { "chosen" => chosen, "train" => train, "again" => facade::train_top(inst) }
        }
        "constraint-stale-item-keeps-nini" => stale_item(k, "你", "抳", "nini"),
        "constraint-stale-item-drops-nihao" => stale_item(k, "你", "晧", "nihao"),
        "train-after-choose-lookup-control" => choose_lookup_control(k),
        "train-after-reparse-and-choose-control"
        | "zhuyin-train-after-reparse-and-choose-control" => reparse_and_choose(k),
        other => panic!("no case {other}"),
    }
}
