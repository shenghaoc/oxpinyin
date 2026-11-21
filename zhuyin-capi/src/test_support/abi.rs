//! Zhuyin C-ABI helpers that the crate's module tests and its verification
//! tests share. `test_support.rs` declares this module, and
//! `tests/it/support.rs` includes it by `#[path]`. The exported functions
//! and types are named through `oxzhuyin_capi`, which the crate root also
//! binds in a test build.

#![allow(dead_code)]
#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

// The verification tests' `support.rs` already brings these in through
// `super`; the module tests' `test_support.rs` does not.
#[allow(
    unused_imports,
    reason = "used by the module tests, redundant in the verification tests"
)]
use oxzhuyin_capi::{
    candidates::{
        zhuyin_get_candidate, zhuyin_get_candidate_string, zhuyin_get_candidate_type,
        zhuyin_get_n_candidate,
    },
    context::{zhuyin_fini, zhuyin_init},
    dict::zhuyin_lookup_tokens,
    instance::{zhuyin_alloc_instance, zhuyin_free_instance},
    iterators::{zhuyin_begin_add_phrases, zhuyin_end_add_phrases, zhuyin_iterator_add_phrase},
    parse::zhuyin_parse_more_chewings,
    sentence::zhuyin_guess_candidates_after_cursor,
    types::{LookupCandidate, ZhuyinContext, ZhuyinInstance, lookup_candidate_type_t},
};

use std::ffi::{c_char, c_uint};

use std::path::Path;

#[cfg(feature = "tkrzw")]
use std::ptr;

/// The environment variable that names the death test a re-executed test
/// binary runs.
pub(crate) const DEATH_TEST: &str = "OXPINYIN_ZHUYIN_CAPI_DEATH_TEST";

/// The environment variable that names the user directory a
/// [`run_in_child`] child works in.
pub(crate) const CHILD_DIR: &str = "OXPINYIN_ZHUYIN_CAPI_CHILD_DIR";

/// Opens the fixture context with no user directory — a NULL one, the
/// corpus driver's shape; `""` would be the working directory — and one
/// instance on it.
pub(crate) fn open() -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    // SAFETY: `std::ptr::null()` is NULL, which `open_at` accepts.
    unsafe { open_at(std::ptr::null()) }
}

/// Opens the fixture context with `user_dir` and one instance on it.
pub(crate) fn open_with_user(
    user_dir: impl AsRef<Path>,
) -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    let user = cstr(user_dir.as_ref().to_str().expect("UTF-8 path"));
    // SAFETY: `user.as_ptr()` is a NUL-terminated string that outlives the call.
    unsafe { open_at(user.as_ptr()) }
}

/// # Safety
///
/// `user` is NULL or a NUL-terminated string that stays valid for the call.
pub(crate) unsafe fn open_at(user: *const c_char) -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let context = unsafe { zhuyin_init(system.as_ptr(), user) };
    assert!(!context.is_null(), "the mini fixture must open");
    let instance = unsafe { zhuyin_alloc_instance(context) };
    assert!(!instance.is_null());
    (context, instance)
}

/// Releases a pair [`open`] / [`open_with_user`] returned.
///
/// # Safety
///
/// `context` is NULL or a live context: `zhuyin_init` returned it and `zhuyin_fini` has not taken
/// it.
///
/// `instance` is NULL or a live instance: `zhuyin_alloc_instance` returned it and
/// `zhuyin_free_instance` has not freed it.
///
/// `instance` is NULL or belongs to `context`. The call frees both, so the caller does not use
/// either again.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn close(context: *mut ZhuyinContext, instance: *mut ZhuyinInstance) {
    unsafe { zhuyin_free_instance(instance) };
    unsafe { zhuyin_fini(context) };
}

/// Parses `keys` (standard chewing keystrokes), fills the
/// composition-anchored candidate window from offset 0, and returns the
/// pointer to candidate `index` — borrowed into the instance's snapshot
/// until the next guess, exactly the contract a C consumer holds.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `zhuyin_alloc_instance` returned it and
/// `zhuyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn candidate(
    instance: *mut ZhuyinInstance,
    keys: &str,
    index: c_uint,
) -> *mut LookupCandidate {
    let input = cstr(keys);
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, input.as_ptr()) },
        keys.len(),
        "the whole keystroke run parses"
    );
    assert!(
        unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) },
        "the composition window fills"
    );
    let mut count = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    assert!(
        usize::try_from(index).expect("small candidate index")
            < usize::try_from(count).expect("candidate count fits"),
        "candidate {index} exists (n = {count})"
    );
    let mut cand: *mut LookupCandidate = std::ptr::null_mut();
    assert!(unsafe { zhuyin_get_candidate(instance, index, &raw mut cand) });
    assert!(!cand.is_null());
    cand
}

/// Reads the malloc'd sentence a getter handed out and frees it with the
/// matching allocator.
///
/// # Safety
///
/// `sentence` is a NUL-terminated string, not NULL, that the library returned to the caller, who
/// owns it. The call frees it with `g_free`, so the caller does not use it again.
pub(crate) unsafe fn take_sentence(sentence: *mut std::os::raw::c_char) -> String {
    assert!(!sentence.is_null(), "the getter allocated a string");
    // SAFETY: `sentence` is a NUL-terminated string this facade just
    // malloc'd for the caller; it is read, then released with the
    // allocator that produced it.
    let decoded = unsafe { std::ffi::CStr::from_ptr(sentence) }
        .to_str()
        .expect("UTF-8")
        .to_owned();
    // SAFETY: `sentence` is the returned GLib string, which `g_free`
    // releases, and is not used after this call.
    unsafe { glib_sys::g_free(sentence.cast()) };
    decoded
}

/// [`assert_aborts`] with the user directory of [`run_in_child`]: `body`
/// gets a fresh user directory that stays on disk for the parent, so a test
/// can read the files the child left before its abort.
pub(crate) fn abort_in_child(name: &str, body: impl FnOnce(&Path)) -> ChildRun {
    let (command, user_dir, scratch) = child_command(name, true, body);
    let output = wait_for_abort(name, command);
    ChildRun {
        stderr: output.stderr,
        user_dir,
        _scratch: scratch,
    }
}

/// In the child, runs `body` on its user directory and exits 0; with
/// `strict` (a death test), a panic in `body` exits with
/// [`OPERATION_PANICKED`]. In the parent, makes the scratch and user
/// directories and answers the command that re-runs the test `name` as that
/// child.
pub(crate) fn child_command(
    name: &str,
    strict: bool,
    body: impl FnOnce(&Path),
) -> (std::process::Command, PathBuf, tempfile::TempDir) {
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let dir = std::env::var_os(CHILD_DIR).expect("the child's user directory");
        if strict {
            exit_on_panic();
        }
        body(Path::new(&dir));
        std::process::exit(0);
    }
    let scratch = tempfile::tempdir().expect("child scratch");
    let user_dir = scratch.path().join("user");
    std::fs::create_dir(&user_dir).expect("child user directory");
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .env(CHILD_DIR, &user_dir)
        .env(if cfg!(windows) { "TMP" } else { "TMPDIR" }, scratch.path());
    (command, user_dir, scratch)
}

/// `zhuyin_init` on `system` with `user` (NULL when `None`). The answer can
/// be NULL.
pub(crate) fn init_on(system: &Path, user: Option<&Path>) -> *mut ZhuyinContext {
    let system = cstr(system.to_str().expect("UTF-8 path"));
    let user = user.map(|user| cstr(user.to_str().expect("UTF-8 path")));
    unsafe {
        zhuyin_init(
            system.as_ptr(),
            user.as_ref().map_or(std::ptr::null(), |user| user.as_ptr()),
        )
    }
}

/// Imports `text` with the chewing `reading` and `count` into `library`.
/// Answers the add.
///
/// # Safety
///
/// `context` is NULL or a live context: `zhuyin_init` returned it and `zhuyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn import(
    context: *mut ZhuyinContext,
    library: u8,
    text: &str,
    reading: &[u8],
    count: i32,
) -> bool {
    let iter = unsafe { zhuyin_begin_add_phrases(context, library) };
    assert!(!iter.is_null(), "the import iterator");
    let text = cstr(text);
    let reading = std::ffi::CString::new(reading).expect("no interior NUL");
    let added = unsafe { zhuyin_iterator_add_phrase(iter, text.as_ptr(), reading.as_ptr(), count) };
    unsafe { zhuyin_end_add_phrases(iter) };
    added
}

/// The tokens `zhuyin_lookup_tokens` finds for `text`.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `zhuyin_alloc_instance` returned it and
/// `zhuyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn tokens_of(instance: *mut ZhuyinInstance, text: &str) -> Vec<u32> {
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    unsafe { zhuyin_lookup_tokens(instance, cstr(text).as_ptr(), array) };
    // SAFETY: the array is live and holds `len` four-byte elements.
    let tokens = unsafe {
        let view = &*array;
        (0..view.len as usize)
            .map(|index| view.data.cast::<u32>().add(index).read_unaligned())
            .collect()
    };
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    tokens
}

/// One GLib log record: its domain and its level bits.
pub(crate) type LogRecord = (Option<String>, u32);

thread_local! {
    static CAPTURED: std::cell::RefCell<Option<Vec<(LogRecord, String)>>> =
        const { std::cell::RefCell::new(None) };
}

/// The process default GLib handler while a test captures: it records on a
/// capturing thread and forwards to GLib's own handler on every other thread.
pub(crate) unsafe extern "C" fn capture_log(
    domain: *const c_char,
    level: glib_sys::GLogLevelFlags,
    message: *const c_char,
    data: glib_sys::gpointer,
) {
    let captured = CAPTURED.with(|logs| {
        let mut logs = logs.borrow_mut();
        let Some(logs) = logs.as_mut() else {
            return false;
        };
        let domain = (!domain.is_null()).then(|| {
            // SAFETY: GLib supplies a live NUL-terminated domain.
            unsafe { std::ffi::CStr::from_ptr(domain) }
                .to_string_lossy()
                .into_owned()
        });
        // SAFETY: GLib supplies the live, NUL-terminated diagnostic message.
        let message = unsafe { std::ffi::CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned();
        logs.push(((domain, level & glib_sys::G_LOG_LEVEL_MASK), message));
        true
    });
    if !captured {
        // SAFETY: GLib's unchanged arguments, for a thread that does not capture.
        unsafe { glib_sys::g_log_default_handler(domain, level, message, data) };
    }
}

/// Captures the GLib log records this thread makes until it is dropped.
pub(crate) struct LogCapture;

impl LogCapture {
    /// Starts the capture on this thread.
    pub fn new() -> Self {
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            // SAFETY: the handler is a plain function for the life of the
            // process and `user_data` is unused.
            unsafe { glib_sys::g_log_set_default_handler(Some(capture_log), std::ptr::null_mut()) };
        });
        CAPTURED.with(|logs| *logs.borrow_mut() = Some(Vec::new()));
        Self
    }

    /// The records since the start or the last take.
    pub fn take(&self) -> Vec<LogRecord> {
        self.take_with_messages()
            .into_iter()
            .map(|(record, _)| record)
            .collect()
    }

    /// Takes the records and their messages in emission order.
    pub fn take_with_messages(&self) -> Vec<(LogRecord, String)> {
        CAPTURED.with(|logs| {
            logs.borrow_mut()
                .as_mut()
                .map(std::mem::take)
                .unwrap_or_default()
        })
    }
}

impl Drop for LogCapture {
    fn drop(&mut self) {
        CAPTURED.with(|logs| *logs.borrow_mut() = None);
    }
}

/// A fresh instance on `context` with `keys` parsed as chewing keystrokes.
///
/// # Safety
///
/// `context` is NULL or a live context: `zhuyin_init` returned it and `zhuyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn chewing(context: *mut ZhuyinContext, keys: &str) -> *mut ZhuyinInstance {
    let instance = unsafe { zhuyin_alloc_instance(context) };
    assert!(!instance.is_null());
    unsafe { zhuyin_parse_more_chewings(instance, cstr(keys).as_ptr()) };
    instance
}

#[cfg(feature = "tkrzw")]
/// The raw bytes of a NUL-terminated string.
///
/// # Safety
///
/// `text` is a NUL-terminated string, not NULL, that stays valid for the call.
pub(crate) unsafe fn bytes_at(text: *const c_char) -> Vec<u8> {
    assert!(!text.is_null());
    // SAFETY: the facade answered a live NUL-terminated string.
    unsafe { std::ffi::CStr::from_ptr(text) }
        .to_bytes()
        .to_vec()
}

#[cfg(feature = "tkrzw")]
/// The candidate rows of the current list: type and text bytes.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `zhuyin_alloc_instance` returned it and
/// `zhuyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn candidate_rows(
    instance: *mut ZhuyinInstance,
) -> Vec<(*mut LookupCandidate, lookup_candidate_type_t, Vec<u8>)> {
    let mut count = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    (0..count)
        .map(|index| {
            let mut candidate: *mut LookupCandidate = ptr::null_mut();
            assert!(unsafe { zhuyin_get_candidate(instance, index, &raw mut candidate) });
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(unsafe { zhuyin_get_candidate_type(instance, candidate, &raw mut kind) });
            let mut text: *const c_char = ptr::null();
            assert!(unsafe { zhuyin_get_candidate_string(instance, candidate, &raw mut text) });
            assert!(
                !text.is_null(),
                "zhuyin_get_candidate_string answered true but wrote a NULL string for row {index}"
            );
            // SAFETY: `text` is not NULL (checked above), and `zhuyin_get_candidate_string`
            // points it at the row's NUL-terminated string, which stays valid until the list is
            // rebuilt; nothing rebuilds it before this read.
            (candidate, kind, unsafe { bytes_at(text) })
        })
        .collect()
}

/// The candidate count of the current list.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `zhuyin_alloc_instance` returned it and
/// `zhuyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn n_candidate(instance: *mut ZhuyinInstance) -> (bool, u32) {
    let mut count = UNTOUCHED;
    let ret = unsafe { zhuyin_get_n_candidate(instance, &raw mut count) };
    (ret, count)
}

/// A context and instance on `system` with `user_dir`, which must open.
pub(crate) fn open_on(system: &Path, user_dir: &Path) -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    let context = init_on(system, Some(user_dir));
    assert!(!context.is_null(), "zhuyin_init opens the pinned data");
    let instance = unsafe { zhuyin_alloc_instance(context) };
    assert!(!instance.is_null());
    (context, instance)
}
