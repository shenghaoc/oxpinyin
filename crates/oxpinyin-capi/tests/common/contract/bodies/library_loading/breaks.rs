//! Libraries `MemoryChunk::mmap` refuses, at each site that maps one, and the
//! guards that keep the pin silent with the same broken library.

use std::path::Path;

use super::{LIBRARY, init, link, nonzero_layout_chunk, sparse_chunk, train_once};
use crate::facade;
use crate::fixtures::private_system;
use crate::support::{J, Kit};

/// `break_library`: replaces the link to library `name` with a file
/// `MemoryChunk::mmap` refuses: none (`missing`), empty (`empty`), a payload
/// whose checksum does not verify (`checksum`), the sparse 2^32+8 chunk
/// (`sparse`), the nonzero-layout chunk, or a directory (`dir-long`). The link
/// is removed first. Layout checks follow `MemoryChunk::mmap`
/// (`memory_chunk.h:479-518`) and `SubPhraseIndex::load`
/// (`phrase_index.cpp:335-362`).
pub(super) fn break_library(k: &Kit, system: &Path, name: &str, how: &str) {
    let path = system.join(name);
    std::fs::remove_file(&path).expect("unlink the library");
    let write = |bytes: &[u8]| std::fs::write(&path, bytes).expect("write the broken library");
    match how {
        "empty" => write(b""),
        "sparse" => sparse_chunk(&path),
        "nonzero-layout" => write(&nonzero_layout_chunk()),
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
/// both facades let go) is unloaded first.
pub(super) fn library_probe(k: &mut Kit, site: &str, how: &str) -> J {
    let system = private_system(k);
    if site == "init" {
        break_library(k, &system, LIBRARY, how);
        return obj! { "ctx" => !init(k, &system).is_null() };
    }
    assert_eq!(site, "load");
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let unload = facade::unload_phrase_library(ctx, 2);
    break_library(k, &system, "gbk_char.bin", how);
    obj! { "unload" => unload, "load" => facade::load_phrase_library(ctx, 2) }
}

/// `library_directory_guard`: a directory at the reserved library (index 0,
/// configured from gb_char), then a save of a trained context.
pub(super) fn directory_guard(k: &mut Kit) -> J {
    let system = private_system(k);
    let conf = system.join("table.conf");
    let mut text = std::fs::read_to_string(&conf).expect("table.conf");
    text = text.replace(
        "default RESERVED NULL NULL NULL NOT_USED",
        "default RESERVED gb_char.table reserved.bin reserved.dbin SYSTEM_FILE",
    );
    let name = "reserved.bin";
    link(&k.data.join("gb_char.bin"), &system.join(name));
    std::fs::remove_file(&conf).expect("unlink table.conf");
    std::fs::write(&conf, text).expect("write table.conf");
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "init failed");
    assert!(train_once(ctx), "train failed");
    break_library(k, &system, name, "dir-long");
    obj! { "ret" => facade::save(ctx) }
}
