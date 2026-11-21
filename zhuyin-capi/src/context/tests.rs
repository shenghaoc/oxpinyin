#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

#[cfg(unix)]
use crate::test_support::directory_reads_as_a_chunk_here;
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::test_support::files_of;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::candidates::zhuyin_train;

#[cfg(unix)]
use crate::context::zhuyin_init;

#[cfg(any(unix, any(feature = "tkrzw", feature = "bdb")))]
use crate::context::zhuyin_save;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::instance::zhuyin_alloc_instance;
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::instance::zhuyin_free_instance;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::parse::zhuyin_parse_more_chewings;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::sentence::zhuyin_guess_sentence;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::test_support::abort_in_child;
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::test_support::open_with_user;
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::test_support::plant_bigram;

#[cfg(any(unix, any(feature = "tkrzw", feature = "bdb")))]
use crate::test_support::cstr;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::types::ZhuyinContext;

/// Trains one sentence on `context`, so that the next save writes.
///
/// # Safety
///
/// `context` is NULL or a live context: `zhuyin_init` returned it and `zhuyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
unsafe fn train_once(context: *mut ZhuyinContext) {
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
        "context::tests::save_aborts_where_a_copy_fails_a_destination_write",
        |user| {
            plant_bigram(
                user,
                vec![
                    (0x0100_0001_u32.to_le_bytes().to_vec(), vec![0; 4]),
                    (0x0100_0002_u32.to_le_bytes().to_vec(), vec![0; 4]),
                ],
            );
            let (context, _instance) = open_with_user(user);
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { train_once(context) };
            assert!(unsafe { zhuyin_save(context) });
            let before = user.parent().expect("the child's scratch").join("before");
            std::fs::create_dir(&before).expect("the before directory");
            for (name, bytes) in files_of(user) {
                std::fs::write(before.join(name), bytes).expect("keep a file");
            }
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { train_once(context) };
            // The first copy writes one record and fails the next.
            oxpinyin::store::copy_faults::fail_destination_write_after(1);
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

/// The session could not be built once the library loop had handed it
/// the system libraries (`OpenError::UserSession`): the pin dies at each
/// counterpart. No input makes those steps fail, so the open's answer
/// is handed to the init's refusal directly.
#[test]
fn a_user_session_fault_aborts_the_init() {
    let stderr = crate::test_support::assert_aborts(
        "context::tests::a_user_session_fault_aborts_the_init",
        || {
            let error =
                oxpinyin::facade::OpenFailure::Runtime(oxpinyin::runtime::OpenError::UserSession(
                    oxpinyin::user::UserStoreError::SessionSeed,
                ));
            super::refuse_init(&error, std::path::Path::new("system"));
        },
    );
    // No Rust panic text or other line: the pin writes none here but
    // glibc's assert line, the register's policy-accepted missing line.
    assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
}

/// One call over a private system directory whose library is a directory
/// (register row 96): init maps `merged.bin` (`zhuyin.cpp:199`), the save and
/// the mask-out map it again (`:589`, `:800`), and the reload of the
/// unloaded library 2 maps `gbk_char.bin` (`:200`).
#[cfg(unix)]
fn map_a_directory_library(site: &str) -> bool {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::private_system(scratch.path());
    let user = scratch.path().join("user");
    std::fs::create_dir_all(&user).expect("user dir");
    let system_c = cstr(system.to_str().expect("UTF-8 path"));
    let user_c = cstr(user.to_str().expect("UTF-8 path"));
    if site == "init" {
        crate::test_support::directory_library(&system, "merged.bin");
        return !unsafe { zhuyin_init(system_c.as_ptr(), user_c.as_ptr()) }.is_null();
    }
    let context = unsafe { zhuyin_init(system_c.as_ptr(), user_c.as_ptr()) };
    assert!(!context.is_null());
    match site {
        "save" => {
            // SAFETY: `context` is a live `zhuyin_init` handle, borrowed
            // uniquely for this statement.
            let ctx = unsafe { crate::state::context_mut(context) };
            ctx.core.user_store_mut().mark_modified();
            crate::test_support::directory_library(&system, "merged.bin");
            unsafe { zhuyin_save(context) }
        }
        "mask" => {
            crate::test_support::directory_library(&system, "merged.bin");
            unsafe { crate::config::zhuyin_mask_out(context, 0xff00_0000, 0x0400_0000) }
        }
        _ => {
            assert!(unsafe { crate::config::zhuyin_unload_phrase_library(context, 2) });
            crate::test_support::directory_library(&system, "gbk_char.bin");
            unsafe { crate::config::zhuyin_load_phrase_library(context, 2) }
        }
    }
}

/// Register row 96 at save; row 76's completed save elsewhere.
// Unix only: a directory opens as a file, so the pin's `MemoryChunk::mmap` judges it by its
// size (`directory_reads_as_a_chunk`). Windows refuses the open, and oxpinyin skips the row
// without a word there (`oxpinyin`, `phrase_libraries.rs`), which no pin observation
// covers.
#[cfg(unix)]
#[test]
fn save_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts(
            "context::tests::save_aborts_on_a_directory_library",
            || {
                let _ = map_a_directory_library("save");
            },
        );
    } else {
        assert!(map_a_directory_library("save"));
    }
}

#[test]
fn the_short_table_row_sequence_writes_no_glib_log() {
    use crate::test_support::{LogCapture, TempUserDir, cstr, short_row_system};
    let scratch = tempfile::tempdir().unwrap();
    let system = short_row_system(scratch.path());
    let user = TempUserDir::new("short-row-log");
    let logs = LogCapture::new();
    let context = crate::test_support::init_on(&system, Some(&user.path));
    assert!(!context.is_null());
    for text in ["ni3hao3", "yi4shu4"] {
        let instance = unsafe { crate::instance::zhuyin_alloc_instance(context) };
        let _ =
            unsafe { crate::parse::zhuyin_parse_more_full_pinyins(instance, cstr(text).as_ptr()) };
        unsafe { crate::sentence::zhuyin_guess_sentence(instance) };
        let mut sentence = std::ptr::null_mut();
        unsafe { crate::sentence::zhuyin_get_sentence(instance, &raw mut sentence) };
        if !sentence.is_null() {
            // SAFETY: `sentence` is the string the library just returned; this code owns it and
            // frees it only here.
            unsafe { crate::test_support::take_sentence(sentence) };
        }
        assert!(unsafe { crate::sentence::zhuyin_guess_candidates_after_cursor(instance, 0) });
        unsafe { crate::instance::zhuyin_free_instance(instance) };
    }
    let instance = unsafe { crate::instance::zhuyin_alloc_instance(context) };
    let _ = unsafe { crate::parse::zhuyin_parse_more_full_pinyins(instance, c"ni3hao3".as_ptr()) };
    unsafe { crate::sentence::zhuyin_guess_sentence(instance) };
    assert!(unsafe { crate::candidates::zhuyin_train(instance) });
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(unsafe {
        crate::test_support::import(context, 7, "测试词", "ㄘㄜˋ ㄕˋ ㄘˊ".as_bytes(), 1)
    });
    assert!(unsafe { crate::context::zhuyin_save(context) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { crate::test_support::close(context, instance) };
    assert!(logs.take().is_empty());
}
