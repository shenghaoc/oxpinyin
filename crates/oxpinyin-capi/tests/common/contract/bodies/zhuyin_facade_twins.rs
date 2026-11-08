//! The zhuyin_facade_twins family's case bodies: each answers the result document the old case returned.
//!
//! The zhuyin twins of the batch-1 out-parameter contracts (batch2 group 14, #542).

use crate::facade;
use crate::readers::{c, owned_out, seeded, tokens_of};
use crate::support::{J, Kit, P, UNTOUCHED};

/// Bytes as lowercase hex (`bytes.hex()`).
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `count` bytes at `pointer` as hex (`ctypes.string_at(...).hex()`).
fn hex_at(pointer: P, count: usize) -> String {
    // SAFETY: the old case read these bytes at the key pointers the facade
    // answered; the facade's function-static slots keep them readable.
    hex(unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), count) })
}

/// A GArray of two-byte elements: its length and contents as hex.
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

/// A fresh, unfreed GArray of two-byte elements.
fn new_keys() -> *mut glib_sys::GArray {
    // SAFETY: a fresh GArray of two-byte elements, left allocated as the case did.
    unsafe { glib_sys::g_array_new(0, 0, 2) }
}

/// `zhuyin_sentence_out`: the sentence's flag and text.
fn zhuyin_sentence_out(inst: P) -> J {
    let mut out = seeded();
    let ret = facade::get_sentence_only(inst, &raw mut out);
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

/// `reads`: the token's first pronunciation, phrase and pronunciation count.
fn reads(k: &mut Kit, token: u32) -> J {
    let inst = k.inst();
    let keys = new_keys();
    let ret = facade::token_get_nth_pronunciation(inst, token, 0, keys.cast());
    let (len, content) = keys_view(keys);
    let (mut length, mut phrase) = (UNTOUCHED, seeded());
    let phrase_ret = facade::token_get_phrase(inst, token, &raw mut length, &raw mut phrase);
    let phrase = owned_out(phrase);
    let mut count = UNTOUCHED;
    let count_ret = facade::token_get_n_pronunciation(inst, token, &raw mut count);
    obj! {
        "nth" => (ret, len, content),
        "phrase" => (phrase_ret, length, phrase),
        "n" => (count_ret, count),
    }
}

/// `user_library_token_unload`: a populated library-7 token around unload and reload.
fn user_library_token_unload(k: &mut Kit) -> J {
    let it = facade::begin_add_phrases(k.ctx(), 7);
    let reading = if facade::ZHUYIN {
        c("ㄋㄧˇ ㄏㄠˇ")
    } else {
        c("ni3'hao3")
    };
    let added = facade::iterator_add_phrase(it, c("你好").as_ptr(), reading.as_ptr(), 1);
    facade::end_add_phrases(it);
    let tokens: Vec<u32> = tokens_of(k, "你好")
        .into_iter()
        .filter(|token| token >> 24 == 7)
        .collect();
    assert!(
        added && tokens.len() == 1,
        "a populated index-7 token is required"
    );
    let token = tokens[0];
    let mut out = obj! { "before" => reads(k, token) };
    out.set("save", facade::save(k.ctx()));
    out.set("unload", facade::unload_phrase_library(k.ctx(), 7));
    out.set("unloaded", reads(k, token));
    out.set("load", facade::load_phrase_library(k.ctx(), 7));
    out.set("reloaded", reads(k, token));
    out
}

fn sentence_before_guess(k: &mut Kit) -> J {
    let inst = k.alloc();
    let mut out = obj! { "fresh" => zhuyin_sentence_out(inst) };
    facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    out.set("parsed", zhuyin_sentence_out(inst));
    facade::guess_sentence(inst);
    out.set("guessed", zhuyin_sentence_out(inst));
    facade::reset(inst);
    out.set("reset", zhuyin_sentence_out(inst));
    let inst = k.alloc();
    facade::parse_more_chewings(inst, c("'").as_ptr());
    let guessed = facade::guess_sentence(inst);
    out.set("keyless", (guessed, zhuyin_sentence_out(inst)));
    out
}

fn character_offset_out(k: &mut Kit) -> J {
    let mut out = J::map();
    for (label, text, phrase, offset) in [
        ("recursion-fails-1", "su3cl3", "啊", 3),
        ("recursion-fails-2", "su3cl3", "你", 6),
        ("hit", "su3cl3", "你好", 5),
        ("reserved-slot", "su3cl3", "你好", 6),
        ("reserved-slot-1", "su3cl3", "你", 6),
        ("no-token", "su3cl3", "x", 2),
        ("empty-phrase", "su3cl3", "", 2),
        ("no-parse", "", "你好", 0),
        ("keyless", "'", "你好", 0),
    ] {
        let inst = k.alloc();
        facade::parse_more_chewings(inst, c(text).as_ptr());
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

fn nth_pronunciation_range(k: &mut Kit) -> J {
    let inst = k.inst();
    // SAFETY: a fresh GArray of four-byte elements, left allocated as the case did.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    facade::lookup_tokens(inst, c("你好").as_ptr(), array.cast());
    let token = crate::readers::garray_u32(array)[0];
    let mut count = UNTOUCHED;
    facade::token_get_n_pronunciation(inst, token, &raw mut count);
    let mut out = obj! { "n" => count };
    for nth in [0, count - 1, count, count + 1, 0xFFFF_FFFF] {
        let keys = new_keys();
        let ret = facade::token_get_nth_pronunciation(inst, token, nth, keys.cast());
        let (len, content) = keys_view(keys);
        let mut row = vec![J::from(ret), J::from(len)];
        if nth < count {
            row.push(J::from(content.clone()));
        }
        out.set(format!("nth {nth}"), row);
        out.set(format!("~content nth {nth}"), content);
    }
    let keys = new_keys();
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
    facade::parse_more_chewings(first, c("su3").as_ptr());
    facade::parse_more_chewings(second, c("cl3").as_ptr());
    let null = std::ptr::null_mut;
    let (mut a, mut b, mut ra, mut rb): (P, P, P, P) = (null(), null(), null(), null());
    let rets = vec![
        facade::get_zhuyin_key(first, 0, &raw mut a),
        facade::get_zhuyin_key(second, 0, &raw mut b),
        facade::get_zhuyin_key_rest(first, 0, &raw mut ra),
        facade::get_zhuyin_key_rest(second, 0, &raw mut rb),
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
            facade::get_zhuyin_key_rest_length(k.inst(), rest.as_mut_ptr().cast(), &raw mut length);
        out.set(format!("{begin}..{end}"), (ret, length));
    }
    out
}

/// `zhuyin_candidates_at_start`: the candidate count after guessing at the start of su3cl3.
fn candidates_at_start(k: &mut Kit) -> u32 {
    let inst = k.alloc();
    facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    facade::guess_candidates_after_cursor(inst, 0);
    let mut count = 0_u32;
    facade::get_n_candidate(inst, &raw mut count);
    count
}

fn unload_effect(k: &mut Kit) -> J {
    let mut out = obj! { "before" => candidates_at_start(k) };
    for index in [3_u8, 2, 4] {
        let unloaded = facade::unload_phrase_library(k.ctx(), index);
        out.set(
            format!("unload {index}"),
            (unloaded, candidates_at_start(k)),
        );
        let loaded = facade::load_phrase_library(k.ctx(), index);
        out.set(format!("load {index}"), (loaded, candidates_at_start(k)));
        out.set(
            format!("load {index} again"),
            facade::load_phrase_library(k.ctx(), index),
        );
    }
    out
}

fn alloc_after_fini(k: &mut Kit) -> J {
    let context = k.init_default();
    let live = facade::alloc_instance(context);
    facade::free_instance(live);
    facade::fini(context);
    let after = facade::alloc_instance(context);
    obj! { "live instance" => !live.is_null(), "~instance after fini" => !after.is_null() }
}

/// The registered cases.
pub fn case(k: &mut Kit, id: &str) -> J {
    match id {
        "user-library-token-unload-pinyin" | "user-library-token-unload-zhuyin" => {
            user_library_token_unload(k)
        }
        "zhuyin-sentence-before-guess" => sentence_before_guess(k),
        "zhuyin-character-offset-out" => character_offset_out(k),
        "zhuyin-token-get-phrase-out" => token_get_phrase_out(k),
        "zhuyin-nth-pronunciation-range" => nth_pronunciation_range(k),
        "zhuyin-static-key-slots" => static_key_slots(k),
        "zhuyin-key-rest-length" => key_rest_length(k),
        "zhuyin-unload-phrase-library-repeat" => {
            let ctx = k.ctx();
            let unload: Vec<bool> = (0..16_u8)
                .chain([2, 2])
                .map(|index| facade::unload_phrase_library(ctx, index))
                .collect();
            obj! { "unload" => unload }
        }
        "zhuyin-unload-phrase-library-effect" => unload_effect(k),
        "zhuyin-alloc-instance-after-fini" => alloc_after_fini(k),
        other => panic!("no case {other}"),
    }
}
