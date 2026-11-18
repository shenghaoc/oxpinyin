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
use crate::facade::PINYIN_DEFAULT_OPTION_WORD as WORD;
use crate::runtime::OpenError;
use crate::user::UserConfLaw;

#[test]
fn missing_system_dir_carries_the_runtime_error() {
    // A child of a fresh dir this test owns: absent by construction, and
    // nothing another process does to a shared temp path can create it.
    let root = tempfile::Builder::new()
        .prefix("oxpinyin-missing-")
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
            OpenError::TableConf(crate::runtime::TableConfError::Unopenable)
        ),
        "got {error:?}"
    );
    assert!(std::error::Error::source(&failure).is_some());
}

#[cfg(unix)]
use super::RemapStep;
use super::SaveOutcome;
#[cfg(unix)]
use crate::ChewingKey;
#[cfg(unix)]
use crate::data::chunk_write::{ChunkItem, build_chunk};
#[cfg(unix)]
use crate::data::phrase_library::PhraseLibrary;
use crate::user::toned_key;
use crate::{PhraseText, SyllableKey};

const GB: u8 = 1;
const GBK: u8 = 2;

/// A context over the committed mini tables of this build's backend,
/// with a user directory of its own.
fn open(user: &Path) -> ContextCore {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/w3");
    let extension = crate::data::DEFAULT_STORE_EXT;
    ContextCore::open(&root.join(extension), Some(user), WORD, UserConfLaw::Pinyin)
        .expect("committed backend fixtures")
}

/// The first token of `library` and its text.
fn first_item(ctx: &ContextCore, library: u8) -> (u32, PhraseText) {
    let token = crate::phrase_index_make_token(library, 1);
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
            crate::data::chunk_format::build_memory_chunk(&payload).unwrap(),
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
                MaskOutOutcome::Journal(crate::user::JournalFault::HeaderToken { library: 1, .. })
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/w3");
    let committed = root.join(crate::data::DEFAULT_STORE_EXT);
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

    use crate::facade::{ContextCore, PINYIN_DEFAULT_OPTION_WORD};

    use crate::user::UserConfLaw;

    fn fixture_dir() -> PathBuf {
        let extension = crate::data::DEFAULT_STORE_EXT;
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("w3")
            .join(extension)
    }

    #[test]
    fn context_finish_finalizes_failed_attachments_while_an_instance_holds_the_runtime() {
        let dir = tempfile::Builder::new()
            .prefix("oxpinyin-missing-index-")
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
    use crate::facade::SaveOutcome;

    use crate::facade::{
        ContextCore, InitRefusal, MaskOutOutcome, OpenFailure, PINYIN_DEFAULT_OPTION_WORD,
    };

    use crate::{data::user_files::SystemVersions, user::UserConfLaw};

    /// The committed fixture set the compiled backend opens.
    fn fixture_dir() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/w3");
        let extension = crate::data::DEFAULT_STORE_EXT;
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
            .prefix(&format!("oxpinyin-libdir-{name}-"))
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
        crate::runtime::directory_reads_as_a_chunk(system)
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
        let conf = crate::runtime::TableConf::load_from_dir(system).expect("fixture table.conf");
        let mut marker =
            crate::data::user_files::UserTableInfo::conform_to(&SystemVersions::from_conf(&conf));
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
                    matches!(error, crate::runtime::OpenError::Journal(_)),
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
        Result<crate::runtime::Runtime, crate::runtime::OpenError>,
        Vec<PathBuf>,
    ) {
        let mut lines = Vec::new();
        let opened = crate::runtime::Runtime::open_with_law_reporting(
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
                matches!(opened, Err(crate::runtime::OpenError::Journal(_))),
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
    // without a word there (`oxpinyin`, `phrase_libraries.rs`), which no pin observation
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

    use crate::facade::{ContextCore, MaskOutOutcome, PINYIN_DEFAULT_OPTION_WORD};

    use crate::user::UserConfLaw;

    const GBK: u8 = 2;

    /// The committed fixture set the compiled backend opens.
    fn fixture_dir() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/w3");
        root.join(crate::data::DEFAULT_STORE_EXT)
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
            let chunk = crate::data::chunk_format::build_memory_chunk(&payload).expect("frame");
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

mod native {
    use crate::OptionBits;
    use crate::facade::test_fixture::{native_fixture::*, *};
    use crate::facade::{ContextCore, ImportPlan, MaskOutOutcome, SaveOutcome};
    use crate::user::UserConfLaw;
    use std::path::Path;

    #[test]
    fn init_reports_an_absent_library_and_succeeds() {
        let Some((_scratch, user, run)) = child(
            "facade::context::tests::native::init_reports_an_absent_library_and_succeeds",
            |user| {
                let system = private_system(user.parent().unwrap());
                std::fs::remove_file(system.join("merged.bin")).unwrap();
                native_context(&system, user, UserConfLaw::Pinyin);
            },
        ) else {
            return;
        };
        let line = format!(
            "mmap {} failed!{}",
            user.parent()
                .unwrap()
                .join("system")
                .join("merged.bin")
                .display(),
            crate::input::TEXT_NEWLINE
        );
        assert!(String::from_utf8_lossy(&run.stderr).contains(&line));
    }

    #[test]
    fn reload_of_an_empty_library_reports_and_answers_true() {
        let Some((_scratch, user, run)) = child(
            "facade::context::tests::native::reload_of_an_empty_library_reports_and_answers_true",
            |user| {
                let system = private_system(user.parent().unwrap());
                let mut context = native_context(&system, user, UserConfLaw::Pinyin);
                assert!(context.unload_phrase_library(2));
                let library = system.join("gbk_char.bin");
                std::fs::remove_file(&library).unwrap();
                std::fs::write(&library, b"").unwrap();
                assert_eq!(context.load_phrase_library(2), Ok(true));
            },
        ) else {
            return;
        };
        let line = format!(
            "mmap {} failed!{}",
            user.parent()
                .unwrap()
                .join("system")
                .join("gbk_char.bin")
                .display(),
            crate::input::TEXT_NEWLINE
        );
        assert!(String::from_utf8_lossy(&run.stderr).contains(&line));
    }

    #[test]
    fn save_reports_an_absent_system_library_and_answers_false() {
        let Some((_scratch, user, run)) = child(
            "facade::context::tests::native::save_reports_an_absent_system_library_and_answers_false",
            |user| {
                let system = private_system(user.parent().unwrap());
                let mut context = native_context(&system, user, UserConfLaw::Pinyin);
                train_once(&mut context);
                std::fs::remove_file(system.join("gb_char.bin")).unwrap();
                assert_eq!(context.save_user(), SaveOutcome::NotSaved);
            },
        ) else {
            return;
        };
        let line = format!(
            "mmap {} failed!{}",
            user.parent()
                .unwrap()
                .join("system")
                .join("gb_char.bin")
                .display(),
            crate::input::TEXT_NEWLINE
        );
        assert!(String::from_utf8_lossy(&run.stderr).contains(&line));
    }

    #[cfg(any(feature = "tkrzw", feature = "bdb"))]
    fn copy_failure(user: &Path) {
        let before = user.parent().unwrap().join("before");
        std::fs::create_dir(&before).unwrap();
        for (name, bytes) in files_of(user) {
            std::fs::write(before.join(name), bytes).unwrap();
        }
        crate::store::copy_faults::fail_source_read_after(1);
        let error = ContextCore::try_open(
            &system_dir(),
            Some(user),
            crate::facade::PINYIN_DEFAULT_OPTION_WORD,
            UserConfLaw::Pinyin,
        )
        .err()
        .expect("copy fails");
        assert!(
            matches!(
                error,
                crate::facade::OpenFailure::Runtime(crate::runtime::OpenError::UserSession(
                    crate::user::UserStoreError::FatalCopy
                ))
            ),
            "{error:?}"
        );
    }

    #[cfg(any(feature = "tkrzw", feature = "bdb"))]
    #[test]
    fn init_returns_the_source_copy_failure_after_writing_the_open_counter() {
        let Some((_scratch, user, _run)) = child(
            "facade::context::tests::native::init_returns_the_source_copy_failure_after_writing_the_open_counter",
            |user| {
                plant_bigram(
                    user,
                    vec![
                        (0x0100_0001_u32.to_le_bytes().to_vec(), vec![0; 4]),
                        (0x0100_0002_u32.to_le_bytes().to_vec(), vec![0; 4]),
                    ],
                );
                copy_failure(user);
            },
        ) else {
            return;
        };
        let before = files_of(&user.parent().unwrap().join("before"));
        let after = files_of(&user);
        let conf = String::from_utf8_lossy(&after["user.conf"]);
        assert!(
            conf.ends_with(&format!("open counter:2{}", crate::input::TEXT_NEWLINE)),
            "{conf}"
        );
        assert_eq!(after["user_bigram.db"], before["user_bigram.db"]);
    }

    #[cfg(any(feature = "tkrzw", feature = "bdb"))]
    #[test]
    fn init_returns_the_user_index_copy_failure_without_changing_the_file() {
        let Some((_scratch, user, _run)) = child(
            "facade::context::tests::native::init_returns_the_user_index_copy_failure_without_changing_the_file",
            |user| {
                set_phrase_table_row(user, "你好", &[0x0100_0001]);
                set_phrase_table_row(user, "再见", &[0x0100_0002]);
                copy_failure(user);
            },
        ) else {
            return;
        };
        assert_eq!(
            files_of(&user)["user_phrase_index.bin"],
            files_of(&user.parent().unwrap().join("before"))["user_phrase_index.bin"]
        );
    }

    /// The same failed second separator as phrase_index.cpp:355; its local
    /// total T is not added at load, but an unload subtracts it (:261-269).
    fn layout_failed_gbk_context(scratch: &Path) -> (ContextCore, u64) {
        let system = private_system(scratch);
        let user = scratch.join("user");
        std::fs::create_dir(&user).unwrap();
        let library = system.join("gbk_char.bin");
        let write = |total: u32| {
            let mut payload = [0_u8; 32];
            for (at, value) in [(0, total), (4, 17), (8, 20), (12, 24)] {
                payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            payload[16] = b'#';
            payload[19] = b'!';
            std::fs::remove_file(&library).unwrap();
            std::fs::write(
                &library,
                crate::data::chunk_format::build_memory_chunk(&payload).unwrap(),
            )
            .unwrap();
        };
        write(0);
        let mut context = native_context(&system, &user, UserConfLaw::Pinyin);
        let total = context.runtime.lm().amplified_total();
        context.finish();
        drop(context);
        assert!(total > 0);
        write(u32::try_from(total).unwrap());
        let context = native_context(&system, &user, UserConfLaw::Pinyin);
        assert_eq!(context.runtime.lm().amplified_total(), total);
        (context, total)
    }

    #[test]
    fn an_unload_inside_an_open_import_zeroes_the_facade_total() {
        let scratch = tempfile::tempdir().unwrap();
        let (mut context, _) = layout_failed_gbk_context(scratch.path());
        assert!(context.unload_phrase_library(2));
        assert_eq!(context.runtime.lm().amplified_total(), 0);
    }

    #[test]
    fn an_import_that_ends_after_the_unload_keeps_the_zero_total() {
        let scratch = tempfile::tempdir().unwrap();
        let (mut context, _) = layout_failed_gbk_context(scratch.path());
        assert!(context.unload_phrase_library(2));
        context.end_import();
        assert_eq!(context.runtime.lm().amplified_total(), 0);
    }

    #[test]
    fn an_unload_after_the_import_ends_keeps_the_facade_total() {
        let scratch = tempfile::tempdir().unwrap();
        let (mut context, total) = layout_failed_gbk_context(scratch.path());
        context.end_import();
        assert!(context.unload_phrase_library(2));
        assert_eq!(context.runtime.lm().amplified_total(), total);
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
        assert!(!native_window(&context, &mut instance, 0).is_empty());
    }

    #[test]
    fn an_increment_after_an_import_into_a_failed_layout_sees_the_item_sum() {
        let scratch = tempfile::tempdir().unwrap();
        let (mut context, _) = layout_failed_gbk_context(scratch.path());
        assert_eq!(
            import_native(&mut context, 2, &[("龘龘", "da2'da2", 9)]),
            [true]
        );
        let token = tokens(&context, "龘龘")
            .into_iter()
            .find(|token| token >> 24 == 2)
            .unwrap();
        assert!(
            context
                .user_store_mut()
                .add_system_unigram_frequency(token, u32::MAX - 27)
        );
    }

    #[test]
    fn a_save_after_an_import_into_a_failed_layout_journals_the_item_sum() {
        let scratch = tempfile::tempdir().unwrap();
        let (mut context, total) = layout_failed_gbk_context(scratch.path());
        assert_eq!(
            import_native(&mut context, 2, &[("龘龘", "da2'da2", 9)]),
            [true]
        );
        assert_eq!(context.save_user(), SaveOutcome::Saved);
        assert!(context.unload_phrase_library(2));
        assert_eq!(context.runtime.lm().amplified_total(), total);
        context.finish();
        let journal = std::fs::read(scratch.path().join("user/gbk_char.dbin")).unwrap();
        let word = |at: usize| u32::from_le_bytes(journal[8 + at..12 + at].try_into().unwrap());
        assert_eq!((word(0), word(4)), (4, 0));
        assert_eq!((u64::from(word(10)), word(14)), (total, 27));
    }

    fn two_headers() -> Vec<u8> {
        [modify_header(0, 1, 1), modify_header(0, 1, 2)].concat()
    }

    #[test]
    fn mask_out_skips_the_log_of_an_unloaded_library() {
        let user = TempUserDir::new("mask-unloaded-log");
        write_user_conf(&user.path, &system_dir());
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        std::fs::write(
            user.path.join("gbk_char.dbin"),
            crate::data::chunk_format::build_memory_chunk(&two_headers()).unwrap(),
        )
        .unwrap();
        assert!(context.unload_phrase_library(2));
        assert_eq!(context.mask_out(0, 0), MaskOutOutcome::Done(true));
    }

    #[test]
    fn mask_out_over_an_unmappable_library_changes_nothing() {
        let Some((_scratch, user, run)) = child(
            "facade::context::tests::native::mask_out_over_an_unmappable_library_changes_nothing",
            |user| {
                write_user_conf(user, &system_dir());
                let system = private_system(user.parent().unwrap());
                let mut context = native_context(&system, user, UserConfLaw::Pinyin);
                assert_eq!(
                    import_native(&mut context, 1, &[("龘龘", "da2'da2", 1)]),
                    [true]
                );
                assert_eq!(context.save_user(), SaveOutcome::Saved);
                assert_eq!(
                    import_native(&mut context, 7, &[("龘龘龘", "da2'da2'da2", 1)]),
                    [true]
                );
                assert!(
                    context
                        .user_store_mut()
                        .add_system_unigram_frequency(0x0100_1225, 7)
                );
                let items = [tokens(&context, "龘龘"), tokens(&context, "龘龘龘")];
                let count = unigram(&context, 0x0100_1225);
                let files = files_of(user);
                let library = system.join("gb_char.bin");
                let original = std::fs::read(&library).unwrap();
                std::fs::remove_file(&library).unwrap();
                assert_eq!(context.mask_out(0, 0), MaskOutOutcome::Done(true));
                assert_eq!(files_of(user), files);
                assert_eq!(unigram(&context, 0x0100_1225), count);
                assert_eq!(
                    [tokens(&context, "龘龘"), tokens(&context, "龘龘龘")],
                    items
                );
                std::fs::write(&library, original).unwrap();
                assert_eq!(context.save_user(), SaveOutcome::Saved);
            },
        ) else {
            return;
        };
        assert_eq!(
            String::from_utf8_lossy(&run.stderr),
            format!(
                "mmap {} failed!{}",
                user.parent()
                    .unwrap()
                    .join("system")
                    .join("gb_char.bin")
                    .display(),
                crate::input::TEXT_NEWLINE
            )
        );
    }

    #[test]
    fn a_masked_user_gram_leaves_the_file_at_the_next_armed_save() {
        let user = TempUserDir::new("issue677");
        let key = 0x0700_0001_u32.to_le_bytes().to_vec();
        let gram = gram_bytes(100, &[(0x0100_0001, 100)]);
        plant_bigram(&user.path, vec![(key.clone(), gram.clone())]);
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let stored = || {
            store_rows(&user.path.join("user_bigram.db"), true)
                .into_iter()
                .find(|(found, _)| *found == key)
                .map(|(_, value)| value)
        };
        assert_eq!(
            import_native(&mut context, 7, &[("达达", "da2'da2", 100)]),
            [true]
        );
        assert_eq!(context.save_user(), SaveOutcome::Saved);
        assert_eq!(
            context.mask_out(0x0f00_0000, 0x0700_0000),
            MaskOutOutcome::Done(true)
        );
        assert_eq!(context.save_user(), SaveOutcome::NotSaved);
        assert_eq!(stored(), Some(gram));
        assert_eq!(
            import_native(&mut context, 7, &[("你你", "ni3'ni3", 100)]),
            [true]
        );
        assert_eq!(context.save_user(), SaveOutcome::Saved);
        assert_eq!(stored(), None);
    }

    #[test]
    fn set_options_before_alloc_controls_incomplete_parse_length() {
        let mut context = open_context(crate::facade::PINYIN_DEFAULT_OPTION_WORD);
        context.set_options(OptionBits::empty());
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nih"), 2);
        assert_eq!(
            native_window(&context, &mut instance, 0)
                .first()
                .unwrap()
                .text()
                .as_str(),
            Some("你")
        );
    }

    #[test]
    fn set_options_remasks_a_live_instance() {
        let mut context = open_context(crate::facade::PINYIN_DEFAULT_OPTION_WORD);
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nih"), 2);
        context.set_options(OptionBits::empty());
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nih"), 2);
        assert_eq!(
            native_window(&context, &mut instance, 0)
                .first()
                .unwrap()
                .text()
                .as_str(),
            Some("你")
        );
        context.set_options(OptionBits::PINYIN_INCOMPLETE);
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nih"), 3);
    }

    #[test]
    fn set_options_use_tone_reaches_the_hanyu_parser() {
        let mut context = open_context(crate::facade::PINYIN_DEFAULT_OPTION_WORD);
        let mut instance = context.alloc_instance();
        context.set_options(OptionBits::PINYIN_INCOMPLETE);
        assert_eq!(instance.bind(&context.runtime).parse_full_more("zai4"), 3);
        context.set_options(OptionBits::PINYIN_INCOMPLETE | OptionBits::USE_TONE);
        for (input, consumed) in [("zai4", 4), ("zhuang4", 7), ("zai6", 3)] {
            assert_eq!(
                instance.bind(&context.runtime).parse_full_more(input),
                consumed
            );
        }
    }

    fn gbk_token(context: &ContextCore) -> u32 {
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("ni"), 2);
        native_window(context, &mut instance, 0)
            .iter()
            .filter_map(|row| row.token())
            .map(crate::PhraseToken::value)
            .find(|token| token >> 24 == 2)
            .unwrap()
    }

    #[test]
    fn a_failed_reload_keeps_the_library_unloaded_until_the_next_load() {
        let scratch = tempfile::tempdir().unwrap();
        let system = private_system(scratch.path());
        let user = scratch.path().join("user");
        std::fs::create_dir(&user).unwrap();
        let mut context = native_context(&system, &user, UserConfLaw::Pinyin);
        let gbk = gbk_token(&context);
        assert!(context.unload_phrase_library(2));
        let library = system.join("gbk_char.bin");
        std::fs::remove_file(&library).unwrap();
        assert_eq!(context.load_phrase_library(2), Ok(true));
        assert!(
            !context
                .user_store_mut()
                .add_system_unigram_frequency(gbk, 1)
        );
        link_file(&system_dir().join("gbk_char.bin"), &library);
        assert_eq!(context.load_phrase_library(2), Ok(true));
        assert!(
            context
                .user_store_mut()
                .add_system_unigram_frequency(gbk, 1)
        );
    }

    #[test]
    fn an_increment_and_an_import_into_an_unloaded_library_answer_false() {
        let user = TempUserDir::new("guards-unloaded");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let gbk = gbk_token(&context);
        assert!(context.unload_phrase_library(2));
        let total = context.runtime.lm().amplified_total();
        assert!(
            !context
                .user_store_mut()
                .add_system_unigram_frequency(gbk, 1)
        );
        let ImportPlan::Add {
            phrase,
            keys,
            original,
        } = plan(&context, 2, "单", "dan")
        else {
            panic!("planning precedes the unloaded-library guard")
        };
        assert!(
            context
                .user_store_mut()
                .add_system_phrase_in(2, original, &phrase, &keys, None)
                .is_err()
        );
        assert_eq!(context.runtime.lm().amplified_total(), total);
        context.end_import();
        assert_eq!(context.load_phrase_library(2), Ok(true));
        assert!(
            context
                .user_store_mut()
                .add_system_unigram_frequency(gbk, 1)
        );
    }

    #[test]
    fn removing_the_last_saved_phrase_from_a_clean_context_does_not_persist() {
        let user = TempUserDir::new("clean-last-removal");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        assert_eq!(
            import_native(&mut context, 7, &[("测侧", "ce'ce", 7)]),
            [true]
        );
        let saved = export_native(&context, 7);
        assert_eq!(saved.len(), 1);
        assert_eq!(context.save_user(), SaveOutcome::Saved);
        context.finish();
        drop(context);
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("cece"), 4);
        let window = native_window(&context, &mut instance, 0);
        let token = window
            .iter()
            .filter_map(|row| row.token())
            .find(|token| token.value() >> 24 == 7)
            .unwrap()
            .value();
        assert!(context.user_store_mut().remove_user_phrase(token).unwrap());
        assert!(export_native(&context, 7).is_empty());
        assert_eq!(context.save_user(), SaveOutcome::NotSaved);
        context.finish();
        drop(context);
        let context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        assert_eq!(export_native(&context, 7), saved);
    }

    #[test]
    fn user_unigram_increment_survives_a_later_save() {
        let user = TempUserDir::new("user-unigram-save");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        assert_eq!(
            import_native(&mut context, 7, &[("龘龘", "da2'da2", 5)]),
            [true]
        );
        let token = context
            .user_store()
            .token_for_phrase("龘龘")
            .unwrap()
            .unwrap();
        let base = unigram(&context, token).unwrap();
        assert_eq!(context.save_user(), SaveOutcome::Saved);
        assert_eq!(
            context.user_store_mut().add_unigram_outcome(token, 9),
            crate::user::UnigramAdd::Accepted
        );
        assert_eq!(context.save_user(), SaveOutcome::NotSaved);
        context.end_import();
        assert_eq!(context.save_user(), SaveOutcome::Saved);
        context.finish();
        drop(context);
        let context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        assert_eq!(unigram(&context, token), Some(base + 9));
    }

    #[test]
    fn oversized_journal_record_payloads_refuse_save_and_keep_modified() {
        use crate::data::chunk_write::{ChunkItem, build_chunk};
        let system = TempSystemDir::new("oversized-journal-record");
        let user = TempUserDir::new("oversized-journal-record");
        let ni = crate::ChewingKey::from_pinyin("ni").unwrap().to_packed();
        let chunk = build_chunk(&[(
            1,
            ChunkItem {
                phrase: vec![u32::from('你'); 128],
                unigram: 100,
                prons: vec![(vec![ni; 128], 1); 255],
            },
        )])
        .unwrap();
        std::fs::write(system.path.join("gb_char.bin"), chunk).unwrap();
        let mut context = native_context(&system.path, &user.path, UserConfLaw::Pinyin);
        let token = 0x0100_0001;
        assert_eq!(unigram(&context, token), Some(100));
        assert!(
            context
                .user_store_mut()
                .add_system_unigram_frequency(token, 1)
        );
        assert_eq!(unigram(&context, token), Some(101));
        context.end_import();
        assert!(context.user_store().is_modified());
        assert_eq!(context.save_user(), SaveOutcome::NotSaved);
        assert!(context.user_store().is_modified());
        assert!(!user.path.join("gb_char.dbin.tmp").exists());
        assert!(!user.path.join("gb_char.dbin").exists());
    }
}

mod native_files {
    use crate::facade::SaveOutcome;
    use crate::facade::test_fixture::{native_fixture::*, *};
    use crate::user::UserConfLaw;

    #[test]
    fn an_unloaded_user_library_hides_its_token_until_the_load() {
        let user = TempUserDir::new("token-unload");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Zhuyin);
        assert_eq!(
            import_native(&mut context, 7, &[("你好", "ㄋㄧˇ ㄏㄠˇ", 1)]),
            [true]
        );
        let found: Vec<_> = tokens(&context, "你好")
            .into_iter()
            .filter(|token| token >> 24 == 7)
            .collect();
        assert_eq!(found.len(), 1);
        let token = found[0];
        let shown = |context: &crate::facade::ContextCore| {
            assert_eq!(
                context.runtime.dict().token_pronunciation_count(token),
                Some(1)
            );
            assert_eq!(
                context.runtime.dict().token_text(token),
                Some("你好".into())
            );
            let bytes = context
                .runtime
                .dict()
                .with_user_nth_pronunciation_words(token, 0, |length, keys| {
                    assert_eq!(length, 2);
                    keys.unwrap().to_vec()
                })
                .unwrap();
            assert_eq!(bytes, [0x2b, 0x30, 0x86, 0x32]);
        };
        shown(&context);
        assert_eq!(context.save_user(), SaveOutcome::Saved);
        assert!(context.unload_phrase_library(7));
        assert_eq!(
            context.runtime.dict().token_pronunciation_count(token),
            None
        );
        assert_eq!(context.runtime.dict().token_text(token), None);
        assert_eq!(
            context
                .runtime
                .dict()
                .with_user_nth_pronunciation_words(token, 0, |_, keys| keys.map(<[u8]>::to_vec)),
            None
        );
        assert_eq!(context.load_phrase_library(7), Ok(true));
        shown(&context);
    }

    /// table_info.cpp:254-261 reads words across the newline; duplicate
    /// gbk_char.dbin destinations differ only in the two facades' rename law.
    fn short_row_session(user: &std::path::Path, law: UserConfLaw) {
        let system = short_row_system(user.parent().unwrap());
        let mut context = native_context(&system, user, law);
        for text in if law == UserConfLaw::Pinyin {
            ["nihao", "yishu"]
        } else {
            ["ni3hao3", "yi4shu4"]
        } {
            let mut instance = native_instance(&context, law);
            if law == UserConfLaw::Pinyin {
                let _ = instance.bind(&context.runtime).parse_full_more(text);
            } else {
                let _ = instance.bind(&context.runtime).parse_full_more_zhuyin(text);
            }
            guess(&context, &mut instance);
            let _sentence = sentence(&context, &instance, 0);
            assert!(
                native_window(&context, &mut instance, 0)
                    .iter()
                    .all(|row| row.token().map(crate::PhraseToken::value) != Some(0x0100_1225))
            );
        }
        let mut instance = native_instance(&context, law);
        let reading = if law == UserConfLaw::Pinyin {
            let _ = instance.bind(&context.runtime).parse_full_more("nihao");
            "ce'shi'ci"
        } else {
            let _ = instance
                .bind(&context.runtime)
                .parse_full_more_zhuyin("ni3hao3");
            "ㄘㄜˋ ㄕˋ ㄘˊ"
        };
        guess(&context, &mut instance);
        assert!(context.train(&instance, 0).unwrap());
        assert_eq!(
            import_native(&mut context, 7, &[("测试词", reading, 1)]),
            [true]
        );
        assert_eq!(context.save_user(), SaveOutcome::Saved);
        context.finish();
        let mut sizes = files_of(user)
            .into_iter()
            .map(|(name, bytes)| (name, bytes.len()))
            .collect::<std::collections::BTreeMap<_, _>>();
        sizes.remove("user.conf");
        assert_eq!(
            sizes.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "addon.bin",
                "gbk_char.dbin",
                "merged.dbin",
                "network.bin",
                "opengram.dbin",
                "user.bin",
                "user_bigram.db",
                "user_phrase_index.bin",
                "user_pinyin_index.bin"
            ]
        );
        for (name, size) in [
            ("addon.bin", 27),
            ("gbk_char.dbin", 26),
            ("merged.dbin", 26),
            ("network.bin", 27),
            ("opengram.dbin", 26),
            ("user.bin", 71),
        ] {
            assert_eq!(sizes[name], size, "{name}");
        }
        let marker = std::fs::read_to_string(user.join("user.conf")).unwrap();
        let nl = crate::input::TEXT_NEWLINE;
        assert!(
            marker.starts_with(&format!(
                "binary format version:7{nl}model data version:14{nl}"
            )) && marker.ends_with(&format!("{nl}open counter:0{nl}")),
            "{marker}"
        );
    }

    #[test]
    fn pinyin_table_conf_919_short_row_merged() {
        let Some((_scratch, user, run)) = child(
            "facade::context::tests::native_files::pinyin_table_conf_919_short_row_merged",
            |user| short_row_session(user, UserConfLaw::Pinyin),
        ) else {
            return;
        };
        let sep = std::path::MAIN_SEPARATOR;
        let nl = crate::input::TEXT_NEWLINE;
        let user = user.display();
        assert_eq!(
            String::from_utf8_lossy(&run.stderr),
            format!(
                "open {user}{sep}user.conf failed.rename {user}{sep}gbk_char.dbin.tmp to {user}{sep}gbk_char.dbin failed.{nl}"
            )
        );
    }
    #[test]
    fn zhuyin_table_conf_919_short_row_merged() {
        let Some((_scratch, user, run)) = child(
            "facade::context::tests::native_files::zhuyin_table_conf_919_short_row_merged",
            |user| short_row_session(user, UserConfLaw::Zhuyin),
        ) else {
            return;
        };
        assert_eq!(
            String::from_utf8_lossy(&run.stderr),
            format!(
                "open {}{}user.conf failed.",
                user.display(),
                std::path::MAIN_SEPARATOR
            )
        );
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn a_save_over_a_failed_layout_journals_against_an_empty_index() {
        let Some((_scratch, user, _run)) = child(
            "facade::context::tests::native_files::a_save_over_a_failed_layout_journals_against_an_empty_index",
            |user| {
                let system = user.parent().unwrap().join("system");
                std::fs::create_dir(&system).unwrap();
                for entry in std::fs::read_dir(contract_data::contract_data_dir()).unwrap() {
                    let entry = entry.unwrap();
                    if entry.file_type().unwrap().is_file() {
                        link_file(&entry.path(), &system.join(entry.file_name()));
                    }
                }
                let mut context = native_context(&system, user, UserConfLaw::Pinyin);
                train_once(&mut context);
                replace_with_failed_layout(&system.join("merged.bin"), 123);
                assert_eq!(context.save_user(), SaveOutcome::Saved);
            },
        ) else {
            return;
        };
        let journal = std::fs::read(user.join("merged.dbin")).unwrap();
        assert_eq!(journal.len(), 38_252);
        use sha2::Digest as _;
        assert_eq!(
            sha2::Sha256::digest(&journal)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            "59eed2f9ef43edc1f6e62b72a6f83c6ac2b83e4a56f9956a19c0452dce92a34a"
        );
        assert_eq!(journal_walk(&journal), (Some((123, 1001)), [1, 1001, 0, 0]));
    }

    #[cfg(feature = "tkrzw")]
    fn journal_walk(data: &[u8]) -> (Option<(u32, u32)>, [u32; 4]) {
        let payload = &data[8..];
        let word = |at: usize| u32::from_le_bytes(payload[at..at + 4].try_into().expect("a word"));
        let half = |at: usize| usize::from(u16::from_le_bytes([payload[at], payload[at + 1]]));
        let (mut at, mut header, mut counts) = (0, None, [0; 4]);
        while at + 10 <= payload.len() {
            match word(at) {
                4 => {
                    header.get_or_insert((word(at + 10), word(at + 14)));
                    counts[0] += 1;
                    at += 10 + 2 * half(at + 8);
                }
                kind @ (1 | 2) => {
                    counts[kind as usize] += 1;
                    at += 10 + half(at + 8);
                }
                3 => {
                    counts[3] += 1;
                    at += 12 + half(at + 8) + half(at + 10);
                }
                _ => break,
            }
        }
        (header, counts)
    }
}
