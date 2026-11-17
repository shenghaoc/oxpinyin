//! Init and save: reports, refusals, conf formats and copy faults.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// The GLib level of a warning.
const WARNING: u32 = 16;

/// An empty `merged.bin` refuses the init: the mmap line goes to stderr and
/// libzhuyin logs one warning (the pin crashes here, SIGSEGV).
#[test]
fn an_empty_library_refuses_the_init_with_the_mmap_line() {
    let run = run_in_child(
        "boundary_tests::init_and_save::an_empty_library_refuses_the_init_with_the_mmap_line",
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
        "boundary_tests::init_and_save::a_save_cut_after_the_chunk_length_word_aborts",
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
        "boundary_tests::init_and_save::init_refusals_write_the_open_and_load_lines",
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
    assert_aborts(
        "boundary_tests::init_and_save::loading_library_zero_aborts",
        || {
            let (context, _instance) = open();
            unsafe { zhuyin_load_phrase_library(context, 0) };
        },
    );
}

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

/// An unknown `database format:` in `table.conf` aborts the init
/// (`table_info.cpp:132`) before any user file is written.
#[test]
fn an_unknown_table_database_format_aborts_before_any_user_file() {
    let run = abort_in_child(
        "boundary_tests::init_and_save::an_unknown_table_database_format_aborts_before_any_user_file",
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
        "boundary_tests::init_and_save::an_unknown_user_database_format_aborts_and_keeps_user_conf",
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

/// A stale `user_phrase_index.bin.tmp` directory: the save renames it over
/// an absent or empty-directory index; over a live file the rename fails, the
/// tmp stays and the live file is kept, with one stderr line (#542). On
/// Windows `g_rename` cannot replace a directory with a directory, so the
/// empty-directory round keeps its tmp too.
#[test]
fn a_stale_index_tmp_directory_is_renamed_unless_a_live_file_holds_the_name() {
    let run = run_in_child(
        "boundary_tests::init_and_save::a_stale_index_tmp_directory_is_renamed_unless_a_live_file_holds_the_name",
        |root| {
            let marker: &[u8] = b"unchanged temporary directory";
            let empty_directory_moves = cfg!(not(windows));
            for (round, (destination, moved)) in [
                ("absent", true),
                ("empty-directory", empty_directory_moves),
                ("live-file", false),
            ]
            .into_iter()
            .enumerate()
            {
                let user = root.join(format!("round-{round}"));
                std::fs::create_dir(&user).expect("a fresh user directory");
                let (context, instance) = open_with_user(&user);
                let train = |instance| {
                    unsafe { zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()) };
                    assert!(unsafe { zhuyin_guess_sentence(instance) });
                    assert!(unsafe { zhuyin_train(instance) });
                };
                train(instance);
                let live = user.join("user_phrase_index.bin");
                let mut before = None;
                if destination == "live-file" {
                    assert!(unsafe { zhuyin_save(context) });
                    before = Some(std::fs::read(&live).expect("the live index"));
                    train(instance);
                } else if destination == "empty-directory" {
                    std::fs::create_dir(&live).expect("an empty destination directory");
                }
                let tmp = user.join("user_phrase_index.bin.tmp");
                std::fs::create_dir(&tmp).expect("the stale temporary directory");
                std::fs::write(tmp.join("stale-marker"), marker).expect("the stale marker");
                assert!(unsafe { zhuyin_save(context) }, "{destination}");
                assert_eq!(tmp.exists(), !moved, "{destination}");
                assert_eq!(
                    std::fs::read(live.join("stale-marker")).ok().as_deref(),
                    moved.then_some(marker),
                    "{destination}"
                );
                if let Some(before) = before {
                    assert_eq!(std::fs::read(&live).expect("the live index"), before);
                }
                close(context, instance);
            }
        },
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    let renames = stderr.matches("user_phrase_index.bin failed.").count();
    assert_eq!(renames, if cfg!(windows) { 2 } else { 1 }, "{stderr}");
}

/// The regular files of `dir` by name, with their bytes.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
fn files_of(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .expect("read the directory")
        .map(|entry| {
            let entry = entry.expect("an entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, std::fs::read(entry.path()).expect("read a file"))
        })
        .collect()
}

/// Trains one sentence on `context`, so that the next save writes.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
fn train_once(context: *mut ZhuyinContext) {
    let instance = unsafe { zhuyin_alloc_instance(context) };
    assert!(!instance.is_null());
    unsafe { zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()) };
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_train(instance) });
    unsafe { zhuyin_free_instance(instance) };
}

/// The pin's copies assert on a failed destination write
/// (`tkrzwdb_utils.h:65`, `bdb_utils.h:61`). `zhuyin_save` renames each
/// file after its copy (`zhuyin.cpp:646-691`), so the save stops with the
/// earlier files renamed, the failed file still staged and its saved file
/// unchanged, and `user.conf` not marked again.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn save_aborts_where_a_copy_fails_a_destination_write() {
    let run = abort_in_child(
        "boundary_tests::init_and_save::save_aborts_where_a_copy_fails_a_destination_write",
        |user| {
            plant_bigram(
                user,
                vec![
                    (0x0100_0001_u32.to_le_bytes().to_vec(), vec![0; 4]),
                    (0x0100_0002_u32.to_le_bytes().to_vec(), vec![0; 4]),
                ],
            );
            let (context, _instance) = open_with_user(user);
            train_once(context);
            assert!(unsafe { zhuyin_save(context) });
            let before = user.parent().expect("the child's scratch").join("before");
            std::fs::create_dir(&before).expect("the before directory");
            for (name, bytes) in files_of(user) {
                std::fs::write(before.join(name), bytes).expect("keep a file");
            }
            train_once(context);
            // The first copy writes one record and fails the next.
            oxpinyin_store::copy_faults::fail_destination_write_after(1);
            unsafe { zhuyin_save(context) };
        },
    );
    let before = files_of(&run.user_dir.parent().expect("scratch").join("before"));
    let after = files_of(&run.user_dir);
    assert_eq!(after.get("user.conf"), before.get("user.conf"));
    let staged: Vec<&String> = after.keys().filter(|name| name.ends_with(".tmp")).collect();
    assert!(!staged.is_empty(), "the copy left no staged file");
    for name in staged {
        let saved = name.strip_suffix(".tmp").expect("a staged name");
        assert_eq!(after.get(saved), before.get(saved), "{saved} was replaced");
    }
}
