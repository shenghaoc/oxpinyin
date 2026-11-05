//! Register row 96: a system library that is a directory whose reported size
//! passes `MemoryChunk::mmap`'s header check, so the pin's header `read`
//! fails and `assert(ret_len == sizeof(length))` aborts it
//! (`memory_chunk.h:493` at 074a2219). The facade returns the fault at the
//! point the pin's library loop maps that library, after the pin's preceding
//! work; the C adapters abort on it.
//!
//! The size is the file system's: on one where a directory does not read as a
//! chunk (`directory_reads_as_a_chunk`), the same directory is row 76's
//! unmappable library (class (b)), and each test asserts that outcome
//! instead.
#![cfg(unix)]

use std::path::{Path, PathBuf};

use oxpinyin_facade::{ContextCore, MaskOutOutcome, PINYIN_DEFAULT_OPTION_WORD, SaveOutcome};
use oxpinyin_user::{SystemVersions, UserConfLaw, persistence};

/// The committed fixture set the compiled backend opens.
fn fixture_dir() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
    let extension = oxpinyin_data::DEFAULT_STORE_EXT;
    root.join(extension)
}

/// A fresh scratch directory for `name` under the temporary directory.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-facade-libdir-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A private system directory: every fixture file linked in.
fn private_system(name: &str) -> PathBuf {
    let fixture = fixture_dir();
    let dir = scratch(&format!("{name}-system"));
    for entry in std::fs::read_dir(&fixture).expect("fixture dir") {
        let entry = entry.expect("fixture entry");
        std::os::unix::fs::symlink(entry.path(), dir.join(entry.file_name()))
            .expect("link a fixture file");
    }
    dir
}

/// Replaces the system library `file` with a non-empty directory whose entry
/// name is 25 bytes long (the contract cases' `dir-long` shape).
fn make_directory(system: &Path, file: &str) {
    let path = system.join(file);
    std::fs::remove_file(&path).expect("unlink the library");
    std::fs::create_dir(&path).expect("directory library");
    std::fs::write(path.join("e".repeat(25)), b"x").expect("fill the directory");
}

/// Whether a directory in `system`'s place reads as a chunk here.
fn reaches_the_assert(system: &Path) -> bool {
    oxpinyin_runtime::directory_reads_as_a_chunk(system)
}

/// The user directory's file names, sorted.
fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("user dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// A `.dbin` holding one `MODIFY_HEADER` with a token: the replay's
/// `next_record` asserts on it (`phrase_index_logger.h:202`, register row
/// 91). Framed as `MemoryChunk::save` frames it: the payload length and the
/// word-XOR checksum, little-endian.
fn plant_header_token_log(user: &Path, file: &str) {
    let mut payload = 4_u32.to_le_bytes().to_vec();
    payload.extend_from_slice(&0x0100_0001_u32.to_le_bytes());
    payload.extend_from_slice(&[4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut checksum = 0_u32;
    for (index, &byte) in payload.iter().enumerate() {
        checksum ^= u32::from(byte) << (8 * (index % 4));
    }
    let mut chunk = u32::try_from(payload.len())
        .expect("short payload")
        .to_le_bytes()
        .to_vec();
    chunk.extend_from_slice(&checksum.to_le_bytes());
    chunk.extend_from_slice(&payload);
    std::fs::write(user.join(file), chunk).expect("write the log");
}

/// A conforming profile for `system` in `user`, written as a save writes it
/// (the marker included), so neither law's `check_format` wipes it.
fn conforming_profile(system: &Path, user: &Path) {
    let conf = oxpinyin_runtime::TableConf::load_from_dir(system).expect("fixture table.conf");
    persistence::save(
        user,
        &persistence::UserState::default(),
        &oxpinyin_user::SystemOriginals::from(std::collections::BTreeMap::new()),
        &SystemVersions::from_conf(&conf),
        1,
    )
    .expect("write the profile");
}

/// Init's library loop maps each default library and merges its log in
/// order (`pinyin.cpp:377-392`): a directory at library 4 stops the open
/// there, after library 1's log merged, so library 1's bad log asserts
/// first (row 91); a directory at library 2 stops it before library 3's bad
/// log is read.
#[test]
fn init_returns_the_fault_where_its_library_loop_maps_the_directory() {
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        let system = private_system("init");
        let user = scratch("init-user");
        conforming_profile(&system, &user);
        let reaches = reaches_the_assert(&system);

        make_directory(&system, "gbk_char.bin");
        plant_header_token_log(&user, "opengram.dbin");
        match ContextCore::try_open(&system, Some(&user), PINYIN_DEFAULT_OPTION_WORD, law) {
            Err(failure) if reaches => {
                assert!(failure.chunk_header_read(), "{law:?}: {failure}");
                assert!(failure.journal_fault().is_none(), "{law:?}");
            }
            Err(failure) => assert!(!failure.chunk_header_read(), "{law:?}: {failure}"),
            Ok(_) => panic!("{law:?}: a directory library must fail the open"),
        }

        plant_header_token_log(&user, "gb_char.dbin");
        let failure = ContextCore::try_open(&system, Some(&user), PINYIN_DEFAULT_OPTION_WORD, law)
            .err()
            .expect("the open fails");
        if reaches {
            assert!(failure.journal_fault().is_some(), "{law:?}: {failure}");
        }
        let _ = std::fs::remove_dir_all(&system);
        let _ = std::fs::remove_dir_all(&user);
    }
}

/// `_write_files` maps each loaded system library at its place in the
/// library loop (`pinyin.cpp:955-956`), so at the directory in library 4
/// libpinyin has written the `.tmp` logs of libraries 1 to 3 and renamed
/// nothing (`pinyin.cpp:940-1128`); libzhuyin renames each library's file as
/// it writes it (`zhuyin.cpp:554-617`), so libraries 1 to 3 are in place.
/// Nothing else is written, and the store's clean-up does not run.
#[test]
fn save_stops_at_the_directory_with_the_pins_files() {
    for (law, expected) in [
        (
            UserConfLaw::Pinyin,
            ["gb_char.dbin.tmp", "gbk_char.dbin.tmp", "opengram.dbin.tmp"],
        ),
        (
            UserConfLaw::Zhuyin,
            ["gb_char.dbin", "gbk_char.dbin", "opengram.dbin"],
        ),
    ] {
        let system = private_system("save");
        let user = scratch("save-user");
        conforming_profile(&system, &user);
        // Without these logs on disk, a rename shows up in the listing.
        for log in ["gb_char.dbin", "gbk_char.dbin", "opengram.dbin"] {
            let _ = std::fs::remove_file(user.join(log));
        }
        let reaches = reaches_the_assert(&system);
        let mut context =
            ContextCore::open(&system, Some(&user), PINYIN_DEFAULT_OPTION_WORD, law).expect("open");
        context
            .user_store_mut()
            .expect("user store")
            .mark_modified();
        let before = listing(&user);
        make_directory(&system, "merged.bin");
        let outcome = context.save_user();
        if reaches {
            assert_eq!(outcome, SaveOutcome::ChunkHeaderRead, "{law:?}");
            let mut want: Vec<String> = expected.iter().map(|name| (*name).to_owned()).collect();
            want.extend(before.iter().cloned());
            want.sort();
            want.dedup();
            assert_eq!(listing(&user), want, "{law:?}");
        } else {
            assert_eq!(outcome, SaveOutcome::Saved, "{law:?}");
        }
        drop(context);
        let _ = std::fs::remove_dir_all(&system);
        let _ = std::fs::remove_dir_all(&user);
    }
}

/// `pinyin_mask_out` masks the user tables, then maps each loaded system
/// library again before it merges its log (`pinyin.cpp:1228-1291`): a
/// directory library is the fault there.
#[test]
fn mask_out_returns_the_fault_at_the_library_reload() {
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        let system = private_system("mask");
        let user = scratch("mask-user");
        let reaches = reaches_the_assert(&system);
        let mut context =
            ContextCore::open(&system, Some(&user), PINYIN_DEFAULT_OPTION_WORD, law).expect("open");
        make_directory(&system, "merged.bin");
        let outcome = context.mask_out(0xff00_0000, 0x0400_0000);
        if reaches {
            assert_eq!(outcome, MaskOutOutcome::ChunkHeaderRead, "{law:?}");
        } else {
            assert_eq!(outcome, MaskOutOutcome::Done(true), "{law:?}");
        }
        drop(context);
        let _ = std::fs::remove_dir_all(&system);
        let _ = std::fs::remove_dir_all(&user);
    }
}
