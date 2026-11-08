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

use std::ffi::c_char;

use super::breaks::break_library;
use super::{LIBRARY, chunk_limits, chunk_stats, init, mask_four, parse, train_once, word};
use crate::facade;
use crate::fixtures::{private_system, write_user_conf};
use crate::readers::{candidate_at, candidate_text, candidate_type, n_candidate};
use crate::support::{J, Kit, take_text};

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

/// `chunk_init_probe`: merged.bin replaced by `how` under a fresh profile, then
/// init and the fixture's sizes.
pub(super) fn init_probe(k: &mut Kit, how: &str) -> J {
    chunk_limits();
    let system = private_system(k);
    let path = system.join(LIBRARY);
    break_library(k, &system, LIBRARY, how);
    let mut out = obj! { "ctx" => !init(k, &system).is_null() };
    chunk_stats(&mut out, &path);
    out
}

/// `chunk_addon_probe`: art.bin replaced by `how`, then the addon load and,
/// with `reload`, an unload and a second load.
pub(super) fn addon_probe(k: &mut Kit, how: &str, reload: bool) -> J {
    chunk_limits();
    let system = private_system(k);
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let path = system.join("art.bin");
    break_library(k, &system, "art.bin", how);
    let mut out = obj! { "first" => facade::load_addon_phrase_library(ctx, 4) };
    if reload {
        out.set("unload", facade::unload_addon_phrase_library(ctx, 4));
        out.set("again", facade::load_addon_phrase_library(ctx, 4));
    }
    chunk_stats(&mut out, &path);
    out
}

/// `chunk_load_probe`: library 2 unloaded, gbk_char.bin replaced by `how`, reloaded.
pub(super) fn load_probe(k: &mut Kit, how: &str) -> J {
    chunk_limits();
    let system = private_system(k);
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let unload = facade::unload_phrase_library(ctx, 2);
    break_library(k, &system, "gbk_char.bin", how);
    obj! { "unload" => unload, "load" => facade::load_phrase_library(ctx, 2) }
}

/// The init on a merged.bin replaced by `how`, then `then` on a trained
/// context; the fixture's sizes either way.
fn trained_probe(k: &mut Kit, how: &str, key: &str, then: fn(crate::support::P) -> bool) -> J {
    chunk_limits();
    let system = private_system(k);
    let path = system.join(LIBRARY);
    break_library(k, &system, LIBRARY, how);
    let ctx = init(k, &system);
    let mut out = obj! { "ctx" => !ctx.is_null() };
    if !ctx.is_null() {
        out.set("train", train_once(ctx));
        out.set(key, then(ctx));
    }
    chunk_stats(&mut out, &path);
    out
}

/// `chunk_save_probe`: a trained save over a merged.bin replaced before init.
pub(super) fn save_probe(k: &mut Kit, how: &str) -> J {
    trained_probe(k, how, "save", facade::save)
}

/// `chunk_mask_probe`: a trained library-4 mask-out over a merged.bin replaced before init.
pub(super) fn mask_probe(k: &mut Kit, how: &str) -> J {
    trained_probe(k, how, "mask_out", mask_four)
}

/// `chunk_reinit_probe`: two inits against a sparse merged.bin, each context finalised.
pub(super) fn reinit_probe(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    let path = system.join(LIBRARY);
    break_library(k, &system, LIBRARY, "sparse");
    let first = init(k, &system);
    if !first.is_null() {
        facade::fini(first);
    }
    let second = init(k, &system);
    let mut out = obj! { "ctxs" => (!first.is_null(), !second.is_null()) };
    if !second.is_null() {
        facade::fini(second);
    }
    chunk_stats(&mut out, &path);
    out
}

/// `chunk_candidates_probe`: a guess through the sparse-library context and at
/// most eight `[type, text]` rows (an empty or absent text as `None`).
pub(super) fn candidates_probe(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    break_library(k, &system, LIBRARY, "sparse");
    let ctx = init(k, &system);
    let mut out = obj! { "ctx" => !ctx.is_null() };
    if ctx.is_null() {
        return out;
    }
    let inst = facade::alloc_instance(ctx);
    out.set("parse", parse(inst, text()));
    out.set("guess_sentence", facade::guess_sentence(inst));
    out.set("guess_candidates", guess_candidates(inst));
    let n = n_candidate(inst);
    out.set("n", n);
    let rows: Vec<J> = (0..n.min(8))
        .map(|index| {
            let cand = candidate_at(inst, index).unwrap_or(std::ptr::null_mut());
            let kind = candidate_type(inst, cand);
            let text = candidate_text(inst, cand).filter(|text| !text.is_empty());
            J::from((kind, text))
        })
        .collect();
    out.set("rows", rows);
    out
}

/// `chunk_token_probe`: the token getters of library 4's first two items after
/// the sparse-library init. The empty sub-index owns no item: every token read
/// refuses, and (pinyin) the library's export range is empty.
pub(super) fn token_probe(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    break_library(k, &system, LIBRARY, "sparse");
    let ctx = init(k, &system);
    let mut out = obj! { "ctx" => !ctx.is_null() };
    if ctx.is_null() {
        return out;
    }
    let inst = facade::alloc_instance(ctx);
    for token in [0x0400_0001_u32, 0x0400_0002] {
        let (mut length, mut text) = (0_u32, std::ptr::null_mut::<c_char>());
        let (mut n, mut freq) = (0_u32, 0_u32);
        let phrase = facade::token_get_phrase(inst, token, &raw mut length, &raw mut text);
        let text = take_text(text);
        let prons = facade::token_get_n_pronunciation(inst, token, &raw mut n);
        let unigram = facade::token_get_unigram_frequency(inst, token, &raw mut freq);
        out.set(
            format!("{token:#x}"),
            J::L(vec![
                phrase.into(),
                length.into(),
                text.into(),
                prons.into(),
                n.into(),
                unigram.into(),
                freq.into(),
            ]),
        );
    }
    if !facade::ZHUYIN {
        let it = facade::begin_get_phrases(ctx, 4);
        out.set("has_next", facade::iterator_has_next_phrase(it));
        facade::end_get_phrases(it);
    }
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

/// `nonzero_total_save_probe`: a trained save with the total-123 malformed
/// merged.bin. The local sub-index total survives in the failed library's log:
/// the file's length and the two words of its MODIFY_HEADER record.
pub(super) fn nonzero_total_save(k: &mut Kit) -> J {
    chunk_limits();
    let system = private_system(k);
    break_library(k, &system, LIBRARY, "nonzero-layout");
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let mut out = obj! { "train" => train_once(ctx) };
    out.set("save", facade::save(ctx));
    let data = std::fs::read(k.user.join("merged.dbin")).expect("merged.dbin");
    let payload = &data[8..];
    out.set("dbin", (data.len(), word(payload, 10), word(payload, 14)));
    out
}

/// `chunk_frame_system`: merged.bin copied exactly or one byte long, then init.
pub(super) fn frame_system(k: &mut Kit, trailing: bool) -> J {
    let system = private_system(k);
    let path = system.join(LIBRARY);
    let mut data = std::fs::read(&path).expect("merged.bin");
    std::fs::remove_file(&path).expect("unlink merged.bin");
    if trailing {
        data.push(1);
    }
    std::fs::write(&path, data).expect("write merged.bin");
    write_user_conf(k);
    obj! { "ctx" => !init(k, &system).is_null() }
}
