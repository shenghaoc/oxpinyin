//! The prefix_conversion family's case bodies: each answers the result document the old case returned.
//!
//! #1277: the prediction and sentence entries convert their UTF-8 prefix to
//! UCS-4. A private system holds 甲?乙 under three `0x1084` keys, 丙 and 戌 under
//! `0x1081`, and the four-character phrase; the system bigram ranks 戌 after the
//! first item. Each case records the entry's answer and its rows or sentence as
//! lossless bytes.

use std::ffi::{CStr, c_char};
use std::path::Path;

use crate::facade;
use crate::fixtures::{craft_hash, gram_bytes, items_system};
use crate::readers::{c, candidate_at, n_candidate};
use crate::support::{J, Kit, P, UNTOUCHED};

/// `PREFIX_WORDS`: the middle word of each converted variant.
fn prefix_word(variant: &str) -> Option<u32> {
    match variant {
        "valid" => Some(0x4e01),
        "surrogate" => Some(0xd800),
        "above-unicode" => Some(0x11_0000),
        "five-byte" => Some(0x20_0000),
        "six-byte" => Some(0x400_0000),
        _ => None,
    }
}

/// `glib_utf8`: `g_unichar_to_utf8`'s one- to six-byte forms (`gutf8.c:546-596`).
pub fn glib_utf8(words: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    for &word in words {
        let (first, length): (u32, usize) = match word {
            0..0x80 => (0, 1),
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

/// `phrase_text_bytes`: a lossless text observation, `"untouched"` for the
/// sentinel, `None` for NULL, else the bytes in hex; `owned` frees them.
pub fn phrase_text_bytes(pointer: *const c_char, owned: bool) -> J {
    if pointer as usize == UNTOUCHED as usize {
        return J::from("untouched");
    }
    if pointer.is_null() {
        return J::Null;
    }
    // SAFETY: a non-NULL, non-sentinel pointer the facade wrote is a NUL-terminated string.
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    if owned {
        // SAFETY: an owned out string is a GLib allocation the caller frees once.
        unsafe { glib_sys::g_free(pointer.cast_mut().cast()) };
    }
    J::from(hex)
}

/// `prefix_system`: the private system and bigram for the middle word `word`.
fn prefix_system(k: &mut Kit, word: u32) {
    let prefix: [u32; 3] = [0x7532, word, 0x4e59];
    let phrase: [u32; 4] = [0x7532, word, 0x4e59, 0x4e19];
    items_system(
        k,
        &[
            (&prefix, &[0x1084; 3], 100),
            (&[0x4e19], &[0x1081], 1000),
            (&[0x620c], &[0x1081], 10),
            (&phrase, &[0x1084, 0x1084, 0x1084, 0x1081], 100),
        ],
        2000,
        false,
    );
    let path = Path::new(&k.data).join("bigram.db");
    std::fs::remove_file(&path).expect("unlink bigram.db");
    craft_hash(
        &path,
        &[(
            0x0100_0001_u32.to_le_bytes().to_vec(),
            gram_bytes(1000, &[(0x0100_0003, 1000)]),
        )],
    );
}

/// `prefix_argument`: the prefix bytes for `variant`, `None` for NULL.
fn prefix_argument(variant: &str) -> Option<Vec<u8>> {
    match variant {
        "null" => None,
        "invalid" => {
            let mut bytes = "甲".as_bytes().to_vec();
            bytes.push(0xff);
            bytes.extend("乙".as_bytes());
            Some(bytes)
        }
        other => Some(glib_utf8(&[
            0x7532,
            prefix_word(other).expect("a converted variant"),
            0x4e59,
        ])),
    }
}

/// Calls `entry` with `variant`'s prefix, NULL for `null`.
fn with_prefix(variant: &str, entry: impl FnOnce(*const c_char) -> bool) -> bool {
    let argument = prefix_argument(variant).map(c);
    entry(
        argument
            .as_ref()
            .map_or(std::ptr::null(), |text| text.as_ptr()),
    )
}

/// The system for `variant`: its own middle word, the valid one otherwise.
fn system_for(k: &mut Kit, variant: &str) {
    let word = prefix_word(variant).unwrap_or(0x4e01);
    prefix_system(k, word);
}

/// `prefix_prediction_case(entry, variant)`.
fn prediction(k: &mut Kit, entry: fn(P, *const c_char) -> bool, variant: &str) -> J {
    system_for(k, variant);
    let inst = k.inst();
    assert!(facade::set_options(k.ctx(), 1 << 5));
    facade::parse_more_full_pinyins(inst, c("ba1").as_ptr());
    facade::guess_sentence(inst);
    facade::guess_candidates(inst, 0, 0x1e);
    let ret = with_prefix(variant, |prefix| entry(inst, prefix));
    let count = n_candidate(inst);
    let rows: Vec<J> = (0..count)
        .map(|index| {
            let cand = candidate_at(inst, index).expect("get_candidate");
            let mut text = UNTOUCHED as usize as *const c_char;
            let mut kind = UNTOUCHED as i32;
            facade::get_candidate_string(inst, cand, &raw mut text);
            facade::get_candidate_type(inst, cand, &raw mut kind);
            J::L(vec![J::from(kind), phrase_text_bytes(text, false)])
        })
        .collect();
    obj! { "ret" => ret, "rows" => rows }
}

/// `prefix_sentence_case(variant)`.
fn sentence(k: &mut Kit, variant: &str) -> J {
    system_for(k, variant);
    let inst = k.inst();
    assert!(facade::set_options(k.ctx(), 1 << 5));
    facade::parse_more_full_pinyins(inst, c("ba1").as_ptr());
    let seeded = if variant == "null" || variant == "invalid" {
        with_prefix("valid", |prefix| {
            facade::guess_sentence_with_prefix(inst, prefix)
        })
    } else {
        facade::guess_sentence(inst)
    };
    let ret = with_prefix(variant, |prefix| {
        facade::guess_sentence_with_prefix(inst, prefix)
    });
    let mut out = UNTOUCHED as usize as *mut c_char;
    let read = if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut out)
    } else {
        facade::get_sentence_at(inst, 0, &raw mut out)
    };
    obj! {
        "seeded" => seeded,
        "ret" => ret,
        "sentence" => J::L(vec![J::from(read), phrase_text_bytes(out, true)]),
    }
}

/// `prefix-convert/{predicted,punctuation}-{variant}` and
/// `prefix-convert/{mode}-sentence-{variant}`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let rest = id
        .strip_prefix("prefix-convert/")
        .unwrap_or_else(|| panic!("{id}: not a prefix-convert case"));
    if let Some(variant) = rest.strip_prefix("predicted-") {
        return prediction(k, facade::guess_predicted_candidates, variant);
    }
    if let Some(variant) = rest.strip_prefix("punctuation-") {
        return prediction(
            k,
            facade::guess_predicted_candidates_with_punctuations,
            variant,
        );
    }
    let mode = if facade::ZHUYIN { "zhuyin" } else { "pinyin" };
    let variant = rest
        .strip_prefix(&format!("{mode}-sentence-"))
        .unwrap_or_else(|| panic!("{id}: not this facade's case"));
    sentence(k, variant)
}
