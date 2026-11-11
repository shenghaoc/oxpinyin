//! Shared fixtures for the `tests/abi` suites.
//!
//! The twin helpers in `src/test_support.rs` serve the inline white-box
//! suites; Rust cannot share a `#[cfg(test)]` module with an integration
//! target, so this file deliberately mirrors the small surface the
//! black-box suites need (`TempUserDir`, `cstr`, `open`).

use std::ffi::CString;
use std::path::PathBuf;

use pinyin_capi::{PinyinContext, PinyinInstance, pinyin_alloc_instance, pinyin_init};

pub fn system_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

pub struct TempUserDir {
    pub path: PathBuf,
    _dir: tempfile::TempDir,
}

impl TempUserDir {
    pub fn new(tag: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(&format!("oxpinyin-capi-{tag}-"))
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

pub fn open(user_dir: &str) -> (*mut PinyinContext, *mut PinyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir);
    let context = pinyin_init(system.as_ptr(), user.as_ptr());
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

/// Runs `body` in a child process and requires an abort:
/// SIGABRT on Unix, fast-fail on Windows. This is how
/// the facade reproduces a pin `assert`/`abort()` (register rows retired
/// from class (c), 2026-10-31 UTC). `name` is the calling test's full path
/// as libtest prints it (`contract::name`); the test binary is re-executed
/// with `--exact name`, and in that child `body` runs and must not return.
pub fn assert_aborts(name: &str, body: impl FnOnce()) {
    #[cfg(unix)]
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
        .env(if cfg!(windows) { "TMP" } else { "TMPDIR" }, scratch.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("re-run the test binary");
    #[cfg(unix)]
    assert_eq!(status.signal(), Some(6), "{name} must die of SIGABRT");
    #[cfg(windows)]
    // Rust 1.99.1, library/std/src/sys/pal/windows/mod.rs:269, implements
    // __fastfail(FAST_FAIL_FATAL_APP_EXIT) with int 0x29 (arm64:275, brk).
    // https://github.com/rust-lang/rust/blob/1.99.1/library/std/src/sys/pal/windows/mod.rs#L269
    // User-mode __fastfail raises 0xC0000409 (STATUS_STACK_BUFFER_OVERRUN):
    // https://learn.microsoft.com/en-us/cpp/intrinsics/fastfail
    // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-erref/596a1078-e883-4972-9bbc-49e60bebca55
    assert_eq!(
        status.code(),
        Some(i32::from_ne_bytes(0xC000_0409_u32.to_ne_bytes())),
        "{name} must die of STATUS_STACK_BUFFER_OVERRUN"
    );
}
