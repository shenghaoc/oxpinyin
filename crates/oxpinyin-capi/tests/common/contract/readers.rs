//! The C-ABI readers the contract case bodies share (`contract_abi.py` and the
//! readers of `contract_case_helpers.py`): each drives one facade call and answers
//! its observation, out-parameters seeded with [`UNTOUCHED`].
#![allow(dead_code)]

use std::ffi::{CString, c_char};

use crate::facade;
use crate::support::{Kit, P, UNTOUCHED, borrowed_text, take_text};

/// A byte string as the C string a facade call takes.
pub fn c(bytes: impl AsRef<[u8]>) -> CString {
    CString::new(bytes.as_ref()).expect("no interior NUL")
}

/// A reader's view of an out string: `"untouched"` when the facade left the
/// seeded pointer alone (`contract_abi.sentence_out`), else the text or `None`.
pub const UNTOUCHED_TEXT: &str = "untouched";

/// `contract_abi.full_inst`: a fresh instance with `text` parsed as full pinyin.
pub fn full_inst(k: &mut Kit, text: &[u8]) -> P {
    let inst = k.alloc();
    facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    inst
}

/// `contract_abi.consumed`: the bytes a fresh instance's `parse` consumes.
pub fn consumed(k: &mut Kit, parse: fn(P, *const c_char) -> usize, text: &[u8]) -> usize {
    let inst = k.alloc();
    parse(inst, c(text).as_ptr())
}

/// `contract_abi.key_out`: the cached instance's single-key parse and the raw
/// 16-bit key it wrote, seeded with UNTOUCHED's low half.
pub fn key_out(k: &mut Kit, text: &[u8]) -> (bool, u16) {
    let inst = k.inst();
    let mut key: u16 = (UNTOUCHED & 0xFFFF) as u16;
    let ret = facade::parse_full_pinyin(inst, c(text).as_ptr(), (&raw mut key).cast());
    (ret, key)
}

/// A candidate pointer of the instance's current list.
pub fn candidate_at(inst: P, index: u32) -> Option<P> {
    let mut cand: P = std::ptr::null_mut();
    facade::get_candidate(inst, index, &raw mut cand).then_some(cand)
}

/// The instance's candidate count.
pub fn n_candidate(inst: P) -> u32 {
    let mut count = 0_u32;
    facade::get_n_candidate(inst, &raw mut count);
    count
}

/// A candidate's type.
pub fn candidate_type(inst: P, cand: P) -> i32 {
    let mut kind = 0_i32;
    facade::get_candidate_type(inst, cand, &raw mut kind);
    kind
}

/// A candidate's borrowed text, invalid UTF-8 replaced.
pub fn candidate_text(inst: P, cand: P) -> Option<String> {
    let mut text: *const c_char = std::ptr::null();
    facade::get_candidate_string(inst, cand, &raw mut text);
    borrowed_text(text)
}

/// `contract_case_helpers.candidate_rows`: the whole list as `(type, text)`
/// rows, `None` where the fetch fails.
pub fn candidate_rows(inst: P) -> Vec<Option<(i32, String)>> {
    (0..n_candidate(inst))
        .map(|index| {
            candidate_at(inst, index).map(|cand| {
                (
                    candidate_type(inst, cand),
                    candidate_text(inst, cand).expect("candidate text"),
                )
            })
        })
        .collect()
}

/// `contract_abi.row_of`: the first candidate of `kind` in the current list.
pub fn row_of(inst: P, kind: i32) -> Option<P> {
    (0..n_candidate(inst))
        .filter_map(|index| candidate_at(inst, index))
        .find(|&cand| candidate_type(inst, cand) == kind)
}

/// `contract_abi.tokens_of`: the cached instance's `lookup_tokens` of `phrase`.
pub fn tokens_of(k: &mut Kit, phrase: &str) -> Vec<u32> {
    let inst = k.inst();
    lookup(inst, phrase.as_bytes())
}

/// `lookup_tokens` of raw `phrase` bytes into a fresh four-byte GArray.
pub fn lookup(inst: P, phrase: &[u8]) -> Vec<u32> {
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    facade::lookup_tokens(inst, c(phrase).as_ptr(), array.cast());
    let tokens = garray_u32(array);
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    tokens
}

/// A GArray's elements as `u32`s.
pub fn garray_u32(array: *mut glib_sys::GArray) -> Vec<u32> {
    // SAFETY: a live GArray of four-byte elements: a non-empty one's `data` holds `len` of them.
    unsafe {
        let view = &*array;
        if view.len == 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts(view.data.cast::<u32>(), view.len as usize).to_vec()
    }
}

/// A GArray's elements as `u16`s.
pub fn garray_u16(array: *mut glib_sys::GArray) -> Vec<u16> {
    // SAFETY: a live GArray of two-byte elements: a non-empty one's `data` holds `len` of them.
    unsafe {
        let view = &*array;
        if view.len == 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts(view.data.cast::<u16>(), view.len as usize).to_vec()
    }
}

/// `contract_abi.unigram_of`: the cached instance's unigram frequency of `token`.
pub fn unigram_of(k: &mut Kit, token: u32) -> (bool, u32) {
    let inst = k.inst();
    let mut freq = UNTOUCHED;
    let ret = facade::token_get_unigram_frequency(inst, token, &raw mut freq);
    (ret, freq)
}

/// An owned out string as the old readers showed it: `"untouched"` when the
/// facade left the seeded pointer, else the decoded text or `None`.
pub fn owned_out(pointer: *mut c_char) -> Option<String> {
    if pointer as usize == UNTOUCHED as usize {
        return Some(UNTOUCHED_TEXT.to_owned());
    }
    take_text(pointer)
}

/// A seeded out pointer for [`owned_out`].
pub fn seeded() -> *mut c_char {
    UNTOUCHED as usize as *mut c_char
}

/// `contract_case_helpers.guess_rows` / `zhuyin_guess_rows`: one candidate
/// guess (pinyin `guess_candidates`, zhuyin `guess_candidates_after_cursor`)
/// and the whole list it leaves.
#[derive(Debug, PartialEq)]
pub struct GuessRows {
    pub ret: bool,
    pub rows: Vec<Option<(i32, String)>>,
}

/// The pinyin facade's `guess_candidates(offset, sort)` and its list.
pub fn guess_rows(inst: P, offset: usize, sort: u32) -> GuessRows {
    let ret = facade::guess_candidates(inst, offset, sort);
    GuessRows {
        ret,
        rows: candidate_rows(inst),
    }
}

/// The zhuyin facade's `guess_candidates_after_cursor(offset)` and its list.
pub fn zhuyin_guess_rows(inst: P, offset: usize) -> GuessRows {
    let ret = facade::guess_candidates_after_cursor(inst, offset);
    GuessRows {
        ret,
        rows: candidate_rows(inst),
    }
}

/// `contract_case_helpers.predicted_rows`: predictions with punctuation for
/// `prefix`, the first `limit` rows and the full count.
pub fn predicted_rows(inst: P, prefix: &str, limit: u32) -> (Vec<(i32, String)>, u32) {
    facade::guess_predicted_candidates_with_punctuations(inst, c(prefix).as_ptr());
    let count = n_candidate(inst);
    let rows = (0..count.min(limit))
        .map(|index| {
            let cand = candidate_at(inst, index).unwrap_or(std::ptr::null_mut());
            (
                candidate_type(inst, cand),
                candidate_text(inst, cand).expect("candidate text"),
            )
        })
        .collect();
    (rows, count)
}

/// One export row: the accessor's answer, the phrase, its reading and count.
pub type ExportRow = (bool, String, String, i32);

/// `contract_case_helpers.remember_export`: the export rows of `phrase` in
/// `library`, every row of it.
pub fn remember_export(k: &mut Kit, library: u32, phrase: &str) -> Vec<ExportRow> {
    let ctx = k.ctx();
    let it = facade::begin_get_phrases(ctx, library);
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(it) {
        let (mut text, mut reading, mut count) =
            (std::ptr::null_mut(), std::ptr::null_mut(), 0_i32);
        let ret =
            facade::iterator_get_next_phrase(it, &raw mut text, &raw mut reading, &raw mut count);
        let (text, reading) = (take_text(text), take_text(reading));
        if text.as_deref() == Some(phrase) {
            rows.push((
                ret,
                text.unwrap_or_default(),
                reading.unwrap_or_default(),
                count,
            ));
        }
    }
    facade::end_get_phrases(it);
    rows
}

/// `contract_case_helpers._add_user_phrase`: imports `text` under each reading
/// into library 7 and requires every add to succeed.
pub fn add_user_phrase(k: &mut Kit, text: &str, readings: &[&[u8]], count: i32) -> Vec<bool> {
    let ctx = k.ctx();
    let it = facade::begin_add_phrases(ctx, 7);
    assert!(!it.is_null(), "begin_add_phrases");
    let added: Vec<bool> = readings
        .iter()
        .map(|reading| {
            facade::iterator_add_phrase(it, c(text).as_ptr(), c(reading).as_ptr(), count)
        })
        .collect();
    facade::end_add_phrases(it);
    assert!(added.iter().all(|&ok| ok), "every add");
    added
}

/// `contract_abi.final_step_sentences`: the sentences the facade answers. The
/// pinyin facade asserts on an index past its rows (`pinyin.cpp:1470-1474`), so
/// each index is probed apart and the list stops at an abort or a `false`.
pub fn final_step_sentences(inst: P) -> Vec<Option<String>> {
    if facade::ZHUYIN {
        let mut out = seeded();
        let ret = facade::get_sentence_only(inst, &raw mut out);
        return if ret {
            vec![owned_out(out)]
        } else {
            Vec::new()
        };
    }
    let mut sentences = Vec::new();
    for index in 0..3_u8 {
        let answer = crate::support::isolated(|| {
            let mut out = seeded();
            let ret = facade::get_sentence_at(inst, index, &raw mut out);
            let text = owned_out(out);
            let mut bytes = vec![u8::from(ret), u8::from(text.is_some())];
            bytes.extend(text.unwrap_or_default().into_bytes());
            bytes
        });
        let Some(answer) = answer else { break };
        if answer[0] == 0 {
            break;
        }
        sentences.push(
            (answer[1] == 1)
                .then(|| String::from_utf8(answer[2..].to_vec()).expect("UTF-8 sentence")),
        );
    }
    sentences
}

/// `contract_case_helpers._segment_out`: each text's `phrase_segment` answer and
/// the tokens it leaves.
pub fn segment_out(
    inst: P,
    texts: &[&str],
) -> std::collections::BTreeMap<String, (bool, Vec<u32>)> {
    texts
        .iter()
        .map(|text| {
            let ret = facade::phrase_segment(inst, c(text).as_ptr());
            let mut count = UNTOUCHED;
            facade::get_n_phrase(inst, &raw mut count);
            let tokens = (0..if count == UNTOUCHED { 0 } else { count })
                .map(|index| {
                    let mut slot = UNTOUCHED;
                    facade::get_phrase_token(inst, index, &raw mut slot);
                    slot
                })
                .collect();
            ((*text).to_owned(), (ret, tokens))
        })
        .collect()
}

/// `contract_case_helpers._facade_total_out`: a `ni3'hao3` sentence guess and its
/// sentences, the first eight candidates under sort word 0x1e and 你好's
/// segmentation, on the cached instance.
#[derive(Debug, PartialEq)]
pub struct FacadeOut {
    pub parsed: usize,
    pub guessed: bool,
    pub sentences: Vec<Option<String>>,
    pub candidates: Vec<Option<(i32, String)>>,
    pub segment: std::collections::BTreeMap<String, (bool, Vec<u32>)>,
}

pub fn facade_total_out(k: &mut Kit) -> FacadeOut {
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c("ni3'hao3").as_ptr());
    let guessed = facade::guess_sentence(inst);
    let sentences = final_step_sentences(inst);
    let mut candidates = guess_rows(inst, 0, 0x1e).rows;
    candidates.truncate(8);
    let segment = segment_out(inst, &["你好"]);
    FacadeOut {
        parsed,
        guessed,
        sentences,
        candidates,
        segment,
    }
}

/// `contract_fixtures.import_rows`: imports `(phrase, reading, count)` rows into
/// `library`, answering each add.
pub fn import_rows(k: &mut Kit, library: u8, rows: &[(&str, &str, i32)]) -> Vec<bool> {
    let it = facade::begin_add_phrases(k.ctx(), library);
    let added = rows
        .iter()
        .map(|(phrase, reading, count)| {
            facade::iterator_add_phrase(it, c(phrase).as_ptr(), c(reading).as_ptr(), *count)
        })
        .collect();
    facade::end_add_phrases(it);
    added
}

/// `contract_case_helpers.phrase_table_reopen`: frees the cached instance and
/// context and opens the context again.
pub fn reopen(k: &mut Kit) {
    k.close();
    k.ctx();
}

/// `contract_case_helpers.d18_export_phrases`: library 7's export rows.
pub fn export_phrases(k: &mut Kit) -> Vec<(bool, Option<String>, Option<String>, i32)> {
    let it = facade::begin_get_phrases(k.ctx(), 7);
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(it) {
        let (mut phrase, mut reading, mut count) =
            (std::ptr::null_mut(), std::ptr::null_mut(), UNTOUCHED as i32);
        let ret =
            facade::iterator_get_next_phrase(it, &raw mut phrase, &raw mut reading, &raw mut count);
        rows.push((ret, take_text(phrase), take_text(reading), count));
    }
    facade::end_get_phrases(it);
    rows
}

/// `contract_case_helpers.row58_rows`: predictions for `prefix` on the cached
/// instance, every row.
#[derive(Debug, PartialEq)]
pub struct Predicted {
    pub ret: bool,
    pub rows: Vec<(i32, String)>,
}

pub fn row58_rows(k: &mut Kit, prefix: &str) -> Predicted {
    let inst = k.inst();
    let ret = facade::guess_predicted_candidates(inst, c(prefix).as_ptr());
    let rows = (0..n_candidate(inst))
        .map(|index| {
            let cand = candidate_at(inst, index).expect("candidate");
            (
                candidate_type(inst, cand),
                candidate_text(inst, cand).expect("candidate text"),
            )
        })
        .collect();
    Predicted { ret, rows }
}

/// `contract_case_helpers._guess_out`: the cached instance's full-pinyin parse
/// of `text` (zhuyin under option word `1 << 5`, asserted), its sentence guess
/// and every final-step sentence, as `{parsed, guessed, sentences}`.
pub fn guess_out(k: &mut Kit, text: &[u8]) -> crate::support::J {
    if facade::ZHUYIN {
        assert!(facade::set_options(k.ctx(), 1 << 5));
    }
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    let guessed = facade::guess_sentence(inst);
    obj! {
        "parsed" => parsed,
        "guessed" => guessed,
        "sentences" => final_step_sentences(inst),
    }
}
