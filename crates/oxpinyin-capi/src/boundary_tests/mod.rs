//! Decisions of the C boundary on the w3 mini fixture: refusals, aborts,
//! out-parameters and the files a call writes. Each test asserts only the
//! deciding values. An abort test asserts the abort status alone: the child's
//! only stderr is the first init's `open …/user.conf failed.` line
//! (docs/findings/upstream-divergences.md#reproduced-library-aborts-write-no-assertion-line:
//! oxpinyin writes no assertion line).

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use std::ffi::c_char;
use std::path::Path;
use std::ptr;

use crate::candidates::{
    pinyin_choose_candidate, pinyin_choose_predicted_candidate, pinyin_get_candidate,
    pinyin_get_candidate_nbest_index, pinyin_get_candidate_type, pinyin_get_n_candidate,
    pinyin_is_user_candidate, pinyin_remove_user_candidate,
};
use crate::config::{
    pinyin_load_phrase_library, pinyin_mask_out, pinyin_set_options, pinyin_unload_phrase_library,
};
use crate::context::{pinyin_fini, pinyin_init, pinyin_save};
use crate::cursor::{
    pinyin_get_left_pinyin_offset, pinyin_get_pinyin_key, pinyin_get_pinyin_key_rest,
    pinyin_get_right_pinyin_offset,
};
use crate::dict::{
    pinyin_token_add_unigram_frequency, pinyin_token_get_n_pronunciation,
    pinyin_token_get_unigram_frequency,
};
use crate::e2e_tests::{capture_rows, import_batch};
use crate::instance::{pinyin_alloc_instance, pinyin_free_instance, pinyin_reset};
use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_bigram_phrases,
    pinyin_bigram_iterator_get_next_phrase, pinyin_bigram_iterator_has_next_phrase,
    pinyin_end_add_phrases,
};
use crate::keys::{pinyin_get_pinyin_is_incomplete, pinyin_parse_full_pinyin};
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::predict::pinyin_guess_predicted_candidates;
use crate::sentence::{
    pinyin_get_character_offset, pinyin_get_sentence, pinyin_guess_candidates,
    pinyin_guess_predicted_candidates_with_punctuations, pinyin_guess_sentence,
};
use crate::test_support::{
    TempUserDir, assert_aborts, craft_hash, craft_tree, craft_user_bigram, craft_user_index, cstr,
    gram_bytes, lookup, open, plant_bigram, private_system, set_phrase_table_row, system_dir,
    write_user_conf,
};
use crate::text::{pinyin_get_double_pinyin_auxiliary_text, pinyin_get_full_pinyin_auxiliary_text};
use crate::types::{
    ChewingKey, LookupCandidate, PinyinContext, PinyinInstance, lookup_candidate_type_t,
};

mod choose;
mod guess_predict;
mod import_export;
mod init_and_save;
mod library_loading;
mod library_tokens;
mod parse_keys;
mod sentence;
mod table_reads;
mod token_getters;
mod training;
mod user_store;

/// The value a test seeds an out-parameter with, to see it left alone.
const UNTOUCHED: u32 = 0x00AB_CDEF;

const NBEST: lookup_candidate_type_t = lookup_candidate_type_t::NBEST_MATCH_CANDIDATE;

const NORMAL: lookup_candidate_type_t = lookup_candidate_type_t::NORMAL_CANDIDATE;

const PREDICTED_BIGRAM: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE;

const PREDICTED_PREFIX: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE;

const PREDICTED_PUNCTUATION: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE;

/// `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN`.
const SEPARATOR_OPTIONS: u32 = 0x18a;

/// A context on `system` and `user`; NULL when init refuses.
fn init_at(system: &Path, user: &Path) -> *mut PinyinContext {
    let system = cstr(system.to_str().expect("UTF-8 path"));
    let user = cstr(user.to_str().expect("UTF-8 path"));
    unsafe { pinyin_init(system.as_ptr(), user.as_ptr()) }
}

/// A context and instance on the mini fixture and `user`.
fn open_at(user: &Path) -> (*mut PinyinContext, *mut PinyinInstance) {
    open(user.to_str().expect("UTF-8 path"))
}

fn alloc(context: *mut PinyinContext) -> *mut PinyinInstance {
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    instance
}

fn close(context: *mut PinyinContext, instance: *mut PinyinInstance) {
    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_fini(context) };
}

fn parse(instance: *mut PinyinInstance, text: impl AsRef<[u8]>) -> usize {
    let text = std::ffi::CString::new(text.as_ref()).expect("no interior NUL");
    unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) }
}

/// The first row of `kind` in the current list.
fn row_of(instance: *mut PinyinInstance, kind: lookup_candidate_type_t) -> *mut LookupCandidate {
    let index = capture_rows(instance)
        .iter()
        .position(|(found, _, _)| *found == kind)
        .unwrap_or_else(|| panic!("a {kind:?} row is listed"));
    candidate_at(instance, index)
}

/// The first row of `kind` whose text is `text`.
fn row_with(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
    text: &str,
) -> *mut LookupCandidate {
    let index = capture_rows(instance)
        .iter()
        .position(|(found, row, _)| *found == kind && row == text)
        .unwrap_or_else(|| panic!("a {kind:?} row {text} is listed"));
    candidate_at(instance, index)
}

fn candidate_at(instance: *mut PinyinInstance, index: usize) -> *mut LookupCandidate {
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut cand,
        )
    });
    cand
}

fn n_candidate(instance: *mut PinyinInstance) -> u32 {
    let mut count = UNTOUCHED;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut count) });
    count
}

fn unigram(instance: *mut PinyinInstance, token: u32) -> (bool, u32) {
    let mut freq = UNTOUCHED;
    let ret = unsafe { pinyin_token_get_unigram_frequency(instance, token, &raw mut freq) };
    (ret, freq)
}

/// `get_sentence(index)` with a seeded out-pointer: the answer, and the
/// text or `None` when the pointer was left alone.
fn sentence_out(instance: *mut PinyinInstance, index: u8) -> (bool, Option<String>) {
    let seed = UNTOUCHED as usize as *mut c_char;
    let mut out = seed;
    let ret = unsafe { pinyin_get_sentence(instance, index, &raw mut out) };
    if out == seed {
        (ret, None)
    } else {
        (ret, Some(crate::ffi::take_owned_cstr(out)))
    }
}

/// The user bigram of a profile on the mini fixture replaced by `rows`
/// under a conforming `user.conf`; the rest of the profile stays.
fn replace_user_bigram(user: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    craft_user_bigram(&user.join("user_bigram.db"), rows);
    write_user_conf(user, &system_dir());
}

/// Imports and saves 龘龘 (da2'da2) in library 7, then closes the context;
/// answers its token.
fn saved_user_phrase(user: &Path) -> u32 {
    let (context, instance) = open_at(user);
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
    assert!(unsafe { pinyin_save(context) });
    let token = lookup(instance, "龘龘")[0];
    assert_eq!(token >> 24, 7);
    close(context, instance);
    token
}

/// The first NORMAL user row of `dada` on a new context over `user`.
fn user_row_of_dada(user: &Path) -> (*mut PinyinInstance, *mut LookupCandidate) {
    let (_context, instance) = open_at(user);
    assert_eq!(parse(instance, "dada"), 4);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    let rows = capture_rows(instance);
    let index = (0..rows.len())
        .find(|&index| {
            let cand = candidate_at(instance, index);
            rows[index].0 == NORMAL && unsafe { pinyin_is_user_candidate(instance, cand) }
        })
        .expect("a NORMAL user row");
    (instance, candidate_at(instance, index))
}

/// The phrase text of `token` through `pinyin_token_get_phrase`.
fn phrase_of(instance: *mut PinyinInstance, token: u32) -> Option<String> {
    let (mut length, mut phrase) = (UNTOUCHED, ptr::null_mut());
    unsafe {
        crate::dict::pinyin_token_get_phrase(instance, token, &raw mut length, &raw mut phrase)
    }
    .then(|| crate::ffi::take_owned_cstr(phrase).as_str().to_owned())
}

/// The newline the C stdio of the host writes for `\n` on stderr: the
/// UCRT's text mode writes `\r\n`.
const NL: &str = if cfg!(windows) { "\r\n" } else { "\n" };

/// A private system next to the child's user directory, and a context on
/// both.
fn private_context(user: &Path) -> (std::path::PathBuf, *mut PinyinContext) {
    let system = private_system(user.parent().expect("the child's scratch"));
    let context = init_at(&system, user);
    assert!(!context.is_null());
    (system, context)
}

fn write_log(user: &Path, name: &str, payload: &[u8]) {
    std::fs::write(
        user.join(name),
        oxpinyin_data::chunk_format::build_memory_chunk(payload).expect("frame"),
    )
    .expect("write the log");
}

/// Every file of `dir` by name, with its bytes.
fn files_of(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .expect("read the directory")
        .map(|entry| {
            let entry = entry.expect("an entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, std::fs::read(entry.path()).expect("read a file"))
        })
        .collect()
}

/// The keys of reading `nth` of `token` as little-endian bytes.
fn reading_bytes(instance: *mut PinyinInstance, token: u32, nth: u32) -> Vec<u8> {
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    assert!(unsafe {
        crate::dict::pinyin_token_get_nth_pronunciation(instance, token, nth, keys.cast())
    });
    // SAFETY: a live GArray of two-byte elements.
    let bytes = unsafe {
        let view = &*keys;
        std::slice::from_raw_parts(view.data.cast::<u8>(), view.len as usize * 2).to_vec()
    };
    // SAFETY: allocated above, freed once.
    unsafe { glib_sys::g_array_free(keys, 1) };
    bytes
}

/// Library 1's export of `phrase`: each row's reading and count.
fn export_rows_of(context: *mut PinyinContext, library: u32, phrase: &str) -> Vec<(String, i32)> {
    crate::e2e_tests::export_of(context, library)
        .into_iter()
        .filter(|(text, _, _)| text == phrase)
        .map(|(_, reading, count)| (reading, count))
        .collect()
}

/// A private system whose library 1 holds 甲?乙 (the middle word `word`)
/// at 100, 丙 at 1000, 戌 at 10 and 甲?乙丙; its bigram ranks 戌 after
/// 甲?乙.
fn prefix_system(parent: &Path, word: u32) -> std::path::PathBuf {
    let prefix = [0x7532, word, 0x4e59];
    let phrase = [0x7532, word, 0x4e59, 0x4e19];
    let system = crate::test_support::items_system(
        &system_dir(),
        parent,
        &[
            (&prefix, &[0x1084; 3], 100),
            (&[0x4e19], &[0x1081], 1000),
            (&[0x620c], &[0x1081], 10),
            (&phrase, &[0x1084, 0x1084, 0x1084, 0x1081], 100),
        ],
        2000,
    );
    craft_hash(
        &system.join("bigram.db"),
        &[(
            0x0100_0001_u32.to_le_bytes().to_vec(),
            gram_bytes(1000, &[(0x0100_0003, 1000)]),
        )],
    );
    system
}

/// GLib's `G_LOG_LEVEL_CRITICAL`.
const CRITICAL: glib_sys::GLogLevelFlags = glib_sys::G_LOG_LEVEL_CRITICAL;

fn ucs4(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| u32::from(ch).to_le_bytes())
        .collect()
}

/// The sentence of row 0 after a fresh `pinyin_guess_sentence`, or `None`
/// when the guess or the read answers `false`.
fn sentence_after_guess(instance: *mut PinyinInstance) -> Option<String> {
    if !unsafe { pinyin_guess_sentence(instance) } {
        return None;
    }
    let mut sentence: *mut c_char = ptr::null_mut();
    if !unsafe { pinyin_get_sentence(instance, 0, &raw mut sentence) } {
        return None;
    }
    // SAFETY: a NUL-terminated GLib string the caller owns.
    let text = unsafe { std::ffi::CStr::from_ptr(sentence) }
        .to_str()
        .expect("UTF-8 sentence")
        .to_owned();
    // SAFETY: freed once.
    unsafe { glib_sys::g_free(sentence.cast()) };
    Some(text)
}
