//! Crafted items inside a system library: shared and overlapping extents, a
//! replacement reloaded without a user directory, and a SYSTEM_FILE above 4.

use std::ffi::c_char;

use super::{at_offset, le, unigram, word};
use crate::facade;
use crate::fixtures::{chunk, private_system};
use crate::readers::{c, lookup};
use crate::support::{J, Kit, P, take_text};

/// `token_get_nth_pronunciation(token, 0)`'s key bytes, empty on failure.
fn nth_key(inst: P, token: u32) -> Vec<u8> {
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 2) };
    let ok = facade::token_get_nth_pronunciation(inst, token, 0, array.cast());
    let keys = if ok {
        crate::readers::garray_u16(array)
    } else {
        Vec::new()
    };
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    keys.iter().flat_map(|key| key.to_le_bytes()).collect()
}

/// `transient_reload_replacement_probe`: a replacement item's bytes and unigram
/// after a NULL-user reload of library 2, then an import over it.
pub(super) fn transient_reload_replacement(k: &mut Kit) -> J {
    let system = private_system(k);
    let ctx = k.init(Some(&system), None);
    assert!(!ctx.is_null(), "transient init failed");
    let inst = facade::alloc_instance(ctx);
    assert!(!inst.is_null());
    let path = system.join("gbk_char.bin");
    let mut payload = std::fs::read(&path).expect("gbk_char.bin")[8..].to_vec();
    let (index, content) = (word(&payload, 4), word(&payload, 8));
    let slots = (content - index - 1) / 4;
    let (slot, token, before_text) = (1..slots)
        .find_map(|slot| {
            let offset = at_offset(word(&payload, at_offset(index + slot * 4)));
            let item = at_offset(content) + offset;
            if offset == 0 || payload[item] != 1 || payload[item + 1] == 0 {
                return None;
            }
            let token = 0x0200_0000 | slot;
            let (mut length, mut text) = (0_u32, std::ptr::null_mut::<c_char>());
            let ret = facade::token_get_phrase(inst, token, &raw mut length, &raw mut text);
            let text = take_text(text).filter(|text| ret && !text.is_empty())?;
            let found = lookup(inst, text.as_bytes());
            (found.len() <= 4
                && found.iter().filter(|t| *t >> 24 == 2).count() == 1
                && found.contains(&token))
            .then_some((slot, token, text))
        })
        .expect("no suitable slot");
    let before_count = unigram(inst, token);
    let before_key = nth_key(inst, token);
    assert!(before_count.0 && before_count.1 < 0xffff_ffef && before_key.len() == 2);
    assert!(facade::unload_phrase_library(ctx, 2));
    let item = at_offset(content + word(&payload, at_offset(index + slot * 4)));
    payload[item + 2..item + 6].copy_from_slice(&le(before_count.1 + 7));
    payload[item + 10] ^= 1;
    std::fs::remove_file(&path).expect("unlink gbk_char.bin");
    std::fs::write(&path, chunk(&payload)).expect("write gbk_char.bin");
    assert!(facade::load_phrase_library(ctx, 2));
    let after_key = nth_key(inst, token);
    let after_count = unigram(inst, token);
    let iterator = facade::begin_add_phrases(ctx, 2);
    let reading = if facade::ZHUYIN { "ㄋㄧˇ" } else { "ni3" };
    let added =
        facade::iterator_add_phrase(iterator, c(&before_text).as_ptr(), c(reading).as_ptr(), 1);
    facade::end_add_phrases(iterator);
    let imported_key = nth_key(inst, token);
    obj! {
        "replacement_key" => after_key != before_key,
        "replacement_unigram" => after_count.0 && after_count.1 == before_count.1 + 7,
        "import" => added,
        "import_replacement_key" => imported_key == after_key,
    }
}
