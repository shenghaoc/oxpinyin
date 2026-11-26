//! Init and save: reports, refusals, conf formats and copy faults.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::fresh_user_conf_line;

use crate::support::LogCapture;
use crate::support::TempUserDir;
use crate::support::abort_in_child;
use crate::support::close;
use crate::support::cstr;
use crate::support::init_on;
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
use crate::support::zhuyin_save;

#[cfg(unix)]
use crate::support::zhuyin_guess_sentence;
#[cfg(unix)]
use crate::support::zhuyin_parse_more_chewings;
#[cfg(unix)]
use crate::support::zhuyin_train;

use oxpinyin::vocab::USER_DICTIONARY;

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

/// libzhuyin opens the directories it is given by their bytes (`g_strdup`
/// and `g_build_filename`, `zhuyin.cpp:275-280`), so a name that is not
/// UTF-8 is a directory like any other, for the system tables and for the
/// user files alike: the session guesses, trains and saves there.
// Unix only: byte-string directory names (macOS refuses them, which the test asserts).
#[cfg(unix)]
#[test]
fn directories_with_non_utf8_names_open() {
    use std::ffi::{CString, OsStr};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let scratch_dir = tempfile::tempdir().expect("scratch");
    let scratch = scratch_dir.path();
    let named = |prefix: &str| {
        let mut name = prefix.as_bytes().to_vec();
        name.extend_from_slice(b"\xff");
        scratch.join(OsStr::from_bytes(&name))
    };
    let system = named("oxzhuyin-system-");
    let user = named("oxzhuyin-");
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
    let context = unsafe { zhuyin_init(system_c.as_ptr(), user_c.as_ptr()) };
    assert!(!context.is_null(), "a non-UTF-8 name is a directory");
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { crate::support::chewing(context, "su3cl3") };
    assert!(unsafe { crate::support::zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_train(instance) });
    assert!(unsafe { crate::support::zhuyin_save(context) });
    assert!(
        user.join("user.conf").is_file(),
        "the save wrote the user directory it was given"
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A default row of type `DICTIONARY` aborts the init in its library loop
/// (`assert(DICTIONARY != table_info->m_file_type)`, `zhuyin.cpp:330`).
/// libzhuyin's `check_format` saves no `user.conf` (`zhuyin.cpp:126-162`),
/// so the abort leaves none.
#[test]
fn a_default_dictionary_row_aborts_the_init_with_no_user_conf() {
    let run = abort_in_child(
        "init_and_save::a_default_dictionary_row_aborts_the_init_with_no_user_conf",
        |user| {
            let system = private_system(user.parent().expect("scratch"));
            let conf = system.join("table.conf");
            let text = std::fs::read_to_string(&conf).expect("table.conf");
            let row = "merged.bin merged.dbin SYSTEM_FILE";
            assert!(text.contains(row));
            std::fs::remove_file(&conf).expect("unlink table.conf");
            std::fs::write(
                &conf,
                text.replace(row, "merged.bin merged.dbin DICTIONARY"),
            )
            .expect("table.conf");
            init_on(&system, Some(user));
        },
    );
    assert!(!run.user_dir.join("user.conf").exists());
}

/// The init joins the system name and `table.conf` with `g_build_filename`
/// (`zhuyin.cpp:279-280`), which drops an empty element: the empty name is
/// `table.conf` in the working directory, and `.` is `./table.conf`. Without
/// that file the load writes its line and the init writes its own and
/// answers NULL (`table_info.cpp:199-203`, `zhuyin.cpp:281-284`). With it,
/// the libraries open from the same directory.
#[test]
fn an_empty_or_dot_system_name_reads_the_working_directory() {
    let run = run_in_child(
        "init_and_save::an_empty_or_dot_system_name_reads_the_working_directory",
        |user| {
            write_user_conf(user, &system_dir());
            let user_c = cstr(user.to_str().expect("UTF-8 path"));
            let bare = user.parent().expect("scratch").join("bare");
            std::fs::create_dir(&bare).expect("a directory with no table.conf");
            std::env::set_current_dir(&bare).expect("enter it");
            assert!(unsafe { zhuyin_init(c"".as_ptr(), user_c.as_ptr()) }.is_null());
            std::env::set_current_dir(system_dir()).expect("enter the fixture");
            let context = unsafe { zhuyin_init(c".".as_ptr(), user_c.as_ptr()) };
            assert!(!context.is_null());
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls
            // into it.
            let instance = unsafe { crate::support::chewing(context, "su3cl3") };
            assert!(unsafe { crate::support::zhuyin_guess_sentence(instance) });
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

/// A reload and a mask-out over a system library that cannot be mapped. Each writes the pin's `mmap %s failed!` line where the
/// pin then dies on the empty chunk. The reload answers `true`
/// and leaves the library unloaded, so the next load maps it
/// (`zhuyin.cpp:186`, `:198-201`). The mask-out answers `true`
/// (`:795-800`).
#[test]
fn a_reload_and_a_mask_out_report_a_library_that_cannot_be_mapped() {
    let run = run_in_child(
        "init_and_save::a_reload_and_a_mask_out_report_a_library_that_cannot_be_mapped",
        |user| {
            let system = private_system(user.parent().expect("scratch"));
            write_user_conf(user, &system);
            let context = init_on(&system, Some(user));
            assert!(!context.is_null());
            let library = system.join("gbk_char.bin");
            let away = system.join("gbk_char.bin.away");
            assert!(unsafe { crate::support::zhuyin_unload_phrase_library(context, 2) });
            std::fs::rename(&library, &away).expect("move the library away");
            std::fs::write(&library, b"").expect("an empty library");
            assert!(unsafe { crate::support::zhuyin_load_phrase_library(context, 2) });
            std::fs::remove_file(&library).expect("remove the empty library");
            std::fs::rename(&away, &library).expect("move the library back");
            assert!(unsafe { crate::support::zhuyin_load_phrase_library(context, 2) });
            assert!(
                !unsafe { crate::support::zhuyin_load_phrase_library(context, 2) },
                "the library is loaded now"
            );
            std::fs::rename(&library, &away).expect("move the library away");
            std::fs::write(&library, b"").expect("an empty library");
            assert!(unsafe { crate::support::zhuyin_mask_out(context, 0xff00_0000, 0x0700_0000) });
            unsafe { zhuyin_fini(context) };
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    let line = format!("mmap {} failed!{NL}", system.join("gbk_char.bin").display());
    assert_eq!(String::from_utf8_lossy(&run.stderr), line.repeat(2));
}

/// A save that cannot map a loaded system library again writes the pin's
/// line and answers `false`. libzhuyin renames each library's
/// log as it writes it (`zhuyin.cpp:586-615`), so the logs of the libraries
/// before the unmappable one are already in place; nothing later is written,
/// `user.conf` included (`:695`). The context stays modified, and the next
/// save, with the library back, writes the whole profile.
#[test]
fn a_save_stops_at_an_unmappable_library_after_the_earlier_renames() {
    let run = run_in_child(
        "init_and_save::a_save_stops_at_an_unmappable_library_after_the_earlier_renames",
        |user| {
            let system = private_system(user.parent().expect("scratch"));
            let context = init_on(&system, Some(user));
            assert!(!context.is_null());
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls
            // into it.
            let instance = unsafe { crate::support::chewing(context, "su3cl3") };
            assert!(unsafe { crate::support::zhuyin_guess_sentence(instance) });
            assert!(unsafe { crate::support::zhuyin_train(instance) });
            let library = system.join("opengram.bin");
            let away = system.join("opengram.bin.away");
            std::fs::rename(&library, &away).expect("move the library away");
            assert!(!unsafe { crate::support::zhuyin_save(context) });
            assert_eq!(
                crate::support::names(user),
                ["gb_char.dbin", "gbk_char.dbin"]
            );
            std::fs::rename(&away, &library).expect("move the library back");
            assert!(unsafe { crate::support::zhuyin_save(context) });
            let saved = crate::support::names(user);
            for name in ["opengram.dbin", "merged.dbin", "user.conf"] {
                assert!(saved.iter().any(|found| found == name), "{name}: {saved:?}");
            }
            // SAFETY: `context` was opened above and is not yet freed; `instance` was opened
            // above and is not yet freed; nothing uses either handle after this call; only this
            // thread calls into them.
            unsafe { close(context, instance) };
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        format!(
            "{}mmap {} failed!{NL}",
            fresh_user_conf_line(&run.user_dir),
            system.join("opengram.bin").display()
        )
    );
}

/// The host's newline for a `\n` that C stdio writes to stderr.
const NL: &str = if cfg!(windows) { "\r\n" } else { "\n" };

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
    let (_root, system, user) = crate::support::cjk_system_and_user();
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
    let newline = oxpinyin::__internal::TEXT_NEWLINE;
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

/// A second `zhuyin_fini` on one context: the pin
/// deletes the context twice (`zhuyin.cpp:741-757`) and dies of SIGSEGV;
/// the call returns silently.
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
