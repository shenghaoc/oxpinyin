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
    inst.candidates[index]
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
    // SAFETY: `sentence` came from `malloc` in `owned_cstr` and is not
    // used after this call.
    unsafe { crate::ffi::free(sentence.cast()) };
    decoded
}

/// The environment variable that names the death test a re-executed test
/// binary runs.
const DEATH_TEST: &str = "OXPINYIN_ZHUYIN_CAPI_DEATH_TEST";

/// Runs `body` in a child process and asserts it dies of SIGABRT, the way
/// the facade reproduces a pin `assert`/`abort()` (the class (c) rows
/// retired 2026-10-31 UTC). `name` is the calling test's full path as
/// libtest prints it (`module::test`): the test binary is re-executed with
/// `--exact name`, and in that child `body` runs and must not return.
#[cfg(unix)]
pub fn assert_aborts(name: &str, body: impl FnOnce()) {
    use std::os::unix::process::ExitStatusExt;
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        body();
        // Reached only when `body` did not abort.
        std::process::exit(0);
    }
    // The child's scratch directories outlive its abort; keep them under
    // one this process removes.
    let scratch = tempfile::tempdir().expect("death-test scratch");
    let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .env("TMPDIR", scratch.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("re-run the test binary");
    assert_eq!(status.signal(), Some(6), "{name} must die of SIGABRT");
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
