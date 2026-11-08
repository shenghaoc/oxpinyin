//! Saves after a system library was replaced, removed or unloaded underneath a
//! trained context.

use super::{
    LIBRARY, chunk_limits, init, log_walk, nonzero_layout_chunk, replace_file, sha256, train_once,
};
use crate::facade;
use crate::fixtures::private_system;
use crate::support::{J, Kit};

/// `save_replacement_probe`: a trained save with merged.bin replaced after its
/// original bytes were loaded. `initial` `invalid` installs the malformed layout
/// before init; `replacement` is `same`, `invalid`, `valid` or `different` (the
/// gbk sub-index's bytes under the merged path). Answers the saved log's length,
/// SHA-256, first header totals and record counts. Under a fatal mask the pin's
/// `_write_files` critical dies before the failing library's log; the subject
/// writes no critical and completes the save (#1297, D1).
pub(super) fn replacement_probe(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let mut out = obj! { "train" => train_once(ctx) };
    replace_file(&system.join(LIBRARY), &nonzero_layout_chunk());
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
