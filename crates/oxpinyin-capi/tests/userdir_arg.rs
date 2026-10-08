//! `pinyin_init`'s user-dir argument: NULL and `""` are different inputs.
//!
//! The pin keeps `g_strdup(userdir)` (`pinyin.cpp:332` at 074a2219) and
//! its guards test the pointer (`:1133`, `:2671`): NULL is no user dir,
//! while `""` is one — `g_build_filename` drops the empty element, so the
//! profile is bare file names in the working directory (#619). The
//! differential that holds every form against the pin is
//! `tools/bisection/run-locale-diff.sh`; this executable holds the
//! boundary conversion where no oracle is built.
//!
//! The working directory is process state. Keep every scenario in the
//! single test below so the default parallel test runner cannot
//! interleave another test with it in this process.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

/// `USER_DICTIONARY` (`novel_types.h:161`).
const USER_DICTIONARY: u8 = 7;

/// The committed mini fixture (`fixtures/w3/<backend ext>`).
fn system_dir() -> CString {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT);
    CString::new(dir.to_str().expect("UTF-8 path")).expect("no interior NUL")
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-capi-userdir-arg-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("readable dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .into_string()
                .expect("UTF-8 file name")
        })
        .collect();
    names.sort();
    names
}

#[test]
fn null_and_empty_user_dirs_are_different_arguments() {
    let system = system_dir();

    // "": the working directory. check_format writes the raised marker
    // there at init, an import arms the dirty flag, and the save that
    // follows writes the profile beside it.
    let empty = scratch("empty");
    std::env::set_current_dir(&empty).expect("enter the scratch dir");
    let context = pinyin_capi::pinyin_init(system.as_ptr(), c"".as_ptr());
    assert!(!context.is_null(), "the fixture data directory must open");
    assert_eq!(names(&empty), ["user.conf"]);
    let iter = pinyin_capi::pinyin_begin_add_phrases(context, USER_DICTIONARY);
    assert!(pinyin_capi::pinyin_iterator_add_phrase(
        iter,
        c"你好".as_ptr(),
        c"nihao".as_ptr(),
        5,
    ));
    pinyin_capi::pinyin_end_add_phrases(iter);
    assert!(
        pinyin_capi::pinyin_save(context),
        "a \"\" user dir saves into the working directory"
    );
    let saved = names(&empty);
    for name in ["user.conf", "user.bin", "user_bigram.db"] {
        assert!(
            saved.iter().any(|n| n == name),
            "{name} missing from {saved:?}"
        );
    }
    pinyin_capi::pinyin_fini(context);

    // NULL: no user dir. Nothing is written — not at init, not by a
    // save, which answers false, and not at fini.
    let null = scratch("null");
    std::env::set_current_dir(&null).expect("enter the scratch dir");
    let context = pinyin_capi::pinyin_init(system.as_ptr(), ptr::null());
    assert!(!context.is_null(), "the fixture data directory must open");
    assert!(
        !pinyin_capi::pinyin_save(context),
        "a NULL user dir never saves"
    );
    pinyin_capi::pinyin_fini(context);
    assert_eq!(names(&null), Vec::<String>::new());

    // #643: empty system path loads cwd, while NULL still names no file.
    std::env::set_current_dir(system.to_str().expect("UTF-8 fixture path"))
        .expect("enter fixture directory");
    let context = pinyin_capi::pinyin_init(c"".as_ptr(), ptr::null());
    assert!(!context.is_null(), "empty system path opens cwd data");
    pinyin_capi::pinyin_fini(context);
    assert!(pinyin_capi::pinyin_init(ptr::null(), ptr::null()).is_null());

    std::env::set_current_dir(std::env::temp_dir()).expect("leave the scratch dirs");
    let _ = std::fs::remove_dir_all(&empty);
    let _ = std::fs::remove_dir_all(&null);
}
