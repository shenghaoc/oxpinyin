//! Init and save: reports, refusals, conf formats and copy faults.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::fresh_user_conf_line;
#[cfg(unix)]
use crate::support::train_once;

use crate::support::{drain_phrases, init_fixture_at};

use crate::support::TempUserDir;
use crate::support::cstr;
use crate::support::open;
use crate::support::pinyin_alloc_instance;
use crate::support::pinyin_begin_add_phrases;
use crate::support::pinyin_begin_get_phrases;
use crate::support::pinyin_end_add_phrases;
use crate::support::pinyin_end_get_phrases;
use crate::support::pinyin_fini;
use crate::support::pinyin_init;
use crate::support::pinyin_iterator_add_phrase;
use crate::support::pinyin_save;
use crate::support::private_system;
use crate::support::system_dir;
use crate::support::write_user_conf;
use crate::support::*;

use std::path::Path;

use std::ptr;

/// A save cut by a four-byte file-size limit aborts after the chunk's
/// length word: the staged log holds four bytes and the
/// marker keeps the raised counter.
// Unix only: RLIMIT_FSIZE and SIGXFSZ.
#[cfg(unix)]
#[test]
fn save_aborts_where_the_chunk_header_write_fails() {
    let run = crate::support::assert_aborts_in_dir(
        "init_and_save::save_aborts_where_the_chunk_header_write_fails",
        |user| {
            let context = init_at(&system_dir(), user);
            assert!(!context.is_null());
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { train_once(context) };
            let limit = libc::rlimit {
                rlim_cur: 4,
                rlim_max: 4,
            };
            // SAFETY: ignores SIGXFSZ and lowers this child's own file-size
            // limit; the child ends in the save.
            unsafe {
                libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &raw const limit), 0);
            }
            unsafe { pinyin_save(context) };
        },
    );
    let staged = std::fs::metadata(run.user_dir.join("gb_char.dbin.tmp")).expect("staged log");
    assert_eq!(staged.len(), 4);
    let conf = std::fs::read_to_string(run.user_dir.join("user.conf")).expect("user.conf");
    let newline = oxpinyin::input::TEXT_NEWLINE;
    assert!(
        conf.ends_with(&format!("open counter:1{newline}")),
        "{conf}"
    );
}

/// A NULL user directory prints the empty-path open line at init and the
/// two write lines at fini, and nothing else.
#[test]
fn a_null_user_directory_reports_its_empty_path() {
    let run = crate::support::run_in_child(
        "init_and_save::a_null_user_directory_reports_its_empty_path",
        |_| {
            let system = cstr(system_dir().to_str().expect("UTF-8 path"));
            let context = unsafe { pinyin_init(system.as_ptr(), ptr::null()) };
            assert!(!context.is_null());
            unsafe { pinyin_fini(context) };
        },
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        format!("open  failed.write  failed.{NL}write  failed.{NL}")
    );
}

/// An unknown database format in table.conf aborts the init before it
/// writes the user directory.
#[test]
fn init_aborts_on_an_unknown_table_conf_database_format() {
    let run = crate::support::assert_aborts_in_dir(
        "init_and_save::init_aborts_on_an_unknown_table_conf_database_format",
        |user| {
            let format = format!(
                "database format:{}",
                oxpinyin::data::DEFAULT_STORE_DB_FORMAT
            );
            let system = conf_system(
                user.parent().expect("scratch"),
                &format,
                "database format:LMDB",
            );
            init_at(&system, user);
        },
    );
    assert!(files_of(&run.user_dir).is_empty());
}

/// A default row of type `DICTIONARY` aborts the init in its library loop
/// (`assert(DICTIONARY != table_info->m_file_type)`, `pinyin.cpp:388`). The
/// loop runs after `check_format`, which has saved `user.conf`
/// (`pinyin.cpp:343`, `:184-187`).
#[test]
fn init_aborts_on_a_default_dictionary_row_after_user_conf() {
    let run = crate::support::assert_aborts_in_dir(
        "init_and_save::init_aborts_on_a_default_dictionary_row_after_user_conf",
        |user| {
            let system = conf_system(
                user.parent().expect("scratch"),
                "merged.bin merged.dbin SYSTEM_FILE",
                "merged.bin merged.dbin DICTIONARY",
            );
            init_at(&system, user);
        },
    );
    assert!(files_of(&run.user_dir).contains_key("user.conf"));
}

/// The init's answers for the two layout faults of a mapped library. Both chunks pass `MemoryChunk::mmap`'s length and checksum
/// tests (`memory_chunk.h:470-520`), so neither writes the `mmap %s failed!`
/// line. A payload shorter than its 17-byte header loads as an empty
/// sub-index and the init succeeds with no GLib log. A separator index
/// outside the payload refuses the init with one warning: the pin reads
/// past the chunk there (`phrase_index.cpp:354-356`).
#[test]
fn init_answers_for_the_two_library_layout_faults() {
    let run = crate::support::run_in_child(
        "init_and_save::init_answers_for_the_two_library_layout_faults",
        |user| {
            let scratch = user.parent().expect("scratch");
            let chunk = |payload: &[u8]| {
                oxpinyin::data::chunk_format::build_memory_chunk(payload).expect("frame")
            };
            let mut outside = [0_u8; 32];
            for (at, value) in [(4, 17_u32), (8, 100), (12, 21)] {
                outside[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            outside[16] = b'#';
            for (name, payload, opens, logged) in [
                ("short", &[][..], true, 0),
                ("outside", &outside[..], false, 1),
            ] {
                let parent = scratch.join(name);
                std::fs::create_dir(&parent).expect("a system parent");
                let system = private_system(&parent);
                std::fs::remove_file(system.join("merged.bin")).expect("unlink the library");
                std::fs::write(system.join("merged.bin"), chunk(payload)).expect("the library");
                let user = parent.join("user");
                std::fs::create_dir(&user).expect("a user directory");
                write_user_conf(&user, &system);
                let logs = crate::support::LogCapture::new();
                let context = init_at(&system, &user);
                assert_eq!(!context.is_null(), opens, "{name}");
                let warning = (Some("libpinyin".to_owned()), glib_sys::G_LOG_LEVEL_WARNING);
                assert_eq!(logs.take(), vec![warning; logged], "{name}");
            }
        },
    );
    assert!(
        run.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

/// A save that cannot map a loaded system library again writes the pin's
/// line and answers `false` (`pinyin.cpp:955-957`); the context stays
/// modified, so the next save, with the library back, writes the profile
/// and answers `true` (`:1136`, `:1141-1146`).
#[test]
fn a_save_retries_after_a_failed_remap() {
    let run = crate::support::run_in_child(
        "init_and_save::a_save_retries_after_a_failed_remap",
        |user| {
            let system = private_system(user.parent().expect("scratch"));
            write_user_conf(user, &system);
            let context = init_at(&system, user);
            assert!(!context.is_null());
            assert_eq!(
                // SAFETY: `context` was opened above and is not yet freed; only this thread calls
                // into it.
                unsafe { import_batch(context, 7, &[("你好", "ni3'hao3", 1)]) },
                [true]
            );
            let library = system.join("gb_char.bin");
            let away = system.join("gb_char.bin.away");
            std::fs::rename(&library, &away).expect("move the library away");
            assert!(!unsafe { pinyin_save(context) });
            assert!(!user.join("user.bin").exists());
            std::fs::rename(&away, &library).expect("move the library back");
            assert!(unsafe { pinyin_save(context) });
            assert!(user.join("user.bin").exists());
            assert!(!unsafe { pinyin_save(context) }, "nothing left to save");
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        format!(
            "mmap {} failed!{}",
            system.join("gb_char.bin").display(),
            crate::support::NL
        )
    );
}

/// The trained pairs of the user bigram, through the export iterator.
///
/// # Safety
///
/// `context` is a live context: `pinyin_init` returned it and `pinyin_fini` has not taken it.
/// Only this thread calls into it.
unsafe fn trained_pairs(context: *mut crate::support::PinyinContext) -> Vec<String> {
    let iter = unsafe { crate::support::pinyin_begin_get_bigram_phrases(context) };
    let mut pairs = Vec::new();
    while unsafe { crate::support::pinyin_bigram_iterator_has_next_phrase(iter) } {
        let mut phrase: *mut crate::support::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::support::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        unsafe {
            crate::support::pinyin_bigram_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            )
        };
        // SAFETY: `phrase` and `pinyin` are this step's outputs, each NULL or a separate string
        // the library returned to this code, which owns them and frees them only here.
        pairs.push(unsafe { crate::support::take_exported_row(phrase, pinyin, count) }.0);
    }
    unsafe { crate::support::pinyin_end_get_bigram_phrases(iter) };
    pairs
}

/// Whether the candidates of `spelling` hold a row that reads `text`.
///
/// # Safety
///
/// `context` is a live context: `pinyin_init` returned it and `pinyin_fini` has not taken it.
/// Only this thread calls into it.
unsafe fn lists(context: *mut crate::support::PinyinContext, spelling: &str, text: &str) -> bool {
    // SAFETY: `context` meets this function's `# Safety` contract.
    let instance = unsafe { alloc(context) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, spelling) }, spelling.len());
    assert!(unsafe { crate::support::pinyin_guess_candidates(instance, 0, 0) });
    // SAFETY: as above.
    let listed = unsafe { capture_rows(instance) }
        .iter()
        .any(|(_, row, _)| row == text);
    unsafe { crate::support::pinyin_free_instance(instance) };
    listed
}

/// A blocked staging name does not stop the save. A directory with a file
/// in it holds `user_pinyin_index.bin.tmp`: the `unlink` and the store into
/// it fail and are ignored (`pinyin.cpp:996-1003`); the rename of a
/// directory over the live file fails and writes one line (`:1091-1097`);
/// the pass goes on to the phrase index and the bigram (`:1102-1126`); the
/// save rewrites `user.conf`, clears the modified flag and answers `true`
/// (`:1139-1146`).
///
/// The save has two pending changes. A trained choice is a new bigram, and
/// the import of 龘鱻 is a new row of the user pinyin index. The next init
/// reads the files back (`pinyin.cpp:352-371`, `:394-402`): the trained pair
/// is exported, so the bigram was renamed; 龘鱻 has its phrase-table token,
/// so the phrase index was renamed; and `daxian` does not list 龘鱻, so the
/// pinyin index is the old one, which still lists 龘龘 for `dada`.
#[test]
fn a_blocked_staging_name_does_not_stop_the_save() {
    let run = crate::support::run_in_child(
        "init_and_save::a_blocked_staging_name_does_not_stop_the_save",
        |user| {
            write_user_conf(user, &system_dir());
            let context = init_at(&system_dir(), user);
            assert!(!context.is_null());
            assert_eq!(
                // SAFETY: `context` was opened above and is not yet freed; only this thread calls
                // into it.
                unsafe { import_batch(context, 7, &[("龘龘", "da2'da2", 5)]) },
                [true]
            );
            assert!(unsafe { pinyin_save(context) });
            // SAFETY: as above.
            assert!(unsafe { trained_pairs(context) }.is_empty());

            // The bigram's pending change: a chosen candidate, then a training.
            // SAFETY: as above.
            let instance = unsafe { alloc(context) };
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls
            // into it.
            assert_eq!(unsafe { parse(instance, "nihao") }, 5);
            // SORT_WITHOUT_LONGER_CANDIDATE.
            assert!(unsafe { crate::support::pinyin_guess_candidates(instance, 0, 0x2) });
            // SAFETY: as above.
            let row = unsafe { crate::support::row_with(instance, NORMAL, "你") };
            // SAFETY: the row belongs to this live instance and no call has invalidated it.
            assert_eq!(
                unsafe { crate::support::pinyin_choose_candidate(instance, 0, row) },
                2
            );
            assert!(unsafe { crate::support::pinyin_guess_sentence(instance) });
            assert!(unsafe { crate::support::pinyin_train(instance, 0) });
            unsafe { crate::support::pinyin_free_instance(instance) };
            // The pinyin index's pending change.
            assert_eq!(
                // SAFETY: as above.
                unsafe { import_batch(context, 7, &[("龘鱻", "da2'xian1", 5)]) },
                [true]
            );
            // SAFETY: as above.
            assert!(unsafe { lists(context, "daxian", "龘鱻") });

            let blocked = user.join("user_pinyin_index.bin.tmp");
            std::fs::create_dir(&blocked).expect("the blocking directory");
            std::fs::write(blocked.join("keep"), "x").expect("a file in it");
            assert!(unsafe { pinyin_save(context) });
            assert!(
                !unsafe { pinyin_save(context) },
                "the save cleared the modified flag"
            );
            assert!(blocked.join("keep").exists());
            std::fs::remove_dir_all(&blocked).expect("remove the blocking directory");
            unsafe { pinyin_fini(context) };

            let context = init_at(&system_dir(), user);
            assert!(!context.is_null());
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls
            // into it.
            let pairs = unsafe { trained_pairs(context) };
            assert!(
                pairs.iter().any(|pair| pair.starts_with('你')),
                "the new bigram: {pairs:?}"
            );
            // SAFETY: as above.
            let instance = unsafe { alloc(context) };
            assert!(
                // SAFETY: `instance` was opened above and is not yet freed; only this thread
                // calls into it.
                unsafe { lookup(instance, "龘鱻") }
                    .iter()
                    .any(|token| token >> 24 == 7),
                "the new phrase index"
            );
            unsafe { crate::support::pinyin_free_instance(instance) };
            // SAFETY: as above.
            assert!(
                !unsafe { lists(context, "daxian", "龘鱻") },
                "the old pinyin index"
            );
            // SAFETY: as above.
            assert!(unsafe { lists(context, "dada", "龘龘") });
            unsafe { pinyin_fini(context) };
        },
    );
    let sep = std::path::MAIN_SEPARATOR;
    let user = run.user_dir.display();
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        format!(
            "rename {user}{sep}user_pinyin_index.bin.tmp to {user}{sep}user_pinyin_index.bin failed.{}",
            crate::support::NL
        )
    );
}

/// The init joins the system name and `table.conf` with `g_build_filename`
/// (`pinyin.cpp:335-336`), which drops an empty element: the empty name is
/// `table.conf` in the working directory, and `.` is `./table.conf`. Without
/// that file the load writes its line and the init writes its own and
/// answers NULL (`table_info.cpp:199-203`, `pinyin.cpp:337-340`). With it,
/// the libraries open from the same directory.
#[test]
fn an_empty_or_dot_system_name_reads_the_working_directory() {
    let run = crate::support::run_in_child(
        "init_and_save::an_empty_or_dot_system_name_reads_the_working_directory",
        |user| {
            write_user_conf(user, &system_dir());
            let user_c = cstr(user.to_str().expect("UTF-8 path"));
            let bare = user.parent().expect("scratch").join("bare");
            std::fs::create_dir(&bare).expect("a directory with no table.conf");
            std::env::set_current_dir(&bare).expect("enter it");
            assert!(unsafe { pinyin_init(c"".as_ptr(), user_c.as_ptr()) }.is_null());
            std::env::set_current_dir(system_dir()).expect("enter the fixture");
            let context = unsafe { pinyin_init(c".".as_ptr(), user_c.as_ptr()) };
            assert!(!context.is_null());
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls
            // into it.
            let instance = unsafe { alloc(context) };
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls
            // into it.
            assert_eq!(unsafe { parse(instance, "nihao") }, 5);
            assert!(unsafe { crate::support::pinyin_guess_sentence(instance) });
            // SAFETY: `context` was opened above and is not yet freed; `instance` was opened
            // above and is not yet freed; nothing uses either handle after this call; only this
            // thread calls into them.
            unsafe { close(context, instance) };
        },
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        format!("open table.conf failed.{NL}load table.conf failed!{NL}")
    );
}

/// An unknown database format in user.conf aborts the init and leaves
/// user.conf as it was.
#[test]
fn init_aborts_on_an_unknown_user_conf_database_format() {
    let run = crate::support::assert_aborts_in_dir(
        "init_and_save::init_aborts_on_an_unknown_user_conf_database_format",
        |user| {
            write_user_conf(user, &system_dir());
            let path = user.join("user.conf");
            let text = std::fs::read_to_string(&path).expect("user.conf");
            let format = format!(
                "database format:{}",
                oxpinyin::data::DEFAULT_STORE_DB_FORMAT
            );
            std::fs::write(&path, text.replace(&format, "database format:LMDB"))
                .expect("write user.conf");
            init_at(&system_dir(), user);
        },
    );
    let conf = std::fs::read_to_string(run.user_dir.join("user.conf")).expect("user.conf");
    assert!(conf.contains("database format:LMDB\n"), "{conf}");
    assert!(conf.ends_with("open counter:0\n"), "{conf}");
}

/// The open counter `user.conf` records, as the next init reads it.
fn recorded_open_counter(user_dir: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(user_dir.join("user.conf"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("open counter:"))
        .and_then(|value| value.trim().parse().ok())
}

// Windows only: the C API's UTF-8 paths reach the wide-character (UTF-16) file API.
#[cfg(windows)]
#[test]
fn utf8_c_api_paths_open_cjk_directories_and_files() {
    let root = tempfile::tempdir().unwrap();
    let system = root.path().join("系统");
    let user = root.path().join("用户");
    std::fs::create_dir(&system).unwrap();
    std::fs::create_dir(&user).unwrap();
    for entry in std::fs::read_dir(system_dir()).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), system.join(entry.file_name())).unwrap();
    }
    std::fs::rename(system.join("gb_char.bin"), system.join("汉字.bin")).unwrap();
    let conf = std::fs::read_to_string(system.join("table.conf")).unwrap();
    std::fs::write(
        system.join("table.conf"),
        conf.replace("gb_char.bin", "汉字.bin"),
    )
    .unwrap();
    let system = cstr(system.to_str().unwrap());
    let user_name = cstr(user.to_str().unwrap());
    let context = unsafe { crate::support::pinyin_init(system.as_ptr(), user_name.as_ptr()) };
    assert!(!context.is_null());
    unsafe {
        pinyin_end_add_phrases(pinyin_begin_add_phrases(
            context,
            oxpinyin::vocab::USER_DICTIONARY,
        ))
    };
    assert!(unsafe { pinyin_save(context) });
    unsafe { crate::support::pinyin_fini(context) };
    assert!(user.join("user.conf").is_file());
}

/// The pin opens the directories it is given by their bytes (`g_strdup`,
/// `pinyin.cpp:331-336`), so a name that is not UTF-8 is a directory like any
/// other, for the system tables and for the user dir alike (#587).
// Unix only: byte-string directory names (macOS refuses them, which the test asserts).
#[cfg(unix)]
#[test]
fn directories_with_non_utf8_names_open() {
    use std::ffi::{CString, OsStr};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    /// Removes the fixture however the test ends, a failed assertion included.
    struct RemoveFixture<'a> {
        system: &'a std::path::Path,
        user: &'a std::path::Path,
    }
    impl Drop for RemoveFixture<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.system);
            let _ = std::fs::remove_dir_all(self.user);
        }
    }

    let scratch_dir = tempfile::tempdir().expect("scratch");
    let scratch = scratch_dir.path();
    let pid = std::process::id();
    let named = |prefix: &str| {
        let mut name = prefix.as_bytes().to_vec();
        name.extend_from_slice(b"\xff");
        name.extend_from_slice(pid.to_string().as_bytes());
        scratch.join(OsStr::from_bytes(&name))
    };
    let system = named("oxpinyin-system-");
    let user = named("oxpinyin-");
    let _ = std::fs::remove_file(&system);
    let _ = std::fs::remove_dir_all(&user);
    let _remove_fixture = RemoveFixture {
        system: &system,
        user: &user,
    };
    // macOS's expected result: APFS refuses both names with `EILSEQ`, so the
    // fixture never exists and no oxpinyin code runs.
    if cfg!(target_os = "macos") {
        for refused in [
            std::os::unix::fs::symlink(system_dir(), &system),
            std::fs::create_dir(&user),
        ] {
            let error = refused.expect_err("APFS refuses a non-UTF-8 name");
            assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
        }
        return;
    }
    std::os::unix::fs::symlink(system_dir(), &system).expect("symlink to the fixtures");
    std::fs::create_dir(&user).expect("user dir");

    let c_path = |path: &std::path::Path| {
        CString::new(path.as_os_str().to_owned().into_vec()).expect("no NUL in the path")
    };
    let (system_c, user_c) = (c_path(&system), c_path(&user));
    let context = unsafe { pinyin_init(system_c.as_ptr(), user_c.as_ptr()) };
    assert!(!context.is_null(), "a non-UTF-8 name is a directory");
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    let _ = unsafe { pinyin_save(context) };
    assert!(
        std::fs::read_dir(&user).expect("user dir").next().is_some(),
        "the context opened the user dir it was given"
    );

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

#[test]
fn two_contexts_on_one_user_dir_are_independent() {
    // #538: a second pinyin_init on a user dir another live context has
    // open is a context of its own, as upstream's is — no state is held
    // per directory. Each init raises the open counter (pinyin.cpp:185-187),
    // B does not see what A learned and has not saved, each save answers
    // for its own context's modifications, and each fini lowers its own
    // copy of the counter (:1194-1200).
    let user_dir = TempUserDir::new("two-contexts");
    let dir = user_dir.path.to_str().expect("UTF-8 path");
    let (ctx_a, inst_a) = open(dir);
    let (ctx_b, inst_b) = open(dir);
    assert_eq!(recorded_open_counter(&user_dir.path), Some(2));

    let iter = unsafe { pinyin_begin_add_phrases(ctx_a, 7) };
    assert!(unsafe {
        pinyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("nihao").as_ptr(), 5)
    });
    unsafe { pinyin_end_add_phrases(iter) };
    let export = unsafe { pinyin_begin_get_phrases(ctx_b, 7) };
    assert!(
        // SAFETY: `export` is a live iterator from its `begin` call, not yet ended; only this
        // thread calls into it.
        unsafe { drain_phrases(export) }.is_empty(),
        "B sees A's unsaved import"
    );
    unsafe { pinyin_end_get_phrases(export) };

    assert!(!unsafe { pinyin_save(ctx_b) }, "B changed nothing");
    assert!(unsafe { pinyin_save(ctx_a) });

    unsafe { crate::support::pinyin_free_instance(inst_a) };
    unsafe { crate::support::pinyin_free_instance(inst_b) };
    unsafe { crate::support::pinyin_fini(ctx_a) };
    assert_eq!(recorded_open_counter(&user_dir.path), Some(0));
    unsafe { crate::support::pinyin_fini(ctx_b) };
    assert_eq!(recorded_open_counter(&user_dir.path), Some(1));
}

/// init's library loop merges `gb_char.dbin` and
/// `next_record` asserts on its non-null `MODIFY_HEADER` token
/// (`phrase_index_logger.h:202`).
#[test]
fn init_aborts_on_a_log_header_token() {
    crate::support::assert_aborts("init_and_save::init_aborts_on_a_log_header_token", || {
        let user_dir = TempUserDir::new("init-header-token");
        crate::support::plant_log(
            &user_dir.path,
            "gb_char.dbin",
            &crate::support::logs::header_token(),
        );
        let _ = init_fixture_at(&user_dir.path);
    });
}

/// A second `pinyin_fini` on one context: the pin
/// deletes the context twice (`pinyin.cpp:1194-1222`) and dies of SIGSEGV;
/// the call returns silently. The init's own `user.conf` line is the
/// only output.
#[test]
fn fini_twice_returns_silently() {
    let run = crate::support::run_in_child("init_and_save::fini_twice_returns_silently", |user| {
        let context = init_fixture_at(user);
        assert!(!context.is_null());
        let instance = unsafe { pinyin_alloc_instance(context) };
        unsafe { crate::support::pinyin_free_instance(instance) };
        unsafe { crate::support::pinyin_fini(context) };
        unsafe { crate::support::pinyin_fini(context) };
    });
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        fresh_user_conf_line(&run.user_dir)
    );
}

/// Init refuses a NULL system directory, a missing one and one without
/// table.conf: each answers NULL after the two `open`/`load` lines.
#[test]
fn init_refuses_a_system_directory_without_table_conf() {
    let run = crate::support::run_in_child(
        "init_and_save::init_refuses_a_system_directory_without_table_conf",
        |user| {
            let user_c = cstr(user.to_str().expect("UTF-8 path"));
            assert!(unsafe { pinyin_init(ptr::null(), user_c.as_ptr()) }.is_null());
            assert!(init_at(Path::new("/nonexistent-system-dir"), user).is_null());
            let system = private_system(user.parent().expect("the child's scratch"));
            std::fs::remove_file(system.join("table.conf")).expect("unlink table.conf");
            assert!(init_at(&system, user).is_null());
        },
    );
    // GLib joins with the separator that last occurs in its arguments: `/`
    // after the literal directory, the platform's own after a native path.
    let conf = run
        .user_dir
        .parent()
        .expect("scratch")
        .join("system")
        .join("table.conf");
    let missing = |conf: &str| format!("open {conf} failed.{NL}load {conf} failed!{NL}");
    let expected = format!(
        "open  failed.{NL}load  failed!{NL}{}{}",
        missing("/nonexistent-system-dir/table.conf"),
        missing(&conf.display().to_string())
    );
    assert_eq!(String::from_utf8_lossy(&run.stderr), expected);
}
