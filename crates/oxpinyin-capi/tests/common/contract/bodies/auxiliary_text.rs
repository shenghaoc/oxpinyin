//! The auxiliary_text family's case bodies: each answers the result document the old case returned.
//!
//! Auxiliary text renderers over full, double and chewing parses (lane C,
//! #542 rows 8 and 9; #925).

use std::ffi::c_char;

use crate::facade;
use crate::readers::{c, owned_out, seeded};
use crate::support::{J, Kit, P};

/// The three renderers in the old order.
const ALL: [&str; 3] = ["full_pinyin", "double_pinyin", "chewing"];

/// A parse entry by its facade name.
fn parser(name: &str) -> fn(P, *const c_char) -> usize {
    match name {
        "parse_more_full_pinyins" => facade::parse_more_full_pinyins,
        "parse_more_double_pinyins" => facade::parse_more_double_pinyins,
        "parse_more_chewings" => facade::parse_more_chewings,
        other => panic!("no parse {other}"),
    }
}

/// `get_<name>_auxiliary_text` at `cursor`: the flag and the out text.
fn render(inst: P, name: &str, cursor: usize) -> J {
    let getter: fn(P, usize, *mut *mut c_char) -> bool = match name {
        "full_pinyin" => facade::get_full_pinyin_auxiliary_text,
        "double_pinyin" => facade::get_double_pinyin_auxiliary_text,
        "chewing" => facade::get_chewing_auxiliary_text,
        other => panic!("no renderer {other}"),
    };
    let mut aux = seeded();
    let ret = getter(inst, cursor, &raw mut aux);
    J::from((ret, owned_out(aux)))
}

/// A fresh instance with `text` parsed by `parse_name`.
fn parsed(k: &mut Kit, parse_name: &str, text: &[u8]) -> P {
    let inst = k.alloc();
    parser(parse_name)(inst, c(text).as_ptr());
    inst
}

/// `aux_matrix`: every renderer at each cursor, a fresh parse per pair.
fn aux_matrix(k: &mut Kit, parse_name: &str, text: &[u8], cursors: &[usize]) -> Vec<(String, J)> {
    let mut out = Vec::new();
    for name in ALL {
        for &cursor in cursors {
            let inst = parsed(k, parse_name, text);
            out.push((format!("{name}@{cursor}"), render(inst, name, cursor)));
        }
    }
    out
}

/// `chewing_cursors`: chewing text at each cursor, a fresh parse per cursor.
fn chewing_cursors(k: &mut Kit, out: &mut J, parse_name: &str, text: &[u8], cursors: &[usize]) {
    let label = std::str::from_utf8(text).expect("UTF-8 input");
    for &cursor in cursors {
        let inst = parsed(k, parse_name, text);
        out.set(
            format!("{parse_name} {label}@{cursor}"),
            render(inst, "chewing", cursor),
        );
    }
}

/// Every cursor of `text` and the maximum `size_t`.
fn every_and_max(text: &[u8]) -> Vec<usize> {
    let mut cursors: Vec<usize> = (0..=text.len()).collect();
    cursors.push(usize::MAX);
    cursors
}

/// `aux_cursors`: the chosen renderers after a full-pinyin parse.
fn aux_cursors(k: &mut Kit, out: &mut J, text: &[u8], cursors: &[usize], names: &[&str]) {
    let label = std::str::from_utf8(text).expect("UTF-8 input");
    for name in names {
        for &cursor in cursors {
            let inst = parsed(k, "parse_more_full_pinyins", text);
            out.set(
                format!("{label} {name}@{cursor}"),
                render(inst, name, cursor),
            );
        }
    }
}

/// `aux_options`: sets the option word, then renders each text.
fn aux_options(k: &mut Kit, word: u32, texts: &[&[u8]]) -> J {
    facade::set_options(k.ctx(), word);
    let mut out = J::map();
    for text in texts {
        let every: Vec<usize> = (0..text.len() + 2).collect();
        aux_cursors(k, &mut out, text, &every, &["full_pinyin", "chewing"]);
        let few = [0, 1, 2, text.len(), text.len() + 1];
        aux_cursors(k, &mut out, text, &few, &["double_pinyin"]);
    }
    out
}

/// A labelled list as a document.
fn document(rows: Vec<(String, J)>) -> J {
    let mut out = J::map();
    for (label, value) in rows {
        out.set(label, value);
    }
    out
}

/// `aux-text-tones`: tone-bearing full and double parses, labelled by parser.
fn tones(k: &mut Kit) -> J {
    let mut out = J::map();
    for (parse_name, text, cursors) in [
        (
            "parse_more_full_pinyins",
            &b"ni3hao4"[..],
            &[0, 1, 2, 3, 4, 5, 7][..],
        ),
        (
            "parse_more_double_pinyins",
            &b"ni3hk4"[..],
            &[0, 1, 2, 3, 4, 5, 6][..],
        ),
    ] {
        for (label, value) in aux_matrix(k, parse_name, text, cursors) {
            out.set(format!("{parse_name} {label}"), value);
        }
    }
    out
}

/// `aux-text-no-parse` and `aux-text-mixed-parses`: every renderer once.
fn single(k: &mut Kit, mixed: bool) -> J {
    let mut out = J::map();
    let shared = mixed.then(|| {
        let inst = k.alloc();
        facade::parse_more_double_pinyins(inst, c("nihk").as_ptr());
        facade::parse_more_full_pinyins(inst, c("xian").as_ptr());
        inst
    });
    for name in ALL {
        let inst = shared.unwrap_or_else(|| k.alloc());
        out.set(name, render(inst, name, if mixed { 2 } else { 0 }));
    }
    out
}

/// The registered `aux-text-*` cases.
pub fn case(k: &mut Kit, id: &str) -> J {
    const FULL: &str = "parse_more_full_pinyins";
    match id {
        "aux-text-full-parse" => document(aux_matrix(k, FULL, b"nihao", &[0, 1, 2, 3, 4, 5, 9])),
        "aux-text-full-parse-apostrophe" => {
            document(aux_matrix(k, FULL, b"xi'an", &[0, 1, 2, 4, 5]))
        }
        "aux-text-double-parse" => document(aux_matrix(
            k,
            "parse_more_double_pinyins",
            b"nihk",
            &[0, 1, 2, 3, 4, 9],
        )),
        "aux-text-chewing-parse" => document(aux_matrix(
            k,
            "parse_more_chewings",
            b"su3cl3",
            &[0, 1, 2, 3, 4, 5, 6, 9],
        )),
        "aux-text-chewing-cursors" => {
            let mut out = J::map();
            for text in [&b"nihao"[..], b"ni1", b"ni3hao4", b"xian"] {
                chewing_cursors(k, &mut out, FULL, text, &every_and_max(text));
            }
            out
        }
        "aux-text-chewing-zhuyin-cursors" => {
            let mut out = J::map();
            let text = b"su3cl3";
            chewing_cursors(
                k,
                &mut out,
                "parse_more_chewings",
                text,
                &every_and_max(text),
            );
            out
        }
        "aux-text-chewing-overrun-cursor" => {
            let mut out = J::map();
            for (parse_name, text) in [
                (FULL, &b"nihao"[..]),
                ("parse_more_chewings", &b"su3cl3"[..]),
            ] {
                let cursors = [text.len() + 1, text.len() + 7, usize::MAX];
                chewing_cursors(k, &mut out, parse_name, text, &cursors);
            }
            out
        }
        "aux-text-tones" => tones(k),
        "aux-text-divided-table" => aux_options(
            k,
            (1 << 5) | (1 << 7),
            &[b"xian", b"jiangnan", b"xianan", b"tiananmen"],
        ),
        "aux-text-resplit-table" => aux_options(
            k,
            (1 << 5) | (1 << 8),
            &[b"zhengfu", b"xianan", b"dianxin", b"changan"],
        ),
        "aux-text-divided-and-resplit" => aux_options(
            k,
            (1 << 5) | (1 << 7) | (1 << 8),
            &[b"xian", b"zhengfu", b"xianan"],
        ),
        "aux-text-fuzzy-keys" => aux_options(
            k,
            (1 << 5) | (1 << 12) | (1 << 15) | (1 << 17),
            &[b"zongguo", b"nan", b"lan", b"zi", b"banggan"],
        ),
        "aux-text-no-parse" => single(k, false),
        "aux-text-mixed-parses" => single(k, true),
        other => panic!("no case {other}"),
    }
}
