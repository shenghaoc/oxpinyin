//! Shared fixtures for crate tests.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::dict::pinyin_token_add_unigram_frequency;

use crate::state::instance_ref;

use crate::context::pinyin_init;

use std::path::Path;

use crate::context::pinyin_fini;

use crate::instance::pinyin_free_instance;

use std::ffi::CString;

use std::os::raw::c_uint;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use std::path::PathBuf;

#[path = "../../oxpinyin-core/tests/support/native_fixture.rs"]
mod native_fixture;
pub(crate) use native_fixture::*;

use crate::candidates::pinyin_get_candidate;

use crate::instance::pinyin_alloc_instance;

use crate::parse::pinyin_parse_more_full_pinyins;

use crate::sentence::pinyin_guess_candidates;

use crate::types::{LookupCandidate, PinyinContext, PinyinInstance};

// Oxpinyin retains the owner for a surviving instance. This is a safety
// contract, not a pin oracle: the returned public handle keeps its identity
// but is no longer live and must not be dereferenced.
pub(crate) fn finalize_public_context(context: *mut PinyinContext, instance: *mut PinyinInstance) {
    assert_eq!(
        unsafe { crate::instance::pinyin_get_context(instance) },
        context
    );
    unsafe { crate::context::pinyin_fini(context) };
    assert!(!crate::live::is_live(context));
    assert_eq!(
        unsafe { crate::instance::pinyin_get_context(instance) },
        context
    );
}

/// Serialises the tests that read the pin's process-wide key slots
/// (`pinyin_get_pinyin_key`, `pinyin_get_pinyin_key_rest`): the slots are
/// statics, so two such tests running on different threads overwrite each
/// other's answers. Hold the guard for the whole test.
pub fn key_slot_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY`.
pub const DEFAULT_SORT: c_uint = 0x1e;

pub fn cstr(value: impl AsRef<str>) -> CString {
    CString::new(value.as_ref().as_bytes()).expect("no interior NUL")
}

/// Parses `text`, guesses candidates, and returns the pointer to candidate
/// `index` (borrowed into the instance's snapshot until the next guess).
pub fn candidate(instance: *mut PinyinInstance, text: &str, index: c_uint) -> *mut LookupCandidate {
    let input = cstr(text);
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        text.len(),
        "full input parses"
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let mut cand: *mut LookupCandidate = std::ptr::null_mut();
    assert!(
        unsafe { pinyin_get_candidate(instance, index, &raw mut cand) },
        "candidate {index} exists"
    );
    assert!(!cand.is_null());
    cand
}

pub fn open(user_dir: &str) -> (*mut PinyinContext, *mut PinyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir);
    let context = unsafe { crate::context::pinyin_init(system.as_ptr(), user.as_ptr()) };
    assert!(
        !context.is_null(),
        "fixture init must open the mini fixture"
    );
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    (context, instance)
}

/// The environment variable that names the death test a re-executed test
/// binary runs.
const DEATH_TEST: &str = "OXPINYIN_CAPI_DEATH_TEST";

/// The exit status of a death-test child whose operation panicked. A panic
/// that reaches a non-unwinding `extern "C"` function aborts the process,
/// so without this status an unrelated panic passes as the pin's abort.
const OPERATION_PANICKED: i32 = 86;

/// Arms the strict check in a death-test child: from here on, a panic
/// writes its message to fd 2 (past libtest's capture, which an abort
/// discards) and ends the child with [`OPERATION_PANICKED`].
fn exit_on_panic() {
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), "death-test operation panicked: {info}");
        std::process::exit(OPERATION_PANICKED);
    }));
}

/// How a death-test child ended.
#[derive(Debug, PartialEq, Eq)]
enum ChildEnd {
    /// The expected abort: SIGABRT on Unix, fast-fail on Windows.
    Aborted,
    /// `body` returned and the child exited 0.
    Returned,
    /// The test harness reported a panic (libtest exits 101).
    Panicked,
    /// The operation under test panicked: the child's hook exited with
    /// [`OPERATION_PANICKED`] before the panic could unwind into an
    /// `extern "C"` function, where it would abort like the pin.
    OperationPanicked,
    /// Any other end, as the status prints it.
    Other(String),
}

fn child_end(status: std::process::ExitStatus) -> ChildEnd {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(unix)]
    let aborted = status.signal() == Some(6);
    // Rust 1.99.1, library/std/src/sys/pal/windows/mod.rs:269, implements
    // __fastfail(FAST_FAIL_FATAL_APP_EXIT) with int 0x29 (arm64:275, brk).
    // https://github.com/rust-lang/rust/blob/1.99.1/library/std/src/sys/pal/windows/mod.rs#L269
    // User-mode __fastfail raises 0xC0000409 (STATUS_STACK_BUFFER_OVERRUN):
    // https://learn.microsoft.com/en-us/cpp/intrinsics/fastfail
    // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-erref/596a1078-e883-4972-9bbc-49e60bebca55
    #[cfg(windows)]
    let aborted = status.code() == Some(i32::from_ne_bytes(0xC000_0409_u32.to_ne_bytes()));
    if aborted {
        ChildEnd::Aborted
    } else {
        match status.code() {
            Some(0) => ChildEnd::Returned,
            Some(101) => ChildEnd::Panicked,
            Some(OPERATION_PANICKED) => ChildEnd::OperationPanicked,
            _ => ChildEnd::Other(format!("{status}")),
        }
    }
}

/// Runs the death-test child `command` for the test `name` and requires
/// that it aborted. Answers its output.
fn wait_for_abort(name: &str, mut command: std::process::Command) -> std::process::Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{name}: the death-test child did not start: {error}"));
    let end = child_end(output.status);
    assert_eq!(
        end,
        ChildEnd::Aborted,
        "{name} must abort: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Runs `body` in a child process and requires an abort:
/// SIGABRT on Unix, fast-fail on Windows. This is how
/// the facade reproduces a pin `assert`/`abort()` (the class (c) rows
/// retired 2026-10-31 UTC). `name` is the calling test's full path as
/// libtest prints it (`module::test`): the test binary is re-executed with
/// `--exact name`, and in that child `body` runs and must not return.
/// A normal return, a panic and a child that does not start each fail
/// with their own message. Answers the child's stderr.
pub fn assert_aborts(name: &str, body: impl FnOnce()) -> Vec<u8> {
    assert_aborts_after_setup(name, || (), |()| body())
}

/// [`assert_aborts`] for an operation that needs setup which panics on
/// purpose (a poisoned mutex): `setup` runs before the strict check is
/// armed, then `operation` runs on its value and must abort. A panic in
/// `operation` fails as [`ChildEnd::OperationPanicked`].
pub fn assert_aborts_after_setup<T>(
    name: &str,
    setup: impl FnOnce() -> T,
    operation: impl FnOnce(T),
) -> Vec<u8> {
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let value = setup();
        exit_on_panic();
        operation(value);
        // Reached only when `operation` did not abort.
        std::process::exit(0);
    }
    // The child's scratch directories outlive its abort; keep them under
    // one this process removes.
    let scratch = tempfile::tempdir().expect("death-test scratch");
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .env(if cfg!(windows) { "TMP" } else { "TMPDIR" }, scratch.path());
    wait_for_abort(name, command).stderr
}

/// The environment variable that names the user directory a
/// [`assert_aborts_in_dir`] child works in.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
const CHILD_DIR: &str = "OXPINYIN_CAPI_CHILD_DIR";

/// What a [`assert_aborts_in_dir`] child left behind.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
pub struct ChildRun {
    /// The user directory the child was given, still on disk while the
    /// parent holds this value.
    pub user_dir: PathBuf,
    _scratch: tempfile::TempDir,
}

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
/// [`assert_aborts`] with a directory the parent keeps: `body` gets a
/// fresh, empty directory, and the parent reads the files the child left in
/// it before the abort. Answers the child's stderr and that directory.
pub fn assert_aborts_in_dir(name: &str, body: impl FnOnce(&std::path::Path)) -> ChildRun {
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let dir = std::env::var_os(CHILD_DIR).expect("the child's directory");
        exit_on_panic();
        body(std::path::Path::new(&dir));
        // Reached only when `body` did not abort.
        std::process::exit(0);
    }
    let scratch = tempfile::tempdir().expect("death-test scratch");
    let user_dir = scratch.path().join("user");
    std::fs::create_dir(&user_dir).expect("child directory");
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .env(CHILD_DIR, &user_dir)
        .env(if cfg!(windows) { "TMP" } else { "TMPDIR" }, scratch.path());
    let _output = wait_for_abort(name, command);
    ChildRun {
        user_dir,
        _scratch: scratch,
    }
}

/// The environment variable that names the file [`assert_aborts_sharing`]
/// maps in the child.
// Unix only: the shared page is an `mmap(MAP_SHARED)` of a file.
#[cfg(unix)]
const DEATH_TEST_PAGE: &str = "OXPINYIN_CAPI_DEATH_TEST_PAGE";

/// The bytes [`assert_aborts_sharing`] shares with the child.
#[cfg(unix)]
pub const SHARED_PAGE: usize = 4096;

/// [`assert_aborts`], with one page the child writes through and this
/// process reads after the abort: a file holding `init` (zero-padded to
/// [`SHARED_PAGE`] bytes) that the child maps `MAP_SHARED`, so what the
/// facade wrote through pointers into the page before it aborted reaches
/// the file. `body` gets the page's address. Answers the page as the child
/// left it.
// Unix only: the shared page is an `mmap(MAP_SHARED)` of a file.
#[cfg(unix)]
pub fn assert_aborts_sharing(name: &str, init: &[u8], body: impl FnOnce(*mut u8)) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let path = std::env::var_os(DEATH_TEST_PAGE).expect("the shared page's file");
        let path = CString::new(path.as_bytes()).expect("a path without NUL");
        // SAFETY: `path` is NUL-terminated; the descriptor stays open for
        // the mapping's life (the process aborts in `body`).
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR) };
        assert!(fd >= 0, "open the shared page");
        // SAFETY: a fresh shared mapping of the whole page-sized file.
        let page = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                SHARED_PAGE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        assert!(page != libc::MAP_FAILED, "map the shared page");
        exit_on_panic();
        body(page.cast());
        // Reached only when `body` did not abort.
        std::process::exit(0);
    }
    let scratch = tempfile::tempdir().expect("death-test scratch");
    let page = scratch.path().join("shared-page");
    let mut bytes = init.to_vec();
    bytes.resize(SHARED_PAGE, 0);
    std::fs::write(&page, &bytes).expect("write the shared page");
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .env(DEATH_TEST_PAGE, &page)
        .env("TMPDIR", scratch.path());
    wait_for_abort(name, command);
    std::fs::read(&page).expect("read the shared page")
}

/// Writes a conforming `user.conf` for the fixture data set and `payload`,
/// framed as a `MemoryChunk`, as the user `.dbin` named `file`: the shape a
/// journal-replay test needs, since a non-conforming profile is wiped
/// before any library loads.
/// The tokens `pinyin_lookup_tokens` writes for `text`.
pub fn lookup(instance: *mut PinyinInstance, text: impl AsRef<[u8]>) -> Vec<u32> {
    let text = CString::new(text.as_ref()).expect("no interior NUL");
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    unsafe { crate::dict::pinyin_lookup_tokens(instance, text.as_ptr(), array.cast()) };
    // SAFETY: a live GArray of four-byte elements: `data` holds `len` of them.
    let tokens = unsafe {
        let view = &*array;
        if view.len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(view.data.cast::<u32>(), view.len as usize).to_vec()
        }
    };
    // SAFETY: allocated above and freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    tokens
}

/// One GLib log record: the domain (`None` for the default domain) and
/// the level.
pub type LogRecord = (Option<String>, glib_sys::GLogLevelFlags);

std::thread_local! {
    static CAPTURED: std::cell::RefCell<Option<Vec<(LogRecord, String)>>> =
        const { std::cell::RefCell::new(None) };
}

unsafe extern "C" fn capture_log(
    domain: *const std::ffi::c_char,
    level: glib_sys::GLogLevelFlags,
    message: *const std::ffi::c_char,
    data: glib_sys::gpointer,
) {
    let captured = CAPTURED.with(|logs| {
        let mut logs = logs.borrow_mut();
        let Some(logs) = logs.as_mut() else {
            return false;
        };
        let domain = (!domain.is_null()).then(|| {
            // SAFETY: GLib hands a live NUL-terminated domain.
            unsafe { std::ffi::CStr::from_ptr(domain) }
                .to_string_lossy()
                .into_owned()
        });
        // SAFETY: GLib supplies the live, NUL-terminated diagnostic message.
        let message = unsafe { std::ffi::CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned();
        logs.push(((domain, level), message));
        true
    });
    if !captured {
        // SAFETY: GLib's unchanged arguments, for a thread with no capture.
        unsafe { glib_sys::g_log_default_handler(domain, level, message, data) };
    }
}

/// Collects the GLib log records of this thread (the `libpinyin`, `GLib`
/// and default domains) while it lives.
pub struct LogCapture {
    handlers: Vec<(Option<&'static std::ffi::CStr>, u32)>,
}

impl LogCapture {
    pub fn new() -> Self {
        CAPTURED.with(|logs| *logs.borrow_mut() = Some(Vec::new()));
        let handlers = [Some(c"libpinyin"), Some(c"GLib"), None]
            .into_iter()
            .map(|domain| {
                // SAFETY: the callback lives for the process and takes no
                // user data; Drop removes it.
                let id = unsafe {
                    glib_sys::g_log_set_handler(
                        domain.map_or(std::ptr::null(), std::ffi::CStr::as_ptr),
                        glib_sys::G_LOG_LEVEL_MASK,
                        Some(capture_log),
                        std::ptr::null_mut(),
                    )
                };
                (domain, id)
            })
            .collect();
        Self { handlers }
    }

    /// The records since the capture began or since the last call.
    pub fn take(&self) -> Vec<LogRecord> {
        self.take_with_messages()
            .into_iter()
            .map(|(record, _)| record)
            .collect()
    }

    /// Takes the records and their messages in emission order.
    pub fn take_with_messages(&self) -> Vec<(LogRecord, String)> {
        CAPTURED.with(|logs| std::mem::take(logs.borrow_mut().as_mut().expect("active capture")))
    }
}

impl Drop for LogCapture {
    fn drop(&mut self) {
        for (domain, id) in &self.handlers {
            // SAFETY: the handler ids installed in `new`.
            unsafe {
                glib_sys::g_log_remove_handler(
                    domain.map_or(std::ptr::null(), std::ffi::CStr::as_ptr),
                    *id,
                );
            }
        }
        CAPTURED.with(|logs| *logs.borrow_mut() = None);
    }
}

mod tests;

pub fn alloc(context: *mut PinyinContext) -> *mut PinyinInstance {
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    instance
}

pub fn close(context: *mut PinyinContext, instance: *mut PinyinInstance) {
    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_fini(context) };
}

/// A context on `system` and `user`; NULL when init refuses.
pub fn init_at(system: &Path, user: &Path) -> *mut PinyinContext {
    let system = cstr(system.to_str().expect("UTF-8 path"));
    let user = cstr(user.to_str().expect("UTF-8 path"));
    unsafe { pinyin_init(system.as_ptr(), user.as_ptr()) }
}

/// A context and instance on the mini fixture and `user`.
pub fn open_at(user: &Path) -> (*mut PinyinContext, *mut PinyinInstance) {
    open(user.to_str().expect("UTF-8 path"))
}

pub fn parse(instance: *mut PinyinInstance, text: impl AsRef<[u8]>) -> usize {
    let text = std::ffi::CString::new(text.as_ref()).expect("no interior NUL");
    unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) }
}

/// The token snapshotted on the candidate pointer.
pub fn token_of(instance: *mut PinyinInstance, cand: *mut LookupCandidate) -> u32 {
    // SAFETY: `cand` was produced by `pinyin_get_candidate` on `instance`
    // and no later guess invalidated the snapshot.
    let inst = unsafe { instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .find(|c| std::ptr::eq(*c, cand.cast::<crate::state::CapiCandidate>()))
        .and_then(|c| c.token.get())
        .expect("a phrase candidate carries its token")
        .value()
}

/// Adds the negation of the loaded facade total to token 0x01000001, so
/// the total the model divides by wraps to zero (register row 61).
pub fn wrap_facade_total_to_zero(instance: *mut PinyinInstance) {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    let total = u32::try_from(total).expect("a guint32 total");
    assert!(unsafe {
        pinyin_token_add_unigram_frequency(instance, 0x0100_0001, total.wrapping_neg())
    });
    // SAFETY: as above.
    let after = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    assert_eq!(after, 0, "the facade total wraps to zero");
}

/// The first row of `kind` whose text is `text`.
pub(crate) fn row_with(
    instance: *mut PinyinInstance,
    kind: crate::types::lookup_candidate_type_t,
    text: &str,
) -> *mut LookupCandidate {
    let index = capture_rows(instance)
        .iter()
        .position(|(found, row, _)| *found == kind && row == text)
        .unwrap_or_else(|| panic!("a {kind:?} row {text} is listed"));
    candidate_at(instance, index)
}

pub(crate) fn candidate_at(instance: *mut PinyinInstance, index: usize) -> *mut LookupCandidate {
    let mut cand: *mut LookupCandidate = std::ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut cand,
        )
    });
    cand
}

pub(crate) fn unigram(instance: *mut PinyinInstance, token: u32) -> (bool, u32) {
    let mut freq = 0x00ab_cdef;
    let ret =
        unsafe { crate::dict::pinyin_token_get_unigram_frequency(instance, token, &raw mut freq) };
    (ret, freq)
}

/// The user bigram of a profile on the mini fixture replaced by `rows`
/// under a conforming `user.conf`; the rest of the profile stays.
pub(crate) fn replace_user_bigram(user: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    craft_user_bigram(&user.join("user_bigram.db"), rows);
    write_user_conf(user, &system_dir());
}

/// Imports and saves 龘龘 (da2'da2) in library 7, then closes the context;
/// answers its token.
pub(crate) fn saved_user_phrase(user: &Path) -> u32 {
    let (context, instance) = open_at(user);
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
    assert!(unsafe { crate::context::pinyin_save(context) });
    let token = lookup(instance, "龘龘")[0];
    assert_eq!(token >> 24, 7);
    close(context, instance);
    token
}

pub(crate) fn ucs4(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| u32::from(ch).to_le_bytes())
        .collect()
}

#[cfg(feature = "tkrzw")]
#[path = "../../oxpinyin-facade/tests/common/contract_data.rs"]
pub(crate) mod contract_data;

use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
    pinyin_end_get_phrases, pinyin_iterator_add_phrase,
};
use crate::types::lookup_candidate_type_t;
use oxpinyin_core::user::UserStore;
use std::ptr;

/// Read the live store for one assertion; no storage reference escapes.
pub(crate) fn with_store<R>(
    instance: *mut PinyinInstance,
    read: impl FnOnce(&UserStore) -> R,
) -> R {
    // SAFETY: tests call this only with a live instance, between serialized
    // C operations. The closure cannot return a borrow into the store.
    let inst = unsafe { instance_ref(instance) };
    read(inst.core.user)
}

/// The first snapshot row of `kind`, with its token.
pub(crate) fn predicted_row(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
) -> (*mut LookupCandidate, u32) {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    let index = inst
        .candidates
        .iter()
        .position(|c| c.candidate_type() == kind && c.token.get().is_some())
        .expect("the prediction holds a row of this kind");
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand,
        )
    });
    (cand, token_of(instance, cand))
}

// ── Pin capture of 2026-11-07 (tools/oracle/pin_capture.py, runs 1 and 2 identical) ──
//
// Each test below names its observation. The values are the pin's on the pinned
// libpinyin-data; every one asserted here depends only on the test's own imports, parse
// and options, so the w3 mini fixture holds it too.

/// Every row of the current candidate list as (type, string, n-best index of an NBEST row).
pub(crate) fn capture_rows(
    instance: *mut PinyinInstance,
) -> Vec<(lookup_candidate_type_t, String, Option<u8>)> {
    let mut count = 0;
    assert!(unsafe { crate::candidates::pinyin_get_n_candidate(instance, &raw mut count) });
    (0..count)
        .map(|index| {
            let mut cand: *mut LookupCandidate = ptr::null_mut();
            assert!(unsafe { pinyin_get_candidate(instance, index, &raw mut cand) });
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(unsafe {
                crate::candidates::pinyin_get_candidate_type(instance, cand, &raw mut kind)
            });
            let mut text: *const crate::types::GChar = ptr::null();
            assert!(unsafe {
                crate::candidates::pinyin_get_candidate_string(instance, cand, &raw mut text)
            });
            // SAFETY: a true answer points `text` at the row's borrowed string.
            let text = unsafe { std::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned();
            let nbest = (kind == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE).then(|| {
                let mut index = 0_u8;
                assert!(unsafe {
                    crate::candidates::pinyin_get_candidate_nbest_index(
                        instance,
                        cand,
                        &raw mut index,
                    )
                });
                index
            });
            (kind, text, nbest)
        })
        .collect()
}

/// Imports ROWS into LIBRARY in one batch; answers each add's flag.
pub(crate) fn import_batch(
    context: *mut crate::types::PinyinContext,
    library: u8,
    rows: &[(&str, &str, i32)],
) -> Vec<bool> {
    let iter = unsafe { pinyin_begin_add_phrases(context, library) };
    assert!(!iter.is_null());
    let added = rows
        .iter()
        .map(|&(phrase, reading, count)| unsafe {
            pinyin_iterator_add_phrase(iter, cstr(phrase).as_ptr(), cstr(reading).as_ptr(), count)
        })
        .collect();
    unsafe { pinyin_end_add_phrases(iter) };
    added
}

/// Drains an export iterator into owned `(phrase, pinyin, count)` triples,
/// freeing each caller-owned buffer as it goes.
pub(crate) fn drain_phrases(iter: *mut crate::types::ExportIterator) -> Vec<(String, String, i32)> {
    let mut rows = Vec::new();
    while unsafe { crate::iterators::pinyin_iterator_has_next_phrase(iter) } {
        let mut phrase: *mut crate::types::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::types::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        assert!(unsafe {
            crate::iterators::pinyin_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            )
        });
        rows.push((take_exported(phrase), take_exported(pinyin), count));
    }
    rows
}

/// Frees a caller-owned buffer the export iterators hand out.
pub(crate) fn free_exported(ptr: *mut std::ffi::c_char) {
    if !ptr.is_null() {
        // SAFETY: the iterator's buffers come from libc `malloc`, directly
        // or through GLib; `free` releases them.
        unsafe {
            free(ptr.cast());
        }
    }
}

/// Reads a caller-owned, NUL-terminated export string and frees it.
pub(crate) fn take_exported(ptr: *mut std::ffi::c_char) -> String {
    // SAFETY: the iterator hands out NUL-terminated, valid UTF-8 text.
    let text = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_str()
        .expect("UTF-8 export string")
        .to_owned();
    free_exported(ptr);
    text
}

/// Library INDEX's export rows, through `begin`/`has_next`/`get_next`.
pub(crate) fn export_of(
    context: *mut crate::types::PinyinContext,
    index: u32,
) -> Vec<(String, String, i32)> {
    let iter = unsafe { pinyin_begin_get_phrases(context, index) };
    assert!(
        !iter.is_null(),
        "begin never answers NULL on a live context (pinyin.cpp:662-687)"
    );
    let rows = drain_phrases(iter);
    unsafe { pinyin_end_get_phrases(iter) };
    rows
}

unsafe extern "C" {
    fn free(ptr: *mut std::ffi::c_void);
}

/// The packed key of `spelling` at `tone`, as an addon item stores it.
pub(crate) fn packed_key(spelling: &str, tone: u8) -> u16 {
    oxpinyin_core::ChewingKey::from_pinyin(spelling)
        .expect("frozen syllable")
        .with_tone(tone)
        .to_packed()
}

/// A private system whose addon library 4 holds 二簧 with two toned
/// readings, an item whose first word is the surrogate 0xD800, and 三次 indexed under `san ci` with no reading in its body.
pub(crate) fn promotion_system(parent: &std::path::Path) -> std::path::PathBuf {
    use crate::test_support::{AddonItem, addon_system, system_dir};
    let readings: [(&[u16], u32); 2] = [
        (&[packed_key("er", 2), packed_key("huang", 2)], 7),
        (&[packed_key("er", 4), packed_key("huang", 1)], 3),
    ];
    let system = addon_system(
        &system_dir(),
        parent,
        &[
            AddonItem {
                text: &[0x4e8c, 0x7c27],
                readings: &readings,
                indexed_as: &[packed_key("er", 0), packed_key("huang", 0)],
                unigram: 11,
            },
            AddonItem {
                text: &[0xD800, 0x4e8c],
                readings: &[(&[packed_key("ba", 1), packed_key("ma", 3)], 5)],
                indexed_as: &[packed_key("ba", 0), packed_key("ma", 0)],
                unigram: 5,
            },
            AddonItem {
                text: &[0x4e09, 0x6b21],
                readings: &[],
                indexed_as: &[packed_key("san", 0), packed_key("ci", 0)],
                unigram: 9,
            },
        ],
    );
    std::fs::write(
        system.join("interpolation2.text"),
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    )
    .expect("interpolation2.text");
    system
}
