use super::writes_null_chunk_line;
use std::path::Path;

// Unix only: the pin's read of a directory as a chunk (`phrase_library::directory_chunk_fault`); `writes_null_chunk_line` is false elsewhere.
#[cfg(unix)]
#[test]
fn a_directory_that_does_not_read_as_a_chunk_gets_the_pin_line_once() {
    let mut asked = 0;
    let writes = writes_null_chunk_line(Path::new("/scratch/sys-1"), |dir| {
        asked += 1;
        assert_eq!(dir, Path::new("/scratch/sys-1"));
        false
    });
    assert_eq!(asked, 1);
    assert!(writes);
}

#[test]
fn a_directory_that_reads_as_a_chunk_gets_no_line() {
    assert!(!writes_null_chunk_line(Path::new("/scratch/sys-1"), |_| {
        true
    }));
}

use super::{ContextCore, OpenFailure};
use crate::PINYIN_DEFAULT_OPTION_WORD as WORD;
use oxpinyin_runtime::OpenError;
use oxpinyin_user::UserConfLaw;

#[test]
fn missing_system_dir_carries_the_runtime_error() {
    // A child of a fresh dir this test owns: absent by construction, and
    // nothing another process does to a shared temp path can create it.
    let root = tempfile::Builder::new()
        .prefix("oxpinyin-facade-missing-")
        .tempdir()
        .expect("temp dir");
    let dir = root.path().join("system");
    let failure = ContextCore::try_open(&dir, None, WORD, UserConfLaw::Pinyin)
        .err()
        .expect("a missing system dir cannot open");
    let OpenFailure::Runtime(error) = &failure;
    // `SystemTableInfo2::load` is the first step (maintainer ruling
    // 2026-10-28 UTC, D3); its `open %s failed.` line and the init's
    // `load %s failed!` name the file.
    assert!(
        matches!(
            error,
            OpenError::TableConf(oxpinyin_runtime::TableConfError::Unopenable)
        ),
        "got {error:?}"
    );
    assert!(std::error::Error::source(&failure).is_some());
}

#[cfg(unix)]
use super::RemapStep;
use super::SaveOutcome;
#[cfg(unix)]
use oxpinyin_core::ChewingKey;
use oxpinyin_core::{PhraseText, SyllableKey};
#[cfg(unix)]
use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};
#[cfg(unix)]
use oxpinyin_data::phrase_library::PhraseLibrary;
use oxpinyin_user::toned_key;

const GB: u8 = 1;
const GBK: u8 = 2;

/// A context over the committed mini tables of this build's backend,
/// with a user directory of its own.
fn open(user: &Path) -> ContextCore {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
    let extension = oxpinyin_data::DEFAULT_STORE_EXT;
    ContextCore::open(&root.join(extension), Some(user), WORD, UserConfLaw::Pinyin)
        .expect("committed backend fixtures")
}

/// The first token of `library` and its text.
fn first_item(ctx: &ContextCore, library: u8) -> (u32, PhraseText) {
    let token = (u32::from(library) << 24) | 1;
    let dict = ctx.runtime.dict();
    let text = dict.token_text(token).expect("text");
    (token, PhraseText::from(text.as_str().expect("scalar text")))
}

/// Imports `token`'s text under a reading of `tone` the chunk does not
/// hold, and arms `m_modified` as `pinyin_end_add_phrases` does
/// (`pinyin.cpp:658`).
fn import(ctx: &mut ContextCore, library: u8, token: u32, text: &PhraseText, tone: u8) {
    let syllable = SyllableKey::from_text("a").expect("syllable").index();
    let length = text.char_count().expect("character count");
    let keys = vec![toned_key(syllable, tone).expect("key"); length];
    ctx.user_store_mut()
        .add_system_phrase_in(library, Some(token), text, &keys, Some(7))
        .expect("import");
    ctx.user_store_mut().mark_modified();
}

/// The readings the session holds for `token` beyond the chunk's own.
fn readings(ctx: &ContextCore, token: u32) -> usize {
    ctx.runtime
        .dict()
        .live_items()
        .read(token, |item| item.n_pronunciations())
        .found()
        .unwrap_or(0)
}

/// 074a2219 pinyin.cpp:1260-1294 visits each library and its log in order.
/// Row 76 refuses an unmappable library without partial masking; row 91
/// preserves the earlier journal assertion before any later mmap failure.
#[rstest::rstest]
#[case::unmappable_without_partial_mask(false)]
#[case::journal_before_later_unmappable(true)]
fn mask_out_preserves_the_first_fault_and_its_effects(#[case] journal_first: bool) {
    use super::MaskOutOutcome;
    let scratch = tempfile::tempdir().unwrap();
    let system = system_with_gbk_naming_no_user_file(scratch.path());
    let conf = system.join("table.conf");
    let text = std::fs::read_to_string(&conf).unwrap();
    std::fs::write(
        &conf,
        text.replace(
            "gbk_char.bin NULL SYSTEM_FILE",
            "gbk_char.bin gbk_char.dbin SYSTEM_FILE",
        ),
    )
    .unwrap();
    let user = scratch.path().join("user");
    std::fs::create_dir(&user).unwrap();
    let mut ctx = ContextCore::open(&system, Some(&user), WORD, UserConfLaw::Pinyin).unwrap();
    let text = PhraseText::from("你");
    let key = toned_key(SyllableKey::from_text("ni").unwrap().index(), 3).unwrap();
    let token = ctx
        .user_store_mut()
        .add_phrase(&text, &[key], Some(7))
        .unwrap();
    if journal_first {
        let mut payload = 4_u32.to_le_bytes().to_vec();
        payload.extend_from_slice(&0x0100_0001_u32.to_le_bytes());
        payload.extend_from_slice(&[4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        std::fs::write(
            user.join("gb_char.dbin"),
            oxpinyin_data::chunk_format::build_memory_chunk(&payload).unwrap(),
        )
        .unwrap();
    }
    // The remap refuses merged.bin through its checksum word, as it would
    // a missing file; Windows deletes or renames no mapped file.
    {
        use std::io::{Read, Seek, SeekFrom, Write};
        let mut merged = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(system.join("merged.bin"))
            .unwrap();
        let mut checksum = [0_u8; 4];
        merged.seek(SeekFrom::Start(4)).unwrap();
        merged.read_exact(&mut checksum).unwrap();
        merged.seek(SeekFrom::Start(4)).unwrap();
        merged.write_all(&checksum.map(|byte| !byte)).unwrap();
    }
    let result = ctx.mask_out(0, 0);
    if journal_first {
        assert!(
            matches!(
                result,
                MaskOutOutcome::Journal(oxpinyin_user::JournalFault::HeaderToken {
                    library: 1,
                    ..
                })
            ),
            "{result:?}"
        );
    } else {
        assert_eq!(result, MaskOutOutcome::Done(true));
        assert_eq!(
            ctx.user_store().lookup().tokens_for_ucs4(&[0x4f60]),
            [token]
        );
    }
}

#[test]
fn unload_then_save_leaves_the_unloaded_librarys_log_alone() {
    let user = tempfile::tempdir().expect("user dir");
    let mut ctx = open(user.path());
    let (gb, gb_text) = first_item(&ctx, GB);
    let (gbk, gbk_text) = first_item(&ctx, GBK);
    import(&mut ctx, GB, gb, &gb_text, 1);
    import(&mut ctx, GBK, gbk, &gbk_text, 1);
    assert_eq!(ctx.save_user(), SaveOutcome::Saved);
    let gb_log = user.path().join("gb_char.dbin");
    let gbk_log = user.path().join("gbk_char.dbin");
    let saved_gb = std::fs::read(&gb_log).expect("gb log");
    let saved_gbk = std::fs::read(&gbk_log).expect("gbk log");
    // Unsaved changes to both libraries, then GBK goes.
    import(&mut ctx, GB, gb, &gb_text, 2);
    import(&mut ctx, GBK, gbk, &gbk_text, 2);
    let with_unsaved = readings(&ctx, gbk);
    assert!(ctx.unload_phrase_library(GBK));
    assert_eq!(readings(&ctx, gbk), 0, "unload drops the sub-index");
    assert_eq!(ctx.save_user(), SaveOutcome::Saved);
    assert_eq!(
        std::fs::read(&gbk_log).expect("gbk log"),
        saved_gbk,
        "the save skips the unloaded library (`pinyin.cpp:928-933`)"
    );
    assert_ne!(
        std::fs::read(&gb_log).expect("gb log"),
        saved_gb,
        "the loaded library is saved"
    );
    // The reload merges the saved log: the unsaved reading is gone.
    assert_eq!(ctx.load_phrase_library(u32::from(GBK)), Ok(true));
    assert_eq!(readings(&ctx, gbk), with_unsaved - 1);
    assert_eq!(ctx.load_phrase_library(u32::from(GBK)), Ok(false));
}

#[test]
fn unload_then_reload_restores_the_saved_library() {
    let user = tempfile::tempdir().expect("user dir");
    let mut ctx = open(user.path());
    let (gbk, text) = first_item(&ctx, GBK);
    import(&mut ctx, GBK, gbk, &text, 1);
    assert_eq!(ctx.save_user(), SaveOutcome::Saved);
    let saved = readings(&ctx, gbk);
    assert!(saved > 0);
    for _ in 0..2 {
        assert!(ctx.unload_phrase_library(GBK));
        assert!(!ctx.runtime.dict().library_visible(u32::from(GBK)));
        assert_eq!(ctx.load_phrase_library(u32::from(GBK)), Ok(true));
        assert_eq!(readings(&ctx, gbk), saved);
    }
}

#[test]
fn reload_after_an_unreadable_log_loads_the_system_chunk_alone() {
    let user = tempfile::tempdir().expect("user dir");
    let mut ctx = open(user.path());
    let (gbk, text) = first_item(&ctx, GBK);
    let original_readings = readings(&ctx, gbk);
    import(&mut ctx, GBK, gbk, &text, 1);
    assert_eq!(ctx.save_user(), SaveOutcome::Saved);
    assert!(readings(&ctx, gbk) > original_readings);
    let log = user.path().join("gbk_char.dbin");
    for unreadable in [None, Some(&b"abc"[..])] {
        assert!(ctx.unload_phrase_library(GBK));
        // `MemoryChunk::load` fails on a missing file and on one shorter
        // than its header (`memory_chunk.h:416-429`): an empty log.
        match unreadable {
            None => std::fs::remove_file(&log).expect("remove log"),
            Some(bytes) => std::fs::write(&log, bytes).expect("short log"),
        }
        assert_eq!(ctx.load_phrase_library(u32::from(GBK)), Ok(true));
        assert_eq!(
            readings(&ctx, gbk),
            original_readings,
            "the system chunk alone"
        );
    }
}

/// The committed mini tables, copied, with the GBK row naming no user
/// file: `default GBK_DICTIONARY gbk_char.table gbk_char.bin NULL
/// SYSTEM_FILE`.
fn system_with_gbk_naming_no_user_file(scratch: &Path) -> std::path::PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
    let committed = root.join(oxpinyin_data::DEFAULT_STORE_EXT);
    let system = scratch.join("system");
    std::fs::create_dir_all(&system).expect("system dir");
    for entry in std::fs::read_dir(committed).expect("committed fixtures") {
        let entry = entry.expect("fixture entry");
        if entry.file_type().expect("file type").is_file() {
            std::fs::copy(entry.path(), system.join(entry.file_name()))
                .expect("copy a fixture file");
        }
    }
    let conf = system.join("table.conf");
    let text = std::fs::read_to_string(&conf).expect("table.conf");
    let row = "gbk_char.bin gbk_char.dbin SYSTEM_FILE";
    assert!(text.contains(row), "the committed GBK row");
    std::fs::write(&conf, text.replace(row, "gbk_char.bin NULL SYSTEM_FILE"))
        .expect("write table.conf");
    system
}

/// `_load_phrase_library` maps a `SYSTEM_FILE` row's chunk whether or not
/// the row names a user file (`pinyin.cpp:246-264`); only the mask-out
/// loop skips such a row (`:1250-1251`). The mask clears for a library
/// the remap mapped, and only for it. A file the remap cannot map is the
/// pin's SIGSEGV (row 76): the library stays unloaded, the call answers
/// `true`, and the next load maps the repaired file. The context has no
/// user directory: with one, the pin reads the directory itself as the
/// row's log, and on a file system where that asserts the init dies.
// Unix only: the mapped system file is removed and replaced under the
// open context; Windows keeps a mapped file in use
// (compatibility-policy.md, "fundamental OS differences").
#[cfg(unix)]
#[test]
fn a_reload_maps_a_system_row_that_names_no_user_file() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = system_with_gbk_naming_no_user_file(scratch.path());
    let mut ctx = ContextCore::open(&system, None, WORD, UserConfLaw::Pinyin).expect("opens");
    let visible = |ctx: &ContextCore| ctx.runtime.dict().library_visible(u32::from(GBK));
    // The total of the chunk the library is mapped from: another file's
    // mapping changes it.
    let mapped_total = |ctx: &ContextCore| {
        ctx.runtime
            .dict()
            .system()
            .libraries()
            .library(u32::from(GBK) << 24)
            .map(PhraseLibrary::total_freq)
    };
    let shipped = mapped_total(&ctx).expect("the GBK library is mapped");
    assert_ne!(shipped, 7);

    // The mask-out loop still skips the row, and maps the others.
    let runtime = &ctx.runtime;
    let mut walk = runtime.begin_system_remap();
    let mut mask_loop = Vec::new();
    while let Some(step) = runtime.remap_next(&mut walk) {
        if let RemapStep::Mapped(index) = step {
            mask_loop.push(index);
        }
    }
    assert!(mask_loop.contains(&GB) && !mask_loop.contains(&GBK));

    assert!(ctx.unload_phrase_library(GBK));
    assert!(!visible(&ctx));

    let chunk = system.join("gbk_char.bin");
    std::fs::remove_file(&chunk).expect("remove the system file");
    assert_eq!(ctx.load_phrase_library(u32::from(GBK)), Ok(true));
    assert!(!visible(&ctx), "the remap mapped nothing, so nothing shows");
    assert_eq!(mapped_total(&ctx), Some(shipped));

    let item = ChunkItem {
        phrase: vec![0x4f60],
        unigram: 7,
        prons: vec![(
            vec![ChewingKey::from_pinyin("ni").expect("key").to_packed()],
            7,
        )],
    };
    std::fs::write(&chunk, build_chunk(&[(1, item)]).expect("chunk")).expect("repair");
    assert_eq!(ctx.load_phrase_library(u32::from(GBK)), Ok(true));
    assert!(visible(&ctx));
    assert_eq!(mapped_total(&ctx), Some(7), "the repaired chunk was mapped");
}

mod failed_attachment_fini {
    //! Context finalization closes the system attachments, open or failed, while
    //! an instance still holds the runtime (compatibility row 42): the pin's
    //! `pinyin_fini` deletes the tables' objects (`pinyin.cpp:1205-1215`) and a
    //! retained instance keeps only the context's in-memory state.

    use std::path::PathBuf;

    use crate::{ContextCore, PINYIN_DEFAULT_OPTION_WORD};

    use oxpinyin_user::UserConfLaw;

    fn fixture_dir() -> PathBuf {
        let extension = oxpinyin_data::DEFAULT_STORE_EXT;
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("w3")
            .join(extension)
    }

    #[test]
    fn context_finish_finalizes_failed_attachments_while_an_instance_holds_the_runtime() {
        let dir = tempfile::Builder::new()
            .prefix("oxpinyin-facade-missing-index-")
            .tempdir()
            .expect("temp dir");
        for entry in std::fs::read_dir(fixture_dir()).expect("w3 fixture") {
            let entry = entry.expect("w3 entry");
            // Everything but the main pinyin index (`SystemDbm::PinyinIndex`).
            if entry.file_name().as_os_str() != std::ffi::OsStr::new("pinyin_index.bin") {
                std::fs::copy(entry.path(), dir.path().join(entry.file_name())).expect("copy");
            }
        }
        let mut context = ContextCore::open(
            dir.path(),
            None,
            PINYIN_DEFAULT_OPTION_WORD,
            UserConfLaw::Pinyin,
        )
        .expect("the pin ignores a failed system attach");
        let instance = context.alloc_instance();
        {
            let runtime = &context.runtime;
            assert!(runtime.pinyin_attachment_failed());
            assert!(!runtime.attachments_finished());
        }
        // Public context fini, with the instance still alive.
        context.finish();
        assert!(context.runtime.attachments_finished());
        // A second fini is inert.
        context.finish();
        assert!(context.runtime.attachments_finished());
        drop(instance);
        drop(context);
    }
}

mod library_directory {
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
    use crate::SaveOutcome;

    use crate::{
        ContextCore, InitRefusal, MaskOutOutcome, OpenFailure, PINYIN_DEFAULT_OPTION_WORD,
    };

    use oxpinyin_user::{SystemVersions, UserConfLaw};

    /// The committed fixture set the compiled backend opens.
    fn fixture_dir() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
        let extension = oxpinyin_data::DEFAULT_STORE_EXT;
        root.join(extension)
    }

    /// A fresh scratch directory for `name` under the temporary directory.
    ///
    /// The directory is a [`tempfile::TempDir`] that this thread keeps until
    /// the test ends: a private system directory outlives the helper that
    /// makes it.
    fn scratch(name: &str) -> PathBuf {
        thread_local! {
            static SCRATCH: std::cell::RefCell<Vec<tempfile::TempDir>> =
                const { std::cell::RefCell::new(Vec::new()) };
        }
        let dir = tempfile::Builder::new()
            .prefix(&format!("oxpinyin-facade-libdir-{name}-"))
            .tempdir()
            .expect("scratch dir");
        let path = dir.path().to_path_buf();
        SCRATCH.with_borrow_mut(|dirs| dirs.push(dir));
        path
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

    /// A conforming profile for `system` in `user`: the marker a save writes,
    /// so neither law's `check_format` wipes it.
    fn conforming_profile(system: &Path, user: &Path) {
        let conf = oxpinyin_runtime::TableConf::load_from_dir(system).expect("fixture table.conf");
        let mut marker =
            oxpinyin_data::user_files::UserTableInfo::conform_to(&SystemVersions::from_conf(&conf));
        marker.open_counter = 1;
        std::fs::write(user.join("user.conf"), marker.to_text()).expect("write the marker");
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
            let failure =
                ContextCore::try_open(&system, Some(&user), PINYIN_DEFAULT_OPTION_WORD, law)
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
        let opened = oxpinyin_runtime::Runtime::open_with_law_reporting(
            system,
            Some(user),
            law,
            &mut |path| lines.push(path.to_path_buf()),
        );
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
                ContextCore::open(&system, Some(&user), PINYIN_DEFAULT_OPTION_WORD, law)
                    .expect("open");
            context.user_store_mut().mark_modified();
            let before = listing(&user);
            make_directory(&system, "merged.bin");
            let outcome = context.save_user();
            if reaches {
                assert_eq!(outcome, SaveOutcome::ChunkHeaderRead, "{law:?}");
                let mut want: Vec<String> =
                    expected.iter().map(|name| (*name).to_owned()).collect();
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
                ContextCore::open(&system, Some(&user), PINYIN_DEFAULT_OPTION_WORD, law)
                    .expect("open");
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
}

mod library_totals {
    //! The facade total that the model divides by, across the library
    //! transitions the context owns: `mask_out`, `unload_phrase_library` and
    //! the import iterator's close step (`end_import`).

    use std::path::{Path, PathBuf};

    use crate::{ContextCore, MaskOutOutcome, PINYIN_DEFAULT_OPTION_WORD};

    use oxpinyin_user::UserConfLaw;

    const GBK: u8 = 2;

    /// The committed fixture set the compiled backend opens.
    fn fixture_dir() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
        root.join(oxpinyin_data::DEFAULT_STORE_EXT)
    }

    /// A private system directory under `parent`: every fixture file copied in,
    /// so the test needs no symbolic-link privilege on any host.
    fn private_system(parent: &Path) -> PathBuf {
        let dir = parent.join("system");
        std::fs::create_dir_all(&dir).expect("private system dir");
        for entry in std::fs::read_dir(fixture_dir()).expect("fixture dir") {
            let entry = entry.expect("fixture entry");
            std::fs::copy(entry.path(), dir.join(entry.file_name())).expect("copy a fixture file");
        }
        dir
    }

    fn open(system: &Path, user: &Path) -> ContextCore {
        ContextCore::open(
            system,
            Some(user),
            PINYIN_DEFAULT_OPTION_WORD,
            UserConfLaw::Pinyin,
        )
        .expect("open")
    }

    fn facade_total(context: &ContextCore) -> u64 {
        context.runtime.lm().amplified_total()
    }

    /// A context on a private system under `scratch` whose `gbk_char.bin`
    /// (library 2) is a checksum-valid, in-bounds chunk that
    /// `SubPhraseIndex::load` refuses at its second separator
    /// (`phrase_index.cpp:355`). Its local total is T, the facade total of the
    /// other libraries, which the facade does not add (`:242-246`). Answers the
    /// context and T.
    fn layout_failed_gbk_context(scratch: &Path) -> (ContextCore, u64) {
        let system = private_system(scratch);
        let user = scratch.join("user");
        std::fs::create_dir(&user).expect("user directory");
        let library = system.join("gbk_char.bin");
        let write = |total: u32| {
            let mut payload = [0_u8; 32];
            for (at, value) in [(0, total), (4, 17), (8, 20), (12, 24)] {
                payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            payload[16] = b'#';
            payload[19] = b'!';
            let chunk = oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame");
            std::fs::write(&library, chunk).expect("write gbk_char.bin");
        };
        write(0);
        let total = facade_total(&open(&system, &user));
        assert!(total > 0, "the other libraries hold a total");
        write(u32::try_from(total).expect("a guint32 total"));
        let context = open(&system, &user);
        assert_eq!(
            facade_total(&context),
            total,
            "the refused library adds nothing"
        );
        (context, total)
    }

    /// `mask_out` maps library 2 again: the failed load takes the old local
    /// total T out of the facade total (`phrase_index.cpp:243-245`), and the
    /// closing compact makes the reread local total zero (`pinyin.cpp:1294`).
    /// The unload then takes out nothing, and the facade total stays T - T.
    #[test]
    fn an_unload_after_mask_out_takes_out_the_compacted_total() {
        let scratch = tempfile::tempdir().expect("scratch");
        let (mut context, _) = layout_failed_gbk_context(scratch.path());
        assert_eq!(context.mask_out(0xFFFF_FFFF, 0), MaskOutOutcome::Done(true));
        assert!(context.unload_phrase_library(GBK));
        assert_eq!(facade_total(&context), 0);
    }

    /// `pinyin_end_add_phrases` compacts the phrase index
    /// (`pinyin.cpp:655-660`), and `compact` skips a missing sub-index
    /// (`phrase_index.cpp:651-652`): the unloaded library's total does not move
    /// into the facade's.
    #[test]
    fn closing_an_import_leaves_the_unloaded_librarys_total_alone() {
        let scratch = tempfile::tempdir().expect("scratch");
        let system = private_system(scratch.path());
        let user = scratch.path().join("user");
        std::fs::create_dir(&user).expect("user directory");
        let mut context = open(&system, &user);
        assert!(context.unload_phrase_library(GBK));
        let total = facade_total(&context);

        context.end_import();

        assert_eq!(facade_total(&context), total);
    }
}
