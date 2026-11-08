//! The matrix_resplit_windows family's case bodies: each answers the result document the old case returned.
//!
//! #682/#683, pin 074a2219: forward resplit mutation, then divided, then fuzzy
//! (`phonetic_key_matrix.cpp:87-228`; `pinyin.cpp:1497-1524`). libzhuyin's
//! full-pinyin seam omits both split passes (`zhuyin.cpp:1017-1040`). #1031:
//! every reader of a facade's one retained matrix (`zhuyin.cpp:1886-1927`,
//! `pinyin.cpp:1514-1522`).

use crate::facade;
use crate::readers::{GuessRows, c, guess_rows, zhuyin_guess_rows};
use crate::support::{J, Kit, P, UNTOUCHED};

/// A `{'ret', 'rows'}` document.
fn rows_j(rows: GuessRows) -> J {
    obj! { "ret" => rows.ret, "rows" => rows.rows }
}

/// The facade's candidate guess at `offset`: pinyin sort word 0x1e, zhuyin after the cursor.
fn guess(inst: P, offset: usize) -> J {
    rows_j(if facade::ZHUYIN {
        zhuyin_guess_rows(inst, offset)
    } else {
        guess_rows(inst, offset, 0x1e)
    })
}

/// `matrix_resplit_windows(text, options)`: candidate windows at every offset.
fn windows(k: &mut Kit, text: &str, options: u32) -> J {
    assert!(facade::set_options(k.ctx(), options));
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    assert_eq!(
        parsed,
        text.len(),
        "characterisation requires a complete parse"
    );
    let windows: Vec<J> = (0..=text.len()).map(|offset| guess(inst, offset)).collect();
    obj! { "parsed" => parsed, "windows" => windows }
}

/// `key_bytes`: the bytes a getter handed out, `None` for NULL.
fn key_bytes(pointer: P, length: usize) -> J {
    if pointer.is_null() {
        return J::Null;
    }
    // SAFETY: a non-NULL key or key rest the getter answered holds at least `length` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) };
    J::from(
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
}

/// The facade's key and key-rest getters.
type KeyGetter = fn(P, usize, *mut P) -> bool;
type RestPositions = fn(P, P, *mut u16, *mut u16) -> bool;
type RestLength = fn(P, P, *mut u16) -> bool;

fn getters() -> (KeyGetter, KeyGetter, RestPositions, RestLength) {
    if facade::ZHUYIN {
        (
            facade::get_zhuyin_key,
            facade::get_zhuyin_key_rest,
            facade::get_zhuyin_key_rest_positions,
            facade::get_zhuyin_key_rest_length,
        )
    } else {
        (
            facade::get_pinyin_key,
            facade::get_pinyin_key_rest,
            facade::get_pinyin_key_rest_positions,
            facade::get_pinyin_key_rest_length,
        )
    }
}

/// One offset's key-rest row: the getter, the bytes, and for a live rest its
/// positions and length.
fn rest_row(inst: P, offset: usize) -> J {
    let (_, key_rest, positions, length_of) = getters();
    let mut rest: P = std::ptr::null_mut();
    let ret = key_rest(inst, offset, &raw mut rest);
    let mut row = vec![J::from(ret), key_bytes(rest, 4)];
    if !rest.is_null() {
        let (mut begin, mut end, mut length) = (0xABCD_u16, 0xABCD_u16, 0xABCD_u16);
        row.push(J::from(positions(inst, rest, &raw mut begin, &raw mut end)));
        row.push(J::from(begin));
        row.push(J::from(end));
        row.push(J::from(length_of(inst, rest, &raw mut length)));
        row.push(J::from(length));
    }
    J::L(row)
}

/// `matrix_resplit_surfaces(text, phrase)`.
fn surfaces(k: &mut Kit, text: &str, phrase: &str) -> J {
    assert!(facade::set_options(k.ctx(), 0x18a));
    let inst = k.inst();
    assert_eq!(
        facade::parse_more_full_pinyins(inst, c(text).as_ptr()),
        text.len()
    );
    let (key_getter, ..) = getters();
    let (mut keys, mut rests, mut characters) = (Vec::new(), Vec::new(), Vec::new());
    for offset in 0..=text.len() {
        let mut key: P = std::ptr::null_mut();
        let ret = key_getter(inst, offset, &raw mut key);
        keys.push(J::L(vec![J::from(ret), key_bytes(key, 2)]));
        rests.push(rest_row(inst, offset));
        let mut count = UNTOUCHED as usize;
        let ret = facade::get_character_offset(inst, c(phrase).as_ptr(), offset, &raw mut count);
        let shown = if count == UNTOUCHED as usize {
            J::from("untouched")
        } else {
            J::from(count)
        };
        characters.push(J::L(vec![J::from(ret), shown]));
    }
    let mut out = obj! { "keys" => keys, "rests" => rests, "characters" => characters };
    out.set("guess 0", guess(inst, 0));
    out.set("guess 2", guess(inst, 2));
    out.set("sentence", facade::guess_sentence(inst));
    out.set("after sentence 0", guess(inst, 0));
    out
}

/// `matrix-resplit-{mode}-{text}-{tables,plain}-{fuzzy,exact}` and
/// `matrix-resplit-surfaces-{mode}-{text}`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let mode = if facade::ZHUYIN { "zhuyin" } else { "pinyin" };
    if let Some(text) = id.strip_prefix(&format!("matrix-resplit-surfaces-{mode}-")) {
        let phrase = match text {
            "xian" => "西安",
            "fangan" => "方案",
            other => panic!("no surface input {other}"),
        };
        return surfaces(k, text, phrase);
    }
    let rest = id
        .strip_prefix(&format!("matrix-resplit-{mode}-"))
        .unwrap_or_else(|| panic!("{id}: not this facade's case"));
    let parts: Vec<&str> = rest.split('-').collect();
    let [text, tables, fuzzy] = parts.as_slice() else {
        panic!("{id}: not a window case")
    };
    let options = if *tables == "tables" { 0x18a } else { 0x0a }
        | if *fuzzy == "fuzzy" { 1 << 17 } else { 0 };
    windows(k, text, options)
}
