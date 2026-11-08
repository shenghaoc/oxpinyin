//! Separators and before-cursor cases (inventory row 67, #1175): #640 leading
//! separators, #698 retained before-cursor windows and #681 zero keys.
//!
//! Each case parses one full-pinyin string and queries one operation at one byte
//! offset, recording output sentinels so NULL and untouched outputs stay apart.

use std::ffi::c_char;

use crate::facade;
use crate::readers::{UNTOUCHED_TEXT, c, owned_out};
use crate::support::{J, Kit, P, UNTOUCHED, borrowed_text};

/// The options the cases set.
const OPTIONS: u32 = 0x18a;

/// The separator input the case index names.
fn separator_input(index: usize) -> &'static str {
    match index {
        0 => "'ni",
        1 => "''ni",
        3 => "'ni'hao",
        4 => "'ni'",
        other => panic!("no separator input {other}"),
    }
}

/// One case: its input, operation, byte offset and options.
struct Probe<'a> {
    text: &'a str,
    op: &'a str,
    off: usize,
    options: u32,
}

fn number(text: &str) -> usize {
    text.parse().expect("a number in the case ID")
}

/// Decodes a case ID into its probe.
fn probe_of(id: &str) -> Probe<'_> {
    let parts: Vec<&str> = id.split('/').collect();
    let mode_matches = |mode: &str| {
        assert_eq!(
            mode == "zhuyin",
            facade::ZHUYIN,
            "{id}: the case's mode is not this facade's"
        );
    };
    match parts.as_slice() {
        ["separator", mode, index, op, off] => {
            mode_matches(mode);
            Probe {
                text: separator_input(number(index)),
                op,
                off: number(off),
                options: OPTIONS,
            }
        }
        _ => panic!("unknown separator_before_cursor case {id}"),
    }
}

/// Runs the case `id` and answers its result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    separator_case(k, &probe_of(id))
}

fn seeded_ptr() -> P {
    UNTOUCHED as usize as P
}

fn is_untouched<T>(pointer: *const T) -> bool {
    pointer as usize == UNTOUCHED as usize
}

/// A borrowed out string: `"untouched"` for the sentinel, `None` for NULL.
fn borrowed_out(pointer: *const c_char) -> Option<String> {
    if is_untouched(pointer) {
        return Some(UNTOUCHED_TEXT.to_owned());
    }
    borrowed_text(pointer)
}

/// The lowercase hex of `len` bytes at `pointer`.
fn hex_at(pointer: P, len: usize) -> String {
    // SAFETY: the facade answered a key or key rest at this pointer, of at
    // least `len` bytes (`ChewingKey` is 2, `ChewingKeyRest` 4).
    let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), len) };
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `separator_case`'s probe: parses `text` with `options` and collects the
/// operation's outputs. It guesses a sentence unless the operation is `parse`,
/// keeps numeric and pointer sentinels, records each candidate's type, text and
/// rank, and copies key and key-rest bytes. Rendered strings are owned and
/// freed; candidate strings are borrowed. The character query takes 阿, 你好 or
/// 你 from the input. No native object is finalised here.
fn separator_case(k: &mut Kit, probe: &Probe<'_>) -> J {
    let &Probe {
        text,
        op,
        off,
        options,
    } = probe;
    let inst = k.inst();
    facade::set_options(k.ctx(), options);
    let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    let mut out = obj! {
        "parse_return_consumed" => parsed,
        "get_parsed_input_length" => facade::get_parsed_input_length(inst),
    };
    out.set("guess_sentence", facade::guess_sentence(inst));
    match op {
        "sentence" => {
            let mut sentence = seeded_ptr().cast::<c_char>();
            let ret = if facade::ZHUYIN {
                facade::get_sentence_only(inst, &raw mut sentence)
            } else {
                facade::get_sentence_at(inst, 0, &raw mut sentence)
            };
            out.set("ret", ret);
            out.set("sentence", owned_out(sentence));
        }
        "candidates" | "before" => candidates(inst, op, off, &mut out),
        "left" | "right" | "offset" => {
            let mut value = UNTOUCHED as usize;
            let ret = match (op, facade::ZHUYIN) {
                ("offset", false) => facade::get_pinyin_offset(inst, off, &raw mut value),
                ("offset", true) => facade::get_zhuyin_offset(inst, off, &raw mut value),
                ("left", false) => facade::get_left_pinyin_offset(inst, off, &raw mut value),
                ("left", true) => facade::get_left_zhuyin_offset(inst, off, &raw mut value),
                (_, false) => facade::get_right_pinyin_offset(inst, off, &raw mut value),
                (_, true) => facade::get_right_zhuyin_offset(inst, off, &raw mut value),
            };
            out.set("ret", ret);
            out.set("out", value);
            out.set("untouched_output", ret || value == UNTOUCHED as usize);
        }
        "character" => {
            let phrase = "你";
            let mut value = UNTOUCHED as usize;
            let ret = facade::get_character_offset(inst, c(phrase).as_ptr(), off, &raw mut value);
            out.set("ret", ret);
            out.set("out", value);
            out.set("phrase", phrase);
            out.set("untouched_output", ret || value == UNTOUCHED as usize);
        }
        _ => key(inst, op, off, &mut out),
    }
    out
}

/// The candidate list after a guess at `off` (pinyin sort word `0x1e`; zhuyin
/// before or after the cursor), with each row's fetch, type, string and, for a
/// pinyin n-best row, its rank.
fn candidates(inst: P, op: &str, off: usize, out: &mut J) {
    let ret = if !facade::ZHUYIN {
        facade::guess_candidates(inst, off, 0x1e)
    } else if op == "before" {
        facade::guess_candidates_before_cursor(inst, off)
    } else {
        facade::guess_candidates_after_cursor(inst, off)
    };
    let mut n = UNTOUCHED;
    let got = facade::get_n_candidate(inst, &raw mut n);
    out.set("ret", ret);
    out.set("get_n", got);
    out.set("n", n);
    let mut rows = Vec::new();
    if got {
        for index in 0..n {
            let mut candidate = seeded_ptr();
            let mut kind = i32::try_from(UNTOUCHED).expect("the sentinel fits i32");
            let mut string = seeded_ptr().cast_const().cast::<c_char>();
            let fetched = facade::get_candidate(inst, index, &raw mut candidate);
            let typed = facade::get_candidate_type(inst, candidate, &raw mut kind);
            let rendered = facade::get_candidate_string(inst, candidate, &raw mut string);
            let mut row = vec![
                J::from(fetched),
                J::from(typed),
                J::from(kind),
                J::from(rendered),
                J::from(borrowed_out(string)),
            ];
            if !facade::ZHUYIN && kind == 1 {
                let mut nbest = 0xab_u8;
                row.push(J::from(facade::get_candidate_nbest_index(
                    inst,
                    candidate,
                    &raw mut nbest,
                )));
                row.push(J::from(nbest));
            }
            rows.push(J::L(row));
        }
    }
    out.set("rows", rows);
}

/// The key (`key`, `render`) or key rest (`rest`) at `off`, its bytes, and for
/// a rest its positions and length, for `render` every string the facade renders.
///
/// zhuyin_get_zhuyin_key and its rest getter read the column at the offset
/// itself, where libpinyin first moves past zero keys
/// (`_compute_pinyin_start`, `pinyin.cpp:2931`).
fn key(inst: P, op: &str, off: usize, out: &mut J) {
    let rest = op == "rest";
    let mut pointer = seeded_ptr();
    let ret = match (rest, facade::ZHUYIN) {
        (false, false) => facade::get_pinyin_key(inst, off, &raw mut pointer),
        (false, true) => facade::get_zhuyin_key(inst, off, &raw mut pointer),
        (true, false) => facade::get_pinyin_key_rest(inst, off, &raw mut pointer),
        (true, true) => facade::get_zhuyin_key_rest(inst, off, &raw mut pointer),
    };
    out.set("ret", ret);
    out.set(
        "pointer",
        if is_untouched(pointer) {
            "untouched"
        } else if pointer.is_null() {
            "null"
        } else {
            "key"
        },
    );
    if !ret {
        out.set(
            "untouched_or_null_pointer",
            pointer.is_null() || is_untouched(pointer),
        );
    }
    if !ret || pointer.is_null() {
        return;
    }
    out.set("bytes", hex_at(pointer, if rest { 4 } else { 2 }));
    if rest {
        let (mut begin, mut end, mut length) = (0xbeef_u16, 0xbeef_u16, 0xbeef_u16);
        let positions_ret = if facade::ZHUYIN {
            facade::get_zhuyin_key_rest_positions(inst, pointer, &raw mut begin, &raw mut end)
        } else {
            facade::get_pinyin_key_rest_positions(inst, pointer, &raw mut begin, &raw mut end)
        };
        out.set("positions_ret", positions_ret);
        out.set("positions", vec![begin, end]);
        let length_ret = if facade::ZHUYIN {
            facade::get_zhuyin_key_rest_length(inst, pointer, &raw mut length)
        } else {
            facade::get_pinyin_key_rest_length(inst, pointer, &raw mut length)
        };
        out.set("length_ret", length_ret);
        out.set("length", length);
    }
}
