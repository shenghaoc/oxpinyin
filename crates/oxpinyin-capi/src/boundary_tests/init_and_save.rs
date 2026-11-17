//! Init and save: reports, refusals, conf formats and copy faults.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// Trains the top sentence of `nihao` once on a new instance.
fn train_once(context: *mut PinyinContext) {
    let instance = alloc(context);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { crate::candidates::pinyin_train(instance, 0) });
    unsafe { pinyin_free_instance(instance) };
}

/// An absent library at init prints `mmap … failed!` and the init still
/// answers a context.
#[test]
fn init_reports_an_absent_library_and_succeeds() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::init_and_save::init_reports_an_absent_library_and_succeeds",
        |user| {
            let system = private_system(user.parent().expect("the child's scratch"));
            std::fs::remove_file(system.join("merged.bin")).expect("unlink merged.bin");
            assert!(!init_at(&system, user).is_null());
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    let line = format!("mmap {} failed!{NL}", system.join("merged.bin").display());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains(&line), "{stderr}");
}

/// A reload of an empty library answers true, prints `mmap … failed!` and
/// leaves the library unloaded.
#[test]
fn reload_of_an_empty_library_reports_and_answers_true() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::init_and_save::reload_of_an_empty_library_reports_and_answers_true",
        |user| {
            let (system, context) = private_context(user);
            assert!(unsafe { crate::config::pinyin_unload_phrase_library(context, 2) });
            let library = system.join("gbk_char.bin");
            std::fs::remove_file(&library).expect("unlink gbk_char.bin");
            std::fs::write(&library, b"").expect("an empty library");
            assert!(unsafe { pinyin_load_phrase_library(context, 2) });
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    let line = format!("mmap {} failed!{NL}", system.join("gbk_char.bin").display());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains(&line), "{stderr}");
}

/// A save whose system library is gone prints `mmap … failed!` and
/// answers false (register row 76's outcome).
#[test]
fn save_reports_an_absent_system_library_and_answers_false() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::init_and_save::save_reports_an_absent_system_library_and_answers_false",
        |user| {
            let (system, context) = private_context(user);
            train_once(context);
            std::fs::remove_file(system.join("gb_char.bin")).expect("unlink gb_char.bin");
            assert!(!unsafe { pinyin_save(context) });
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    let line = format!("mmap {} failed!{NL}", system.join("gb_char.bin").display());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains(&line), "{stderr}");
}

/// A save cut by a four-byte file-size limit aborts after the chunk's
/// length word: the staged log holds four bytes and the
/// marker keeps the raised counter.
// Unix only: RLIMIT_FSIZE and SIGXFSZ.
#[cfg(unix)]
#[test]
fn save_aborts_where_the_chunk_header_write_fails() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::init_and_save::save_aborts_where_the_chunk_header_write_fails",
        |user| {
            let context = init_at(&system_dir(), user);
            assert!(!context.is_null());
            train_once(context);
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
    let newline = oxpinyin_input::TEXT_NEWLINE;
    assert!(
        conf.ends_with(&format!("open counter:1{newline}")),
        "{conf}"
    );
}

/// Init refuses a NULL system directory, a missing one and one without
/// table.conf: each answers NULL after the two `open`/`load` lines.
#[test]
fn init_refuses_a_system_directory_without_table_conf() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::init_and_save::init_refuses_a_system_directory_without_table_conf",
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

/// A NULL user directory prints the empty-path open line at init and the
/// two write lines at fini, and nothing else.
#[test]
fn a_null_user_directory_reports_its_empty_path() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::init_and_save::a_null_user_directory_reports_its_empty_path",
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

/// A private system whose table.conf has `from` replaced by `to`.
fn conf_system(parent: &Path, from: &str, to: &str) -> std::path::PathBuf {
    let system = private_system(parent);
    let path = system.join("table.conf");
    let text = std::fs::read_to_string(system_dir().join("table.conf")).expect("table.conf");
    assert!(text.contains(from), "table.conf holds {from}");
    std::fs::remove_file(&path).expect("unlink the linked table.conf");
    std::fs::write(&path, text.replace(from, to)).expect("write table.conf");
    system
}

/// An unknown database format in table.conf aborts the init before it
/// writes the user directory.
#[test]
fn init_aborts_on_an_unknown_table_conf_database_format() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::init_and_save::init_aborts_on_an_unknown_table_conf_database_format",
        |user| {
            let format = format!("database format:{}", oxpinyin_data::DEFAULT_STORE_DB_FORMAT);
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

/// An unknown database format in user.conf aborts the init and leaves
/// user.conf as it was.
#[test]
fn init_aborts_on_an_unknown_user_conf_database_format() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::init_and_save::init_aborts_on_an_unknown_user_conf_database_format",
        |user| {
            write_user_conf(user, &system_dir());
            let path = user.join("user.conf");
            let text = std::fs::read_to_string(&path).expect("user.conf");
            let format = format!("database format:{}", oxpinyin_data::DEFAULT_STORE_DB_FORMAT);
            std::fs::write(&path, text.replace(&format, "database format:LMDB"))
                .expect("write user.conf");
            init_at(&system_dir(), user);
        },
    );
    let conf = std::fs::read_to_string(run.user_dir.join("user.conf")).expect("user.conf");
    assert!(conf.contains("database format:LMDB\n"), "{conf}");
    assert!(conf.ends_with("open counter:0\n"), "{conf}");
}

/// An absolute USER_FILE name stays under the user directory:
/// `g_build_filename(user, "/…/nest/u.bin")` is `<user>/…/nest/u.bin`.
// Unix only: the table.conf row spells a POSIX absolute path.
#[cfg(unix)]
#[test]
fn an_absolute_user_file_lands_under_the_user_directory() {
    let scratch = tempfile::tempdir().expect("scratch");
    let user = scratch.path().join("user");
    let nested = format!("{}/nest", user.display());
    let beneath = format!("{}{nested}", user.display());
    std::fs::create_dir_all(&nested).expect("the nested directory");
    std::fs::create_dir_all(&beneath).expect("the user-rooted directory");
    let system = conf_system(
        scratch.path(),
        "user.bin USER_FILE",
        &format!("{nested}/u.bin USER_FILE"),
    );
    let context = init_at(&system, &user);
    assert!(!context.is_null());
    assert_eq!(import_batch(context, 7, &[("你好", "ni3'hao3", 1)]), [true]);
    assert!(unsafe { pinyin_save(context) });
    unsafe { pinyin_fini(context) };
    assert!(Path::new(&beneath).join("u.bin").is_file());
    assert!(!Path::new(&nested).join("u.bin").exists());
}

/// Copies the regular files of `user` to `before`, next to it.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
fn keep_before(user: &Path) {
    let before = user.parent().expect("the child's scratch").join("before");
    std::fs::create_dir(&before).expect("the before directory");
    for (name, bytes) in files_of(user) {
        std::fs::write(before.join(name), bytes).expect("keep a file");
    }
}

/// Two whole user-bigram rows: two predecessors, each with an empty gram.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
fn two_bigram_rows() -> Vec<(Vec<u8>, Vec<u8>)> {
    vec![
        (0x0100_0001_u32.to_le_bytes().to_vec(), vec![0; 4]),
        (0x0100_0002_u32.to_le_bytes().to_vec(), vec![0; 4]),
    ]
}

/// The pin's `copy_tkrzwdb` and `copy_bdb` assert on a source walk that
/// fails (`tkrzwdb_utils.h:72`, `bdb_utils.h:67`): init stops in the copy
/// after one record, with the raised open counter already written
/// (`pinyin.cpp:172-218` runs before the loads) and the profile unchanged.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn init_aborts_where_a_copy_fails_a_source_read() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::init_and_save::init_aborts_where_a_copy_fails_a_source_read",
        |user| {
            plant_bigram(user, two_bigram_rows());
            keep_before(user);
            oxpinyin_store::copy_faults::fail_source_read_after(1);
            init_at(&system_dir(), user);
        },
    );
    let before = files_of(&run.user_dir.parent().expect("scratch").join("before"));
    let after = files_of(&run.user_dir);
    let conf = String::from_utf8_lossy(&after["user.conf"]).into_owned();
    let newline = oxpinyin_input::TEXT_NEWLINE;
    assert!(
        conf.ends_with(&format!("open counter:2{newline}")),
        "{conf}"
    );
    assert_eq!(after["user_bigram.db"], before["user_bigram.db"]);
}

/// The same stop in `PhraseLargeTable3::load_db`'s copy
/// (`phrase_large_table3_tkrzwdb.cpp:95`, `phrase_large_table3_bdb.cpp:141`):
/// a profile whose only rows are two user phrase-index rows stops after the
/// first, with the profile unchanged.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn init_aborts_where_a_user_index_copy_fails_a_source_read() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::init_and_save::init_aborts_where_a_user_index_copy_fails_a_source_read",
        |user| {
            set_phrase_table_row(user, "你好", &[0x0100_0001]);
            set_phrase_table_row(user, "再见", &[0x0100_0002]);
            keep_before(user);
            oxpinyin_store::copy_faults::fail_source_read_after(1);
            init_at(&system_dir(), user);
        },
    );
    let before = files_of(&run.user_dir.parent().expect("scratch").join("before"));
    let after = files_of(&run.user_dir);
    assert_eq!(
        after["user_phrase_index.bin"],
        before["user_phrase_index.bin"]
    );
}

/// The control: a user bigram that does not open is the pin's soft
/// `return false` (`ngram_tkrzwdb.cpp:55-56`, `ngram_bdb.cpp:67-70`), so no
/// copy runs and init answers a context, though every source read fails.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn init_survives_a_user_bigram_that_does_not_open() {
    crate::test_support::run_in_child(
        "boundary_tests::init_and_save::init_survives_a_user_bigram_that_does_not_open",
        |user| {
            plant_bigram(user, two_bigram_rows());
            let bigram = user.join("user_bigram.db");
            std::fs::remove_file(&bigram).expect("unlink the user bigram");
            std::fs::create_dir(&bigram).expect("a directory in its place");
            oxpinyin_store::copy_faults::fail_source_read_after(0);
            assert!(!init_at(&system_dir(), user).is_null());
        },
    );
}

/// `copy_tkrzwdb` and `copy_bdb` return `false` without an assert when no
/// source iterator is made (`tkrzwdb_utils.h:52-54`, `bdb_utils.h:49-52`),
/// and `load_db` ignores the answer (`ngram_tkrzwdb.cpp:58`,
/// `chewing_large_table2_tkrzwdb.cpp:104`): init answers a context, though
/// every source read of a copy would fail.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn init_survives_a_copy_whose_source_iterator_is_not_made() {
    crate::test_support::run_in_child(
        "boundary_tests::init_and_save::init_survives_a_copy_whose_source_iterator_is_not_made",
        |user| {
            plant_bigram(user, two_bigram_rows());
            oxpinyin_store::copy_faults::fail_iterator_creation_after(0);
            oxpinyin_store::copy_faults::fail_source_read_after(0);
            assert!(!init_at(&system_dir(), user).is_null());
        },
    );
}

/// The pin's copies assert on a failed destination write
/// (`tkrzwdb_utils.h:65`, `bdb_utils.h:61`): the save stops in
/// `_write_files` after one record, before `_rename_files` and
/// `mark_version` (`pinyin.cpp:1141-1143`), so no saved file is replaced.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn save_aborts_where_a_copy_fails_a_destination_write() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::init_and_save::save_aborts_where_a_copy_fails_a_destination_write",
        |user| {
            plant_bigram(user, two_bigram_rows());
            let context = init_at(&system_dir(), user);
            assert!(!context.is_null());
            // `pinyin_save` saves a modified context only (`pinyin.cpp:1136`).
            train_once(context);
            assert!(unsafe { pinyin_save(context) });
            keep_before(user);
            train_once(context);
            // The first copy writes one record and fails the next.
            oxpinyin_store::copy_faults::fail_destination_write_after(1);
            unsafe { pinyin_save(context) };
        },
    );
    let before = files_of(&run.user_dir.parent().expect("scratch").join("before"));
    let after = files_of(&run.user_dir);
    for (name, bytes) in &before {
        assert_eq!(after.get(name), Some(bytes), "{name} was replaced");
    }
    assert!(
        after.keys().any(|name| name.ends_with(".tmp")),
        "the copy left no staged file: {:?}",
        after.keys()
    );
}
