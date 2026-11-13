//! Shared fixtures for crate tests — the zhuyin twin of
//! `oxpinyin-capi/src/test_support.rs`.

use std::ffi::CString;
use std::os::raw::{c_char, c_uint};
use std::path::{Path, PathBuf};

use crate::candidates::{zhuyin_get_candidate, zhuyin_get_n_candidate};
use crate::context::{zhuyin_fini, zhuyin_init};
use crate::instance::zhuyin_alloc_instance;
use crate::parse::zhuyin_parse_more_chewings;
use crate::sentence::zhuyin_guess_candidates_after_cursor;
use crate::state::instance_mut;
use crate::types::{LookupCandidate, ZhuyinContext, ZhuyinInstance};

/// Serialises the tests that read the pin's process-wide key slots
/// (`zhuyin_get_zhuyin_key`, `zhuyin_get_zhuyin_key_rest`): the slots are
/// statics, so two such tests running on different threads overwrite each
/// other's answers. Hold the guard for the whole test.
pub fn key_slot_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
    _dir: tempfile::TempDir,
}

impl TempUserDir {
    /// Creates a fresh, uniquely named scratch dir.
    pub fn new(tag: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(&format!("oxpinyin-zhuyin-capi-{tag}-"))
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

/// Opens the fixture context with no user directory — a NULL one, the
/// corpus driver's shape; `""` would be the working directory — and one
/// instance on it.
pub fn open() -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    open_at(std::ptr::null())
}

/// Opens the fixture context with `user_dir` and one instance on it.
pub fn open_with_user(user_dir: impl AsRef<Path>) -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    let user = cstr(user_dir.as_ref().to_str().expect("UTF-8 path"));
    open_at(user.as_ptr())
}

fn open_at(user: *const c_char) -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let context = zhuyin_init(system.as_ptr(), user);
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
    inst.state.candidates[index]
        .text_str()
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
    f(inst.core.user)
}

/// The token the candidate pointer's row was published with.
pub fn token_of(instance: *mut ZhuyinInstance, cand: *mut LookupCandidate) -> u32 {
    // SAFETY: `cand` was produced by a guess on `instance` and no later
    // guess invalidated the snapshot.
    let inst = unsafe { instance_mut(instance) };
    let row = inst
        .candidates
        .iter()
        .find(|c| std::ptr::eq(*c, cand.cast::<crate::state::CapiCandidate>()))
        .expect("the candidate is in the snapshot");
    row.token
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
    // SAFETY: `sentence` is the returned GLib string, which `g_free`
    // releases, and is not used after this call.
    unsafe { glib_sys::g_free(sentence.cast()) };
    decoded
}

/// The environment variable that names the death test a re-executed test
/// binary runs.
const DEATH_TEST: &str = "OXPINYIN_ZHUYIN_CAPI_DEATH_TEST";

/// How long a death-test child may run. Each body is a few facade calls
/// over the mini fixture; the limit only stops a hung child. The twin of
/// `oxpinyin-capi/src/test_support.rs`.
const CHILD_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

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
    /// The child ran past [`CHILD_LIMIT`] and was killed. Never an abort.
    TimedOut,
    /// Any other end, as the status prints it.
    Other(String),
}

fn child_end(status: std::process::ExitStatus, elapsed: std::time::Duration) -> ChildEnd {
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
    } else if elapsed >= CHILD_LIMIT {
        ChildEnd::TimedOut
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
fn wait_for_abort(name: &str, command: std::process::Command) -> std::process::Output {
    let started = std::time::Instant::now();
    let output = assert_cmd::Command::from_std(command)
        .timeout(CHILD_LIMIT)
        .output()
        .unwrap_or_else(|error| panic!("{name}: the death-test child did not start: {error}"));
    let end = child_end(output.status, started.elapsed());
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
/// A normal return, a panic, a timeout and a child that does not start
/// each fail with their own message. Answers the child's stderr.
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
/// [`run_in_child`] child works in.
const CHILD_DIR: &str = "OXPINYIN_ZHUYIN_CAPI_CHILD_DIR";

/// What a [`run_in_child`] child left behind.
pub struct ChildRun {
    /// The child's fd 2, byte for byte: the C-style `fprintf(stderr, ...)`
    /// lines and GLib's default log handler write there directly.
    pub stderr: Vec<u8>,
    /// The user directory the child was given, still on disk while the
    /// parent holds this value.
    pub user_dir: PathBuf,
    _scratch: tempfile::TempDir,
}

/// Runs `body` in a child process that exits 0 when it returns, the way
/// [`assert_aborts`] runs one that must abort, and answers the child's
/// stderr. `body` gets a fresh, empty user directory the parent created, so
/// the parent can name it in the expected text. `name` is the calling
/// test's full path as libtest prints it (`module::test`). A child that
/// panics or dies fails the parent's assertion on its exit status.
pub fn run_in_child(name: &str, body: impl FnOnce(&Path)) -> ChildRun {
    let (command, user_dir, scratch) = child_command(name, false, body);
    let output = assert_cmd::Command::from_std(command)
        .timeout(CHILD_LIMIT)
        .output()
        .expect("re-run the test binary");
    // libtest reports a child's panic on stdout.
    assert!(
        output.status.success(),
        "{name} child failed ({:?}): {}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    ChildRun {
        stderr: output.stderr,
        user_dir,
        _scratch: scratch,
    }
}

/// [`assert_aborts`] with the user directory of [`run_in_child`]: `body`
/// gets a fresh user directory that stays on disk for the parent, so a test
/// can read the files the child left before its abort.
pub fn abort_in_child(name: &str, body: impl FnOnce(&Path)) -> ChildRun {
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
fn child_command(
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

/// Writes a conforming `user.conf` for the fixture data set and `payload`,
/// framed as a `MemoryChunk`, as the user `.dbin` named `file`: the shape a
/// journal-replay test needs, since a non-conforming profile is wiped
/// before any library loads.
pub fn plant_log(user_dir: &std::path::Path, file: &str, payload: &[u8]) {
    write_user_conf(user_dir, &system_dir());
    std::fs::write(
        user_dir.join(file),
        oxpinyin_data::chunk_format::build_memory_chunk(payload).expect("frame"),
    )
    .expect("write the log");
}

/// Writes a conforming profile for the fixture data set whose user bigram
/// holds `rows`, `(key, value)` verbatim — values the library never writes,
/// such as one too short for its `guint32` total (register row 85).
pub fn plant_bigram(user_dir: &std::path::Path, rows: Vec<(Vec<u8>, Vec<u8>)>) {
    use oxpinyin_store::WriteStore as _;
    let conf = oxpinyin_data::table_info::TableConf::load_from_dir(&system_dir())
        .expect("fixture table.conf");
    let versions = oxpinyin_data::user_files::SystemVersions::from_conf(&conf);
    let mut marker = oxpinyin_data::user_files::UserTableInfo::conform_to(&versions);
    marker.open_counter = 1;
    std::fs::write(user_dir.join("user.conf"), marker.to_text()).expect("write user.conf");
    oxpinyin_store::DefaultStore::write_user_bigram(
        &user_dir.join(oxpinyin_data::user_files::UserDbm::Bigram.file_name()),
        &rows,
    )
    .expect("write the user bigram");
}

/// Writes a `user.conf` in `user_dir` that conforms to the `table.conf` of
/// `system`, so the next init keeps the user files beside it.
pub fn write_user_conf(user_dir: &Path, system: &Path) {
    let conf = oxpinyin_data::table_info::TableConf::load_from_dir(system).expect("table.conf");
    let versions = oxpinyin_data::user_files::SystemVersions::from_conf(&conf);
    std::fs::write(
        user_dir.join("user.conf"),
        oxpinyin_data::user_files::UserTableInfo::conform_to(&versions).to_text(),
    )
    .expect("write user.conf");
}

/// Replaces the tree DBM at `path` with one that holds exactly `rows`, raw
/// keys and values that the library may never write: a system table. Only
/// the tkrzw pinned-data tests craft system tables.
#[cfg(feature = "tkrzw")]
pub fn craft_tree(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin_store::{WriteStore as _, WriteTxn as _};
    if path.exists() {
        std::fs::remove_file(path).expect("replace the DBM");
    }
    let store = oxpinyin_store::DefaultStore::create(path).expect("create the DBM");
    store
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("write the DBM rows");
}

/// Replaces the user index at `path` with one that holds exactly `rows`,
/// raw keys and values that the library may never write. It is written as
/// the library's own save writes a user index: a snapshot on Kyoto
/// Cabinet, a tree file on tkrzw and Berkeley DB.
pub fn craft_user_index(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin_store::WriteStore as _;
    if path.exists() {
        std::fs::remove_file(path).expect("replace the user index");
    }
    oxpinyin_store::DefaultStore::write_user_index(path, |sink| {
        for (key, value) in rows {
            sink(key, value)?;
        }
        Ok(())
    })
    .expect("write the user index");
}

/// Each character of `text` as a little-endian UCS-4 word: the key of the
/// user phrase index.
pub fn ucs4_key(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| u32::from(ch).to_le_bytes())
        .collect()
}

/// `zhuyin_init` on `system` with `user` (NULL when `None`). The answer can
/// be NULL.
pub fn init_on(system: &Path, user: Option<&Path>) -> *mut ZhuyinContext {
    let system = cstr(system.to_str().expect("UTF-8 path"));
    let user = user.map(|user| cstr(user.to_str().expect("UTF-8 path")));
    zhuyin_init(
        system.as_ptr(),
        user.as_ref().map_or(std::ptr::null(), |user| user.as_ptr()),
    )
}

/// Imports `text` with the chewing `reading` and `count` into `library`.
/// Answers the add.
pub fn import(
    context: *mut ZhuyinContext,
    library: u8,
    text: &str,
    reading: &[u8],
    count: i32,
) -> bool {
    let iter = crate::iterators::zhuyin_begin_add_phrases(context, library);
    assert!(!iter.is_null(), "the import iterator");
    let text = cstr(text);
    let reading = std::ffi::CString::new(reading).expect("no interior NUL");
    let added =
        crate::iterators::zhuyin_iterator_add_phrase(iter, text.as_ptr(), reading.as_ptr(), count);
    crate::iterators::zhuyin_end_add_phrases(iter);
    added
}

/// The tokens `zhuyin_lookup_tokens` finds for `text`.
pub fn tokens_of(instance: *mut ZhuyinInstance, text: &str) -> Vec<u32> {
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    crate::dict::zhuyin_lookup_tokens(instance, cstr(text).as_ptr(), array);
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
pub type LogRecord = (Option<String>, u32);

thread_local! {
    static CAPTURED: std::cell::RefCell<Option<Vec<(LogRecord, String)>>> =
        const { std::cell::RefCell::new(None) };
}

/// The process default GLib handler while a test captures: it records on a
/// capturing thread and forwards to GLib's own handler on every other thread.
unsafe extern "C" fn capture_log(
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
pub struct LogCapture;

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

/// Writes the pin-written profile `case_id` of `testdata/pin-profiles.txt`
/// into `dir`: each file at its size, the listed bytes at their offsets, and
/// zero elsewhere.
/// The profiles hold tkrzw files, so only the tkrzw pinned-data tests read them.
#[cfg(feature = "tkrzw")]
pub fn write_pin_profile(case_id: &str, dir: &Path) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/pin-profiles.txt");
    let text = std::fs::read_to_string(&path).expect("testdata/pin-profiles.txt");
    let mut active = false;
    let mut file: Option<(PathBuf, Vec<u8>)> = None;
    let flush = |file: &mut Option<(PathBuf, Vec<u8>)>| {
        if let Some((name, bytes)) = file.take() {
            std::fs::write(name, bytes).expect("write a profile file");
        }
    };
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        if let Some(id) = line.strip_prefix("profile ") {
            flush(&mut file);
            active = id == case_id;
            continue;
        }
        if !active {
            continue;
        }
        if let Some(rest) = line.strip_prefix("file ") {
            flush(&mut file);
            let (name, size) = rest.split_once(' ').expect("file <name> <size>");
            let size = size.parse().expect("a file size");
            file = Some((dir.join(name), vec![0; size]));
        } else {
            let (offset, hex) = line.split_once(' ').expect("<offset> <hex>");
            let offset: usize = offset.parse().expect("a byte offset");
            let (_, bytes) = file.as_mut().expect("a run follows its file line");
            for (at, pair) in hex.as_bytes().chunks(2).enumerate() {
                let pair = std::str::from_utf8(pair).expect("hex");
                bytes[offset + at] = u8::from_str_radix(pair, 16).expect("hex");
            }
        }
    }
    flush(&mut file);
    assert!(dir.join("user.conf").is_file(), "no profile {case_id}");
}

/// A private system directory under `parent`: every fixture file linked in,
/// so a test can replace one library (register row 96).
pub fn private_system(parent: &std::path::Path) -> PathBuf {
    let dir = parent.join("system");
    std::fs::create_dir_all(&dir).expect("private system dir");
    for entry in std::fs::read_dir(system_dir()).expect("fixture dir") {
        let entry = entry.expect("fixture entry");
        link_file(&entry.path(), &dir.join(entry.file_name()));
    }
    dir
}

/// A symbolic link at `link` to `target`, on every OS; on Windows the
/// process needs the symbolic-link privilege (Developer Mode or an elevated
/// process), and the fixture is never copied in its place.
pub fn link_file(target: &std::path::Path, link: &std::path::Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("link a fixture file");
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(target, link) {
        // ERROR_PRIVILEGE_NOT_HELD
        assert_ne!(
            error.raw_os_error(),
            Some(1314),
            "link a fixture file {}: this process lacks the symbolic-link privilege",
            link.display()
        );
        panic!("link a fixture file {}: {error}", link.display());
    }
}

/// Replaces the system library `file` with a non-empty directory whose entry
/// name is 25 bytes long. Where a directory reads as a chunk
/// ([`oxpinyin_data::phrase_library::directory_reads_as_a_chunk`]) the
/// pin's `MemoryChunk::mmap` asserts on its header read
/// (`memory_chunk.h:493`, register row 96); elsewhere it is row 76's
/// unmappable library. Answers which.
#[cfg(unix)]
pub fn directory_library(system: &std::path::Path, file: &str) -> bool {
    let path = system.join(file);
    std::fs::remove_file(&path).expect("unlink the library");
    std::fs::create_dir(&path).expect("directory library");
    std::fs::write(path.join("e".repeat(25)), b"x").expect("fill the directory");
    oxpinyin_data::phrase_library::directory_reads_as_a_chunk(&path)
}

/// Hand-built `PhraseIndexLogger` records for the journal-replay tests
/// (`phrase_index_logger.h:158-215` layout, little-endian).
pub mod logs {
    fn record(kind: u32, token: u32, tail: &[u8]) -> Vec<u8> {
        [&kind.to_le_bytes()[..], &token.to_le_bytes(), tail].concat()
    }

    /// A `MODIFY_HEADER` carrying token `0x01000001` (row 91).
    #[must_use]
    pub fn header_token() -> Vec<u8> {
        record(4, 0x0100_0001, &[4, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    }

    /// A null-token `MODIFY_HEADER`.
    #[must_use]
    pub fn header() -> Vec<u8> {
        record(4, 0, &[4, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    }

    /// An ADD of a one-byte payload (row 108).
    #[must_use]
    pub fn short_add() -> Vec<u8> {
        record(1, 0x0100_0005, &[1, 0, 1])
    }

    /// An ADD of a six-byte item at slot `0x99`.
    #[must_use]
    pub fn add() -> Vec<u8> {
        record(1, 0x0100_0099, &[6, 0, 0, 0, 1, 0, 0, 0])
    }

    /// A MODIFY of slot 1 whose old chunk no item matches: `merge` returns
    /// there (`phrase_index.cpp:483-484`).
    #[must_use]
    pub fn mismatched_modify() -> Vec<u8> {
        record(
            3,
            0x0100_0001,
            &[6, 0, 6, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        )
    }
}

mod death_tests {
    /// A panic that reaches this non-unwinding function aborts the process,
    /// as a facade panic inside an entry point would.
    extern "C" fn panic_in_an_entry_point() {
        panic!("an unrelated panic");
    }

    /// The witness: a death test whose operation panics at a C boundary
    /// fails with its own result, not as the pin's abort.
    #[test]
    fn a_panic_at_the_c_boundary_is_not_the_pins_abort() {
        const NAME: &str =
            "test_support::death_tests::a_panic_at_the_c_boundary_is_not_the_pins_abort";
        let parent = std::panic::catch_unwind(|| {
            super::assert_aborts(NAME, || panic_in_an_entry_point());
        });
        let message = parent.expect_err("a panic must not pass as an abort");
        let message = message
            .downcast_ref::<String>()
            .expect("the helper's assertion message");
        assert!(message.contains("OperationPanicked"), "{message}");
        assert!(message.contains("an unrelated panic"), "{message}");
    }

    /// The control: an operation that aborts directly still passes.
    #[test]
    fn a_direct_abort_passes() {
        super::assert_aborts("test_support::death_tests::a_direct_abort_passes", || {
            std::process::abort()
        });
    }
}
