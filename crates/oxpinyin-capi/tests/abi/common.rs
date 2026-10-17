//! Shared fixtures for the `tests/abi` suites.
//!
//! The twin helpers in `src/test_support.rs` serve the inline white-box
//! suites; Rust cannot share a `#[cfg(test)]` module with an integration
//! target, so this file deliberately mirrors the small surface the
//! black-box suites need (`TempUserDir`, `cstr`, `open`).

use std::ffi::CString;
use std::path::PathBuf;

use pinyin_capi::{
    PinyinContext, PinyinInstance, oxpinyin_init_for_fixtures, pinyin_alloc_instance,
};

pub fn system_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

pub struct TempUserDir {
    pub path: PathBuf,
    _dir: tempfile::TempDir,
}

impl TempUserDir {
    pub fn new(tag: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(&format!("oxpinyin-capi-{tag}-"))
            .suffix(".d")
            .tempdir()
            .expect("temp user dir");
        Self {
            path: dir.path().to_path_buf(),
            _dir: dir,
        }
    }
}

pub fn cstr(value: impl AsRef<str>) -> CString {
    CString::new(value.as_ref().as_bytes()).expect("no interior NUL")
}

pub fn open(user_dir: &str) -> (*mut PinyinContext, *mut PinyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir);
    let context = oxpinyin_init_for_fixtures(system.as_ptr(), user.as_ptr());
    assert!(
        !context.is_null(),
        "fixture init must open the mini fixture"
    );
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    (context, instance)
}
