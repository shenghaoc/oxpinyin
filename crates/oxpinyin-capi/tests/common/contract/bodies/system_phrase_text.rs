//! The system_phrase_text family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 46 (#1175, #894): a private system library whose stored phrase
//! holds a raw UCS-4 word no API import would write (a surrogate, a word above
//! Unicode, NUL), shown losslessly through the export, the sentence, the
//! candidate list and a chosen-and-trained sentence (`lookup/lookup.cpp:39-68`,
//! `pinyin.cpp:1372-1387,2577-2587`, `zhuyin.cpp:895-910,1646-1666`).

use std::ffi::c_char;

use crate::facade;
use crate::fixtures::items_system;
use crate::readers::{c, candidate_at, candidate_type, unigram_of};
use crate::support::{J, Kit, P, UNTOUCHED};

/// `PHRASE_TEXT_WORDS`: the middle word of the stored phrase, by variant.
pub fn phrase_text_word(variant: &str) -> u32 {
    match variant {
        "valid" => 0x4e01,
        "surrogate" => 0xd800,
        "above-unicode" => 0x0011_0000,
        "nul" => 0,
        other => panic!("no phrase text variant {other}"),
    }
}

/// `phrase_text_system`: a private library 1 of two items, 甲<word>乙 over
/// da1'da1'da1 and 丙 over ba1, each with unigram 100.
pub fn phrase_text_system(k: &mut Kit, variant: &str) {
    let text = [0x7532, phrase_text_word(variant), 0x4e59];
    items_system(
        k,
        &[(&text, &[0x1084; 3], 100), (&[0x4e19], &[0x1081], 100)],
        200,
        false,
    );
}

/// `phrase_text_bytes`: an out string as hex of its bytes, never replaced:
/// `"untouched"` for the seeded pointer, `None` for NULL; freed when `owned`.
pub fn phrase_text_bytes(pointer: *const c_char, owned: bool) -> Option<String> {
    if pointer as usize == UNTOUCHED as usize {
        return Some("untouched".to_owned());
    }
    if pointer.is_null() {
        return None;
    }
    // SAFETY: a non-NULL, non-seeded pointer the facade wrote is a
    // NUL-terminated string that lives at least until it is freed below.
    let bytes = unsafe { std::ffi::CStr::from_ptr(pointer) }.to_bytes();
    let hex = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    if owned {
        // SAFETY: an owned out string is a GLib allocation the caller frees once.
        unsafe { glib_sys::g_free(pointer.cast_mut().cast()) };
    }
    Some(hex)
}

/// A seeded out pointer for [`phrase_text_bytes`].
pub fn seeded_text() -> *mut c_char {
    UNTOUCHED as usize as *mut c_char
}

/// The facade's sentence 0 (zhuyin: its one sentence) into `out`.
pub fn sentence_into(inst: P, out: &mut *mut c_char) -> bool {
    if facade::ZHUYIN {
        facade::get_sentence_only(inst, out)
    } else {
        facade::get_sentence_at(inst, 0, out)
    }
}

/// Every candidate of the current list as `(type, text hex)`, every accessor
/// required to succeed; the texts are borrowed.
pub fn candidate_text_rows(inst: P) -> Vec<(i32, Option<String>)> {
    let mut count = UNTOUCHED;
    assert!(facade::get_n_candidate(inst, &raw mut count));
    (0..count)
        .map(|index| {
            let mut candidate: P = std::ptr::null_mut();
            assert!(facade::get_candidate(inst, index, &raw mut candidate));
            let mut text: *const c_char = seeded_text();
            assert!(facade::get_candidate_string(inst, candidate, &raw mut text));
            let mut kind = UNTOUCHED as i32;
            assert!(facade::get_candidate_type(inst, candidate, &raw mut kind));
            (kind, phrase_text_bytes(text, false))
        })
        .collect()
}

/// Runs the case `id` and answers its result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    let rest = id.strip_prefix("phrase-text/").expect("a phrase-text case");
    let (_mode, rest) = rest.split_once('-').expect("a mode");
    let (variant, surface) = rest.rsplit_once('-').expect("a surface");
    phrase_text_system(k, variant);
    if surface == "export" && facade::ZHUYIN {
        // zhuyin.h and libzhuyin.ver export no iterator: the symbol table
        // this facade links answers for it.
        let exports: Vec<(&str, bool)> = [
            "begin_get_phrases",
            "iterator_has_next_phrase",
            "iterator_get_next_phrase",
            "end_get_phrases",
        ]
        .into_iter()
        .map(|name| (name, !facade::ZHUYIN))
        .collect();
        let mut map = J::map();
        for (name, present) in exports {
            map.set(name, present);
        }
        return obj! { "exports" => map };
    }
    assert!(facade::set_options(k.ctx(), 1 << 5));
    match surface {
        "export" => obj! { "rows" => phrase_text_rows(k) },
        "choose" => phrase_text_choose(k),
        "sentence" => {
            let inst = k.inst();
            let parsed = facade::parse_more_full_pinyins(inst, c("da1da1da1ba1").as_ptr());
            let guessed = facade::guess_sentence(inst);
            let mut text = seeded_text();
            let ret = sentence_into(inst, &mut text);
            obj! {
                "parsed" => parsed, "guessed" => guessed, "ret" => ret,
                "text" => phrase_text_bytes(text, true),
            }
        }
        "candidate" => {
            let inst = k.inst();
            let parsed = facade::parse_more_full_pinyins(inst, c("da1da1da1").as_ptr());
            let ret = if facade::ZHUYIN {
                facade::guess_candidates_after_cursor(inst, 0)
            } else {
                facade::guess_candidates(inst, 0, 0x1f)
            };
            obj! { "parsed" => parsed, "ret" => ret, "rows" => candidate_text_rows(inst) }
        }
        other => panic!("no surface {other}"),
    }
}

/// `phrase_text_rows`: the private library's export rows (at most two),
/// text and reading as hex.
fn phrase_text_rows(k: &mut Kit) -> Vec<(bool, Option<String>, Option<String>, i32)> {
    let it = facade::begin_get_phrases(k.ctx(), 1);
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(it) {
        assert!(rows.len() < 2, "fixture exported an unexpected extra row");
        let (mut text, mut reading, mut count) = (seeded_text(), seeded_text(), UNTOUCHED as i32);
        let ret =
            facade::iterator_get_next_phrase(it, &raw mut text, &raw mut reading, &raw mut count);
        rows.push((
            ret,
            phrase_text_bytes(text, true),
            phrase_text_bytes(reading, true),
            count,
        ));
    }
    facade::end_get_phrases(it);
    rows
}

/// The first NORMAL candidate of the current list and its borrowed text hex.
fn first_normal(inst: P) -> (P, Option<String>) {
    let mut count = UNTOUCHED;
    assert!(facade::get_n_candidate(inst, &raw mut count));
    for index in 0..count {
        let cand = candidate_at(inst, index).expect("a candidate");
        let mut text: *const c_char = seeded_text();
        assert!(facade::get_candidate_string(inst, cand, &raw mut text));
        if candidate_type(inst, cand) == 2 {
            return (cand, phrase_text_bytes(text, false));
        }
    }
    panic!("missing the stored phrase candidate")
}

/// `phrase_text_choose`: the stored phrase chosen, the sentence read under
/// the constraint and trained.
fn phrase_text_choose(k: &mut Kit) -> J {
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c("da1da1da1ba1").as_ptr());
    let listed = if facade::ZHUYIN {
        facade::guess_candidates_after_cursor(inst, 0)
    } else {
        // SORT_WITHOUT_SENTENCE (0x1) would train at choose time instead.
        facade::guess_candidates(inst, 0, 0x1e)
    };
    let (candidate, chosen) = first_normal(inst);
    let cursor = facade::choose_candidate(inst, 0, candidate);
    let guessed = facade::guess_sentence(inst);
    let mut sentence = seeded_text();
    let read = sentence_into(inst, &mut sentence);
    let sentence = phrase_text_bytes(sentence, true);
    let trained = facade::train_top(inst);
    let unigrams: Vec<(bool, u32)> = [1, 2]
        .into_iter()
        .map(|slot| unigram_of(k, 0x0100_0000 | slot))
        .collect();
    let mut out = obj! {
        "parsed" => parsed, "listed" => listed, "chosen" => chosen, "cursor" => cursor,
        "guessed" => guessed, "read" => read, "sentence" => sentence, "trained" => trained,
        "unigrams" => unigrams,
    };
    if !facade::ZHUYIN {
        out.set("rows", phrase_text_rows(k));
    }
    out
}
