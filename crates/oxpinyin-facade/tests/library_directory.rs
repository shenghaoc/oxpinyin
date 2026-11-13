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
use std::path::{Path, PathBuf};

#[cfg(unix)]
use oxpinyin_facade::SaveOutcome;
use oxpinyin_facade::{
    ContextCore, InitRefusal, MaskOutOutcome, OpenFailure, PINYIN_DEFAULT_OPTION_WORD,
};
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
        let link = dir.join(entry.file_name());
        #[cfg(unix)]
        std::os::unix::fs::symlink(entry.path(), &link).expect("link a fixture file");
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_file(entry.path(), &link) {
            // ERROR_PRIVILEGE_NOT_HELD: a symbolic link needs the privilege
            // (Developer Mode or an elevated process); nothing is copied.
            assert_ne!(
                error.raw_os_error(),
                Some(1314),
                "link a fixture file {}: this process lacks the symbolic-link privilege",
                link.display()
            );
            panic!("link a fixture file {}: {error}", link.display());
        }
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
#[cfg(unix)]
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
    persistence::save_state_reporting(
        user,
        &persistence::UserState::default(),
        &oxpinyin_user::SystemOriginals::from(oxpinyin_user::LibraryTable::new()),
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
            Err(failure) => {
                let OpenFailure::Runtime(error) = &failure;
                if reaches {
                    // The directory's abort, not library 3's journal.
                    assert_eq!(
                        failure.init_refusal(&system),
                        InitRefusal::Abort,
                        "{law:?}: {failure}"
                    );
                    assert!(error.chunk_header_read(), "{law:?}: {failure}");
                } else {
                    assert!(!error.chunk_header_read(), "{law:?}: {failure}");
                }
            }
            Ok(_) => panic!("{law:?}: a directory library must fail the open"),
        }

        plant_header_token_log(&user, "gb_char.dbin");
        let failure = ContextCore::try_open(&system, Some(&user), PINYIN_DEFAULT_OPTION_WORD, law)
            .err()
            .expect("the open fails");
        if reaches {
            let OpenFailure::Runtime(error) = &failure;
            assert_eq!(
                failure.init_refusal(&system),
                InitRefusal::Abort,
                "{law:?}: {failure}"
            );
            assert!(
                matches!(error, oxpinyin_runtime::OpenError::Journal(_)),
                "{law:?}: {failure}"
            );
        }
        let _ = std::fs::remove_dir_all(&system);
        let _ = std::fs::remove_dir_all(&user);
    }
}

/// An open and the lines it reports: only the libraries the library loop
/// reached (`pinyin.cpp:253-256`, `zhuyin.cpp:197-200`).
fn open_lines(
    system: &Path,
    user: &Path,
    law: UserConfLaw,
) -> (
    Result<oxpinyin_runtime::Runtime, oxpinyin_runtime::OpenError>,
    Vec<PathBuf>,
) {
    let mut lines = Vec::new();
    let opened =
        oxpinyin_runtime::Runtime::open_with_law_reporting(system, Some(user), law, &mut |path| {
            lines.push(path.to_path_buf())
        });
    (opened, lines)
}

/// Replaces the system library `file` with one too short for a chunk header.
fn make_unmappable(system: &Path, file: &str) {
    let path = system.join(file);
    std::fs::remove_file(&path).expect("unlink the library");
    std::fs::write(&path, b"abc").expect("short library");
}

/// Library 1's log asserts before the loop opens library 2 or library 4, so
/// neither writes its line and the fault is the log's. With a sound log the
/// loop reaches them: library 2's line, then library 4's refusal.
#[test]
fn init_reports_only_what_the_library_loop_reached() {
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        let system = private_system("lines");
        let user = scratch("lines-user");
        conforming_profile(&system, &user);
        std::fs::remove_file(system.join("gbk_char.bin")).expect("unlink the library");
        make_unmappable(&system, "merged.bin");

        let (opened, lines) = open_lines(&system, &user, law);
        let error = opened.err().expect("the open fails");
        assert_eq!(
            error.unmappable_library(),
            Some(system.join("merged.bin").as_path()),
            "{law:?}"
        );
        assert_eq!(lines, [system.join("gbk_char.bin")], "{law:?}");

        plant_header_token_log(&user, "gb_char.dbin");
        let (opened, lines) = open_lines(&system, &user, law);
        assert!(
            matches!(opened, Err(oxpinyin_runtime::OpenError::Journal(_))),
            "{law:?}"
        );
        assert!(lines.is_empty(), "{law:?}: {lines:?}");
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
// Unix only: a directory opens as a file, so the pin's `MemoryChunk::mmap` judges it by its
// size (`directory_reads_as_a_chunk`). Windows refuses the open, and oxpinyin skips the row
// without a word there (`oxpinyin-data`, `phrase_libraries.rs`), which no pin observation
// covers.
#[cfg(unix)]
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
        context.user_store_mut().mark_modified();
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
