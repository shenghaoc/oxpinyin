//! The pin's ignored `SubPhraseIndex::load` failure. `MemoryChunk::mmap` accepts
//! a header whose length word matches `(file_size - 8)` truncated to `guint32`
//! (`memory_chunk.h:499`) and whose checksum verifies; the pinned
//! `SubPhraseIndex::load` then rejects the layout with a `g_return_val_if_fail`
//! critical (`phrase_index.cpp:354`), which `_load_phrase_library` ignores
//! (`pinyin.cpp:264-277`, `zhuyin.cpp:208-221`) and the caller reports as
//! success. The sparse 2^32+8 chunk is one instance (its truncated payload is
//! 0); the eight-byte header-only chunk is the control that separates the
//! truncation from the ignored layout failure. The file replaces merged.bin,
//! gbk_char.bin (the one unloadable system library) or art.bin (the pinyin
//! facade's addon entry).

use super::breaks::break_library;
use super::{LIBRARY, chunk_limits, chunk_stats, init, parse, word};
use crate::facade;
use crate::fixtures::{private_system, write_user_conf};
use crate::support::{J, Kit};

/// `LIBRARY_FACADES`' text for the facade's parse call.
fn text() -> &'static [u8] {
    if facade::ZHUYIN { b"su3cl3" } else { b"nihao" }
}

/// The facade's candidate guess at offset 0 (pinyin sort word 0).
fn guess_candidates(inst: crate::support::P) -> bool {
    if facade::ZHUYIN {
        facade::guess_candidates_after_cursor(inst, 0)
    } else {
        facade::guess_candidates(inst, 0, 0)
    }
}

/// `chunk_init_probe`: merged.bin replaced by the sparse chunk under a fresh
/// profile, then init and the fixture's sizes.
pub(super) fn init_probe(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    let path = system.join(LIBRARY);
    break_library(k, &system, LIBRARY, "sparse");
    let mut out = obj! { "ctx" => !init(k, &system).is_null() };
    chunk_stats(&mut out, &path);
    out
}

/// `nonzero_total_candidates_probe`: a guess with a malformed merged.bin whose
/// sub-index total is nonzero, after a token update that tries to wrap the
/// facade total to zero. The loaded valid libraries' stored totals are summed
/// modulo 2^32; the failed merged.bin is excluded by the pin. Both facades
/// reproduce the `0 < total_freq` abort (row 61), after the init's layout
/// critical.
pub(super) fn nonzero_total_candidates(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    break_library(k, &system, LIBRARY, "nonzero-layout");
    let total =
        ["gb_char.bin", "gbk_char.bin", "opengram.bin"]
            .iter()
            .fold(0_u32, |total, name| {
                let bytes = std::fs::read(system.join(name)).expect("a system library");
                total.wrapping_add(word(&bytes, 8))
            });
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let inst = facade::alloc_instance(ctx);
    let delta = 0_u32.wrapping_sub(total);
    let mut out = obj! { "add" => facade::token_add_unigram_frequency(inst, 0x0400_0001, delta) };
    out.set("parse", parse(inst, text()));
    out.set("guess_sentence", facade::guess_sentence(inst));
    out.set("ret", guess_candidates(inst));
    out
}

/// `chunk_frame_system`: merged.bin copied exactly or one byte long, then init.
pub(super) fn frame_system(k: &mut Kit) -> J {
    let system = private_system(k);
    let path = system.join(LIBRARY);
    let data = std::fs::read(&path).expect("merged.bin");
    std::fs::remove_file(&path).expect("unlink merged.bin");
    std::fs::write(&path, data).expect("write merged.bin");
    write_user_conf(k);
    obj! { "ctx" => !init(k, &system).is_null() }
}
