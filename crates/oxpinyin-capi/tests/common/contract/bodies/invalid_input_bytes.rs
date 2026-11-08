//! Invalid UTF-8 bytes in parse and import arguments (#587).

use std::collections::BTreeMap;

use crate::facade;
use crate::readers::{c, consumed, key_out};
use crate::support::{Kit, take_text};

/// Python's `repr` of a byte string, the old case's map label.
fn repr(bytes: &[u8]) -> String {
    let mut out = String::from("b'");
    for &byte in bytes {
        match byte {
            b'\'' => out.push_str("\\'"),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(char::from(byte)),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out.push('\'');
    out
}

/// Each input's consumed bytes on a fresh instance, under its repr.
fn consumed_all(
    k: &mut Kit,
    parse: fn(crate::support::P, *const std::ffi::c_char) -> usize,
    label: &str,
    texts: &[&[u8]],
) -> BTreeMap<String, usize> {
    texts
        .iter()
        .map(|text| (format!("{label}{}", repr(text)), consumed(k, parse, text)))
        .collect()
}

pub fn parse_more_full(k: &mut Kit) -> BTreeMap<String, usize> {
    let texts: [&[u8]; 5] = [
        b"ni\xffhao",
        b"nihao\xff",
        b"\xffnihao",
        b"ni\xe4\xbd",
        b"ni\xc0\x80hao",
    ];
    consumed_all(k, facade::parse_more_full_pinyins, "", &texts)
}

pub fn parse_more_double(k: &mut Kit) -> BTreeMap<String, usize> {
    consumed_all(
        k,
        facade::parse_more_double_pinyins,
        "",
        &[b"ni\xffhk", b"nihk\xff", b"\xffnihk"],
    )
}

pub fn parse_more_chewing(k: &mut Kit) -> BTreeMap<String, usize> {
    consumed_all(
        k,
        facade::parse_more_chewings,
        "",
        &[b"su\xff3", b"su3\xff", b"\xffsu3"],
    )
}

#[derive(Debug, PartialEq)]
pub struct AddPhrase {
    pub added: Vec<bool>,
    pub rows: Vec<(Option<String>, Option<String>, i32)>,
}

/// Imports two readings with invalid bytes, then exports library 7.
pub fn add_phrase_pinyin(k: &mut Kit) -> AddPhrase {
    let ctx = k.ctx();
    let it = facade::begin_add_phrases(ctx, 7);
    let added = vec![
        facade::iterator_add_phrase(it, c("你好").as_ptr(), c(b"ni'ha\xffo").as_ptr(), 1),
        facade::iterator_add_phrase(it, c("世界").as_ptr(), c(b"shi'jie\xff").as_ptr(), 1),
    ];
    facade::end_add_phrases(it);
    let out = facade::begin_get_phrases(ctx, 7);
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(out) {
        let (mut phrase, mut pinyin, mut n) = (std::ptr::null_mut(), std::ptr::null_mut(), 0_i32);
        facade::iterator_get_next_phrase(out, &raw mut phrase, &raw mut pinyin, &raw mut n);
        rows.push((take_text(phrase), take_text(pinyin), n));
    }
    AddPhrase { added, rows }
}

#[derive(Debug, PartialEq)]
pub struct WholeArgument {
    pub parse_full_pinyin: (bool, u16),
    pub lookup_tokens: bool,
    pub remember: bool,
    pub add_bad_phrase: bool,
    pub parsed: usize,
}

/// Entry points that convert the whole argument and refuse invalid bytes.
pub fn whole_argument(k: &mut Kit) -> WholeArgument {
    let parse_full_pinyin = key_out(k, b"ni\xff");
    let inst = k.inst();
    // SAFETY: a fresh GArray of four-byte elements, left allocated as the case did.
    let tokens = unsafe { glib_sys::g_array_new(0, 0, 4) };
    let lookup_tokens = facade::lookup_tokens(
        inst,
        c(["你".as_bytes(), b"\xff"].concat()).as_ptr(),
        tokens.cast(),
    );
    let parsed = facade::parse_more_full_pinyins(inst, c("nihaoshijie").as_ptr());
    let remember = facade::remember_user_input(
        inst,
        c(["你好世界".as_bytes(), b"\xff"].concat()).as_ptr(),
        1,
    );
    let it = facade::begin_add_phrases(k.ctx(), 7);
    let add_bad_phrase = facade::iterator_add_phrase(
        it,
        c(["你好".as_bytes(), b"\xff"].concat()).as_ptr(),
        c("ni'hao").as_ptr(),
        1,
    );
    facade::end_add_phrases(it);
    WholeArgument {
        parse_full_pinyin,
        lookup_tokens,
        remember,
        add_bad_phrase,
        parsed,
    }
}

/// The zhuyin facade's full-pinyin and chewing parses of invalid bytes.
pub fn zhuyin_parse_more(k: &mut Kit) -> BTreeMap<String, usize> {
    let mut out = consumed_all(
        k,
        facade::parse_more_full_pinyins,
        "parse_more_full_pinyins ",
        &[b"ni3\xffhao3", b"ni3hao3\xff", b"\xffni3"],
    );
    out.extend(consumed_all(
        k,
        facade::parse_more_chewings,
        "parse_more_chewings ",
        &[b"su\xff3", b"su3\xff", b"\xffsu3"],
    ));
    out
}

/// The zhuyin import of a whole, a tail-invalid and a mid-invalid reading.
pub fn zhuyin_add_phrase(k: &mut Kit) -> BTreeMap<String, bool> {
    let readings: [(&str, Vec<u8>); 3] = [
        ("whole", "ㄋㄧˇ ㄏㄠˇ".as_bytes().to_vec()),
        ("tail-invalid", ["ㄋㄧˇ ㄏㄠˇ".as_bytes(), b"\xff"].concat()),
        (
            "mid-invalid",
            ["ㄋㄧˇ".as_bytes(), b"\xff", "ㄏㄠˇ".as_bytes()].concat(),
        ),
    ];
    let mut out = BTreeMap::new();
    for (label, reading) in readings {
        let it = facade::begin_add_phrases(k.ctx(), 7);
        out.insert(
            label.to_owned(),
            facade::iterator_add_phrase(it, c("你好").as_ptr(), c(&reading).as_ptr(), 1),
        );
        facade::end_add_phrases(it);
    }
    out
}
