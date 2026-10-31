//! Shared fixtures for crate tests.

use std::ffi::CString;
use std::os::raw::c_uint;
use std::path::PathBuf;

use crate::candidates::pinyin_get_candidate;
use crate::instance::pinyin_alloc_instance;
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::pinyin_guess_candidates;
use crate::types::{LookupCandidate, PinyinContext, PinyinInstance};

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
    let context = crate::context::oxpinyin_init_for_fixtures(system.as_ptr(), user.as_ptr());
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
#[cfg(unix)]
const DEATH_TEST: &str = "OXPINYIN_CAPI_DEATH_TEST";

/// Runs `body` in a child process and asserts it dies of SIGABRT, the way
/// the facade reproduces a pin `assert`/`abort()` (the class (c) rows
/// retired 2026-10-31 UTC). `name` is the calling test's full path as
/// libtest prints it (`module::test`): the test binary is re-executed with
/// `--exact name`, and in that child `body` runs and must not return.
/// Answers the child's stderr.
#[cfg(unix)]
pub fn assert_aborts(name: &str, body: impl FnOnce()) -> Vec<u8> {
    use std::os::unix::process::ExitStatusExt;
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        body();
        // Reached only when `body` did not abort.
        std::process::exit(0);
    }
    // The child's scratch directories outlive its abort; keep them under
    // one this process removes.
    let scratch = tempfile::tempdir().expect("death-test scratch");
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .env("TMPDIR", scratch.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("re-run the test binary");
    assert_eq!(
        output.status.signal(),
        Some(6),
        "{name} must die of SIGABRT"
    );
    output.stderr
}

/// The environment variable that names the file [`assert_aborts_sharing`]
/// maps in the child.
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
#[cfg(unix)]
pub fn assert_aborts_sharing(name: &str, init: &[u8], body: impl FnOnce(*mut u8)) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::process::ExitStatusExt;
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
        body(page.cast());
        // Reached only when `body` did not abort.
        std::process::exit(0);
    }
    let scratch = tempfile::tempdir().expect("death-test scratch");
    let page = scratch.path().join("shared-page");
    let mut bytes = init.to_vec();
    bytes.resize(SHARED_PAGE, 0);
    std::fs::write(&page, &bytes).expect("write the shared page");
    let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .env(DEATH_TEST_PAGE, &page)
        .env("TMPDIR", scratch.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("re-run the test binary");
    assert_eq!(status.signal(), Some(6), "{name} must die of SIGABRT");
    std::fs::read(&page).expect("read the shared page")
}
