//! Shared fixtures for crate tests.

use std::ffi::CString;
use std::os::raw::c_uint;
use std::path::PathBuf;

use crate::context::pinyin_init_for_fixtures;
use crate::instance::pinyin_alloc_instance;
use crate::types::{PinyinContext, PinyinInstance};

/// `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY`.
pub(crate) const DEFAULT_SORT: c_uint = 0x1e;

pub(crate) fn system_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
}

pub(crate) struct TempUserDir {
    pub(crate) path: PathBuf,
}

impl TempUserDir {
    pub(crate) fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("oxpinyin-capi-{tag}-{}.d", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp user dir");
        Self { path }
    }
}

impl Drop for TempUserDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(crate) struct TempSystemDir {
    pub(crate) path: PathBuf,
}

impl TempSystemDir {
    pub(crate) fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-capi-system-{tag}-{}.d",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp system dir");
        for name in ["pinyin_index.redb", "phrase_index.redb", "bigram.redb"] {
            std::fs::copy(system_dir().join(name), path.join(name)).expect("copy redb table");
        }
        Self { path }
    }

    pub(crate) fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.path.join(name), contents).expect("write temp system file");
    }
}

impl Drop for TempSystemDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn cstr(value: impl AsRef<str>) -> CString {
    CString::new(value.as_ref().as_bytes()).expect("no interior NUL")
}

pub(crate) fn open(user_dir: &str) -> (*mut PinyinContext, *mut PinyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir);
    let context = pinyin_init_for_fixtures(system.as_ptr(), user.as_ptr());
    assert!(
        !context.is_null(),
        "fixture init must open the mini fixture"
    );
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    (context, instance)
}
