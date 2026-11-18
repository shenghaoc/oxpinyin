//! Init and save: reports, refusals, conf formats and copy faults.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{drain_phrases, init_fixture_at};

#[cfg(unix)]
use crate::support::PinyinContext;
#[cfg(unix)]
use crate::support::pinyin_free_instance;
#[cfg(unix)]
use crate::support::pinyin_guess_sentence;

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

#[cfg(unix)]
/// Trains the top sentence of `nihao` once on a new instance.
fn train_once(context: *mut PinyinContext) {
    let instance = alloc(context);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { crate::support::pinyin_train(instance, 0) });
    unsafe { pinyin_free_instance(instance) };
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
                oxpinyin::user::data::DEFAULT_STORE_DB_FORMAT
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
                oxpinyin::user::data::DEFAULT_STORE_DB_FORMAT
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

/// The `open <user dir>/user.conf failed.` line a fresh user directory
/// makes `pinyin_init` write: `UserTableInfo::load` prints it when the
/// marker is missing, with no trailing newline (`table_info.cpp:332` at
/// 074a2219).
fn fresh_user_conf_line(user_dir: &std::path::Path) -> String {
    // `g_build_filename` joins with the host's separator.
    format!(
        "open {}{}user.conf failed.",
        user_dir.display(),
        std::path::MAIN_SEPARATOR
    )
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
            oxpinyin::user::USER_DICTIONARY,
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
        drain_phrases(export).is_empty(),
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

/// Register row 91: init's library loop merges `gb_char.dbin` and
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

/// A second `pinyin_fini` on one context (#573, register row 105): the pin
/// deletes the context twice (`pinyin.cpp:1194-1222`) and dies of SIGSEGV;
/// the ruling is a silent return. The init's own `user.conf` line is the
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
