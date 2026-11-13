//! Shared fixtures for crate tests.

use std::ffi::CString;
use std::os::raw::c_uint;
use std::path::PathBuf;

use crate::candidates::pinyin_get_candidate;
use crate::instance::pinyin_alloc_instance;
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::pinyin_guess_candidates;
use crate::types::{LookupCandidate, PinyinContext, PinyinInstance};

// Oxpinyin retains the owner for a surviving instance. This is a safety
// contract, not a pin oracle: the returned public handle keeps its identity
// but is no longer live and must not be dereferenced.
pub(crate) fn finalize_public_context(context: *mut PinyinContext, instance: *mut PinyinInstance) {
    assert_eq!(crate::instance::pinyin_get_context(instance), context);
    crate::context::pinyin_fini(context);
    assert!(!crate::live::is_live(context));
    assert_eq!(crate::instance::pinyin_get_context(instance), context);
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

pub fn system_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

/// A uniquely named scratch dir, removed when the owning guard drops.
fn scratch_dir(prefix: &str, what: &str) -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(".d")
        .tempdir()
        .expect(what);
    (dir.path().to_path_buf(), dir)
}

pub struct TempUserDir {
    pub(crate) path: PathBuf,
    _dir: tempfile::TempDir,
}

impl TempUserDir {
    pub(crate) fn new(tag: &str) -> Self {
        let (path, dir) = scratch_dir(&format!("oxpinyin-capi-{tag}-"), "temp user dir");
        Self { path, _dir: dir }
    }
}

pub struct TempSystemDir {
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
    let context = crate::context::pinyin_init(system.as_ptr(), user.as_ptr());
    assert!(
        !context.is_null(),
        "fixture init must open the mini fixture"
    );
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    (context, instance)
}

/// The environment variable that names the death test a re-executed test
/// binary runs.
const DEATH_TEST: &str = "OXPINYIN_CAPI_DEATH_TEST";

/// How long a death-test child may run. Each body is a few facade calls
/// over the mini fixture; the limit only stops a hung child.
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
const CHILD_DIR: &str = "OXPINYIN_CAPI_CHILD_DIR";

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
pub fn run_in_child(name: &str, body: impl FnOnce(&std::path::Path)) -> ChildRun {
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let dir = std::env::var_os(CHILD_DIR).expect("the child's user directory");
        body(std::path::Path::new(&dir));
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

/// Sets one row of the user phrase table (`user_phrase_index.bin`) to
/// `tokens`, keyed by the UCS-4 text of `phrase`, in the profile under
/// `user_dir`: the stored state is loaded when a profile exists (a fresh
/// directory starts empty), the row replaced, and the profile saved again
/// in the running backend's format. This is the crafted index of register
/// row 93, whose tokens the library never wrote.
pub fn set_phrase_table_row(user_dir: &std::path::Path, phrase: &str, tokens: &[u32]) {
    use oxpinyin_store::{RawReadStore as _, WriteStore as _};
    if !user_dir.join("user.conf").exists() {
        let conf = oxpinyin_data::table_info::TableConf::load_from_dir(&system_dir())
            .expect("fixture table.conf");
        let versions = oxpinyin_data::user_files::SystemVersions::from_conf(&conf);
        let mut marker = oxpinyin_data::user_files::UserTableInfo::conform_to(&versions);
        marker.open_counter = 1;
        std::fs::write(user_dir.join("user.conf"), marker.to_text()).expect("write user.conf");
    }
    let path = user_dir.join(oxpinyin_data::user_files::UserDbm::PhraseIndex.file_name());
    let mut rows = std::collections::BTreeMap::new();
    if path.exists() {
        oxpinyin_store::DefaultStore::open_user_index(&path)
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
    oxpinyin_store::DefaultStore::write_user_index(&path, |sink| {
        rows.iter().try_for_each(|(key, value)| sink(key, value))
    })
    .expect("write the phrase table");
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

/// Writes a `user.conf` that conforms to the `table.conf` of `system`, with
/// open counter 0, so the next init keeps the profile under `user_dir`.
pub fn write_user_conf(user_dir: &std::path::Path, system: &std::path::Path) {
    let conf =
        oxpinyin_data::table_info::TableConf::load_from_dir(system).expect("system table.conf");
    let versions = oxpinyin_data::user_files::SystemVersions::from_conf(&conf);
    std::fs::write(
        user_dir.join("user.conf"),
        oxpinyin_data::user_files::UserTableInfo::conform_to(&versions).to_text(),
    )
    .expect("write user.conf");
}

/// A tree DBM of the running backend at `path` that holds exactly `rows`,
/// in place of any file there: a system table with records the library
/// never writes.
pub fn craft_tree(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
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

/// A hash DBM of the running backend at `path` that holds exactly `rows`, in
/// place of any file there: a system bigram with grams the library never
/// writes.
pub fn craft_hash(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin_store::{WriteStore as _, WriteTxn as _};
    if path.exists() {
        std::fs::remove_file(path).expect("replace the DBM");
    }
    let store = oxpinyin_store::DefaultStore::create_hash(path).expect("create the DBM");
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
pub fn craft_user_index(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
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

/// Replaces the user bigram at `path` with one that holds exactly `rows`,
/// grams that the library may never write. It is written as the library's
/// own save writes the user bigram: a snapshot on Kyoto Cabinet, a hash
/// file on tkrzw and Berkeley DB.
pub fn craft_user_bigram(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin_store::WriteStore as _;
    let mut rows = rows.to_vec();
    rows.sort();
    oxpinyin_store::DefaultStore::write_user_bigram(path, &rows).expect("write the user bigram");
}

/// Every record of the user container at `path`, sorted. `hash` reads the
/// user bigram; otherwise the file is a user index. Each is read as the
/// library reads it on the running backend.
pub fn store_rows(path: &std::path::Path, hash: bool) -> Vec<(Vec<u8>, Vec<u8>)> {
    use oxpinyin_store::RawReadStore as _;
    use std::ops::Bound;
    let store = if hash {
        oxpinyin_store::DefaultStore::open_user_bigram(path)
    } else {
        oxpinyin_store::DefaultStore::open_user_index(path)
    }
    .unwrap_or_else(|error| panic!("open {}: {error:?}", path.display()));
    let mut rows = Vec::new();
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows.push((key.to_vec(), value.to_vec()));
            Ok(())
        })
        .expect("read the container");
    rows.sort();
    rows
}

/// A stored `SingleGram`: the total, then `(token, count)` pairs
/// (`ngram.cpp:48-50`).
#[must_use]
pub fn gram_bytes(total: u32, items: &[(u32, u32)]) -> Vec<u8> {
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
pub fn modify_header(token: u32, old: u32, new: u32) -> Vec<u8> {
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
pub fn lookup(instance: *mut PinyinInstance, text: impl AsRef<[u8]>) -> Vec<u32> {
    let text = CString::new(text.as_ref()).expect("no interior NUL");
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    crate::dict::pinyin_lookup_tokens(instance, text.as_ptr(), array.cast());
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

/// Writes the files of the pin-written profile `case` from
/// `testdata/pin-profiles.txt` into `dir`, byte for byte.
/// The profiles hold tkrzw files, so only the tkrzw pinned-data tests read them.
#[cfg(feature = "tkrzw")]
pub fn write_pin_profile(case: &str, dir: &std::path::Path) {
    let text = include_str!("../testdata/pin-profiles.txt");
    let mut in_case = false;
    let mut file: Option<(String, Vec<u8>)> = None;
    let mut found = false;
    let flush = |file: &mut Option<(String, Vec<u8>)>| {
        if let Some((name, bytes)) = file.take() {
            std::fs::write(dir.join(name), bytes).expect("write a profile file");
        }
    };
    for line in text.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut words = line.split(' ');
        match words.next() {
            Some("profile") => {
                flush(&mut file);
                in_case = words.next() == Some(case);
                found |= in_case;
            }
            Some("file") if in_case => {
                flush(&mut file);
                let name = words.next().expect("a file name").to_owned();
                let size = words
                    .next()
                    .expect("a size")
                    .parse()
                    .expect("a decimal size");
                file = Some((name, vec![0; size]));
            }
            Some(offset) if in_case => {
                let (_, bytes) = file.as_mut().expect("a byte run follows a file line");
                let at: usize = offset.parse().expect("a decimal offset");
                let hex = words.next().expect("the run's bytes");
                for (index, pair) in hex.as_bytes().chunks(2).enumerate() {
                    let pair = std::str::from_utf8(pair).expect("ASCII hex");
                    bytes[at + index] = u8::from_str_radix(pair, 16).expect("hex byte");
                }
            }
            _ => {}
        }
    }
    flush(&mut file);
    assert!(found, "no profile {case} in pin-profiles.txt");
}

/// One library-1 item of [`items_system`]: its text as code points, its
/// keys and its unigram.
pub type SystemItem<'a> = (&'a [u32], &'a [u16], u32);

/// A private system under `parent`, linked from `source`, whose library 1
/// holds exactly `items` (tokens 0x01000001 on, in order) under `total`,
/// with matching pinyin and phrase indexes. Answers its path.
pub fn items_system(
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
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame"),
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
