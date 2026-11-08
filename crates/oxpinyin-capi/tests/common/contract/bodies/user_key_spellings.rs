//! The user_key_spellings family's case bodies: each answers the result document the old case returned.
//!
//! #1418: user readings whose key is not a typed-input spelling. libpinyin's
//! import parses under PINYIN_CORRECT_ALL | USE_TONE (`pinyin.cpp:630-638`);
//! libzhuyin's parses chewing (`zhuyin.cpp:515-523`). A crafted `user.bin`
//! word is read back verbatim (`phrase_index.cpp:33-50,335-362`).

use std::ffi::{CStr, c_char};

use crate::facade;
use crate::fixtures::{chunk, ucs4_key};
use crate::readers::{c, candidate_at, garray_u16, import_rows, tokens_of, unigram_of};
use crate::support::{J, Kit, UNTOUCHED};

/// The user library.
const LIBRARY: u8 = 7;

/// PINYIN_INCOMPLETE, USE_TONE and PINYIN_CORRECT_ALL (074a2219 `pinyin_custom2.h:34,36,79`).
const PINYIN_LOOKUP_OPTIONS: u32 = (1 << 3) | (1 << 5) | (0xFF << 21);

/// `READINGS`: tag, mode, phrase, import reading and typed lookup.
const READINGS: [(&str, &str, &str, &str, &str); 6] = [
    ("egn", "pinyin", "嗯", "egn", "egn"),
    ("emg", "pinyin", "嗯", "emg", "emg"),
    ("nuen", "pinyin", "嫩", "nuen", "nuen"),
    ("den", "zhuyin", "扽", "ㄉㄣ", "ㄉㄣ"),
    ("den-tone4", "zhuyin", "扽", "ㄉㄣˋ", "ㄉㄣˋ"),
    ("eng", "zhuyin", "嗯", "ㄥ", "ㄥ"),
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `phrase_text_bytes`: `"untouched"` for the sentinel, `None` for NULL, else
/// the bytes in hex; `owned` frees them.
fn phrase_text_bytes(pointer: *const c_char, owned: bool) -> J {
    if pointer as usize == UNTOUCHED as usize {
        return J::from("untouched");
    }
    if pointer.is_null() {
        return J::Null;
    }
    // SAFETY: a non-NULL, non-sentinel pointer the facade wrote is a NUL-terminated string.
    let text = hex(unsafe { CStr::from_ptr(pointer) }.to_bytes());
    if owned {
        // SAFETY: an owned out string is a GLib allocation the caller frees once.
        unsafe { glib_sys::g_free(pointer.cast_mut().cast()) };
    }
    J::from(text)
}

/// `_reload_pron_keys`: a token's pronunciation count and each reading's
/// two-byte keys as hex.
fn reload_pron_keys(k: &mut Kit, token: u32) -> J {
    let inst = k.inst();
    let mut count = UNTOUCHED;
    let ok = facade::token_get_n_pronunciation(inst, token, &raw mut count);
    let mut out = vec![J::from(ok), J::from(count)];
    for nth in 0..if ok { count } else { 0 } {
        // SAFETY: a fresh GArray of two-byte elements, freed below.
        let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
        facade::token_get_nth_pronunciation(inst, token, nth, keys.cast());
        let bytes: Vec<u8> = garray_u16(keys)
            .iter()
            .flat_map(|key| key.to_le_bytes())
            .collect();
        // SAFETY: the array was allocated above and is freed once.
        unsafe { glib_sys::g_array_free(keys, 1) };
        out.push(J::from(hex(&bytes)));
    }
    J::L(out)
}

/// The library's user tokens of `phrase`.
fn user_tokens(k: &mut Kit, phrase: &str) -> Vec<u32> {
    tokens_of(k, phrase)
        .into_iter()
        .filter(|token| token >> 24 == u32::from(LIBRARY))
        .collect()
}

/// The pinyin export of the library, at most four rows.
fn export(k: &mut Kit) -> J {
    let iterator = facade::begin_get_phrases(k.ctx(), u32::from(LIBRARY));
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(iterator) {
        assert!(rows.len() < 4, "bounded fixture export");
        let mut text = UNTOUCHED as usize as *mut c_char;
        let mut spelling = UNTOUCHED as usize as *mut c_char;
        let mut count = UNTOUCHED as i32;
        let got = facade::iterator_get_next_phrase(
            iterator,
            &raw mut text,
            &raw mut spelling,
            &raw mut count,
        );
        rows.push(J::L(vec![
            J::from(got),
            phrase_text_bytes(text, true),
            phrase_text_bytes(spelling, true),
            J::from(count),
        ]));
    }
    facade::end_get_phrases(iterator);
    J::L(rows)
}

/// The rows of the cached instance's list whose text is `phrase`: index, type and bytes.
fn phrase_candidates(k: &mut Kit, phrase: &str) -> J {
    let inst = k.inst();
    let mut count = UNTOUCHED;
    assert!(facade::get_n_candidate(inst, &raw mut count));
    let wanted = J::from(hex(phrase.as_bytes()));
    let mut rows = Vec::new();
    for index in 0..count {
        let candidate = candidate_at(inst, index).expect("get_candidate");
        let mut text = UNTOUCHED as usize as *const c_char;
        let mut kind = UNTOUCHED as i32;
        assert!(facade::get_candidate_string(inst, candidate, &raw mut text));
        assert!(facade::get_candidate_type(inst, candidate, &raw mut kind));
        let found = phrase_text_bytes(text, false);
        if found == wanted {
            rows.push(J::L(vec![J::from(index), J::from(kind), found]));
        }
    }
    J::L(rows)
}

/// `observe`: the stored keys, unigrams, export and typed lookup.
fn observe(k: &mut Kit, phrase: &str, typed: &str) -> J {
    let tokens = user_tokens(k, phrase);
    let readings: Vec<J> = tokens
        .iter()
        .map(|&token| reload_pron_keys(k, token))
        .collect();
    let unigrams: Vec<J> = tokens
        .iter()
        .map(|&token| J::from(unigram_of(k, token)))
        .collect();
    let mut result = obj! { "tokens" => tokens, "readings" => readings, "unigrams" => unigrams };
    if facade::ZHUYIN {
        let inst = k.inst();
        result.set(
            "parsed",
            facade::parse_more_chewings(inst, c(typed).as_ptr()),
        );
        result.set("listed", facade::guess_candidates_after_cursor(inst, 0));
    } else {
        result.set("export", export(k));
        assert!(facade::set_options(k.ctx(), PINYIN_LOOKUP_OPTIONS));
        let inst = k.inst();
        result.set(
            "parsed",
            facade::parse_more_full_pinyins(inst, c(typed).as_ptr()),
        );
        result.set("listed", facade::guess_candidates(inst, 0, 0x1e));
    }
    result.set("candidates", phrase_candidates(k, phrase));
    result
}

/// Frees the cached instance and context, opening them first where needed.
fn close(k: &mut Kit) {
    let inst = k.inst();
    facade::free_instance(inst);
    facade::fini(k.ctx());
    k.forget();
}

/// `user_key_spelling(mode, phrase, reading, typed)`.
fn spelling(k: &mut Kit, phrase: &str, reading: &str, typed: &str) -> J {
    assert_eq!(import_rows(k, LIBRARY, &[(phrase, reading, 5)]), [true]);
    let mut out = obj! { "imported" => observe(k, phrase, typed) };
    out.set("saved", facade::save(k.ctx()));
    let saved = std::fs::read(k.user.join("user.bin")).expect("user.bin");
    out.set("saved_chunk", hex(&saved));
    close(k);
    out.set("reopened", observe(k, phrase, typed));
    out
}

/// `user_stored_key(mode, variant)`: one crafted key word read back.
fn stored_key(k: &mut Kit, variant: &str) -> J {
    let reading = if facade::ZHUYIN { "ㄋㄧˇ" } else { "ni3" };
    assert_eq!(import_rows(k, LIBRARY, &[("你", reading, 5)]), [true]);
    let token = user_tokens(k, "你")[0];
    let imported = reload_pron_keys(k, token);
    assert!(facade::save(k.ctx()));
    close(k);
    let path = k.user.join("user.bin");
    let mut payload = std::fs::read(&path).expect("user.bin")[8..].to_vec();
    let text = ucs4_key("你");
    let positions: Vec<usize> = (0..=payload.len() - text.len())
        .filter(|&at| payload[at..at + text.len()] == text[..])
        .collect();
    assert_eq!(positions.len(), 1, "one imported item to patch");
    let at = positions[0] + text.len();
    let mut word = u16::from_le_bytes([payload[at], payload[at + 1]]);
    assert_eq!(imported.items()[2], J::from(hex(&payload[at..at + 2])));
    word = match variant {
        "tone6" => (word & !0x7000) | (6 << 12),
        "0x8000" => 0x8000,
        _ => (word & 0x7000) | 0x1f,
    };
    payload[at..at + 2].copy_from_slice(&word.to_le_bytes());
    std::fs::write(&path, chunk(&payload)).expect("write user.bin");
    let mut out = obj! { "imported" => imported, "stored" => hex(&word.to_le_bytes()) };
    let mut length = UNTOUCHED;
    let mut phrase = UNTOUCHED as usize as *mut c_char;
    let ok = facade::token_get_phrase(k.inst(), token, &raw mut length, &raw mut phrase);
    out.set(
        "phrase",
        J::L(vec![
            J::from(ok),
            J::from(length),
            phrase_text_bytes(phrase, true),
        ]),
    );
    out.set("reopened", reload_pron_keys(k, token));
    out
}

/// `user-key-spelling/{mode}-{tag}` and `user-stored-key/{mode}-{variant}`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let mode = if facade::ZHUYIN { "zhuyin" } else { "pinyin" };
    if let Some(variant) = id.strip_prefix(&format!("user-stored-key/{mode}-")) {
        return stored_key(k, variant);
    }
    let tag = id
        .strip_prefix(&format!("user-key-spelling/{mode}-"))
        .unwrap_or_else(|| panic!("{id}: not this facade's case"));
    let (_, _, phrase, reading, typed) = READINGS
        .iter()
        .find(|(name, reading_mode, ..)| *name == tag && *reading_mode == mode)
        .unwrap_or_else(|| panic!("{id}: no reading"));
    spelling(k, phrase, reading, typed)
}
