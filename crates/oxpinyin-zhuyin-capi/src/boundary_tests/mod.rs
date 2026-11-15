//! C-boundary decisions of the zhuyin facade on the w3 mini fixture: the
//! return values, out-parameters, aborts and stderr lines of the pin at
//! 074a2219 that no lower-layer test holds. An abort test asserts the abort
//! status only
//! (`docs/findings/upstream-divergences.md#reproduced-library-aborts-write-no-assertion-line`:
//! oxpinyin writes no assertion line).

use std::os::raw::c_char;
use std::path::Path;
use std::ptr;

use crate::candidates::{
    zhuyin_get_candidate, zhuyin_get_candidate_string, zhuyin_get_candidate_type,
    zhuyin_get_n_candidate, zhuyin_train,
};
use crate::config::{
    zhuyin_load_phrase_library, zhuyin_mask_out, zhuyin_set_chewing_scheme, zhuyin_set_options,
    zhuyin_unload_phrase_library,
};
use crate::context::{zhuyin_fini, zhuyin_init, zhuyin_save};
use crate::cursor::{
    zhuyin_get_left_zhuyin_offset, zhuyin_get_right_zhuyin_offset, zhuyin_get_zhuyin_key,
    zhuyin_get_zhuyin_key_rest, zhuyin_get_zhuyin_key_rest_length,
    zhuyin_get_zhuyin_key_rest_positions,
};
use crate::dict::{
    zhuyin_token_add_unigram_frequency, zhuyin_token_get_n_pronunciation,
    zhuyin_token_get_nth_pronunciation, zhuyin_token_get_phrase,
};
use crate::instance::{zhuyin_alloc_instance, zhuyin_free_instance};
use crate::keys::{zhuyin_get_pinyin_string, zhuyin_get_zhuyin_string, zhuyin_parse_chewing};
use crate::parse::{
    zhuyin_get_parsed_input_length, zhuyin_in_chewing_keyboard, zhuyin_parse_more_chewings,
    zhuyin_parse_more_full_pinyins,
};
use crate::phrase::{zhuyin_get_n_phrase, zhuyin_get_phrase_token, zhuyin_phrase_segment};
use crate::sentence::{
    zhuyin_get_character_offset, zhuyin_get_sentence, zhuyin_guess_candidates_after_cursor,
    zhuyin_guess_candidates_before_cursor, zhuyin_guess_sentence,
};
use crate::test_support::{
    LogCapture, TempUserDir, abort_in_child, assert_aborts, close, craft_user_index, cstr, import,
    init_on, open, open_with_user, plant_bigram, private_system, run_in_child, system_dir,
    take_sentence, tokens_of, ucs4_key, write_user_conf,
};
use crate::types::{ChewingKey, ChewingKeyRest, LookupCandidate, ZhuyinContext, ZhuyinInstance};

mod import_and_user_store;
mod init_and_save;
mod null_arguments;
mod parse_keys;
mod sentence_and_candidates;
mod table_reads;
mod token_getters;

/// The out-parameter seed that shows an untouched output.
const UNTOUCHED: u32 = 0x00AB_CDEF;

/// A seeded pointer out-parameter.
const SEEDED: usize = 0x00AB_CDEF;

/// The GLib level of a critical.
const CRITICAL: u32 = 8;

/// A fresh instance on `context` with `keys` parsed as chewing keystrokes.
fn chewing(context: *mut ZhuyinContext, keys: &str) -> *mut ZhuyinInstance {
    let instance = zhuyin_alloc_instance(context);
    assert!(!instance.is_null());
    zhuyin_parse_more_chewings(instance, cstr(keys).as_ptr());
    instance
}

/// `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN`.
const SEPARATOR_OPTIONS: u32 = 0x18a;

/// A fresh instance with `text` parsed as full pinyin under `options`.
fn full_pinyin(context: *mut ZhuyinContext, options: u32, text: &str) -> *mut ZhuyinInstance {
    assert!(zhuyin_set_options(context, options));
    let instance = zhuyin_alloc_instance(context);
    assert!(!instance.is_null());
    zhuyin_parse_more_full_pinyins(instance, cstr(text).as_ptr());
    instance
}

/// The sentence getter's answer and text; `None` for an untouched output.
fn sentence(instance: *mut ZhuyinInstance) -> (bool, Option<String>) {
    let mut text = SEEDED as *mut c_char;
    let ret = zhuyin_get_sentence(instance, &raw mut text);
    let text = (text as usize != SEEDED).then(|| take_sentence(text));
    (ret, text)
}

fn candidate_at(instance: *mut ZhuyinInstance, index: u32) -> *mut LookupCandidate {
    let mut candidate: *mut LookupCandidate = ptr::null_mut();
    assert!(zhuyin_get_candidate(instance, index, &raw mut candidate));
    candidate
}

/// The keys of a GArray of two-byte elements as lowercase hex.
fn keys_hex(array: *mut glib_sys::GArray) -> String {
    // SAFETY: a live GArray of two-byte elements with `len` of them.
    unsafe {
        let view = &*array;
        (0..view.len as usize)
            .flat_map(|index| {
                view.data
                    .cast::<u16>()
                    .add(index)
                    .read_unaligned()
                    .to_le_bytes()
            })
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

/// The `nth` pronunciation of `token`: the answer, the key count and the keys.
fn nth_pronunciation(instance: *mut ZhuyinInstance, token: u32, nth: u32) -> (bool, u32, String) {
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    let ret = zhuyin_token_get_nth_pronunciation(instance, token, nth, keys);
    // SAFETY: the array is live.
    let len = unsafe { (*keys).len };
    let hex = keys_hex(keys);
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(keys, 1) };
    (ret, len, hex)
}

/// The phrase of `token`: the answer, the length and the text, the seeds
/// kept where untouched.
fn token_phrase(instance: *mut ZhuyinInstance, token: u32) -> (bool, u32, Option<String>) {
    let mut length = UNTOUCHED;
    let mut text = SEEDED as *mut c_char;
    let ret = zhuyin_token_get_phrase(instance, token, &raw mut length, &raw mut text);
    let text = (text as usize != SEEDED).then(|| take_sentence(text));
    (ret, length, text)
}

/// The library-7 tokens of `text`.
fn user_tokens(instance: *mut ZhuyinInstance, text: &str) -> Vec<u32> {
    tokens_of(instance, text)
        .into_iter()
        .filter(|token| token >> 24 == 7)
        .collect()
}

/// The elements of a zero-terminated GArray of `width`-byte elements; the
/// terminator is checked.
fn elements(array: *mut glib_sys::GArray, width: usize) -> Vec<u32> {
    // SAFETY: a live zero-terminated GArray of `width`-byte elements.
    unsafe {
        let view = &*array;
        let read = |index: usize| {
            if width == 4 {
                view.data.cast::<u32>().add(index).read_unaligned()
            } else {
                u32::from(view.data.cast::<u16>().add(index).read_unaligned())
            }
        };
        assert_eq!(read(view.len as usize), 0, "the zero terminator");
        (0..view.len as usize).map(read).collect()
    }
}
