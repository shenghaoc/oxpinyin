//! Test helpers that the pinyin and zhuyin C crates share: the death-test
//! and child-process harness, the scratch-directory readers and the
//! segment-log capture. Each crate's `test_support.rs` and
//! `tests/it/support.rs` include this file by `#[path]`. The includer's
//! `abi.rs` defines `DEATH_TEST` and `CHILD_DIR`, the environment variables
//! of that crate's test children.

#![allow(dead_code)]

use super::{CHILD_DIR, DEATH_TEST};

use std::cell::RefCell;

use std::ffi::{CStr, CString, c_char, c_void};

use std::path::{Path, PathBuf};

/// Serialises the tests that read the pin's process-wide key slots
/// (`pinyin_get_pinyin_key`, `pinyin_get_pinyin_key_rest`): the slots are
/// statics, so two such tests running on different threads overwrite each
/// other's answers. Hold the guard for the whole test.
pub(crate) fn key_slot_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn cstr(value: impl AsRef<str>) -> CString {
    CString::new(value.as_ref().as_bytes()).expect("no interior NUL")
}

/// The exit status of a death-test child whose operation panicked. A panic
/// that reaches a non-unwinding `extern "C"` function aborts the process,
/// so without this status an unrelated panic passes as the pin's abort.
pub(crate) const OPERATION_PANICKED: i32 = 86;

/// Arms the strict check in a death-test child: from here on, a panic
/// writes its message to fd 2 (past libtest's capture, which an abort
/// discards) and ends the child with [`OPERATION_PANICKED`].
pub(crate) fn exit_on_panic() {
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), "death-test operation panicked: {info}");
        std::process::exit(OPERATION_PANICKED);
    }));
}

/// How a death-test child ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ChildEnd {
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

pub(crate) fn child_end(status: std::process::ExitStatus) -> ChildEnd {
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
pub(crate) fn wait_for_abort(
    name: &str,
    mut command: std::process::Command,
) -> std::process::Output {
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
/// the facade reproduces a pin `assert`/`abort()`. `name` is the calling test's full path as
/// libtest prints it (`module::test`): the test binary is re-executed with
/// `--exact name`, and in that child `body` runs and must not return.
/// A normal return, a panic and a child that does not start each fail
/// with their own message. Answers the child's stderr.
pub(crate) fn assert_aborts(name: &str, body: impl FnOnce()) -> Vec<u8> {
    assert_aborts_after_setup(name, || (), |()| body())
}

/// [`assert_aborts`] for an operation that needs setup which panics on
/// purpose (a poisoned mutex): `setup` runs before the strict check is
/// armed, then `operation` runs on its value and must abort. A panic in
/// `operation` fails as [`ChildEnd::OperationPanicked`].
pub(crate) fn assert_aborts_after_setup<T>(
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

/// What a [`run_in_child`] child left behind.
pub(crate) struct ChildRun {
    /// The child's fd 2, byte for byte: the C-style `fprintf(stderr, ...)`
    /// lines and GLib's default log handler write there directly.
    pub stderr: Vec<u8>,
    /// The user directory the child was given, still on disk while the
    /// parent holds this value.
    pub user_dir: PathBuf,
    pub(crate) _scratch: tempfile::TempDir,
}

/// [`assert_aborts`] with a directory the parent keeps: `body` gets a
/// fresh, empty directory, and the parent reads the files the child left in
/// it before the abort. Answers the child's stderr and that directory.
pub(crate) fn assert_aborts_in_dir(name: &str, body: impl FnOnce(&std::path::Path)) -> ChildRun {
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
    let output = wait_for_abort(name, command);
    ChildRun {
        stderr: output.stderr,
        user_dir,
        _scratch: scratch,
    }
}

/// The value a test seeds an out-parameter with, to see it left alone.
pub(crate) const UNTOUCHED: u32 = 0x00AB_CDEF;

/// `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN`.
pub(crate) const SEPARATOR_OPTIONS: u32 = 0x18a;

/// Every file of `dir` by name, with its bytes.
pub(crate) fn files_of(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .expect("read the directory")
        .map(|entry| {
            let entry = entry.expect("an entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, std::fs::read(entry.path()).expect("read a file"))
        })
        .collect()
}

/// Whether a directory reads as a chunk on this host's temporary file
/// system; where it does not, the directory library cannot be mapped.
#[cfg(unix)]
pub(crate) fn directory_reads_as_a_chunk_here() -> bool {
    let probe = tempfile::tempdir().expect("probe dir");
    std::fs::write(probe.path().join("e".repeat(25)), b"x").expect("fill the probe");
    oxpinyin::data::phrase_library::directory_reads_as_a_chunk(probe.path())
}

/// The descriptors this process holds on files under `dir`.
// Linux only: /proc/self/fd.
#[cfg(target_os = "linux")]
pub(crate) fn open_descriptors(dir: &std::path::Path) -> usize {
    let dir = dir.canonicalize().expect("system dir");
    std::fs::read_dir("/proc/self/fd")
        .expect("descriptor table")
        .filter_map(Result::ok)
        .filter(|entry| {
            std::fs::read_link(entry.path()).is_ok_and(|target| target.starts_with(&dir))
        })
        .count()
}

/// The committed mini fixture (`fixtures/w3/<backend ext>`).
pub(crate) fn system_dir_cstr() -> CString {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin::data::DEFAULT_STORE_EXT);
    CString::new(dir.to_str().expect("UTF-8 path")).expect("no interior NUL")
}

pub(crate) fn names(dir: &Path) -> Vec<String> {
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

/// The `open <user dir>/user.conf failed.` line a fresh user directory
/// makes `pinyin_init` or `zhuyin_init` write: `UserTableInfo::load` prints it when the
/// marker is missing, with no trailing newline (`table_info.cpp:332` at
/// 074a2219).
pub(crate) fn fresh_user_conf_line(user_dir: &std::path::Path) -> String {
    // `g_build_filename` joins with the host's separator.
    format!(
        "open {}{}user.conf failed.",
        user_dir.display(),
        std::path::MAIN_SEPARATOR
    )
}

pub(crate) type Log = (Option<String>, glib_sys::GLogLevelFlags, String);

thread_local! {
    pub(crate) static LOGS: RefCell<Option<Vec<Log>>> = const { RefCell::new(None) };
}

pub(crate) unsafe extern "C" fn capture_segment_log(
    domain: *const c_char,
    level: glib_sys::GLogLevelFlags,
    message: *const c_char,
    data: *mut c_void,
) {
    let captured = LOGS.with(|logs| {
        let mut logs = logs.borrow_mut();
        let Some(logs) = logs.as_mut() else {
            return false;
        };
        let domain = (!domain.is_null()).then(|| {
            // SAFETY: GLib supplies a live NUL-terminated domain;
            // the default domain was excluded by the NULL check.
            unsafe { CStr::from_ptr(domain) }
                .to_string_lossy()
                .into_owned()
        });
        // SAFETY: GLib's log message is live and NUL-terminated.
        let message = unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned();
        logs.push((domain, level, message));
        true
    });
    if !captured {
        // SAFETY: Forward GLib's unchanged arguments for other test
        // threads, whose thread-local capture is inactive.
        unsafe { glib_sys::g_log_default_handler(domain, level, message, data) };
    }
}
