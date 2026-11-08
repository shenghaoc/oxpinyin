//! The punctuation family's case bodies: each answers the result document the old case returned.
//!
//! #1144: `punct.bin` decoding (`PunctTableEntry::unescape`,
//! `src/storage/punct_table.cpp:57-94` at pin 074a2219). #928: punctuation order
//! and duplicates (`src/pinyin.cpp:2463-2494`). #1272: an aliased prefix for the
//! punctuation entry, whose borrowed form the pin is never driven with.

use std::ffi::{CStr, c_char};
use std::path::Path;

use crate::facade;
use crate::fixtures::{craft_tree, write_user_conf};
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, full_inst, n_candidate, owned_out, seeded,
    tokens_of,
};
use crate::support::{J, Kit, P, UNTOUCHED};

/// The punctuation candidate type.
const PUNCTUATION: i32 = 8;

/// `punct_words`: `u32` words little-endian, the `punct.bin` value layout.
fn punct_words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// `words` then `tail` bytes.
fn with_tail(words: &[u32], tail: &[u8]) -> Vec<u8> {
    let mut bytes = punct_words(words);
    bytes.extend(tail);
    bytes
}

/// Copies the directory tree `from` into `to` (`shutil.copytree`).
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("copy directory");
    for entry in std::fs::read_dir(from).expect("read the system data") {
        let entry = entry.expect("system entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            crate::fixtures::writable_copy(&entry.path(), &target);
        }
    }
}

/// `punct_fixture`: replaces the system `punct.bin` with `rows` (every token of
/// each text gets its value) in a copy of the system tree, closing the kit.
fn punct_fixture(k: &mut Kit, rows: &[(&str, Vec<u8>)]) {
    let mut table = Vec::new();
    for (text, value) in rows {
        for token in tokens_of(k, text) {
            table.push((token.to_le_bytes().to_vec(), value.clone()));
        }
    }
    k.close();
    let copied = tempfile::Builder::new()
        .prefix("sys-")
        .tempdir_in(&k.scratch)
        .expect("copied system")
        .keep();
    copy_tree(&k.data, &copied);
    k.data = copied;
    craft_tree(&k.data.join("punct.bin"), &table);
}

/// A borrowed C string's bytes as hex, `None` for NULL.
fn hex_of(pointer: *const c_char) -> J {
    if pointer.is_null() {
        return J::Null;
    }
    // SAFETY: a non-NULL string the facade handed out is NUL-terminated.
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    J::from(
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
}

/// A candidate's borrowed string pointer.
fn string_of(inst: P, cand: P) -> *const c_char {
    let mut text: *const c_char = std::ptr::null();
    facade::get_candidate_string(inst, cand, &raw mut text);
    text
}

/// `punct_rows`: the punctuation prediction for `prefix`, its punctuation rows'
/// bytes, the first rows' kinds and the other rows' count.
fn punct_rows(k: &mut Kit, prefix: &str) -> J {
    let inst = k.inst();
    let ret = facade::guess_predicted_candidates_with_punctuations(inst, c(prefix).as_ptr());
    let mut count = 0_u32;
    assert!(facade::get_n_candidate(inst, &raw mut count));
    let (mut kinds, mut puncts) = (Vec::new(), Vec::new());
    for index in 0..count {
        let mut cand: P = std::ptr::null_mut();
        facade::get_candidate(inst, index, &raw mut cand);
        let kind = candidate_type(inst, cand);
        kinds.push(kind);
        if kind == PUNCTUATION {
            puncts.push(hex_of(string_of(inst, cand)));
        }
    }
    let others = kinds.len() - puncts.len();
    kinds.truncate(puncts.len() + 2);
    obj! { "ret" => ret, "puncts" => J::L(puncts), "kinds" => kinds, "others" => others }
}

/// `PUNCT_DECODE_CASES[name]`.
fn decode_value(name: &str) -> Vec<u8> {
    match name {
        "valid" => punct_words(&[0xFF0C, 0, 0x3002, 0]),
        "zero-length" => Vec::new(),
        "surrogate" => punct_words(&[0xD800, 0, 0xFF0C, 0xDFFF, 0]),
        "above-unicode" => {
            punct_words(&[0x11_0000, 0, 0x20_0000, 0, 0x400_0000, 0, 0xFFFF_FFFF, 0])
        }
        "unterminated" => punct_words(&[0xFF0C, 0, 0x3002, 0x3001]),
        "unterminated-zero" => punct_words(&[0xFF0C, 0, 0]),
        "misaligned-terminated" => with_tail(&[0xFF0C, 0], b"A"),
        "misaligned-unterminated" => with_tail(&[0xFF0C], b"AB"),
        "misaligned-short" => b"ABC".to_vec(),
        "empty" => punct_words(&[0, 0, 0xFF0C, 0]),
        "empty-leading" => punct_words(&[0, 0xFF0C, 0, 0x3002, 0]),
        other => panic!("no decode case {other}"),
    }
}

/// `punct_decode_case(value)`: `value` under 我, predicted after 我.
fn decode(k: &mut Kit, name: &str) -> J {
    let value = decode_value(name);
    // The misaligned-terminated and misaligned-short values are the two the old
    // case compared by value; they also differ from every other one.
    let misaligned =
        value == decode_value("misaligned-terminated") || value == decode_value("misaligned-short");
    if misaligned {
        write_user_conf(k);
    }
    punct_fixture(k, &[("我", value)]);
    let out = punct_rows(k, "我");
    if misaligned {
        return obj! {
            "ret" => out.get("ret").expect("ret").clone(),
            "puncts" => out.get("puncts").expect("puncts").clone(),
        };
    }
    out
}

/// `PUNCT_DEDUP_CASES[name]`.
fn dedup_rows(name: &str) -> Vec<(&'static str, Vec<u8>)> {
    match name {
        "distinct" => vec![("我", punct_words(&[0xFF0C, 0, 0x3002, 0, 0xFF01, 0]))],
        "within" => vec![(
            "我",
            punct_words(&[0xFF0C, 0, 0x3002, 0, 0xFF0C, 0, 0x3002, 0]),
        )],
        "across-prefixes" => vec![
            ("们", punct_words(&[0xFF0C, 0, 0xFF01, 0])),
            ("我们", punct_words(&[0x3002, 0, 0xFF0C, 0])),
        ],
        "empty" => vec![("我", punct_words(&[0, 0, 0, 0, 0xFF0C, 0]))],
        "empty-forms" => vec![("我", punct_words(&[0, 0, 0, 0xFF0C, 0, 0x3002, 0]))],
        "non-scalar" => vec![
            ("们", punct_words(&[0xD800, 0, 0x11_0000, 0])),
            ("我们", punct_words(&[0x11_0000, 0, 0xD800, 0])),
        ],
        other => panic!("no dedup case {other}"),
    }
}

/// `punct_dedup_case(rows, prefix)`.
fn dedup(k: &mut Kit, name: &str) -> J {
    let rows = dedup_rows(name);
    let prefix = if rows.iter().any(|(text, _)| *text == "我们") {
        "我们"
    } else {
        "我"
    };
    punct_fixture(k, &rows);
    punct_rows(k, prefix)
}

/// `punct-dedup/candidate`: the first predicted phrase for 我 stored as its punctuation.
fn dedup_candidate(k: &mut Kit) -> J {
    let inst = k.inst();
    assert!(facade::guess_predicted_candidates(inst, c("我").as_ptr()));
    let cand = candidate_at(inst, 0).expect("get_candidate");
    let first = candidate_text(inst, cand).expect("get_candidate_string");
    let mut words: Vec<u32> = first.chars().map(u32::from).collect();
    words.extend([0, 0xFF0C, 0]);
    punct_fixture(k, &[("我", punct_words(&words))]);
    let mut out = punct_rows(k, "我");
    let inst = k.inst();
    let start = out.get("puncts").expect("puncts").items().len();
    let mut texts = Vec::new();
    for index in start..start + 3 {
        let mut cand: P = std::ptr::null_mut();
        if facade::get_candidate(inst, u32::try_from(index).expect("index"), &raw mut cand) {
            texts.push(hex_of(string_of(inst, cand)));
        }
    }
    out.set("first predicted", J::L(texts));
    out
}

/// `contract_abi.sentence_out`: the pinyin facade's indexed sentence.
fn sentence_out(inst: P, index: u8) -> J {
    let mut out = seeded();
    let ret = facade::get_sentence_at(inst, index, &raw mut out);
    J::from((ret, owned_out(out)))
}

/// The row index of a `predicted` alias source: the first non-punctuation row
/// of a punctuation prediction after 我 whose text, predicted on a second
/// instance, gives rows.
fn predicted_source(k: &mut Kit, inst: P) -> u32 {
    facade::guess_predicted_candidates_with_punctuations(inst, c("我").as_ptr());
    let probe_inst = k.alloc();
    let mut index = 0;
    loop {
        let cand = candidate_at(inst, index).expect("get_candidate");
        if candidate_type(inst, cand) != PUNCTUATION {
            let text = c(candidate_text(inst, cand).expect("text"));
            facade::guess_predicted_candidates_with_punctuations(probe_inst, text.as_ptr());
            if n_candidate(probe_inst) != 0 {
                return index;
            }
        }
        index += 1;
    }
}

/// `punct_alias_case(source, aliased)`.
fn alias(k: &mut Kit, source: &str, aliased: bool) -> J {
    let inst = full_inst(k, b"nihao");
    facade::guess_sentence(inst);
    let index = if source == "candidates" {
        facade::guess_candidates(inst, 0, 0x1E);
        0
    } else {
        predicted_source(k, inst)
    };
    let before: Vec<J> = (0..3).map(|i| sentence_out(inst, i)).collect();
    let mut cand = UNTOUCHED as usize as P;
    assert!(facade::get_candidate(inst, index, &raw mut cand));
    let mut text = UNTOUCHED as usize as *const c_char;
    assert!(facade::get_candidate_string(inst, cand, &raw mut text));
    // SAFETY: the getter answered a live NUL-terminated candidate string.
    let owned = unsafe { CStr::from_ptr(text) }.to_owned();
    let prefix = if aliased { text } else { owned.as_ptr() };
    let ret = facade::guess_predicted_candidates_with_punctuations(inst, prefix);
    let mut count = UNTOUCHED;
    assert!(facade::get_n_candidate(inst, &raw mut count));
    let kinds: Vec<i32> = (0..count.min(4))
        .map(|i| {
            let cand = candidate_at(inst, i).expect("get_candidate");
            candidate_type(inst, cand)
        })
        .collect();
    let after: Vec<J> = (0..3).map(|i| sentence_out(inst, i)).collect();
    obj! {
        "ret" => ret,
        "candidates" => count,
        "kinds" => kinds,
        "sentences kept" => before == after,
    }
}

/// `punct-decode/*`, `punct-dedup/*` and `punct-alias/*`.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(name) = id.strip_prefix("punct-decode/") {
        return decode(k, name);
    }
    if id == "punct-dedup/candidate" {
        return dedup_candidate(k);
    }
    if let Some(name) = id.strip_prefix("punct-dedup/") {
        return dedup(k, name);
    }
    let rest = id
        .strip_prefix("punct-alias/")
        .unwrap_or_else(|| panic!("{id}: not a punctuation case"));
    let (form, source) = rest.split_once('-').expect("form and source");
    alias(k, source, form == "borrowed")
}
