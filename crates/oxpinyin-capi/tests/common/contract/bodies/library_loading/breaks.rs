//! Libraries `MemoryChunk::mmap` refuses, at each site that maps one, and the
//! guards that keep the pin silent with the same broken library.

use std::path::Path;

use super::{
    LIBRARY, chunk_limits, init, le, link, mask_four, nonzero_layout_chunk, sparse_chunk,
    train_once,
};
use crate::facade;
use crate::fixtures::{chunk, private_system};
use crate::support::{J, Kit};

/// `break_library`: replaces the link to library `name` with a file
/// `MemoryChunk::mmap` refuses: none (`missing`), empty (`empty`), shorter than
/// the 8-byte header (`short`), a wrong length word (`bad-length`) or a payload
/// whose checksum does not verify (`checksum`); or with the header-only 8-byte
/// chunk (`header-only`) or the sparse 2^32+8 chunk (`sparse`), which both map
/// but whose sub-index the pin refuses while main refuses the chunk; or with the
/// layouts `oob-layout`, `short-chunk`, `nonzero-layout`, `second-separator`,
/// `third-separator`, or a directory (`dir-long`). The link is removed first.
/// Layout checks follow `MemoryChunk::mmap` (`memory_chunk.h:479-518`) and
/// `SubPhraseIndex::load` (`phrase_index.cpp:335-362`).
pub(super) fn break_library(k: &Kit, system: &Path, name: &str, how: &str) {
    let path = system.join(name);
    std::fs::remove_file(&path).expect("unlink the library");
    let write = |bytes: &[u8]| std::fs::write(&path, bytes).expect("write the broken library");
    let layout = |words: &[(usize, u32)], marks: &[(usize, u8)], length: usize| {
        let mut payload = vec![0_u8; length];
        for (at, value) in words {
            payload[*at..at + 4].copy_from_slice(&le(*value));
        }
        for (at, mark) in marks {
            payload[*at] = *mark;
        }
        chunk(&payload)
    };
    match how {
        "empty" => write(b""),
        "short" => write(&[0; 4]),
        // A 16-byte file, payload 8, declared length 7: the pin's truncated
        // length check refuses it before the checksum.
        "bad-length" => write(&[&le(7)[..], &le(0), &[0; 8]].concat()),
        "header-only" => write(&[0; 8]),
        "sparse" => sparse_chunk(&path),
        // An in-bounds four-word header whose second separator index lies past
        // the payload. The pin reads the separator outside the mapped chunk
        // (`phrase_index.cpp:354-356`); oxpinyin refuses the layout.
        "oob-layout" => write(&layout(
            &[(0, 123), (4, 17), (8, 1 << 28), (12, 24)],
            &[(16, b'#')],
            32,
        )),
        // A payload shorter than the four-word header: the pin's
        // `SubPhraseIndex::load` reads the index words past the mapped chunk.
        "short-chunk" => write(&layout(&[(0, 123), (4, 17)], &[], 8)),
        "nonzero-layout" => write(&nonzero_layout_chunk()),
        // A valid header and checksum whose second or third
        // `SubPhraseIndex::load` separator is in bounds but is not '#': the
        // pin's checks at `phrase_index.cpp:355-356`.
        "second-separator" => write(&layout(
            &[(4, 17), (8, 20), (12, 21)],
            &[(16, b'#'), (19, b'!')],
            32,
        )),
        "third-separator" => write(&layout(
            &[(4, 17), (8, 20), (12, 24)],
            &[(16, b'#'), (19, b'#'), (23, b'!')],
            32,
        )),
        "checksum" => {
            let mut data = std::fs::read(k.data.join(name)).expect("the shipped library");
            *data.last_mut().expect("a non-empty library") ^= 0xff;
            write(&data);
        }
        // A non-empty directory in the library's place. The size the pin reads
        // from `lseek(SEEK_END)` is the file system's, so which
        // `MemoryChunk::mmap` site is reached follows it
        // (`memory_chunk.h:479-483` against `:493`).
        "dir-long" => {
            std::fs::create_dir_all(&path).expect("the directory");
            std::fs::write(path.join("e".repeat(25)), b"x").expect("the directory's file");
        }
        "missing" => {}
        other => panic!("no break {other}"),
    }
}

/// `library_probe`: breaks a private library before the selected operation.
/// At `init` the profile is fresh; at `load` the GBK library (index 2, the one
/// both facades let go) is unloaded first; at `addon` art.bin (addon 4); at
/// `save` and `mask-out` the context trains first.
pub(super) fn library_probe(k: &mut Kit, site: &str, how: &str) -> J {
    let system = private_system(k);
    if site == "init" {
        break_library(k, &system, LIBRARY, how);
        return obj! { "ctx" => !init(k, &system).is_null() };
    }
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    match site {
        "load" => {
            let unload = facade::unload_phrase_library(ctx, 2);
            break_library(k, &system, "gbk_char.bin", how);
            obj! { "unload" => unload, "load" => facade::load_phrase_library(ctx, 2) }
        }
        "addon" => {
            let unload = facade::unload_addon_phrase_library(ctx, 4);
            break_library(k, &system, "art.bin", how);
            obj! { "unload" => unload, "load" => facade::load_addon_phrase_library(ctx, 4) }
        }
        "save" => {
            let train = train_once(ctx);
            break_library(k, &system, LIBRARY, how);
            obj! { "train" => train, "save" => facade::save(ctx) }
        }
        "mask-out" => {
            let train = train_once(ctx);
            break_library(k, &system, LIBRARY, how);
            obj! { "train" => train, "mask_out" => mask_four(ctx) }
        }
        other => panic!("no library site {other}"),
    }
}

/// `library_directory_guard`: a directory at a reserved library (index 0,
/// configured from gb_char) or at the unloaded library 2, then a save or a
/// library-4 mask-out of a trained context.
pub(super) fn directory_guard(k: &mut Kit, site: &str, guard: &str) -> J {
    let system = private_system(k);
    let conf = system.join("table.conf");
    let mut text = std::fs::read_to_string(&conf).expect("table.conf");
    let mut name = if guard == "unloaded" {
        "gbk_char.bin"
    } else {
        "gb_char.bin"
    };
    if guard == "reserved" {
        text = text.replace(
            "default RESERVED NULL NULL NULL NOT_USED",
            "default RESERVED gb_char.table reserved.bin reserved.dbin SYSTEM_FILE",
        );
        name = "reserved.bin";
        link(&k.data.join("gb_char.bin"), &system.join(name));
    }
    std::fs::remove_file(&conf).expect("unlink table.conf");
    std::fs::write(&conf, text).expect("write table.conf");
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    assert!(train_once(ctx), "train failed");
    if guard == "unloaded" {
        assert!(facade::unload_phrase_library(ctx, 2), "unload failed");
    }
    break_library(k, &system, name, "dir-long");
    let ret = if site == "save" {
        facade::save(ctx)
    } else {
        mask_four(ctx)
    };
    obj! { "ret" => ret }
}

/// `library_control`: the guards that keep the pin silent with the same broken
/// library: it is already loaded (a load asks for nothing), it is unloaded (a
/// save and a mask-out skip it), nothing is modified (a save writes nothing), or
/// it is whole (a reload maps it). The context is finalised.
pub(super) fn library_control(k: &mut Kit, site: &str) -> J {
    let system = private_system(k);
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let mut out = J::map();
    match site {
        "load-already-loaded" => {
            break_library(k, &system, "gbk_char.bin", "checksum");
            out.set("load", facade::load_phrase_library(ctx, 2));
        }
        "load-whole" => {
            out.set("unload", facade::unload_phrase_library(ctx, 2));
            out.set("load", facade::load_phrase_library(ctx, 2));
        }
        "addon-already-loaded" => {
            out.set("first", facade::load_addon_phrase_library(ctx, 4));
            break_library(k, &system, "art.bin", "checksum");
            out.set("again", facade::load_addon_phrase_library(ctx, 4));
        }
        "save-unmodified" => {
            break_library(k, &system, LIBRARY, "checksum");
            out.set("save", facade::save(ctx));
        }
        "save-unloaded" | "mask-out-unloaded" => {
            out.set("train", train_once(ctx));
            out.set("unload", facade::unload_phrase_library(ctx, 2));
            break_library(k, &system, "gbk_char.bin", "checksum");
            if site == "save-unloaded" {
                out.set("save", facade::save(ctx));
            } else {
                out.set("mask_out", mask_four(ctx));
            }
        }
        other => panic!("no control site {other}"),
    }
    facade::fini(ctx);
    out
}

/// `crash_layout_init_probe`: gbk_char.bin replaced by the layout `how`, then init.
pub(super) fn crash_layout_init(k: &mut Kit, how: &str) -> J {
    chunk_limits();
    let system = private_system(k);
    break_library(k, &system, "gbk_char.bin", how);
    obj! { "init" => !init(k, &system).is_null() }
}
