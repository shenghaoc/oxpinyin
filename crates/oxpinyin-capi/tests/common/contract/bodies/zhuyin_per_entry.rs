//! The zhuyin_per_entry family's case bodies: each answers the result document the old case returned.
//!
//! The zhuyin facade's per-entry contracts: single-key parse, keyboard
//! membership, key rests, phrase results and string getters (#563).

use std::ffi::{CStr, c_char};

use crate::facade;
use crate::readers::{c, owned_out, seeded, tokens_of};
use crate::support::{J, Kit, P, UNTOUCHED, borrowed_text};

/// The sixteen-bit key sentinel (`UNTOUCHED & 0xffff`).
const KEY_SEED: u16 = (UNTOUCHED & 0xFFFF) as u16;

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

/// Python's `str` of an optional scheme.
fn scheme_text(scheme: Option<i32>) -> String {
    scheme.map_or_else(|| "None".to_owned(), |scheme| scheme.to_string())
}

/// `zy_strv`: a keyboard probe's `gchar **`, decoded and freed.
fn zy_strv(pointer: *mut *mut c_char) -> J {
    if pointer as usize == UNTOUCHED as usize {
        return "untouched".into();
    }
    if pointer.is_null() {
        return J::Null;
    }
    let mut out = Vec::new();
    // SAFETY: a non-NULL vector the facade handed over is a NULL-terminated
    // array of NUL-terminated strings the caller owns and frees once.
    unsafe {
        let mut index = 0;
        while !(*pointer.add(index)).is_null() {
            out.push(
                CStr::from_ptr(*pointer.add(index))
                    .to_string_lossy()
                    .into_owned(),
            );
            index += 1;
        }
        glib_sys::g_strfreev(pointer);
    }
    out.into()
}

/// `parse_chewing_probe`: each scheme's single-key parses of `texts`.
fn parse_chewing_probe(k: &mut Kit, schemes: &[Option<i32>], texts: &[&[u8]]) -> J {
    let mut out = J::map();
    for &scheme in schemes {
        if let Some(scheme) = scheme {
            facade::set_chewing_scheme(k.ctx(), scheme);
        }
        let mut row = J::map();
        for text in texts {
            let mut key = KEY_SEED;
            let ret = facade::parse_chewing(k.inst(), c(text).as_ptr(), (&raw mut key).cast());
            row.set(repr(text), (ret, key));
        }
        out.set(scheme_text(scheme), row);
    }
    out
}

/// `in_chewing_probe`: each byte's keyboard membership and symbols.
fn in_chewing_probe(k: &mut Kit, scheme: Option<i32>, options: Option<u32>, keys: &[u8]) -> J {
    if let Some(scheme) = scheme {
        facade::set_chewing_scheme(k.ctx(), scheme);
    }
    if let Some(options) = options {
        facade::set_options(k.ctx(), options);
    }
    let mut row = J::map();
    for &key in keys {
        let mut symbols = UNTOUCHED as usize as *mut *mut c_char;
        let ret = facade::in_chewing_keyboard(k.inst(), key as c_char, &raw mut symbols);
        row.set(repr(&[key]), (ret, zy_strv(symbols)));
    }
    row
}

/// The optional position accessor's four argument shapes on a live rest.
fn positions(inst: P, slot: P, row: &mut J) {
    let (mut begin, mut end) = (0xABCD_u16, 0xABCD_u16);
    let ret = facade::get_zhuyin_key_rest_positions(inst, slot, &raw mut begin, &raw mut end);
    row.set("both", (ret, begin, end));
    let mut begin = 0xABCD_u16;
    let ret =
        facade::get_zhuyin_key_rest_positions(inst, slot, &raw mut begin, std::ptr::null_mut());
    row.set("begin-only", (ret, begin));
    let mut end = 0xABCD_u16;
    let ret = facade::get_zhuyin_key_rest_positions(inst, slot, std::ptr::null_mut(), &raw mut end);
    row.set("end-only", (ret, end));
    let ret = facade::get_zhuyin_key_rest_positions(
        inst,
        slot,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    row.set("neither", ret);
}

/// `key_rest_positions_probe`: rest lookups and optional positions.
fn key_rest_positions_probe(k: &mut Kit, text: &str, offsets: &[usize]) -> J {
    facade::set_options(k.ctx(), 0x1aa);
    let inst = k.inst();
    facade::parse_more_chewings(inst, c(text).as_ptr());
    let mut out = J::map();
    for &offset in offsets {
        let mut slot = UNTOUCHED as usize as P;
        let got = facade::get_zhuyin_key_rest(inst, offset, &raw mut slot);
        let shown = if slot as usize == UNTOUCHED as usize {
            "untouched"
        } else if slot.is_null() {
            "null"
        } else {
            "set"
        };
        let mut row = obj! { "rest_ret" => got, "rest" => shown };
        if got && !slot.is_null() {
            positions(inst, slot, &mut row);
        }
        out.set(offset.to_string(), row);
    }
    out
}

/// `read`: the phrase count, its tokens and two out-of-range slots.
fn read_phrases(inst: P, prefix: &str, out: &mut J) {
    let mut count = UNTOUCHED;
    let n_ret = facade::get_n_phrase(inst, &raw mut count);
    let limit = if count == UNTOUCHED { 0 } else { count };
    let slot_at = |index: u32| {
        let mut slot = 0xDEAD_BEEF_u32;
        let ret = facade::get_phrase_token(inst, index, &raw mut slot);
        J::from((ret, slot))
    };
    let rows: Vec<J> = (0..limit).map(slot_at).collect();
    let past: Vec<J> = [limit, 0xFFFF_FFFF].into_iter().map(slot_at).collect();
    out.set(format!("{prefix}-n"), (n_ret, count));
    out.set(format!("{prefix}-tokens"), rows);
    out.set(format!("{prefix}-past"), past);
}

/// `phrase_tokens_probe`: segmentations and a final reset on one instance.
fn phrase_tokens_probe(k: &mut Kit, texts: &[&[u8]]) -> J {
    let inst = k.inst();
    let mut empty = J::map();
    read_phrases(inst, "empty", &mut empty);
    let mut out = obj! { "empty-instance" => empty };
    for text in texts {
        let mut row = obj! { "segment" => facade::phrase_segment(inst, c(text).as_ptr()) };
        read_phrases(inst, "row", &mut row);
        out.set(repr(text), row);
    }
    facade::phrase_segment(inst, c("你好").as_ptr());
    facade::reset(inst);
    let mut reset = J::map();
    read_phrases(inst, "reset", &mut reset);
    out.set("after-reset", reset);
    out
}

/// `zhuyin-string-getters`: both string getters on a parsed key and a zero key.
fn string_getters(k: &mut Kit) -> J {
    let inst = k.inst();
    let mut key = 0_u16;
    assert!(facade::parse_chewing(
        inst,
        c("su3").as_ptr(),
        (&raw mut key).cast()
    ));
    let mut zero = 0_u16;
    let mut out = J::map();
    for scheme in [None, Some(1), Some(2), Some(3)] {
        if let Some(scheme) = scheme {
            assert!(facade::set_full_pinyin_scheme(k.ctx(), scheme));
        }
        for (label, getter) in [
            (
                "zhuyin_string",
                facade::get_zhuyin_string as fn(P, P, *mut *mut c_char) -> bool,
            ),
            ("pinyin_string", facade::get_pinyin_string),
        ] {
            let scheme = scheme_text(scheme);
            let mut string = seeded();
            let ret = getter(inst, (&raw mut key).cast(), &raw mut string);
            out.set(format!("{label} scheme {scheme}"), (ret, owned_out(string)));
            let mut string = seeded();
            let ret = getter(inst, (&raw mut zero).cast(), &raw mut string);
            out.set(
                format!("{label} scheme {scheme} zero-key"),
                (ret, owned_out(string)),
            );
        }
    }
    out
}

/// `zhuyin-cursor-offset-range`: five cursor offsets of a parsed instance.
fn cursor_offset_range(k: &mut Kit) -> J {
    let inst = k.alloc();
    facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    let mut out = J::map();
    for cursor in [0_usize, 1, 2, 3, 6] {
        let mut res = UNTOUCHED as usize;
        let ret = facade::get_zhuyin_offset(inst, cursor, &raw mut res);
        let res: J = if res == UNTOUCHED as usize {
            "untouched".into()
        } else {
            res.into()
        };
        out.set(cursor.to_string(), (ret, res));
    }
    out
}

/// `zhuyin-null-instance-defined-getters`: five getters with a NULL instance.
fn null_instance_getters(k: &mut Kit) -> J {
    let inst = k.inst();
    facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    let mut key = 0_u16;
    assert!(facade::parse_chewing(
        inst,
        c("su3").as_ptr(),
        (&raw mut key).cast()
    ));
    let mut rest = UNTOUCHED as usize as P;
    assert!(facade::get_zhuyin_key_rest(inst, 0, &raw mut rest) && !rest.is_null());
    assert!(facade::guess_candidates_after_cursor(inst, 0));
    let mut cand = UNTOUCHED as usize as P;
    assert!(facade::get_candidate(inst, 0, &raw mut cand) && !cand.is_null());
    let null: P = std::ptr::null_mut();
    let mut string = seeded();
    let ret = facade::get_zhuyin_string(null, (&raw mut key).cast(), &raw mut string);
    let mut out = obj! { "zhuyin_string" => (ret, owned_out(string)) };
    let (mut begin, mut end) = (0xABCD_u16, 0xABCD_u16);
    let ret = facade::get_zhuyin_key_rest_positions(null, rest, &raw mut begin, &raw mut end);
    out.set("key_rest_positions", (ret, begin, end));
    let mut length = 0xBEEF_u16;
    let ret = facade::get_zhuyin_key_rest_length(null, rest, &raw mut length);
    out.set("key_rest_length", (ret, length));
    let mut kind = 0x7FFF_FFFF_i32;
    let ret = facade::get_candidate_type(null, cand, &raw mut kind);
    out.set("candidate_type", (ret, kind));
    let mut text = UNTOUCHED as usize as *const c_char;
    let ret = facade::get_candidate_string(null, cand, &raw mut text);
    let text: J = if text as usize == UNTOUCHED as usize {
        "untouched".into()
    } else {
        borrowed_text(text).into()
    };
    out.set("candidate_string", (ret, text));
    out
}

/// `zhuyin-token-get-phrase-optional-out`: the four out-parameter shapes.
fn token_get_phrase_optional_out(k: &mut Kit) -> J {
    let known = tokens_of(k, "你好")[0];
    let inst = k.inst();
    let mut out = J::map();
    for (label, token) in [("known", known), ("unknown", 0xFFFF_FFFF)] {
        let (mut length, mut text) = (UNTOUCHED, seeded());
        let both = facade::token_get_phrase(inst, token, &raw mut length, &raw mut text);
        let length: J = if length == UNTOUCHED {
            "untouched".into()
        } else {
            length.into()
        };
        out.set(label, (both, length, owned_out(text)));
        let mut length = UNTOUCHED;
        let ret = facade::token_get_phrase(inst, token, &raw mut length, std::ptr::null_mut());
        out.set(format!("{label}-len-only"), (ret, length));
        let mut text = seeded();
        let ret = facade::token_get_phrase(inst, token, std::ptr::null_mut(), &raw mut text);
        out.set(format!("{label}-text-only"), (ret, owned_out(text)));
        out.set(
            format!("{label}-neither"),
            facade::token_get_phrase(inst, token, std::ptr::null_mut(), std::ptr::null_mut()),
        );
    }
    out
}

/// The registered cases.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(scheme) = id.strip_prefix("zhuyin-in-chewing-keyboard-scheme-") {
        let scheme = scheme.parse().expect("a scheme");
        return in_chewing_probe(k, Some(scheme), None, b"sui3");
    }
    match id {
        "zhuyin-parse-chewing-key" => parse_chewing_probe(
            k,
            &[
                None,
                Some(1),
                Some(2),
                Some(3),
                Some(4),
                Some(5),
                Some(6),
                Some(8),
                Some(9),
            ],
            &[
                b"su3", b"cl3", b"su", b"", b"zzz", b"su3cl3", b"su3x", b"1", b"3", b"ni",
            ],
        ),
        "zhuyin-in-chewing-keyboard-key" => in_chewing_probe(k, None, None, b"su31azq!~\x80\x00i"),
        "zhuyin-in-chewing-keyboard-no-tone" => in_chewing_probe(k, None, Some(0), b"s31"),
        "zhuyin-key-rest-positions-out" => key_rest_positions_probe(k, "su3cl3", &[0, 1, 2]),
        "zhuyin-phrase-result-tokens" => {
            phrase_tokens_probe(k, &["你好".as_bytes(), b"", b"123", "你好世界".as_bytes()])
        }
        "zhuyin-string-getters" => string_getters(k),
        "zhuyin-cursor-offset-range" => cursor_offset_range(k),
        "zhuyin-null-instance-defined-getters" => null_instance_getters(k),
        "zhuyin-token-get-phrase-optional-out" => token_get_phrase_optional_out(k),
        other => panic!("no case {other}"),
    }
}
