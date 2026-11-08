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

/// The word a malformed phrase holds around its invalid byte.
const WORD: u32 = 0x4e01;

/// A nullable C string's pointer.
fn ptr(text: &Option<CString>) -> *const c_char {
    text.as_ref().map_or(std::ptr::null(), |text| text.as_ptr())
}

/// `c_string_full_key_apostrophe`: the single-key parse of a malformed
/// apostrophe spelling keeps the key sentinel.
fn full_key_apostrophe(k: &mut Kit) -> J {
    let mut key = KEY_SEED;
    let ret = facade::parse_full_pinyin(k.inst(), c(b"ni\xff'").as_ptr(), (&raw mut key).cast());
    obj! { "ret" => ret, "untouched key" => key == KEY_SEED }
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

/// `c_string_words`: import, lookup or remember of malformed or NULL text.
fn words(k: &mut Kit, operation: &str, variant: &str) -> J {
    let words = [0x7532, WORD, 0x4e59];
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
        other => panic!("no word variant {other}"),
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

/// `c_string_direct_later_invalid`: a failed later segment keeps the earlier key.
fn direct_reading(k: &mut Kit) -> J {
    let reading = ["ㄉㄚ ".as_bytes(), b"\xff"].concat();
    let iterator = facade::begin_add_phrases(k.ctx(), 7);
    let ret = facade::iterator_add_phrase(iterator, c("甲").as_ptr(), c(reading).as_ptr(), 5);
    facade::end_add_phrases(iterator);
    obj! { "ret" => ret, "tokens" => tokens_of(k, "甲") }
}

/// `c_string_segment_state_null`: a seeded segment keeps its count at NULL.
fn null_state(k: &mut Kit) -> J {
    let inst = k.inst();
    assert!(facade::phrase_segment(inst, c("你好").as_ptr()));
    let ret = facade::phrase_segment(inst, std::ptr::null());
    let mut count = UNTOUCHED;
    facade::get_n_phrase(inst, &raw mut count);
    obj! { "ret" => ret, "count" => count }
}

/// `c_string_parse_null`: seeded parse state and key sentinel at NULL.
fn parse_null(k: &mut Kit, entry: &str) -> J {
    let inst = k.inst();
    let seeded = if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c("su3cl3").as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr())
    };
    assert!(seeded > 0);
    if entry.starts_with("parse_more_") {
        let parse: fn(P, *const c_char) -> usize = match entry {
            "parse_more_full_pinyins" => facade::parse_more_full_pinyins,
            "parse_more_chewings" => facade::parse_more_chewings,
            other => panic!("no entry {other}"),
        };
        let ret = parse(inst, std::ptr::null());
        let length = facade::get_parsed_input_length(inst);
        return obj! { "ret" => ret, "length" => length };
    }
    let parse: fn(P, *const c_char, P) -> bool = match entry {
        "parse_full_pinyin" => facade::parse_full_pinyin,
        "parse_chewing" => facade::parse_chewing,
        other => panic!("no entry {other}"),
    };
    let mut key = KEY_SEED;
    let ret = parse(inst, std::ptr::null(), (&raw mut key).cast());
    obj! { "ret" => ret, "untouched key" => key == KEY_SEED }
}

/// `c_string_character_offset_invalid_past_matrix`: a malformed two-word phrase
/// at an offset past the matrix answers false.
fn character_offset(k: &mut Kit) -> J {
    items_system(
        k,
        &[
            (&[WORD][..], &[0x3084_u16][..], 100),
            (&[0x4e19_u32][..], &[0x3084_u16][..], 100),
        ],
        200,
        false,
    );
    let inst = k.inst();
    assert!(facade::parse_more_full_pinyins(inst, c("da3da3").as_ptr()) > 0);
    let phrase = c(["甲".as_bytes(), b"\xff"].concat());
    let mut length = UNTOUCHED as usize;
    let ret = facade::get_character_offset(inst, phrase.as_ptr(), 99, &raw mut length);
    obj! { "ret" => ret }
}

/// `c-string-input/{mode}-...`: dispatches on the registered case ID.
pub fn case(k: &mut Kit, id: &str) -> J {
    let mode = if facade::ZHUYIN { "zhuyin-" } else { "pinyin-" };
    let rest = id
        .strip_prefix("c-string-input/")
        .and_then(|rest| rest.strip_prefix(mode))
        .unwrap_or_else(|| panic!("no case {id}"));
    match rest {
        "full-key-apostrophe-invalid" => return full_key_apostrophe(k),
        "direct-later-invalid" => return direct_reading(k),
        "character-offset-invalid-past-matrix" => return character_offset(k),
        "segment-state-null" => return null_state(k),
        _ => {}
    }
    if rest.starts_with("parse_") {
        let (entry, null) = rest.rsplit_once('-').expect("an entry and a variant");
        assert_eq!(null, "null");
        return parse_null(k, entry);
    }
    let (operation, variant) = rest.split_once('-').expect("an operation and a variant");
    words(k, operation, variant)
}
