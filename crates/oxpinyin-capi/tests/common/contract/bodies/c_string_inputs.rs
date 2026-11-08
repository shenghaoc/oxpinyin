//! The c_string_inputs family's case bodies: each answers the result document the old case returned.
//!
//! C string inputs: malformed, non-scalar and NULL text at each facade entry
//! (inventory row 61, #1279).

use std::ffi::{CString, c_char};

use crate::facade;
use crate::fixtures::items_system;
use crate::readers::{c, garray_u32, tokens_of};
use crate::support::{J, Kit, P, UNTOUCHED};

/// The sixteen-bit key sentinel (`UNTOUCHED & 0xffff`).
const KEY_SEED: u16 = (UNTOUCHED & 0xFFFF) as u16;

/// `PREFIX_WORDS`: the first word each variant spells, the valid word otherwise.
fn prefix_word(variant: &str) -> u32 {
    match variant {
        "surrogate" => 0xd800,
        "above-unicode" => 0x11_0000,
        "five-byte" => 0x20_0000,
        "six-byte" => 0x400_0000,
        _ => 0x4e01,
    }
}

/// `glib_utf8`: `g_unichar_to_utf8`'s one- to six-byte forms (gutf8.c:546-596).
fn glib_utf8(words: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    for &word in words {
        let (first, length) = match word {
            0..0x80 => (0_u32, 1),
            0x80..0x800 => (0xc0, 2),
            0x800..0x1_0000 => (0xe0, 3),
            0x1_0000..0x20_0000 => (0xf0, 4),
            0x20_0000..0x400_0000 => (0xf8, 5),
            _ => (0xfc, 6),
        };
        let mut rest = word;
        let mut tail = Vec::new();
        for _ in 1..length {
            tail.push(0x80 | (rest & 0x3f) as u8);
            rest >>= 6;
        }
        out.push((first | rest) as u8);
        out.extend(tail.iter().rev());
    }
    out
}

/// A nullable C string's pointer.
fn ptr(text: &Option<CString>) -> *const c_char {
    text.as_ref().map_or(std::ptr::null(), |text| text.as_ptr())
}

/// `c_string_full_key` / `c_string_full_key_apostrophe`: the single-key parse
/// of `spelling` on the cached instance.
fn full_key(k: &mut Kit, spelling: &[u8], apostrophe: bool) -> J {
    let mut key = KEY_SEED;
    let ret = facade::parse_full_pinyin(k.inst(), c(spelling).as_ptr(), (&raw mut key).cast());
    if apostrophe {
        return obj! { "ret" => ret, "untouched key" => key == KEY_SEED };
    }
    let key: J = if key == KEY_SEED {
        "untouched".into()
    } else {
        key.into()
    };
    obj! { "ret" => ret, "key" => key }
}

/// `lookup_tokens` of `text` into a fresh four-byte array: the answer and tokens.
fn lookup_array(inst: P, text: *const c_char) -> (bool, Vec<u32>) {
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    let found = facade::lookup_tokens(inst, text, array.cast());
    let tokens = garray_u32(array);
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    (found, tokens)
}

/// `c_string_words`: import, lookup or remember of accepted, malformed or NULL text.
fn words(k: &mut Kit, operation: &str, variant: &str) -> J {
    let word = prefix_word(variant);
    let words = [0x7532, word, 0x4e59];
    let mut items: Vec<(&[u32], &[u16], u32)> = vec![(&words[..], &[0x1084_u16; 3][..], 100)];
    items.extend(
        words
            .iter()
            .map(|value| (std::slice::from_ref(value), &[0x1084_u16][..], 100)),
    );
    items_system(k, &items, 400, false);
    let text = match variant {
        "invalid" => Some(c(["甲".as_bytes(), b"\xff", "乙".as_bytes()].concat())),
        "null" => None,
        _ => Some(c(glib_utf8(&words))),
    };
    let ret: J = match operation {
        "import" => {
            let iterator = facade::begin_add_phrases(k.ctx(), 7);
            let reading = if facade::ZHUYIN {
                c("ㄉㄚ ㄉㄚ ㄉㄚ")
            } else {
                c("da1'da1'da1")
            };
            let ret = facade::iterator_add_phrase(iterator, ptr(&text), reading.as_ptr(), 5);
            facade::end_add_phrases(iterator);
            ret.into()
        }
        "remember" => {
            assert!(facade::set_options(k.ctx(), 1 << 5));
            let parsed = facade::parse_more_full_pinyins(k.inst(), c("da1da1da1").as_ptr());
            assert_eq!(parsed, 9);
            facade::remember_user_input(k.inst(), ptr(&text), 5).into()
        }
        _ => J::Null,
    };
    if text.is_none() {
        return obj! { "ret" => ret };
    }
    let (found, tokens) = lookup_array(k.inst(), ptr(&text));
    obj! { "ret" => ret, "found" => found, "tokens" => tokens }
}

/// `c_string_direct_reading`: a failed later segment keeps the earlier key.
fn direct_reading(k: &mut Kit, variant: &str) -> J {
    let reading = match variant {
        "later-invalid" => ["ㄉㄚ ".as_bytes(), b"\xff"].concat(),
        "same-invalid" => ["ㄉㄚ".as_bytes(), b"\xff"].concat(),
        other => panic!("no direct variant {other}"),
    };
    let iterator = facade::begin_add_phrases(k.ctx(), 7);
    let ret = facade::iterator_add_phrase(iterator, c("甲").as_ptr(), c(reading).as_ptr(), 5);
    facade::end_add_phrases(iterator);
    obj! { "ret" => ret, "tokens" => tokens_of(k, "甲") }
}

/// `c_string_null_state`: seeded segment or lookup state before a NULL or valid call.
fn null_state(k: &mut Kit, operation: &str, is_null: bool) -> J {
    let inst = k.inst();
    assert!(facade::phrase_segment(inst, c("你好").as_ptr()));
    if operation == "segment" {
        let text = (!is_null).then(|| c("你好"));
        let ret = facade::phrase_segment(inst, ptr(&text));
        let mut count = UNTOUCHED;
        facade::get_n_phrase(inst, &raw mut count);
        return obj! { "ret" => ret, "count" => count };
    }
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    assert!(facade::lookup_tokens(inst, c("你").as_ptr(), array.cast()));
    let text = (!is_null).then(|| c("你"));
    let ret = facade::lookup_tokens(inst, ptr(&text), array.cast());
    // SAFETY: the array is live: its header's length is readable.
    let count = unsafe { (*array).len };
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    obj! { "ret" => ret, "count" => count }
}

/// `c_string_parse_null`: seeded parse state and key sentinel before a NULL or valid call.
fn parse_null(k: &mut Kit, entry: &str, is_null: bool) -> J {
    let inst = k.inst();
    let seeded = if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c("su3cl3").as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr())
    };
    assert!(seeded > 0);
    if entry.starts_with("parse_more_") {
        let text = (!is_null).then(|| {
            c(if entry == "parse_more_chewings" {
                "su3"
            } else {
                "ni3"
            })
        });
        let parse: fn(P, *const c_char) -> usize = match entry {
            "parse_more_full_pinyins" => facade::parse_more_full_pinyins,
            "parse_more_chewings" => facade::parse_more_chewings,
            "parse_more_double_pinyins" => facade::parse_more_double_pinyins,
            other => panic!("no entry {other}"),
        };
        let ret = parse(inst, ptr(&text));
        let length = facade::get_parsed_input_length(inst);
        return obj! { "ret" => ret, "length" => length };
    }
    let text = (!is_null).then(|| {
        c(if entry == "parse_chewing" {
            "su3"
        } else {
            "ni3"
        })
    });
    let parse: fn(P, *const c_char, P) -> bool = match entry {
        "parse_full_pinyin" => facade::parse_full_pinyin,
        "parse_chewing" => facade::parse_chewing,
        "parse_double_pinyin" => facade::parse_double_pinyin,
        other => panic!("no entry {other}"),
    };
    let mut key = KEY_SEED;
    let ret = parse(inst, ptr(&text), (&raw mut key).cast());
    obj! { "ret" => ret, "untouched key" => key == KEY_SEED }
}

/// `c_string_character_offset`: the offset of a two-word phrase whose first word is the variant's.
fn character_offset(k: &mut Kit, variant: &str) -> J {
    let word = prefix_word(variant);
    items_system(
        k,
        &[
            (&[word][..], &[0x3084_u16][..], 100),
            (&[0x4e19_u32][..], &[0x3084_u16][..], 100),
        ],
        200,
        false,
    );
    let inst = k.inst();
    assert!(facade::parse_more_full_pinyins(inst, c("da3da3").as_ptr()) > 0);
    let phrase = match variant.split("-past").next().expect("a variant") {
        "invalid" => Some(c(["甲".as_bytes(), b"\xff"].concat())),
        "null" => None,
        _ => Some(c(glib_utf8(&[word, 0x4e19]))),
    };
    let mut length = UNTOUCHED as usize;
    let past = variant.ends_with("-past-matrix");
    let ret = facade::get_character_offset(
        inst,
        ptr(&phrase),
        if past { 99 } else { 5 },
        &raw mut length,
    );
    if past {
        return obj! { "ret" => ret };
    }
    let length: J = if length == UNTOUCHED as usize {
        "untouched".into()
    } else {
        length.into()
    };
    obj! { "ret" => ret, "length" => length }
}

/// `c-string-input/{mode}-...`: dispatches on the registered case ID.
pub fn case(k: &mut Kit, id: &str) -> J {
    let mode = if facade::ZHUYIN { "zhuyin-" } else { "pinyin-" };
    let rest = id
        .strip_prefix("c-string-input/")
        .and_then(|rest| rest.strip_prefix(mode))
        .unwrap_or_else(|| panic!("no case {id}"));
    match rest {
        "full-key-apostrophe-invalid" => return full_key(k, b"ni\xff'", true),
        "full-key-valid" => return full_key(k, b"ni3", false),
        "full-key-malformed" => return full_key(k, b"n\xff3", false),
        _ => {}
    }
    if let Some(variant) = rest.strip_prefix("direct-") {
        return direct_reading(k, variant);
    }
    if let Some(variant) = rest.strip_prefix("character-offset-") {
        return character_offset(k, variant);
    }
    for operation in ["segment", "lookup"] {
        if let Some(null) = rest.strip_prefix(&format!("{operation}-state-")) {
            return null_state(k, operation, null == "null");
        }
    }
    if rest.starts_with("parse_") {
        let (entry, null) = rest.rsplit_once('-').expect("an entry and a variant");
        return parse_null(k, entry, null == "null");
    }
    let (operation, variant) = rest.split_once('-').expect("an operation and a variant");
    words(k, operation, variant)
}
