//! Init and save: reports, refusals, conf formats and copy faults.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::fresh_user_conf_line;

#[cfg(unix)]
use crate::support::directory_reads_as_a_chunk_here;
#[cfg(unix)]
use crate::support::map_a_directory_library;

use crate::support::init_at;

use crate::support::LogCapture;
use crate::support::TempUserDir;
use crate::support::abort_in_child;
use crate::support::assert_aborts;
use crate::support::close;
use crate::support::cstr;
use crate::support::init_on;
use crate::support::open;
use crate::support::open_with_user;
use crate::support::private_system;
use crate::support::run_in_child;
use crate::support::system_dir;
use crate::support::write_user_conf;
use crate::support::zhuyin_begin_add_phrases;
use crate::support::zhuyin_end_add_phrases;
use crate::support::zhuyin_fini;
use crate::support::zhuyin_free_instance;
use crate::support::zhuyin_init;
use crate::support::zhuyin_load_phrase_library;
use crate::support::zhuyin_save;

#[cfg(unix)]
use crate::support::zhuyin_guess_sentence;
#[cfg(unix)]
use crate::support::zhuyin_parse_more_chewings;
#[cfg(unix)]
use crate::support::zhuyin_train;

use oxpinyin::USER_DICTIONARY;

use std::path::Path;

use std::ptr;

/// The GLib level of a warning.
const WARNING: u32 = 16;

/// Replaces the `database format:` token of a `table.conf` or `user.conf`
/// text with `LMDB`.
fn unknown_format(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.starts_with("database format:") {
                "database format:LMDB".to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// An empty `merged.bin` refuses the init: the mmap line goes to stderr and
/// libzhuyin logs one warning (the pin crashes here, SIGSEGV).
#[test]
fn an_empty_library_refuses_the_init_with_the_mmap_line() {
    let run = run_in_child(
        "init_and_save::an_empty_library_refuses_the_init_with_the_mmap_line",
        |user| {
            let system = private_system(user.parent().expect("scratch"));
            std::fs::remove_file(system.join("merged.bin")).expect("unlink the library");
            std::fs::write(system.join("merged.bin"), b"").expect("an empty library");
            let logs = LogCapture::new();
            assert!(init_on(&system, Some(user)).is_null());
            assert_eq!(logs.take(), [(Some("libzhuyin".to_owned()), WARNING)]);
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    let line = format!(
        "mmap {} failed!{}",
        system.join("merged.bin").display(),
        if cfg!(windows) { "\r\n" } else { "\n" }
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.ends_with(&line), "{stderr}");
}

/// A save that the file-size limit cuts after the chunk's length word aborts
/// on the checksum word (`memory_chunk.h:547`). Unix only: `RLIMIT_FSIZE`.
#[cfg(unix)]
#[test]
fn a_save_cut_after_the_chunk_length_word_aborts() {
    let run = abort_in_child(
        "init_and_save::a_save_cut_after_the_chunk_length_word_aborts",
        |user| {
            let (context, instance) = open_with_user(user);
            unsafe { zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()) };
            assert!(unsafe { zhuyin_guess_sentence(instance) });
            assert!(unsafe { zhuyin_train(instance) });
            let limit = libc::rlimit {
                rlim_cur: 4,
                rlim_max: 4,
            };
            // SAFETY: ignores SIGXFSZ and lowers this child's own file-size limit.
            unsafe {
                libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &raw const limit), 0);
            }
            unsafe { zhuyin_save(context) };
        },
    );
    let tmp = run.user_dir.join("gb_char.dbin.tmp");
    assert_eq!(std::fs::metadata(&tmp).expect("the cut tmp file").len(), 4);
}

/// Each init refusal writes the pin's two stderr lines and answers NULL; an
/// init with a NULL user directory opens with the one `open  failed.` line
/// and no marker line.
#[test]
fn init_refusals_write_the_open_and_load_lines() {
    let run = run_in_child(
        "init_and_save::init_refusals_write_the_open_and_load_lines",
        |user| {
            let context = init_on(&system_dir(), None);
            assert!(!context.is_null());
            unsafe { zhuyin_fini(context) };
            let user_c = cstr(user.to_str().expect("UTF-8 path"));
            assert!(unsafe { zhuyin_init(ptr::null(), user_c.as_ptr()) }.is_null());
            assert!(init_on(Path::new("/nonexistent-system-dir"), Some(user)).is_null());
            let system = private_system(user.parent().expect("scratch"));
            std::fs::remove_file(system.join("table.conf")).expect("unlink table.conf");
            assert!(init_on(&system, Some(user)).is_null());
        },
    );
    let nl = if cfg!(windows) { "\r\n" } else { "\n" };
    let conf = run
        .user_dir
        .parent()
        .expect("scratch")
        .join("system")
        .join("table.conf");
    let conf = conf.display();
    let expected = format!(
        "open  failed.open  failed.{nl}load  failed!{nl}\
         open /nonexistent-system-dir/table.conf failed.{nl}\
         load /nonexistent-system-dir/table.conf failed!{nl}\
         open {conf} failed.{nl}load {conf} failed!{nl}"
    );
    assert_eq!(String::from_utf8_lossy(&run.stderr), expected);
}

/// Library 0 is the reserved row, neither `SYSTEM_FILE` nor `USER_FILE`:
/// the load asserts (`zhuyin.cpp:372`).
#[test]
fn loading_library_zero_aborts() {
    assert_aborts("init_and_save::loading_library_zero_aborts", || {
        let (context, _instance) = open();
        unsafe { zhuyin_load_phrase_library(context, 0) };
    });
}

/// An unknown `database format:` in `table.conf` aborts the init
/// (`table_info.cpp:132`) before any user file is written.
#[test]
fn an_unknown_table_database_format_aborts_before_any_user_file() {
    let run = abort_in_child(
        "init_and_save::an_unknown_table_database_format_aborts_before_any_user_file",
        |user| {
            let system = private_system(user.parent().expect("scratch"));
            let conf = system.join("table.conf");
            let text = std::fs::read_to_string(&conf).expect("table.conf");
            std::fs::remove_file(&conf).expect("unlink table.conf");
            std::fs::write(&conf, unknown_format(&text)).expect("table.conf");
            init_on(&system, Some(user));
        },
    );
    let left: Vec<_> = std::fs::read_dir(&run.user_dir)
        .expect("user dir")
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// An unknown `database format:` in the user's `user.conf` aborts the init
/// (`zhuyin.cpp:126-132`), and the file stays as it was.
#[test]
fn an_unknown_user_database_format_aborts_and_keeps_user_conf() {
    let run = abort_in_child(
        "init_and_save::an_unknown_user_database_format_aborts_and_keeps_user_conf",
        |user| {
            write_user_conf(user, &system_dir());
            let path = user.join("user.conf");
            let text = std::fs::read_to_string(&path).expect("user.conf");
            std::fs::write(&path, unknown_format(&text)).expect("user.conf");
            init_on(&system_dir(), Some(user));
        },
    );
    let text = std::fs::read_to_string(run.user_dir.join("user.conf")).expect("user.conf");
    assert!(text.contains("database format:LMDB\n"), "{text}");
    assert!(text.ends_with("open counter:0\n"), "{text}");
}

// ---------------------------------------------------------------------
// Ports of contract-diff cases whose pin side reads freed or
// heap-dependent memory (undefined behaviour), so no frozen pin
// reference can hold them. Each asserts oxpinyin's defined behaviour;
// the expectations come from the pin's source at 074a2219 and the
// project's rulings, not from running the pin.
// ---------------------------------------------------------------------

// Windows only: the C API's UTF-8 paths reach the wide-character (UTF-16) file API.
#[cfg(windows)]
#[test]
fn utf8_c_api_paths_open_cjk_directories_and_files() {
    let root = tempfile::tempdir().unwrap();
    let system = root.path().join("系统");
    let user = root.path().join("用户");
    std::fs::create_dir(&system).unwrap();
    std::fs::create_dir(&user).unwrap();
    for entry in std::fs::read_dir(crate::support::system_dir()).unwrap() {
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
    let system = crate::support::cstr(system.to_str().unwrap());
    let user_name = crate::support::cstr(user.to_str().unwrap());
    let context = unsafe { crate::support::zhuyin_init(system.as_ptr(), user_name.as_ptr()) };
    assert!(!context.is_null());
    // End arms the writer without changing the phrase data.
    unsafe { zhuyin_end_add_phrases(zhuyin_begin_add_phrases(context, USER_DICTIONARY)) };
    assert!(unsafe { zhuyin_save(context) });
    unsafe { crate::support::zhuyin_fini(context) };
    assert!(user.join("user.conf").is_file());
}

/// #523's libzhuyin half: `zhuyin_init` only reads `user.conf` and
/// `zhuyin_fini` writes nothing (`zhuyin.cpp:126-162`, `:741-757`); the
/// one write is `zhuyin_save`'s `mark_version`, whose fresh
/// `UserTableInfo` carries a counter of 0 (`:164-176`, `:695`). No run of
/// launches can raise the counter toward the limit, so none can wipe.
#[test]
fn zhuyin_launches_never_move_the_open_counter() {
    let user_dir = TempUserDir::new("zhuyin-open-counter");
    let marker = user_dir.path.join("user.conf");

    let (context, instance) = open_with_user(&user_dir.path);
    assert!(!marker.exists(), "zhuyin_init wrote user.conf");
    // End arms m_modified with or without an add (zhuyin.cpp:543).
    unsafe { zhuyin_end_add_phrases(zhuyin_begin_add_phrases(context, USER_DICTIONARY)) };
    assert!(unsafe { zhuyin_save(context) });
    let saved = std::fs::read(&marker).expect("zhuyin_save writes user.conf");
    let newline = oxpinyin::input::TEXT_NEWLINE;
    assert!(
        String::from_utf8_lossy(&saved).ends_with(&format!("open counter:0{newline}")),
        "zhuyin_save's marker carries a fresh counter"
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    assert_eq!(
        std::fs::read(&marker).expect("marker"),
        saved,
        "zhuyin_fini wrote user.conf"
    );

    for _ in 0..10 {
        let (context, instance) = open_with_user(&user_dir.path);
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
    }
    assert_eq!(std::fs::read(&marker).expect("marker"), saved);
}

/// Register row 91 on the reload path: `zhuyin_load_phrase_library` of an
/// unloaded library merges its saved `.dbin` (`zhuyin.cpp:219`).
#[test]
fn reload_aborts_on_a_log_header_token() {
    crate::support::assert_aborts("init_and_save::reload_aborts_on_a_log_header_token", || {
        let user_dir = TempUserDir::new("reload-header-token");
        crate::support::plant_log(&user_dir.path, "gbk_char.dbin", &[]);
        let context = init_at(&user_dir.path);
        assert!(!context.is_null());
        assert!(unsafe { crate::support::zhuyin_unload_phrase_library(context, 2) });
        crate::support::plant_log(
            &user_dir.path,
            "gbk_char.dbin",
            &crate::support::logs::header_token(),
        );
        let _ = unsafe { crate::support::zhuyin_load_phrase_library(context, 2) };
    });
}

/// Register row 96 at the reload of an unloaded library.
// Unix only, for the reason `oxpinyin`'s `save_stops_at_the_directory_with_the_pins_files` gives.
#[cfg(unix)]
#[test]
fn reload_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::support::assert_aborts(
            "init_and_save::reload_aborts_on_a_directory_library",
            || {
                let _ = map_a_directory_library("reload");
            },
        );
    } else {
        assert!(map_a_directory_library("reload"));
    }
}

/// A second `zhuyin_fini` on one context (#573, register row 105): the pin
/// deletes the context twice (`zhuyin.cpp:741-757`) and dies of SIGSEGV;
/// the ruling is a silent return.
#[test]
fn fini_twice_returns_silently() {
    let run = crate::support::run_in_child("init_and_save::fini_twice_returns_silently", |user| {
        let (context, instance) = open_with_user(user);
        unsafe { zhuyin_free_instance(instance) };
        unsafe { zhuyin_fini(context) };
        unsafe { zhuyin_fini(context) };
    });
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        fresh_user_conf_line(&run.user_dir)
    );
}
