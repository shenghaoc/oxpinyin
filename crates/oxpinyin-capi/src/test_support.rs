//! Shared fixtures for crate tests.

use std::ffi::CString;
use std::os::raw::c_uint;
use std::path::PathBuf;

use crate::candidates::pinyin_get_candidate;
use crate::instance::pinyin_alloc_instance;
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::pinyin_guess_candidates;
use crate::types::{LookupCandidate, PinyinContext, PinyinInstance};

/// `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY`.
pub const DEFAULT_SORT: c_uint = 0x1e;

pub fn system_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

pub struct TempUserDir {
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

pub struct TempSystemDir {
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
        // The whole fixture directory of the compiled-in backend
        // (`fixtures/w3/<ext>`: the DBMs, the chunk files, table.conf),
        // so this helper works under every backend gate.
        for entry in std::fs::read_dir(system_dir()).expect("fixture dir") {
            let entry = entry.expect("fixture entry");
            if entry.file_type().expect("file type").is_file() {
                std::fs::copy(entry.path(), path.join(entry.file_name())).expect("copy fixture");
            }
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

pub fn cstr(value: impl AsRef<str>) -> CString {
    CString::new(value.as_ref().as_bytes()).expect("no interior NUL")
}

/// Parses `text`, guesses candidates, and returns the pointer to candidate
/// `index` (borrowed into the instance's snapshot until the next guess).
pub fn candidate(instance: *mut PinyinInstance, text: &str, index: c_uint) -> *mut LookupCandidate {
    let input = cstr(text);
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, input.as_ptr()),
        text.len(),
        "full input parses"
    );
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let mut cand: *mut LookupCandidate = std::ptr::null_mut();
    assert!(
        pinyin_get_candidate(instance, index, &raw mut cand),
        "candidate {index} exists"
    );
    assert!(!cand.is_null());
    cand
}

pub fn open(user_dir: &str) -> (*mut PinyinContext, *mut PinyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir);
    let context = crate::context::oxpinyin_init_for_fixtures(system.as_ptr(), user.as_ptr());
    assert!(
        !context.is_null(),
        "fixture init must open the mini fixture"
    );
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    (context, instance)
}
