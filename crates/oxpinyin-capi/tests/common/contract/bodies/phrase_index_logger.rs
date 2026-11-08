//! The phrase_index_logger family's case bodies: each answers the result document the old case returned.
//!
//! Phrase-index logger journals at init, mask-out and reload (inventory row 37, #525, #1175).

use std::path::Path;

use crate::facade;
use crate::fixtures::{modify_header, private_system, write_log, write_user_conf};
use crate::readers::c;
use crate::support::{J, Kit, P};

/// Library 1's journal.
const GB: &str = "gb_char.dbin";
/// Library 2's journal.
const GBK: &str = "gbk_char.dbin";

/// `_HEADER_TOKEN`: a header record with a non-null token.
fn header_token() -> Vec<u8> {
    modify_header(0x0100_0001, 1, 1)
}

/// `_TWO_HEADERS`: two header records.
fn two_headers() -> Vec<u8> {
    [modify_header(0, 1, 1), modify_header(0, 1, 2)].concat()
}

/// `_BAD_TAIL`: a full record head with an invalid type.
fn bad_tail() -> Vec<u8> {
    [0x99_u32.to_le_bytes(), 0_u32.to_le_bytes()].concat()
}

/// `_KEPT_ADD`: a complete ADD a mask on library 1's slot 9 keeps.
fn kept_add() -> Vec<u8> {
    [
        &1_u32.to_le_bytes()[..],
        &0x0100_0005_u32.to_le_bytes(),
        &8_u16.to_le_bytes(),
        &[1, 0, 7, 0, 0, 0, 0, 0],
    ]
    .concat()
}

/// `_GBK_SHORT_ADD`: library 2's one-byte ADD.
fn gbk_short_add() -> Vec<u8> {
    [
        &1_u32.to_le_bytes()[..],
        &0x0200_0005_u32.to_le_bytes(),
        &1_u16.to_le_bytes(),
        b"\x01",
    ]
    .concat()
}

/// The successful init on the kit's (or `system`'s) data.
fn init_ok(k: &Kit, system: Option<&Path>) -> P {
    let ctx = k.init(Some(system.unwrap_or(&k.data)), Some(&k.user));
    assert!(!ctx.is_null(), "init failed");
    ctx
}

/// `{'ret': ...}`.
fn ret(value: bool) -> J {
    obj! { "ret" => value }
}

/// `logger_init_probe`: a journal crafted before the only init.
fn init_probe(k: &Kit, payload: &[u8]) -> J {
    write_user_conf(k);
    write_log(k, payload, GB);
    ret(!k.init_default().is_null())
}

/// `logger_mask_out_probe`: a journal corrupted after init, then a mask.
fn mask_out_probe(k: &Kit, payload: &[u8], mask: u32, value: u32) -> J {
    write_user_conf(k);
    let ctx = init_ok(k, None);
    write_log(k, payload, GB);
    ret(facade::mask_out(ctx, mask, value))
}

/// `logger_mask_out_later_unmappable`: library 4's system chunk gone after init.
fn mask_out_later_unmappable(k: &Kit, payload: &[u8]) -> J {
    let system = private_system(k);
    write_user_conf(k);
    let ctx = init_ok(k, Some(&system));
    write_log(k, payload, GB);
    std::fs::remove_file(system.join("merged.bin")).expect("unlink merged.bin");
    ret(facade::mask_out(ctx, 0, 0))
}

/// Library 2's chunk gone, library 4's a directory with a long entry.
fn break_library(system: &Path) {
    std::fs::remove_file(system.join("gbk_char.bin")).expect("unlink gbk_char.bin");
    std::fs::remove_file(system.join("merged.bin")).expect("unlink merged.bin");
    std::fs::create_dir_all(system.join("merged.bin")).expect("merged.bin directory");
    std::fs::write(system.join("merged.bin").join("e".repeat(25)), b"x").expect("long entry");
}

/// `logger_later_directory`: the journal fault in library 1 before library 4's directory.
fn later_directory(k: &Kit, payload: &[u8], at_init: bool) -> J {
    let system = private_system(k);
    write_user_conf(k);
    if at_init {
        write_log(k, payload, GB);
        break_library(&system);
        return ret(!k.init(Some(&system), Some(&k.user)).is_null());
    }
    let ctx = init_ok(k, Some(&system));
    write_log(k, payload, GB);
    break_library(&system);
    ret(facade::mask_out(ctx, 0, 0))
}

/// `logger_mask_out_unloaded_probe`: a journal for the library unloaded first.
fn mask_out_unloaded(k: &Kit, payload: &[u8]) -> J {
    write_user_conf(k);
    let ctx = init_ok(k, None);
    write_log(k, payload, GBK);
    assert!(facade::unload_phrase_library(ctx, 2), "the unload failed");
    ret(facade::mask_out(ctx, 0, 0))
}

/// `logger_mask_out_after_save`: a saved profile, then a journal the mask aborts on.
fn mask_out_after_save(k: &Kit, payload: &[u8]) -> J {
    write_user_conf(k);
    let ctx = init_ok(k, None);
    let reading = if facade::ZHUYIN {
        "ㄋㄧˇ ㄏㄠˇ"
    } else {
        "ni3'hao3"
    };
    let it = facade::begin_add_phrases(ctx, 7);
    let added = facade::iterator_add_phrase(it, c("你好").as_ptr(), c(reading).as_ptr(), 1);
    facade::end_add_phrases(it);
    assert!(added, "the setup phrase did not add");
    assert!(facade::save(ctx));
    write_log(k, payload, GB);
    ret(facade::mask_out(ctx, 0, 0))
}

/// `logger_reload_probe`: the unloaded GBK library loaded again over its journal.
fn reload_probe(k: &Kit, payload: &[u8]) -> J {
    write_user_conf(k);
    let ctx = init_ok(k, None);
    assert!(facade::unload_phrase_library(ctx, 2), "the unload failed");
    write_log(k, payload, GBK);
    ret(facade::load_phrase_library(ctx, 2))
}

/// `logger_mask_out_two_libraries`: journals for libraries 1 and 2, then a mask.
fn mask_out_two(k: &Kit, first: &[u8], second: &[u8], mask: u32, value: u32) -> J {
    write_user_conf(k);
    let ctx = init_ok(k, None);
    write_log(k, first, GB);
    write_log(k, second, GBK);
    ret(facade::mask_out(ctx, mask, value))
}

/// The case `id`'s result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    let name = id
        .strip_suffix("-pinyin")
        .or_else(|| id.strip_suffix("-zhuyin"))
        .expect("a facade suffix");
    let one_header = modify_header(0, 1, 1);
    match name {
        "abort-init-log-header-token" => init_probe(k, &header_token()),
        "abort-mask-out-log-header-token" => mask_out_probe(k, &header_token(), 0, 0),
        "abort-mask-out-multiple-log-headers" => mask_out_probe(k, &two_headers(), 0, 0),
        "abort-mask-out-multiple-log-headers-bad-tail" => {
            mask_out_probe(k, &[two_headers(), bad_tail()].concat(), 0, 0)
        }
        "abort-mask-out-log-header-untouched" => mask_out_after_save(k, &two_headers()),
        "mask-out-unloaded-library-log" => mask_out_unloaded(k, &two_headers()),
        "init-multiple-log-headers" => init_probe(k, &two_headers()),
        "abort-mask-out-log-header-token-later-unmappable" => {
            mask_out_later_unmappable(k, &header_token())
        }
        "abort-init-log-header-token-later-directory" => later_directory(k, &header_token(), true),
        "abort-mask-out-log-header-token-later-directory" => {
            later_directory(k, &header_token(), false)
        }
        "mask-out-log-kept-add-bad-tail" => {
            let payload = [one_header, kept_add(), bad_tail()].concat();
            mask_out_probe(k, &payload, 0x0FFF_FFFF, 0x0100_0009)
        }
        "mask-out-log-header-bad-tail" => {
            mask_out_probe(k, &[modify_header(0, 1, 2), bad_tail()].concat(), 0, 0)
        }
        "abort-reload-log-header-token" => reload_probe(k, &modify_header(0x0200_0001, 1, 1)),
        "abort-reload-short-add" => reload_probe(k, &gbk_short_add()),
        "abort-mask-out-second-library-multiple-log-headers" => {
            mask_out_two(k, &one_header, &two_headers(), 0, 0)
        }
        "mask-out-second-library-other-bits" => {
            mask_out_two(k, &one_header, &two_headers(), 0x0F00_0000, 0x0100_0000)
        }
        other => panic!("unknown phrase_index_logger case {other}"),
    }
}
