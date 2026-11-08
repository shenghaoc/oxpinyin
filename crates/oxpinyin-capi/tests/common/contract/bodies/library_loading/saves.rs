//! Saves after a system library was replaced, removed or unloaded underneath a
//! trained context.

use super::breaks::break_library;
use super::{
    LIBRARY, chunk_limits, init, le, le16, link, log_walk, modify_header, nonzero_layout_chunk,
    replace_file, sha256, train_once, word, write_log,
};
use crate::facade;
use crate::fixtures::{private_system, write_user_conf};
use crate::support::{J, Kit};

/// `save_replacement_probe`: a trained save with merged.bin replaced after its
/// original bytes were loaded. `initial` `invalid` installs the malformed layout
/// before init; `replacement` is `same`, `invalid`, `valid` or `different` (the
/// gbk sub-index's bytes under the merged path). Answers the saved log's length,
/// SHA-256, first header totals and record counts. Under a fatal mask the pin's
/// `_write_files` critical dies before the failing library's log; the subject
/// writes no critical and completes the save (#1297, D1).
pub(super) fn replacement_probe(k: &mut Kit, initial: &str, replacement: &str) -> J {
    chunk_limits();
    let system = private_system(k);
    if initial == "invalid" {
        break_library(k, &system, LIBRARY, "nonzero-layout");
    }
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let mut out = obj! { "train" => train_once(ctx) };
    if replacement != "same" {
        let data = match replacement {
            "invalid" => nonzero_layout_chunk(),
            "valid" => std::fs::read(k.data.join(LIBRARY)).expect("merged.bin"),
            _ => std::fs::read(k.data.join("gbk_char.bin")).expect("gbk_char.bin"),
        };
        replace_file(&system.join(LIBRARY), &data);
    }
    out.set("save", facade::save(ctx));
    let data = std::fs::read(k.user.join("merged.dbin")).expect("merged.dbin");
    let (header, counts) = log_walk(&data);
    out.set(
        "merged",
        J::L(vec![data.len().into(), sha256(&data).into(), header]),
    );
    out.set("records", counts);
    out
}

/// `save_changed_with_override_probe`: a loaded user override of a gb_char item
/// (token 0x01001225 from 你 to 龘) plus a different valid gb_char chunk on disk:
/// the save diff must union the disk, the loaded original and the user override
/// (`diff_records_against`). The layouts are `SubPhraseIndex::load` and
/// `PhraseItem::get_phrase_string` (`phrase_index.cpp`) and
/// `PhraseIndexLogger::next_record` (`phrase_index_logger.h`).
pub(super) fn changed_with_override(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    let bytes = std::fs::read(k.data.join("gb_char.bin")).expect("gb_char.bin");
    let payload = &bytes[8..];
    let at = |value: u32| usize::try_from(value).expect("an offset");
    let (total, index, content) = (word(payload, 0), word(payload, 4), word(payload, 8));
    let token = 0x0100_1225_u32;
    let slot = token & 0x00FF_FFFF;
    let offset = at(content + word(payload, at(index + slot * 4)));
    let (length, prons) = (
        usize::from(payload[offset]),
        usize::from(payload[offset + 1]),
    );
    let size = 6 + length * 4 + prons * (length * 2 + 4);
    let old = &payload[offset..offset + size];
    assert!(length == 1 && word(old, 6) == u32::from('你'));
    let new = [&old[..6], &le(u32::from('龘')), &old[10..]].concat();
    let record = [
        &le(3)[..],
        &le(slot),
        &le16(old.len()),
        &le16(new.len()),
        old,
        &new,
    ]
    .concat();
    write_user_conf(k);
    write_log(
        k,
        &[modify_header(0, total, total), record].concat(),
        "gb_char.dbin",
    );
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let mut out = obj! { "train" => train_once(ctx) };
    let data = std::fs::read(k.data.join("gbk_char.bin")).expect("gbk_char.bin");
    replace_file(&system.join("gb_char.bin"), &data);
    out.set("save", facade::save(ctx));
    let data = std::fs::read(k.user.join("gb_char.dbin")).expect("gb_char.dbin");
    let (header, counts) = log_walk(&data);
    out.set(
        "dbin",
        J::L(vec![data.len().into(), sha256(&data).into(), header]),
    );
    out.set("records", counts);
    out
}

/// `save_missing_system_probe`: a modified save after the first system chunk is
/// removed. The pin prints `mmap %s failed!` and dies in the empty chunk's
/// `SubPhraseIndex::load` (row 76); the subject prints the line, stops at that
/// library and answers `false`, the registered row 76 safe outcome.
pub(super) fn missing_system(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let mut out = obj! { "train" => train_once(ctx) };
    std::fs::remove_file(system.join("gb_char.bin")).expect("unlink gb_char.bin");
    out.set("save", facade::save(ctx));
    out
}

/// The user directory's `.dbin` and `.tmp` names, sorted.
fn log_files(k: &Kit) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(&k.user)
        .expect("the user directory")
        .map(|entry| entry.expect("a user entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".dbin") || name.ends_with(".tmp"))
        .collect();
    names.sort();
    names
}

/// `save_remap_retry_probe`: a failed save that stops at a library whose file
/// cannot be mapped again, then a repair and a retry (register row 76, ruling
/// 2026-11-01). Library two's system file is removed and a stale
/// `gbk_char.dbin.tmp` is left in the user directory. The pin writes `mmap %s
/// failed!` at library two and dies. The subject stops there: the save answers
/// `false`, library one's files stand as the facade's order leaves them, the
/// stale `.tmp` is untouched, no marker is written, and the session stays dirty
/// and usable; once the file is repaired the next save writes every library.
pub(super) fn remap_retry(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let mut out = obj! { "train" => train_once(ctx) };
    let stale = k.user.join("gbk_char.dbin.tmp");
    std::fs::write(&stale, b"stale").expect("the stale .tmp");
    let conf = k.user.join("user.conf");
    let read_conf = || {
        conf.exists()
            .then(|| std::fs::read(&conf).expect("user.conf"))
    };
    let marker = read_conf();
    let library = system.join("gbk_char.bin");
    std::fs::remove_file(&library).expect("unlink gbk_char.bin");
    out.set("save", facade::save(ctx));
    out.set("stopped-files", log_files(k));
    out.set(
        "stale-tmp",
        stale
            .exists()
            .then(|| String::from_utf8(std::fs::read(&stale).expect("stale")).expect("UTF-8")),
    );
    out.set("marker-kept", read_conf() == marker);
    // The session is dirty and usable: a second edit trains, and a repaired
    // file lets the next save run to the end.
    out.set("retrain", train_once(ctx));
    link(&k.data.join("gbk_char.bin"), &library);
    out.set("retry-save", facade::save(ctx));
    out.set("saved-files", log_files(k));
    let data = std::fs::read(k.user.join("merged.dbin")).expect("merged.dbin");
    out.set("merged-records", log_walk(&data).1);
    out
}

/// `save_unloaded_replacement_probe`: library 2 unloaded, its file replaced by a
/// different valid chunk, then a save. `_write_files` skips a library whose
/// `get_range` has no sub-index (`pinyin.cpp:928-933`, `zhuyin.cpp:563-568`;
/// `zhuyin_save` at `zhuyin.cpp:560-568`), so the pin never remaps it.
pub(super) fn unloaded_replacement(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let mut out = obj! { "train" => train_once(ctx) };
    out.set("unload", facade::unload_phrase_library(ctx, 2));
    let data = std::fs::read(k.data.join(LIBRARY)).expect("merged.bin");
    replace_file(&system.join("gbk_char.bin"), &data);
    out.set("save", facade::save(ctx));
    out
}
