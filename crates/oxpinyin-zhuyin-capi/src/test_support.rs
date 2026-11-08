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
    f(inst
        .core
        .user
        .as_ref()
        .expect("instance carries a user store"))
}

/// The token of the window row the candidate pointer was snapshotted from.
pub fn token_of(instance: *mut ZhuyinInstance, cand: *mut LookupCandidate) -> u32 {
    // SAFETY: `cand` was produced by a guess on `instance` and no later
    // guess invalidated the snapshot.
    let inst = unsafe { instance_mut(instance) };
    let row = inst
        .candidates
        .iter()
        .find(|c| std::ptr::eq(*c, cand.cast::<crate::state::CapiCandidate>()))
        .expect("the candidate is in the snapshot");
    inst.core
        .anchored_window
        .as_ref()
        .and_then(|(_, window)| window.get(row.source_index as usize))
        .and_then(oxpinyin_engine::Candidate::token)
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

/// How a death-test child ended.
#[derive(Debug, PartialEq, Eq)]
enum ChildEnd {
    /// The expected abort: SIGABRT on Unix, fast-fail on Windows.
    Aborted,
    /// `body` returned and the child exited 0.
    Returned,
    /// The test harness reported a panic (libtest exits 101).
    Panicked,
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
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        body();
        // Reached only when `body` did not abort.
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
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let dir = std::env::var_os(CHILD_DIR).expect("the child's user directory");
        body(Path::new(&dir));
        std::process::exit(0);
    }
    let scratch = tempfile::tempdir().expect("child scratch");
    let user_dir = scratch.path().join("user");
    std::fs::create_dir(&user_dir).expect("child user directory");
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .env(CHILD_DIR, &user_dir)
        .env("TMPDIR", scratch.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
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

/// Writes a conforming `user.conf` for the fixture data set and `payload`,
/// framed as a `MemoryChunk`, as the user `.dbin` named `file`: the shape a
/// journal-replay test needs, since a non-conforming profile is wiped
/// before any library loads.
pub fn plant_log(user_dir: &std::path::Path, file: &str, payload: &[u8]) {
    let conf = oxpinyin_data::table_info::TableConf::load_from_dir(&system_dir())
        .expect("fixture table.conf");
    let versions = oxpinyin_data::user_files::SystemVersions::from_conf(&conf);
    std::fs::write(
        user_dir.join("user.conf"),
        oxpinyin_data::user_files::UserTableInfo::conform_to(&versions).to_text(),
    )
    .expect("write user.conf");
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
    let conf = oxpinyin_data::table_info::TableConf::load_from_dir(&system_dir())
        .expect("fixture table.conf");
    let state = oxpinyin_user::persistence::UserState {
        bigram: rows,
        ..oxpinyin_user::persistence::UserState::default()
    };
    oxpinyin_user::persistence::save(
        user_dir,
        &state,
        &oxpinyin_user::SystemOriginals::from(std::collections::BTreeMap::new()),
        &oxpinyin_data::user_files::SystemVersions::from_conf(&conf),
        1,
    )
    .expect("write the profile");
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
