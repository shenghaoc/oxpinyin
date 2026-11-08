//! The cursor_sentence_refusals family's case bodies: each answers the result document the old case returned.
//!
//! Cursor, offset, sentence-index and auxiliary-text refusals (batch2 group
//! 12b, #525).

use std::ffi::c_char;

use crate::facade;
use crate::readers::{c, full_inst, owned_out, seeded};
use crate::support::{J, Kit, P, UNTOUCHED};

/// `contract_abi.sentence_out`: the indexed sentence's flag and text.
fn sentence_out(inst: P, index: u8) -> (bool, Option<String>) {
    let mut out = seeded();
    let ret = facade::get_sentence_at(inst, index, &raw mut out);
    (ret, owned_out(out))
}

/// A seeded pointer out-parameter.
fn seeded_ptr() -> P {
    UNTOUCHED as usize as P
}

/// A renderer by its name.
fn renderer(name: &str) -> fn(P, usize, *mut *mut c_char) -> bool {
    match name {
        "full_pinyin" | "full-pinyin" => facade::get_full_pinyin_auxiliary_text,
        "double_pinyin" | "double-pinyin" => facade::get_double_pinyin_auxiliary_text,
        "chewing" => facade::get_chewing_auxiliary_text,
        other => panic!("no renderer {other}"),
    }
}

/// An offset getter by its name.
fn offset_getter(name: &str) -> fn(P, usize, *mut usize) -> bool {
    match name {
        "left" => facade::get_left_pinyin_offset,
        "right" => facade::get_right_pinyin_offset,
        other => panic!("no offset getter {other}"),
    }
}

/// `leading_apostrophe_aux`: the renderer's flag at cursor two after `text`.
fn leading_apostrophe_aux(k: &mut Kit, name: &str, text: &[u8]) -> J {
    let inst = full_inst(k, text);
    let mut aux = seeded();
    obj! { "ret" => renderer(name)(inst, 2, &raw mut aux) }
}

/// The offset getter's flag at `offset` after `text`.
fn offset_ret(k: &mut Kit, name: &str, text: &[u8], offset: usize) -> J {
    let inst = full_inst(k, text);
    let mut out = UNTOUCHED as usize;
    obj! { "ret" => offset_getter(name)(inst, offset, &raw mut out) }
}

/// 你好's character offset flag at `offset` after `text`.
fn character_offset_ret(k: &mut Kit, text: &[u8], offset: usize) -> J {
    let inst = full_inst(k, text);
    let mut out = UNTOUCHED as usize;
    obj! { "ret" => facade::get_character_offset(inst, c("你好").as_ptr(), offset, &raw mut out) }
}

/// `get-pinyin-key-size-max-empty-matrix`: both key getters at `SIZE_MAX`.
fn size_max_empty(k: &mut Kit) -> J {
    let inst = k.alloc();
    let (mut key, mut rest) = (seeded_ptr(), seeded_ptr());
    let key_ret = facade::get_pinyin_key(inst, usize::MAX, &raw mut key);
    let key_out = J::from((key_ret, key.is_null()));
    let rest_ret = facade::get_pinyin_key_rest(inst, usize::MAX, &raw mut rest);
    obj! { "key" => key_out, "rest" => (rest_ret, rest.is_null()) }
}

/// `cursor-and-sentence-neighbours`: the graceful neighbours of the sites above.
fn neighbours(k: &mut Kit) -> J {
    let inst = full_inst(k, b"nihao");
    facade::guess_sentence(inst);
    let mut out = obj! { "sentence 0" => sentence_out(inst, 0) };
    let fresh = k.alloc();
    out.set("sentence of fresh", sentence_out(fresh, 3));
    for (label, name, offset) in [
        ("left 2", "left", 2),
        ("right 2", "right", 2),
        ("right 4", "right", 4),
    ] {
        let mut res = UNTOUCHED as usize;
        let ret = offset_getter(name)(inst, offset, &raw mut res);
        let res: J = if res == UNTOUCHED as usize {
            "untouched".into()
        } else {
            res.into()
        };
        out.set(label, (ret, res));
    }
    let mut key: u16 = (UNTOUCHED & 0xFFFF) as u16;
    let ret = facade::parse_full_pinyin(inst, c("ni").as_ptr(), (&raw mut key).cast());
    out.set("parse ni", (ret, key));
    for offset in [0, 1] {
        let mut ptr = seeded_ptr();
        let ret = facade::get_pinyin_key(inst, offset, &raw mut ptr);
        out.set(format!("key {offset}"), (ret, ptr.is_null()));
    }
    for cursor in [1, 2, 5] {
        let mut aux = seeded();
        let ret = facade::get_double_pinyin_auxiliary_text(inst, cursor, &raw mut aux);
        out.set(format!("double aux {cursor}"), (ret, owned_out(aux)));
    }
    out
}

/// The registered cases.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(name) = id
        .strip_prefix("abort-")
        .and_then(|rest| rest.strip_suffix("-auxiliary-text-leading-apostrophe"))
    {
        return leading_apostrophe_aux(k, name, b"'nihao");
    }
    match id {
        "abort-full-pinyin-auxiliary-text-doubled-leading-apostrophe" => {
            leading_apostrophe_aux(k, "full_pinyin", b"''ni")
        }
        "abort-get-sentence-past-rows" => {
            let inst = full_inst(k, b"nihao");
            facade::guess_sentence(inst);
            obj! { "ret" => sentence_out(inst, 5).0 }
        }
        "abort-get-left-pinyin-offset-after-separator" => offset_ret(k, "left", b"ni'hao", 3),
        "abort-get-right-pinyin-offset-after-separator" => offset_ret(k, "right", b"ni'hao", 3),
        "abort-get-right-pinyin-offset-nihao-5" => offset_ret(k, "right", b"nihao", 5),
        "abort-get-character-offset-past-matrix" => character_offset_ret(k, b"nihao", 99),
        "abort-get-character-offset-after-separator" => character_offset_ret(k, b"ni'hao", 3),
        "abort-parse-full-pinyin-apostrophe" => {
            let mut key: u16 = (UNTOUCHED & 0xFFFF) as u16;
            let ret = facade::parse_full_pinyin(k.inst(), c("n'i").as_ptr(), (&raw mut key).cast());
            obj! { "ret" => ret }
        }
        "abort-get-pinyin-key-empty-matrix" => {
            let mut out = seeded_ptr();
            obj! { "ret" => facade::get_pinyin_key(k.alloc(), 0, &raw mut out) }
        }
        "abort-get-pinyin-key-rest-empty-matrix" => {
            let mut out = seeded_ptr();
            obj! { "ret" => facade::get_pinyin_key_rest(k.alloc(), 0, &raw mut out) }
        }
        "get-pinyin-key-size-max-empty-matrix" => size_max_empty(k),
        "abort-double-auxiliary-text-cut-three-bytes-in" => {
            let inst = full_inst(k, b"zhong");
            let mut aux = seeded();
            obj! { "ret" => facade::get_double_pinyin_auxiliary_text(inst, 3, &raw mut aux) }
        }
        "cursor-and-sentence-neighbours" => neighbours(k),
        other => panic!("no case {other}"),
    }
}
