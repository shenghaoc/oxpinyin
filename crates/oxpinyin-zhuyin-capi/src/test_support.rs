//! Shared fixtures for crate tests — the zhuyin twin of
//! `oxpinyin-capi/src/test_support.rs`.

use std::ffi::CString;
use std::os::raw::c_uint;
use std::path::{Path, PathBuf};

use crate::candidates::{zhuyin_get_candidate, zhuyin_get_n_candidate};
use crate::context::{zhuyin_fini, zhuyin_init};
use crate::instance::zhuyin_alloc_instance;
use crate::parse::zhuyin_parse_more_chewings;
use crate::sentence::zhuyin_guess_candidates_after_cursor;
use crate::state::instance_mut;
use crate::types::{LookupCandidate, ZhuyinContext, ZhuyinInstance};

/// The committed mini fixture (`fixtures/w3/<backend ext>`), the same
/// data directory the pinyin crate's e2e tests open.
pub fn system_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

/// A scratch user directory removed on drop.
pub struct TempUserDir {
    /// The directory to pass as the init's user dir.
    pub path: PathBuf,
}

impl TempUserDir {
    /// Creates (after clearing) a unique per-process scratch dir.
    pub fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-zhuyin-capi-{tag}-{}.d",
            std::process::id()
        ));
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

pub fn cstr(value: impl AsRef<str>) -> CString {
    CString::new(value.as_ref().as_bytes()).expect("no interior NUL")
}

/// Opens the fixture context with no user directory (the corpus
/// driver's shape) and one instance on it.
pub fn open() -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    open_with_user("")
}

/// Opens the fixture context with `user_dir` and one instance on it.
pub fn open_with_user(user_dir: impl AsRef<Path>) -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir.as_ref().to_str().expect("UTF-8 path"));
    let context = zhuyin_init(system.as_ptr(), user.as_ptr());
    assert!(!context.is_null(), "the mini fixture must open");
    let instance = zhuyin_alloc_instance(context);
    assert!(!instance.is_null());
    (context, instance)
}

/// Releases a pair [`open`] / [`open_with_user`] returned.
pub fn close(context: *mut ZhuyinContext, instance: *mut ZhuyinInstance) {
    crate::instance::zhuyin_free_instance(instance);
    zhuyin_fini(context);
}

/// Parses `keys` (standard chewing keystrokes), fills the
/// composition-anchored candidate window from offset 0, and returns the
/// pointer to candidate `index` — borrowed into the instance's snapshot
/// until the next guess, exactly the contract a C consumer holds.
pub fn candidate(instance: *mut ZhuyinInstance, keys: &str, index: c_uint) -> *mut LookupCandidate {
    let input = cstr(keys);
    assert_eq!(
        zhuyin_parse_more_chewings(instance, input.as_ptr()),
        keys.len(),
        "the whole keystroke run parses"
    );
    assert!(
        zhuyin_guess_candidates_after_cursor(instance, 0),
        "the composition window fills"
    );
    let mut count = 0;
    assert!(zhuyin_get_n_candidate(instance, &raw mut count));
    assert!(
        usize::try_from(index).expect("small candidate index")
            < usize::try_from(count).expect("candidate count fits"),
        "candidate {index} exists (n = {count})"
    );
    let mut cand: *mut LookupCandidate = std::ptr::null_mut();
    assert!(zhuyin_get_candidate(instance, index, &raw mut cand));
    assert!(!cand.is_null());
    cand
}

/// The text a snapshot row carries, by row index.
pub fn candidate_text(instance: *mut ZhuyinInstance, index: usize) -> String {
    // SAFETY: `instance` is live and was produced by
    // `zhuyin_alloc_instance`; the borrow ends with this function.
    let inst = unsafe { instance_mut(instance) };
    inst.candidates[index]
        .text
        .to_str()
        .expect("candidate text is UTF-8")
        .to_owned()
}

/// Runs `f` against the instance's user store handle (the same
/// connection the entry points write through; every update commits
/// before returning). The borrow is scoped to the callback, so it cannot
/// outlive the instance or its close.
pub fn with_store<R>(
    instance: *mut ZhuyinInstance,
    f: impl FnOnce(&oxpinyin_user::UserStore) -> R,
) -> R {
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`; the store is a value field, and the shared
    // reference lives only for the duration of `f`.
    let inst = unsafe { instance_mut(instance) };
    f(inst
        .core
        .user
        .as_ref()
        .expect("instance carries a user store"))
}

/// The token snapshotted on the candidate pointer.
pub fn token_of(instance: *mut ZhuyinInstance, cand: *mut LookupCandidate) -> u32 {
    // SAFETY: `cand` was produced by a guess on `instance` and no later
    // guess invalidated the snapshot.
    let inst = unsafe { instance_mut(instance) };
    inst.candidates
        .iter()
        .find(|c| std::ptr::eq(*c, cand.cast::<crate::state::CapiCandidate>()))
        .and_then(|c| c.token)
        .expect("a phrase candidate carries its token")
        .value()
}

/// Reads the malloc'd sentence a getter handed out and frees it with the
/// matching allocator.
pub fn take_sentence(sentence: *mut std::os::raw::c_char) -> String {
    assert!(!sentence.is_null(), "the getter allocated a string");
    // SAFETY: `sentence` is a NUL-terminated string this facade just
    // malloc'd for the caller; it is read, then released with the
    // allocator that produced it.
    let decoded = unsafe { std::ffi::CStr::from_ptr(sentence) }
        .to_str()
        .expect("UTF-8")
        .to_owned();
    // SAFETY: `sentence` came from `malloc` in `owned_cstr` and is not
    // used after this call.
    unsafe { crate::ffi::free(sentence.cast()) };
    decoded
}
