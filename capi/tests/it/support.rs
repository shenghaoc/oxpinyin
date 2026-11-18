//! Helpers more than one verification topic uses: the fixture handles, the
//! scratch directories, and the abort and child-process helpers. The
//! exported C functions and types come from the rlib's crate root.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use std::ffi::CStr;

pub use pinyin_capi::*;

use std::ffi::{CString, c_char, c_uint};

use std::path::{Path, PathBuf};

use std::ptr;

/// Serialises the tests that read the pin's process-wide key slots
/// (`pinyin_get_pinyin_key`, `pinyin_get_pinyin_key_rest`): the slots are
/// statics, so two such tests running on different threads overwrite each
/// other's answers. Hold the guard for the whole test.
pub(crate) fn key_slot_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY`.
pub(crate) const DEFAULT_SORT: c_uint = 0x1e;

pub(crate) fn system_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin::user::data::DEFAULT_STORE_EXT)
}

/// A uniquely named scratch dir, removed when the owning guard drops.
pub(crate) fn scratch_dir(prefix: &str, what: &str) -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(".d")
        .tempdir()
        .expect(what);
    (dir.path().to_path_buf(), dir)
}

pub(crate) struct TempUserDir {
    pub(crate) path: PathBuf,
    _dir: tempfile::TempDir,
}

impl TempUserDir {
    pub(crate) fn new(tag: &str) -> Self {
        let (path, dir) = scratch_dir(&format!("oxpinyin-capi-{tag}-"), "temp user dir");
        Self { path, _dir: dir }
    }
}

pub(crate) struct TempSystemDir {
    pub(crate) path: PathBuf,
    _dir: tempfile::TempDir,
}

impl TempSystemDir {
    pub(crate) fn new(tag: &str) -> Self {
        let (path, dir) = scratch_dir(&format!("oxpinyin-capi-system-{tag}-"), "temp system dir");
        // The whole fixture directory of the compiled-in backend
        // (`fixtures/w3/<ext>`: the DBMs, the chunk files, table.conf),
        // so this helper works under every backend gate.
        for entry in std::fs::read_dir(system_dir()).expect("fixture dir") {
            let entry = entry.expect("fixture entry");
            if entry.file_type().expect("file type").is_file() {
                std::fs::copy(entry.path(), path.join(entry.file_name())).expect("copy fixture");
            }
        }
        Self { path, _dir: dir }
    }

    pub(crate) fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.path.join(name), contents).expect("write temp system file");
    }
}

pub(crate) fn cstr(value: impl AsRef<str>) -> CString {
    CString::new(value.as_ref().as_bytes()).expect("no interior NUL")
}

/// Parses `text`, guesses candidates, and returns the pointer to candidate
/// `index` (borrowed into the instance's snapshot until the next guess).
pub(crate) fn candidate(
    instance: *mut PinyinInstance,
    text: &str,
    index: c_uint,
) -> *mut LookupCandidate {
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

pub(crate) fn open(user_dir: &str) -> (*mut PinyinContext, *mut PinyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir);
    let context = unsafe { crate::support::pinyin_init(system.as_ptr(), user.as_ptr()) };
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
pub(crate) const DEATH_TEST: &str = "OXPINYIN_CAPI_DEATH_TEST";

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
/// the facade reproduces a pin `assert`/`abort()` (the class (c) rows
/// retired 2026-10-31 UTC). `name` is the calling test's full path as
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

/// The environment variable that names the user directory a
/// [`run_in_child`] child works in.
pub(crate) const CHILD_DIR: &str = "OXPINYIN_CAPI_CHILD_DIR";

/// What a [`run_in_child`] child left behind.
pub(crate) struct ChildRun {
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
pub(crate) fn run_in_child(name: &str, body: impl FnOnce(&std::path::Path)) -> ChildRun {
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let dir = std::env::var_os(CHILD_DIR).expect("the child's user directory");
        body(std::path::Path::new(&dir));
        std::process::exit(0);
    }
    let scratch = tempfile::tempdir().expect("child scratch");
    let user_dir = scratch.path().join("user");
    std::fs::create_dir(&user_dir).expect("child user directory");
    let output = crate::test_child::run_exact(
        name,
        [
            (DEATH_TEST, std::ffi::OsStr::new(name)),
            (CHILD_DIR, user_dir.as_os_str()),
            ("TMPDIR", scratch.path().as_os_str()),
        ],
    );
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

/// The environment variable that names the file [`assert_aborts_sharing`]
/// maps in the child.
// Unix only: the shared page is an `mmap(MAP_SHARED)` of a file.
#[cfg(unix)]
pub(crate) const DEATH_TEST_PAGE: &str = "OXPINYIN_CAPI_DEATH_TEST_PAGE";

/// The bytes [`assert_aborts_sharing`] shares with the child.
#[cfg(unix)]
pub(crate) const SHARED_PAGE: usize = 4096;

/// [`assert_aborts`], with one page the child writes through and this
/// process reads after the abort: a file holding `init` (zero-padded to
/// [`SHARED_PAGE`] bytes) that the child maps `MAP_SHARED`, so what the
/// facade wrote through pointers into the page before it aborted reaches
/// the file. `body` gets the page's address. Answers the page as the child
/// left it.
// Unix only: the shared page is an `mmap(MAP_SHARED)` of a file.
#[cfg(unix)]
pub(crate) fn assert_aborts_sharing(
    name: &str,
    init: &[u8],
    body: impl FnOnce(*mut u8),
) -> Vec<u8> {
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
pub(crate) fn plant_log(user_dir: &std::path::Path, file: &str, payload: &[u8]) {
    let conf = oxpinyin::user::data::table_info::TableConf::load_from_dir(&system_dir())
        .expect("fixture table.conf");
    let versions = oxpinyin::user::data::user_files::SystemVersions::from_conf(&conf);
    std::fs::write(
        user_dir.join("user.conf"),
        oxpinyin::user::data::user_files::UserTableInfo::conform_to(&versions).to_text(),
    )
    .expect("write user.conf");
    std::fs::write(
        user_dir.join(file),
        oxpinyin::user::data::chunk_format::build_memory_chunk(payload).expect("frame"),
    )
    .expect("write the log");
}

/// Writes a conforming profile for the fixture data set whose user bigram
/// holds `rows`, `(key, value)` verbatim — values the library never writes,
/// such as one too short for its `guint32` total (register row 85).
pub(crate) fn plant_bigram(user_dir: &std::path::Path, rows: Vec<(Vec<u8>, Vec<u8>)>) {
    use oxpinyin::store::WriteStore as _;
    let conf = oxpinyin::user::data::table_info::TableConf::load_from_dir(&system_dir())
        .expect("fixture table.conf");
    let versions = oxpinyin::user::data::user_files::SystemVersions::from_conf(&conf);
    let mut marker = oxpinyin::user::data::user_files::UserTableInfo::conform_to(&versions);
    marker.open_counter = 1;
    std::fs::write(user_dir.join("user.conf"), marker.to_text()).expect("write user.conf");
    oxpinyin::store::DefaultStore::write_user_bigram(
        &user_dir.join(oxpinyin::user::data::user_files::UserDbm::Bigram.file_name()),
        &rows,
    )
    .expect("write the user bigram");
}

/// Sets one row of the user phrase table (`user_phrase_index.bin`) to
/// `tokens`, keyed by the UCS-4 text of `phrase`, in the profile under
/// `user_dir`: the stored state is loaded when a profile exists (a fresh
/// directory starts empty), the row replaced, and the profile saved again
/// in the running backend's format. This is the crafted index of register
/// row 93, whose tokens the library never wrote.
pub(crate) fn set_phrase_table_row(user_dir: &std::path::Path, phrase: &str, tokens: &[u32]) {
    use oxpinyin::store::{RawReadStore as _, WriteStore as _};
    if !user_dir.join("user.conf").exists() {
        let conf = oxpinyin::user::data::table_info::TableConf::load_from_dir(&system_dir())
            .expect("fixture table.conf");
        let versions = oxpinyin::user::data::user_files::SystemVersions::from_conf(&conf);
        let mut marker = oxpinyin::user::data::user_files::UserTableInfo::conform_to(&versions);
        marker.open_counter = 1;
        std::fs::write(user_dir.join("user.conf"), marker.to_text()).expect("write user.conf");
    }
    let path = user_dir.join(oxpinyin::user::data::user_files::UserDbm::PhraseIndex.file_name());
    let mut rows = std::collections::BTreeMap::new();
    if path.exists() {
        oxpinyin::store::DefaultStore::open_user_index(&path)
            .expect("open the phrase table")
            .copy_raw(|key, value| {
                rows.insert(key.to_vec(), value.to_vec());
            })
            .expect("read the phrase table");
    }
    let key = phrase
        .chars()
        .flat_map(|c| (c as u32).to_le_bytes())
        .collect();
    let value = tokens.iter().flat_map(|t| t.to_le_bytes()).collect();
    rows.insert(key, value);
    oxpinyin::store::DefaultStore::write_user_index(&path, |sink| {
        rows.iter().try_for_each(|(key, value)| sink(key, value))
    })
    .expect("write the phrase table");
}

/// A private system directory under `parent`: every fixture file linked in,
/// so a test can replace one library (register row 96).
pub(crate) fn private_system(parent: &std::path::Path) -> PathBuf {
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
pub(crate) fn link_file(target: &std::path::Path, link: &std::path::Path) {
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
/// ([`oxpinyin::user::data::phrase_library::directory_reads_as_a_chunk`]) the
/// pin's `MemoryChunk::mmap` asserts on its header read
/// (`memory_chunk.h:493`, register row 96); elsewhere it is row 76's
/// unmappable library. Answers which.
#[cfg(unix)]
pub(crate) fn directory_library(system: &std::path::Path, file: &str) -> bool {
    let path = system.join(file);
    std::fs::remove_file(&path).expect("unlink the library");
    std::fs::create_dir(&path).expect("directory library");
    std::fs::write(path.join("e".repeat(25)), b"x").expect("fill the directory");
    oxpinyin::user::data::phrase_library::directory_reads_as_a_chunk(&path)
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

    /// An ADD of a six-byte item at slot `0x99`.
    #[must_use]
    pub fn add() -> Vec<u8> {
        record(1, 0x0100_0099, &[6, 0, 0, 0, 1, 0, 0, 0])
    }
}

/// Writes a `user.conf` that conforms to the `table.conf` of `system`, with
/// open counter 0, so the next init keeps the profile under `user_dir`.
pub(crate) fn write_user_conf(user_dir: &std::path::Path, system: &std::path::Path) {
    let conf = oxpinyin::user::data::table_info::TableConf::load_from_dir(system)
        .expect("system table.conf");
    let versions = oxpinyin::user::data::user_files::SystemVersions::from_conf(&conf);
    std::fs::write(
        user_dir.join("user.conf"),
        oxpinyin::user::data::user_files::UserTableInfo::conform_to(&versions).to_text(),
    )
    .expect("write user.conf");
}

/// A tree DBM of the running backend at `path` that holds exactly `rows`,
/// in place of any file there: a system table with records the library
/// never writes.
pub(crate) fn craft_tree(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin::store::{WriteStore as _, WriteTxn as _};
    if path.exists() {
        std::fs::remove_file(path).expect("replace the DBM");
    }
    let store = oxpinyin::store::DefaultStore::create(path).expect("create the DBM");
    store
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("write the DBM rows");
}

/// A hash DBM of the running backend at `path` that holds exactly `rows`, in
/// place of any file there: a system bigram with grams the library never
/// writes.
pub(crate) fn craft_hash(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin::store::{WriteStore as _, WriteTxn as _};
    if path.exists() {
        std::fs::remove_file(path).expect("replace the DBM");
    }
    let store = oxpinyin::store::DefaultStore::create_hash(path).expect("create the DBM");
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
pub(crate) fn craft_user_index(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin::store::WriteStore as _;
    if path.exists() {
        std::fs::remove_file(path).expect("replace the user index");
    }
    oxpinyin::store::DefaultStore::write_user_index(path, |sink| {
        for (key, value) in rows {
            sink(key, value)?;
        }
        Ok(())
    })
    .expect("write the user index");
}

/// Replaces the user bigram at `path` with one that holds exactly `rows`,
/// grams that the library may never write. It is written as the library's
/// own save writes the user bigram: a snapshot on Kyoto Cabinet, a hash
/// file on tkrzw and Berkeley DB.
pub(crate) fn craft_user_bigram(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin::store::WriteStore as _;
    let mut rows = rows.to_vec();
    rows.sort();
    oxpinyin::store::DefaultStore::write_user_bigram(path, &rows).expect("write the user bigram");
}

/// A stored `SingleGram`: the total, then `(token, count)` pairs
/// (`ngram.cpp:48-50`).
#[must_use]
pub(crate) fn gram_bytes(total: u32, items: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = total.to_le_bytes().to_vec();
    for (token, count) in items {
        bytes.extend(token.to_le_bytes());
        bytes.extend(count.to_le_bytes());
    }
    bytes
}

/// A `LOG_MODIFY_HEADER` record: type 4, the token, a `u16` length, then
/// the old and new totals (`phrase_index_logger.h:158-215`).
#[must_use]
pub(crate) fn modify_header(token: u32, old: u32, new: u32) -> Vec<u8> {
    [
        &4_u32.to_le_bytes()[..],
        &token.to_le_bytes(),
        &4_u16.to_le_bytes(),
        &old.to_le_bytes(),
        &new.to_le_bytes(),
    ]
    .concat()
}

/// The tokens `pinyin_lookup_tokens` writes for `text`.
pub(crate) fn lookup(instance: *mut PinyinInstance, text: impl AsRef<[u8]>) -> Vec<u32> {
    let text = CString::new(text.as_ref()).expect("no interior NUL");
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    unsafe { crate::support::pinyin_lookup_tokens(instance, text.as_ptr(), array.cast()) };
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
pub(crate) type LogRecord = (Option<String>, glib_sys::GLogLevelFlags);

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
pub(crate) struct LogCapture {
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

/// One library-1 item of [`items_system`]: its text as code points, its
/// keys and its unigram.
pub(crate) type SystemItem<'a> = (&'a [u32], &'a [u16], u32);

/// A private system under `parent`, linked from `source`, whose library 1
/// holds exactly `items` (tokens 0x01000001 on, in order) under `total`,
/// with matching pinyin and phrase indexes. Answers its path.
pub(crate) fn items_system(
    source: &std::path::Path,
    parent: &std::path::Path,
    items: &[SystemItem<'_>],
    total: u32,
) -> PathBuf {
    use std::collections::BTreeMap;
    let system = parent.join("system");
    std::fs::create_dir_all(&system).expect("private system dir");
    for entry in std::fs::read_dir(source).expect("system dir") {
        let entry = entry.expect("system entry");
        link_file(&entry.path(), &system.join(entry.file_name()));
    }
    let mut content = vec![0_u8; 8];
    let mut offsets = vec![0_u32];
    let mut phonetic: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut phrases: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    for (slot, (text, keys, unigram)) in items.iter().enumerate() {
        let token = 0x0100_0000 | u32::try_from(slot + 1).expect("slot");
        let raw_text: Vec<u8> = text.iter().flat_map(|word| word.to_le_bytes()).collect();
        let raw_keys: Vec<u8> = keys.iter().flat_map(|word| word.to_le_bytes()).collect();
        offsets.push(u32::try_from(content.len()).expect("offset"));
        content.extend([u8::try_from(text.len()).expect("length"), 1]);
        content.extend(unigram.to_le_bytes());
        content.extend(&raw_text);
        content.extend(&raw_keys);
        content.extend(10_u32.to_le_bytes());
        let index: Vec<u8> = keys
            .iter()
            .flat_map(|word| (word & 4095).to_le_bytes())
            .collect();
        for end in (2..index.len()).step_by(2) {
            phonetic.entry(index[..end].to_vec()).or_default();
        }
        let mut item = token.to_le_bytes().to_vec();
        item.extend(&raw_keys);
        item.resize(item.len().div_ceil(4) * 4, 0);
        phonetic.entry(index).or_default().extend(item);
        for end in (4..raw_text.len()).step_by(4) {
            phrases.entry(raw_text[..end].to_vec()).or_default();
        }
        phrases
            .entry(raw_text)
            .or_default()
            .extend(token.to_le_bytes());
    }
    let count = u32::try_from(offsets.len()).expect("offsets");
    let index_two = 17 + count * 4 + 1;
    let content_len = u32::try_from(content.len()).expect("content");
    let mut payload: Vec<u8> = [total, 17, index_two, index_two + content_len + 1]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    payload.push(b'#');
    payload.extend(offsets.iter().flat_map(|value| value.to_le_bytes()));
    payload.push(b'#');
    payload.extend(&content);
    payload.push(b'#');
    let gb_char = system.join("gb_char.bin");
    std::fs::remove_file(&gb_char).expect("unlink gb_char.bin");
    std::fs::write(
        &gb_char,
        oxpinyin::user::data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("gb_char.bin");
    for (name, rows) in [
        ("pinyin_index.bin", phonetic),
        ("phrase_index.bin", phrases),
    ] {
        let path = system.join(name);
        std::fs::remove_file(&path).expect("unlink an index");
        craft_tree(&path, &rows.into_iter().collect::<Vec<_>>());
    }
    system
}

/// One item of [`addon_system`]'s addon library 4.
pub(crate) struct AddonItem<'a> {
    /// The stored UCS-4 words.
    pub text: &'a [u32],
    /// The stored readings: packed keys, tones included, and a count.
    pub readings: &'a [(&'a [u16], u32)],
    /// The keys the addon pinyin index files the item under. An item with
    /// no reading is indexed all the same, as the pin's index and item
    /// bodies are separate files.
    pub indexed_as: &'a [u16],
    /// The stored unigram.
    pub unigram: u32,
}

/// A private system under `parent`, linked from `source`, whose addon
/// library 4 holds exactly `items` (tokens 0x04000001 on, in order), with
/// matching addon pinyin and phrase indexes. Answers its path.
pub(crate) fn addon_system(
    source: &std::path::Path,
    parent: &std::path::Path,
    items: &[AddonItem<'_>],
) -> PathBuf {
    use std::collections::BTreeMap;
    let system = parent.join("system");
    std::fs::create_dir_all(&system).expect("private system dir");
    for entry in std::fs::read_dir(source).expect("system dir") {
        let entry = entry.expect("system entry");
        link_file(&entry.path(), &system.join(entry.file_name()));
    }
    let words =
        |words: &[u16]| -> Vec<u8> { words.iter().flat_map(|word| word.to_le_bytes()).collect() };
    let mut content = vec![0_u8; 8];
    let mut offsets = vec![0_u32];
    let mut phonetic: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut phrases: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut total = 0_u32;
    for (slot, item) in items.iter().enumerate() {
        let token = 0x0400_0000 | u32::try_from(slot + 1).expect("slot");
        let raw_text: Vec<u8> = item
            .text
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        total = total.wrapping_add(item.unigram);
        offsets.push(u32::try_from(content.len()).expect("offset"));
        content.extend([
            u8::try_from(item.text.len()).expect("length"),
            u8::try_from(item.readings.len()).expect("readings"),
        ]);
        content.extend(item.unigram.to_le_bytes());
        content.extend(&raw_text);
        for (keys, count) in item.readings {
            content.extend(words(keys));
            content.extend(count.to_le_bytes());
        }
        let raw_keys = words(item.indexed_as);
        let index: Vec<u8> = item
            .indexed_as
            .iter()
            .flat_map(|word| (word & 4095).to_le_bytes())
            .collect();
        for end in (2..index.len()).step_by(2) {
            phonetic.entry(index[..end].to_vec()).or_default();
        }
        let mut record = token.to_le_bytes().to_vec();
        record.extend(&raw_keys);
        record.resize(record.len().div_ceil(4) * 4, 0);
        phonetic.entry(index).or_default().extend(record);
        for end in (4..raw_text.len()).step_by(4) {
            phrases.entry(raw_text[..end].to_vec()).or_default();
        }
        phrases
            .entry(raw_text)
            .or_default()
            .extend(token.to_le_bytes());
    }
    let count = u32::try_from(offsets.len()).expect("offsets");
    let index_two = 17 + count * 4 + 1;
    let content_len = u32::try_from(content.len()).expect("content");
    let mut payload: Vec<u8> = [total, 17, index_two, index_two + content_len + 1]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    payload.push(b'#');
    payload.extend(offsets.iter().flat_map(|value| value.to_le_bytes()));
    payload.push(b'#');
    payload.extend(&content);
    payload.push(b'#');
    let art = system.join("art.bin");
    std::fs::remove_file(&art).expect("unlink art.bin");
    std::fs::write(
        &art,
        oxpinyin::user::data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("art.bin");
    for (name, rows) in [
        ("addon_pinyin_index.bin", phonetic),
        ("addon_phrase_index.bin", phrases),
    ] {
        let path = system.join(name);
        std::fs::remove_file(&path).expect("unlink an addon index");
        craft_tree(&path, &rows.into_iter().collect::<Vec<_>>());
    }
    system
}

/// The value a test seeds an out-parameter with, to see it left alone.
pub(crate) const UNTOUCHED: u32 = 0x00AB_CDEF;

pub(crate) const NBEST: lookup_candidate_type_t = lookup_candidate_type_t::NBEST_MATCH_CANDIDATE;

pub(crate) const NORMAL: lookup_candidate_type_t = lookup_candidate_type_t::NORMAL_CANDIDATE;

pub(crate) const PREDICTED_BIGRAM: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE;

pub(crate) const PREDICTED_PREFIX: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE;

pub(crate) const PREDICTED_PUNCTUATION: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE;

/// `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN`.
pub(crate) const SEPARATOR_OPTIONS: u32 = 0x18a;

/// A context on `system` and `user`; NULL when init refuses.
pub(crate) fn init_at(system: &Path, user: &Path) -> *mut PinyinContext {
    let system = cstr(system.to_str().expect("UTF-8 path"));
    let user = cstr(user.to_str().expect("UTF-8 path"));
    unsafe { pinyin_init(system.as_ptr(), user.as_ptr()) }
}

/// A context and instance on the mini fixture and `user`.
pub(crate) fn open_at(user: &Path) -> (*mut PinyinContext, *mut PinyinInstance) {
    open(user.to_str().expect("UTF-8 path"))
}

pub(crate) fn alloc(context: *mut PinyinContext) -> *mut PinyinInstance {
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    instance
}

pub(crate) fn close(context: *mut PinyinContext, instance: *mut PinyinInstance) {
    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_fini(context) };
}

pub(crate) fn parse(instance: *mut PinyinInstance, text: impl AsRef<[u8]>) -> usize {
    let text = std::ffi::CString::new(text.as_ref()).expect("no interior NUL");
    unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) }
}

/// The first row of `kind` in the current list.
pub(crate) fn row_of(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
) -> *mut LookupCandidate {
    let index = capture_rows(instance)
        .iter()
        .position(|(found, _, _)| *found == kind)
        .unwrap_or_else(|| panic!("a {kind:?} row is listed"));
    candidate_at(instance, index)
}

/// The first row of `kind` whose text is `text`.
pub(crate) fn row_with(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
    text: &str,
) -> *mut LookupCandidate {
    let index = capture_rows(instance)
        .iter()
        .position(|(found, row, _)| *found == kind && row == text)
        .unwrap_or_else(|| panic!("a {kind:?} row {text} is listed"));
    candidate_at(instance, index)
}

pub(crate) fn candidate_at(instance: *mut PinyinInstance, index: usize) -> *mut LookupCandidate {
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut cand,
        )
    });
    cand
}

pub(crate) fn n_candidate(instance: *mut PinyinInstance) -> u32 {
    let mut count = UNTOUCHED;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut count) });
    count
}

pub(crate) fn unigram(instance: *mut PinyinInstance, token: u32) -> (bool, u32) {
    let mut freq = UNTOUCHED;
    let ret = unsafe { pinyin_token_get_unigram_frequency(instance, token, &raw mut freq) };
    (ret, freq)
}

/// `get_sentence(index)` with a seeded out-pointer: the answer, and the
/// text or `None` when the pointer was left alone.
pub(crate) fn sentence_out(instance: *mut PinyinInstance, index: u8) -> (bool, Option<String>) {
    let seed = UNTOUCHED as usize as *mut c_char;
    let mut out = seed;
    let ret = unsafe { pinyin_get_sentence(instance, index, &raw mut out) };
    if out == seed {
        (ret, None)
    } else {
        (ret, Some(crate::support::take_owned_cstr(out)))
    }
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
    assert!(unsafe { pinyin_save(context) });
    let token = lookup(instance, "龘龘")[0];
    assert_eq!(token >> 24, 7);
    close(context, instance);
    token
}

/// The first NORMAL user row of `dada` on a new context over `user`.
pub(crate) fn user_row_of_dada(user: &Path) -> (*mut PinyinInstance, *mut LookupCandidate) {
    let (_context, instance) = open_at(user);
    assert_eq!(parse(instance, "dada"), 4);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    let rows = capture_rows(instance);
    let index = (0..rows.len())
        .find(|&index| {
            let cand = candidate_at(instance, index);
            rows[index].0 == NORMAL && unsafe { pinyin_is_user_candidate(instance, cand) }
        })
        .expect("a NORMAL user row");
    (instance, candidate_at(instance, index))
}

/// The newline the C stdio of the host writes for `\n` on stderr: the
/// UCRT's text mode writes `\r\n`.
pub(crate) const NL: &str = if cfg!(windows) { "\r\n" } else { "\n" };

pub(crate) fn write_log(user: &Path, name: &str, payload: &[u8]) {
    std::fs::write(
        user.join(name),
        oxpinyin::user::data::chunk_format::build_memory_chunk(payload).expect("frame"),
    )
    .expect("write the log");
}

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

/// The keys of reading `nth` of `token` as little-endian bytes.
pub(crate) fn reading_bytes(instance: *mut PinyinInstance, token: u32, nth: u32) -> Vec<u8> {
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    assert!(unsafe {
        crate::support::pinyin_token_get_nth_pronunciation(instance, token, nth, keys.cast())
    });
    // SAFETY: a live GArray of two-byte elements.
    let bytes = unsafe {
        let view = &*keys;
        std::slice::from_raw_parts(view.data.cast::<u8>(), view.len as usize * 2).to_vec()
    };
    // SAFETY: allocated above, freed once.
    unsafe { glib_sys::g_array_free(keys, 1) };
    bytes
}

/// Library 1's export of `phrase`: each row's reading and count.
pub(crate) fn export_rows_of(
    context: *mut PinyinContext,
    library: u32,
    phrase: &str,
) -> Vec<(String, i32)> {
    crate::support::export_of(context, library)
        .into_iter()
        .filter(|(text, _, _)| text == phrase)
        .map(|(_, reading, count)| (reading, count))
        .collect()
}

/// A private system whose library 1 holds 甲?乙 (the middle word `word`)
/// at 100, 丙 at 1000, 戌 at 10 and 甲?乙丙; its bigram ranks 戌 after
/// 甲?乙.
pub(crate) fn prefix_system(parent: &Path, word: u32) -> std::path::PathBuf {
    let prefix = [0x7532, word, 0x4e59];
    let phrase = [0x7532, word, 0x4e59, 0x4e19];
    let system = crate::support::items_system(
        &system_dir(),
        parent,
        &[
            (&prefix, &[0x1084; 3], 100),
            (&[0x4e19], &[0x1081], 1000),
            (&[0x620c], &[0x1081], 10),
            (&phrase, &[0x1084, 0x1084, 0x1084, 0x1081], 100),
        ],
        2000,
    );
    craft_hash(
        &system.join("bigram.db"),
        &[(
            0x0100_0001_u32.to_le_bytes().to_vec(),
            gram_bytes(1000, &[(0x0100_0003, 1000)]),
        )],
    );
    system
}

/// GLib's `G_LOG_LEVEL_CRITICAL`.
pub(crate) const CRITICAL: glib_sys::GLogLevelFlags = glib_sys::G_LOG_LEVEL_CRITICAL;

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
    assert!(unsafe { crate::support::pinyin_get_n_candidate(instance, &raw mut count) });
    (0..count)
        .map(|index| {
            let mut cand: *mut LookupCandidate = ptr::null_mut();
            assert!(unsafe { pinyin_get_candidate(instance, index, &raw mut cand) });
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(unsafe {
                crate::support::pinyin_get_candidate_type(instance, cand, &raw mut kind)
            });
            let mut text: *const crate::support::GChar = ptr::null();
            assert!(unsafe {
                crate::support::pinyin_get_candidate_string(instance, cand, &raw mut text)
            });
            // SAFETY: a true answer points `text` at the row's borrowed string.
            let text = unsafe { std::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned();
            let nbest = (kind == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE).then(|| {
                let mut index = 0_u8;
                assert!(unsafe {
                    crate::support::pinyin_get_candidate_nbest_index(instance, cand, &raw mut index)
                });
                index
            });
            (kind, text, nbest)
        })
        .collect()
}

/// Imports ROWS into LIBRARY in one batch; answers each add's flag.
pub(crate) fn import_batch(
    context: *mut crate::support::PinyinContext,
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

/// Library INDEX's export rows, through `begin`/`has_next`/`get_next`.
pub(crate) fn export_of(
    context: *mut crate::support::PinyinContext,
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

/// Copies a string the C API handed out and frees it with `g_free`.
pub(crate) fn take_owned_cstr(ptr: *mut c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: The callers pass NUL-terminated buffers this library
    // returned.
    let text = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: `ptr` is a returned string the caller owns; `g_free` is its
    // deallocator.
    unsafe { glib_sys::g_free(ptr.cast()) };
    text
}

/// Drains an export iterator into owned `(phrase, pinyin, count)` triples,
/// freeing each caller-owned buffer as it goes.
pub(crate) fn drain_phrases(
    iter: *mut crate::support::ExportIterator,
) -> Vec<(String, String, i32)> {
    let mut rows = Vec::new();
    while unsafe { crate::support::pinyin_iterator_has_next_phrase(iter) } {
        let mut phrase: *mut crate::support::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::support::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        assert!(unsafe {
            crate::support::pinyin_iterator_get_next_phrase(
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

/// A private system whose addon library 4 holds 二簧 with two toned
/// readings, an item whose first word is the surrogate 0xD800, and 三次 indexed under `san ci` with no reading in its body.
pub(crate) fn promotion_system(parent: &std::path::Path) -> std::path::PathBuf {
    use crate::support::{AddonItem, addon_system, system_dir};
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

/// The packed key of `spelling` at `tone`, as an addon item stores it.
pub(crate) fn packed_key(spelling: &str, tone: u8) -> u16 {
    oxpinyin::ChewingKey::from_pinyin(spelling)
        .expect("frozen syllable")
        .with_tone(tone)
        .to_packed()
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

// The deallocator for buffers the export iterators hand out (GLib and the
// conversion encoder allocate with libc `malloc`, which `g_free` and `free`
// both release on Linux).
unsafe extern "C" {
    fn free(ptr: *mut std::ffi::c_void);
}

pub(crate) const PARITY: u32 = 0x18a;

/// Whether a directory reads as a chunk on this host's temporary file
/// system (register row 96); where it does not, a directory library is row
/// 76's unmappable one (class (b)).
#[cfg(unix)]
pub(crate) fn directory_reads_as_a_chunk_here() -> bool {
    let probe = tempfile::tempdir().expect("probe dir");
    std::fs::write(probe.path().join("e".repeat(25)), b"x").expect("fill the probe");
    oxpinyin::user::data::phrase_library::directory_reads_as_a_chunk(probe.path())
}

/// A gram with one item to `successor` at count 100: above the export
/// threshold, so the walk hands out its row at count 200.
pub(crate) fn whole_gram(successor: u32) -> Vec<u8> {
    [
        100_u32.to_le_bytes(),
        successor.to_le_bytes(),
        100_u32.to_le_bytes(),
    ]
    .concat()
}

/// Opens `user_dir` over the fixture, answering the raw handle.
pub(crate) fn init_fixture_at(user_dir: &std::path::Path) -> *mut crate::support::PinyinContext {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir.to_str().expect("UTF-8 path"));
    unsafe { pinyin_init(system.as_ptr(), user.as_ptr()) }
}

/// The predecessor tokens of the bigram export fixture, in the order the
/// walk loads them (the backend's key order), and the successor both name.
pub(crate) fn export_order_fixture() -> (u32, u32, u32) {
    let scratch = TempUserDir::new("export-order-probe");
    let (context, instance) = open(scratch.path.to_str().expect("UTF-8 path"));
    // The first three characters the fixture's dictionary holds.
    let found: Vec<(&str, u32)> = ["你", "好", "我", "的", "是", "中", "国", "人"]
        .into_iter()
        .filter_map(|text| lookup(instance, text).first().map(|&token| (text, token)))
        .take(3)
        .collect();
    assert_eq!(found.len(), 3, "the fixture holds three of the characters");
    let ((ni_text, ni), (hao_text, hao), (_, wo)) = (found[0], found[1], found[2]);
    unsafe { crate::support::pinyin_fini(context) };
    let probe = TempUserDir::new("export-order");
    crate::support::plant_bigram(
        &probe.path,
        vec![
            (ni.to_le_bytes().to_vec(), whole_gram(hao)),
            (wo.to_le_bytes().to_vec(), whole_gram(hao)),
        ],
    );
    let context = init_fixture_at(&probe.path);
    let iter = unsafe { crate::support::pinyin_begin_get_bigram_phrases(context) };
    let mut rows = Vec::new();
    while unsafe { crate::support::pinyin_bigram_iterator_has_next_phrase(iter) } {
        let mut phrase: *mut crate::support::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::support::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        unsafe {
            crate::support::pinyin_bigram_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            )
        };
        rows.push((take_exported(phrase), take_exported(pinyin), count));
    }
    unsafe { crate::support::pinyin_end_get_bigram_phrases(iter) };
    unsafe { crate::support::pinyin_fini(context) };
    assert_eq!(rows.len(), 2, "{rows:?}");
    if rows[0].0 == format!("{ni_text}{hao_text}") {
        (ni, wo, hao)
    } else {
        (wo, ni, hao)
    }
}

/// A context over a private system directory, with its user dir.
#[cfg(unix)]
pub(crate) fn init_private(
    scratch: &std::path::Path,
) -> (std::path::PathBuf, *mut crate::support::PinyinContext) {
    let system = crate::support::private_system(scratch);
    let user = scratch.join("user");
    std::fs::create_dir_all(&user).expect("user dir");
    let system_c = cstr(system.to_str().expect("UTF-8 path"));
    let user_c = cstr(user.to_str().expect("UTF-8 path"));
    (system, unsafe {
        pinyin_init(system_c.as_ptr(), user_c.as_ptr())
    })
}

// ---------------------------------------------------------------------
// NULL arguments the pin never reads (row 41's non-crashing probes). Each
// test hands valid objects and the one NULL argument the pin ignores; the
// expectations are the pin's answer at 074a2219 (cited beside each), not a
// pin run.
// ---------------------------------------------------------------------

/// A parsed pinyin key and key rest at offset 0 of `text`, borrowed from the
/// process-wide key slots (hold `key_slot_guard`).
pub(crate) fn parsed_key_and_rest(
    instance: *mut PinyinInstance,
    text: &str,
) -> (
    *mut crate::support::ChewingKey,
    *mut crate::support::ChewingKeyRest,
) {
    let input = cstr(text);
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        text.len()
    );
    let mut key = ptr::null_mut();
    assert!(unsafe { crate::support::pinyin_get_pinyin_key(instance, 0, &raw mut key) });
    let mut rest = ptr::null_mut();
    assert!(unsafe { crate::support::pinyin_get_pinyin_key_rest(instance, 0, &raw mut rest) });
    (key, rest)
}

/// A partial item tail (`4 + 8k + 1` bytes): row 98's value, which the pin's
/// `retrieve_all` reads past (class (b)).
pub(crate) fn partial_gram(successor: u32) -> Vec<u8> {
    let mut gram = whole_gram(successor);
    gram.push(1);
    gram
}

/// One call that maps a system library again over a private system
/// directory whose library is a directory (register row 96): the save and
/// the mask-out map `merged.bin` (`pinyin.cpp:955`, `:1264`), the reload of
/// the unloaded GBK library maps `gbk_char.bin` (`:256`).
#[cfg(unix)]
pub(crate) fn remap_a_directory_library(site: &str) -> bool {
    let scratch = tempfile::tempdir().expect("scratch");
    let (system, context) = init_private(scratch.path());
    assert!(!context.is_null());
    match site {
        "mask" => {
            crate::support::directory_library(&system, "merged.bin");
            unsafe { pinyin_mask_out(context, 0xff00_0000, 0x0400_0000) }
        }
        "addon" => {
            crate::support::directory_library(&system, "art.bin");
            unsafe { crate::support::pinyin_load_addon_phrase_library(context, 4) }
        }
        _ => {
            assert!(unsafe { crate::support::pinyin_unload_phrase_library(context, 2) });
            crate::support::directory_library(&system, "gbk_char.bin");
            unsafe { crate::support::pinyin_load_phrase_library(context, 2) }
        }
    }
}

unsafe extern "C" {
    #[link_name = "free"]
    fn libc_free(ptr: *mut core::ffi::c_void);
}

/// Reads a caller-owned rendered string (`g_free`-releasable) and frees it.
pub(crate) fn take(rendered: *mut pinyin_capi::GChar) -> Option<String> {
    if rendered.is_null() {
        return None;
    }
    // SAFETY: The getters return a NUL-terminated UTF-8 buffer or null.
    let text = Some(
        unsafe { CStr::from_ptr(rendered) }
            .to_str()
            .expect("UTF-8 render")
            .to_owned(),
    );
    // SAFETY: The buffer is a returned string this test owns; GLib
    // allocates with malloc, which libc `free` releases.
    unsafe {
        libc_free(rendered.cast());
    }
    text
}
