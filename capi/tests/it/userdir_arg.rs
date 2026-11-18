//! `pinyin_init`'s user-dir argument: NULL and `""` are different inputs.
//!
//! The pin keeps `g_strdup(userdir)` (`pinyin.cpp:332` at 074a2219) and
//! its guards test the pointer (`:1133`, `:2671`): NULL is no user dir,
//! while `""` is one — `g_build_filename` drops the empty element, so the
//! profile is bare file names in the working directory (#619). This
//! module holds that law where no oracle is built.
//!
//! The working directory is process state.
//! This module needs nextest's process-per-test isolation.
//! Keep every scenario in the
//! single test below so the default parallel test runner cannot
//! interleave another test with it in this process.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{names, system_dir_cstr};

use std::path::PathBuf;

use std::ptr;

/// `USER_DICTIONARY` (`novel_types.h:161`).
const USER_DICTIONARY: u8 = 7;

/// A fresh scratch directory. The thread keeps its `TempDir`, which
/// removes the directory when the test's thread ends.
fn scratch(tag: &str) -> PathBuf {
    thread_local! {
        static SCRATCH: std::cell::RefCell<Vec<tempfile::TempDir>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    let dir = tempfile::Builder::new()
        .prefix(&format!("oxpinyin-capi-userdir-arg-{tag}-"))
        .tempdir()
        .expect("scratch dir");
    let path = dir.path().to_path_buf();
    SCRATCH.with_borrow_mut(|dirs| dirs.push(dir));
    path
}

#[test]
fn null_and_empty_user_dirs_are_different_arguments() {
    let system = system_dir_cstr();

    // "": the working directory. check_format writes the raised marker
    // there at init, an import arms the dirty flag, and the save that
    // follows writes the profile beside it.
    let empty = scratch("empty");
    std::env::set_current_dir(&empty).expect("enter the scratch dir");
    let context = unsafe { pinyin_capi::pinyin_init(system.as_ptr(), c"".as_ptr()) };
    assert!(!context.is_null(), "the fixture data directory must open");
    assert_eq!(names(&empty), ["user.conf"]);
    let iter = unsafe { pinyin_capi::pinyin_begin_add_phrases(context, USER_DICTIONARY) };
    assert!(unsafe {
        pinyin_capi::pinyin_iterator_add_phrase(iter, c"你好".as_ptr(), c"nihao".as_ptr(), 5)
    });
    unsafe { pinyin_capi::pinyin_end_add_phrases(iter) };
    assert!(
        unsafe { pinyin_capi::pinyin_save(context) },
        "a \"\" user dir saves into the working directory"
    );
    let saved = names(&empty);
    for name in ["user.conf", "user.bin", "user_bigram.db"] {
        assert!(
            saved.iter().any(|n| n == name),
            "{name} missing from {saved:?}"
        );
    }
    unsafe { pinyin_capi::pinyin_fini(context) };

    // NULL: no user dir. Nothing is written — not at init, not by a
    // save, which answers false, and not at fini.
    let null = scratch("null");
    std::env::set_current_dir(&null).expect("enter the scratch dir");
    let context = unsafe { pinyin_capi::pinyin_init(system.as_ptr(), ptr::null()) };
    assert!(!context.is_null(), "the fixture data directory must open");
    assert!(
        !unsafe { pinyin_capi::pinyin_save(context) },
        "a NULL user dir never saves"
    );
    unsafe { pinyin_capi::pinyin_fini(context) };
    assert_eq!(names(&null), Vec::<String>::new());

    // #643: empty system path loads cwd, while NULL still names no file.
    std::env::set_current_dir(system.to_str().expect("UTF-8 fixture path"))
        .expect("enter fixture directory");
    let context = unsafe { pinyin_capi::pinyin_init(c"".as_ptr(), ptr::null()) };
    assert!(!context.is_null(), "empty system path opens cwd data");
    unsafe { pinyin_capi::pinyin_fini(context) };
    assert!(unsafe { pinyin_capi::pinyin_init(ptr::null(), ptr::null()) }.is_null());

    std::env::set_current_dir(std::env::temp_dir()).expect("leave the scratch dirs");
    let _ = std::fs::remove_dir_all(&empty);
    let _ = std::fs::remove_dir_all(&null);
}
