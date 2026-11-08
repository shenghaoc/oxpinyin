//! The pinyin_out_parameters family's case bodies: each answers the result document the old case returned.
//!
//! Pinyin return values and out-parameters the pin leaves alone or writes
//! (lane C, #542).

use crate::facade;
use crate::readers::{c, owned_out, seeded, tokens_of};
use crate::support::{J, Kit, P, UNTOUCHED};

/// Python's `repr` of a byte string: single quotes unless the bytes hold a
/// single quote and no double quote.
fn repr(bytes: &[u8]) -> String {
    let quote = if bytes.contains(&b'\'') && !bytes.contains(&b'"') {
        b'"'
    } else {
        b'\''
    };
    let mut out = String::from("b");
    out.push(char::from(quote));
    for &byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            _ if byte == quote => out.push_str(&format!("\\{}", char::from(byte))),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7e => out.push(char::from(byte)),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out.push(char::from(quote));
    out
}

/// `contract_abi.sentence_out`: the indexed sentence's flag and text.
fn sentence_out(inst: P, index: u8) -> J {
    let mut out = seeded();
    let ret = facade::get_sentence_at(inst, index, &raw mut out);
    J::from((ret, owned_out(out)))
}

/// A seeded integer out-parameter: `"untouched"` or its value.
fn untouched_or<T: Into<J> + PartialEq>(value: T, seed: T) -> J {
    if value == seed {
        "untouched".into()
    } else {
        value.into()
    }
}

/// Bytes as lowercase hex (`bytes.hex()`).
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `count` bytes at `pointer` as hex (`ctypes.string_at(...).hex()`).
fn hex_at(pointer: P, count: usize) -> String {
    // SAFETY: the old case read these bytes at the key pointers the facade
    // answered; the pin's function-static slots keep them readable.
    hex(unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), count) })
}

fn empty_parse_sentence(k: &mut Kit) -> J {
    let mut out = J::map();
    for text in [&b""[..], b"!"] {
        let inst = k.alloc();
        let n = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        let guessed = facade::guess_sentence(inst);
        out.set(repr(text), (n, guessed, sentence_out(inst, 0)));
    }
    let inst = k.alloc();
    out.set(
        "fresh",
        (facade::guess_sentence(inst), sentence_out(inst, 0)),
    );
    out
}

fn character_offset_out(k: &mut Kit) -> J {
    let (full, double) = ("full", "double");
    let mut out = J::map();
    for (label, parse, text, phrase, offset) in [
        ("recursion-fails-1", full, "nihao", "啊", 3),
        ("recursion-fails-2", full, "nihao", "你", 5),
        ("recursion-fails-3", full, "nihao", "好", 5),
        ("hit", full, "nihao", "你好", 5),
        ("no-token", full, "nihao", "x", 2),
        ("empty-phrase", full, "nihao", "", 2),
        ("no-parse", full, "", "你好", 0),
        ("keyless", full, "'", "你好", 0),
        ("double-fails", double, "nihk", "啊", 2),
        ("double-hit", double, "nihk", "你好", 4),
    ] {
        let inst = k.alloc();
        if parse == full {
            facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        } else {
            facade::parse_more_double_pinyins(inst, c(text).as_ptr());
        }
        let mut length = UNTOUCHED as usize;
        let ret = facade::get_character_offset(inst, c(phrase).as_ptr(), offset, &raw mut length);
        out.set(label, (ret, untouched_or(length, UNTOUCHED as usize)));
    }
    out
}

fn token_get_phrase_out(k: &mut Kit) -> J {
    let mut out = J::map();
    for token in [0xFFFF_FFFF_u32, 0x0DEA_DBEE, 0x0100_0000] {
        let (mut length, mut text) = (UNTOUCHED, seeded());
        let ret = facade::token_get_phrase(k.inst(), token, &raw mut length, &raw mut text);
        out.set(
            format!("{token:#x}"),
            (ret, untouched_or(length, UNTOUCHED), owned_out(text)),
        );
    }
    out
}

/// A GArray's length and its contents as hex (two-byte elements).
fn keys_view(keys: *mut glib_sys::GArray) -> (u32, String) {
    // SAFETY: a live GArray of two-byte elements: a non-empty one's `data` holds `len` of them.
    unsafe {
        let view = &*keys;
        if view.len == 0 {
            return (0, String::new());
        }
        let bytes = std::slice::from_raw_parts(view.data.cast::<u8>(), view.len as usize * 2);
        (view.len, hex(bytes))
    }
}

fn nth_pronunciation_range(k: &mut Kit) -> J {
    let token = tokens_of(k, "你好")[0];
    let inst = k.inst();
    let mut count = UNTOUCHED;
    facade::token_get_n_pronunciation(inst, token, &raw mut count);
    let mut out = obj! { "n" => count };
    for nth in [0, count - 1, count, count + 1, 0xFFFF_FFFF] {
        // SAFETY: a fresh GArray of two-byte elements, left allocated as the case did.
        let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
        let ret = facade::token_get_nth_pronunciation(inst, token, nth, keys.cast());
        let (len, content) = keys_view(keys);
        let mut row = vec![J::from(ret), J::from(len)];
        if nth < count {
            row.push(J::from(content.clone()));
        }
        out.set(format!("nth {nth}"), row);
        out.set(format!("~content nth {nth}"), content);
    }
    // SAFETY: a fresh GArray of two-byte elements, left allocated as the case did.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    let seed: u16 = 0x1234;
    // SAFETY: appends one two-byte element from a live local.
    unsafe { glib_sys::g_array_append_vals(keys, (&raw const seed).cast(), 1) };
    let ret = facade::token_get_nth_pronunciation(inst, 0xFFFF_FFFF, 0, keys.cast());
    // SAFETY: the array is live: its header's length is readable.
    let len = unsafe { (*keys).len };
    out.set("unknown token", (ret, len));
    out
}

fn static_key_slots(k: &mut Kit) -> J {
    let (first, second) = (k.alloc(), k.alloc());
    facade::parse_more_full_pinyins(first, c("ni").as_ptr());
    facade::parse_more_full_pinyins(second, c("hao").as_ptr());
    let null = std::ptr::null_mut;
    let (mut a, mut b, mut ra, mut rb): (P, P, P, P) = (null(), null(), null(), null());
    let rets = vec![
        facade::get_pinyin_key(first, 0, &raw mut a),
        facade::get_pinyin_key(second, 0, &raw mut b),
        facade::get_pinyin_key_rest(first, 0, &raw mut ra),
        facade::get_pinyin_key_rest(second, 0, &raw mut rb),
    ];
    let mut out = obj! { "ret" => rets };
    out.set("key same pointer", a == b);
    out.set("rest same pointer", ra == rb);
    out.set("first key now", hex_at(a, 2));
    out.set("first rest now", hex_at(ra, 4));
    facade::free_instance(first);
    out.set("key after free", hex_at(a, 2));
    out.set("rest after free", hex_at(ra, 4));
    out
}

fn key_rest_length(k: &mut Kit) -> J {
    let mut out = J::map();
    for (begin, end) in [
        (0_u16, 2_u16),
        (5, 2),
        (2, 2),
        (0, 65535),
        (65535, 0),
        (1, 65535),
    ] {
        let mut length: u16 = 0xBEEF;
        let mut rest = [begin, end];
        let ret =
            facade::get_pinyin_key_rest_length(k.inst(), rest.as_mut_ptr().cast(), &raw mut length);
        out.set(format!("{begin}..{end}"), (ret, length));
    }
    out
}

fn sentence_before_guess(k: &mut Kit) -> J {
    let inst = k.alloc();
    let both = |inst| J::from((sentence_out(inst, 0), sentence_out(inst, 255)));
    let mut out = obj! { "fresh" => both(inst) };
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    out.set("parsed", both(inst));
    facade::guess_sentence(inst);
    out.set("guessed", sentence_out(inst, 0));
    facade::reset(inst);
    out.set("reset", sentence_out(inst, 0));
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    out.set("reparsed", sentence_out(inst, 0));
    facade::guess_sentence(inst);
    facade::parse_more_full_pinyins(inst, c("xian").as_ptr());
    out.set("guessed then reparsed", sentence_out(inst, 0));
    out
}

fn sentence_before_guess_schemes(k: &mut Kit) -> J {
    let inst = k.alloc();
    facade::parse_more_double_pinyins(inst, c("nihk").as_ptr());
    let mut out = obj! { "double" => sentence_out(inst, 0) };
    let inst = k.alloc();
    facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    out.set("chewing", sentence_out(inst, 0));
    out
}

fn guess_sentence_keyless(k: &mut Kit) -> J {
    let mut out = J::map();
    for text in [&b"'"[..], b"''", b"", b"!", b"nihao'"] {
        let inst = k.alloc();
        let n = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        let ret = facade::guess_sentence(inst);
        out.set(repr(text), (n, ret, sentence_out(inst, 0)));
    }
    for text in [&b"'"[..], b"!"] {
        let inst = k.alloc();
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
        let first = facade::guess_sentence(inst);
        facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        let again = facade::guess_sentence(inst);
        out.set(
            format!("stale after {}", repr(text)),
            (first, again, sentence_out(inst, 0)),
        );
        let inst = k.alloc();
        facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        let ret = facade::guess_sentence_with_prefix(inst, c("你").as_ptr());
        out.set(
            format!("with-prefix {}", repr(text)),
            (ret, sentence_out(inst, 0)),
        );
    }
    out
}

fn guess_candidates_keyless(k: &mut Kit) -> J {
    let mut out = J::map();
    for text in [&b"'"[..], b"'''", b"!", b"", b"ni'"] {
        for stale in [false, true] {
            let inst = k.alloc();
            if stale {
                facade::parse_more_full_pinyins(inst, c("ni").as_ptr());
                facade::guess_sentence(inst);
            }
            facade::parse_more_full_pinyins(inst, c(text).as_ptr());
            let ret = facade::guess_candidates(inst, 0, 0x1E);
            let mut count = UNTOUCHED;
            facade::get_n_candidate(inst, &raw mut count);
            let stale_text = if stale { "True" } else { "False" };
            out.set(
                format!("{} stale={stale_text}", repr(text)),
                (ret, count > 0),
            );
        }
    }
    out
}

fn sentence_with_prefix_bytes(k: &mut Kit) -> J {
    let mut out = J::map();
    for (label, prefix) in [
        ("invalid", b"\xff\xfe".to_vec()),
        ("valid", "你".as_bytes().to_vec()),
        ("empty", Vec::new()),
        ("tail-invalid", ["你".as_bytes(), b"\xff"].concat()),
    ] {
        let inst = k.alloc();
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
        let ret = facade::guess_sentence_with_prefix(inst, c(&prefix).as_ptr());
        out.set(label, (ret, sentence_out(inst, 0), sentence_out(inst, 1)));
    }
    out
}

/// The registered cases.
pub fn case(k: &mut Kit, id: &str) -> J {
    match id {
        "empty-parse-sentence" => empty_parse_sentence(k),
        "character-offset-out" => character_offset_out(k),
        "token-get-phrase-out" => token_get_phrase_out(k),
        "nth-pronunciation-range" => nth_pronunciation_range(k),
        "static-key-slots" => static_key_slots(k),
        "key-rest-length" => key_rest_length(k),
        "sentence-before-guess" => sentence_before_guess(k),
        "sentence-before-guess-schemes" => sentence_before_guess_schemes(k),
        "guess-sentence-keyless" => guess_sentence_keyless(k),
        "guess-candidates-keyless" => guess_candidates_keyless(k),
        "unload-phrase-library-repeat" => {
            let ctx = k.ctx();
            let unload: Vec<bool> = [2, 2, 2, 1, 3, 15, 2]
                .into_iter()
                .map(|index| facade::unload_phrase_library(ctx, index))
                .collect();
            obj! { "unload" => unload }
        }
        "sentence-with-prefix-bytes" => sentence_with_prefix_bytes(k),
        other => panic!("no case {other}"),
    }
}
