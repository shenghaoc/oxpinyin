//! Death tests: a test that must end the process re-runs itself as a child
//! and checks how the child died, as the C adapters' `assert_aborts` does.

/// The variable that names the one test a child runs for real.
const DEATH_TEST: &str = "OXPINYIN_STORE_DEATH_TEST";

/// Run `body` in a child copy of this test binary that runs only the test
/// `name` (its full path, as `--exact` takes it), and require the child to
/// abort (SIGABRT on Unix, fast-fail on Windows). Answers the child's stderr.
///
/// # Panics
///
/// When the child does not abort as required by the host platform.
pub(crate) fn assert_aborts(name: &str, body: impl FnOnce()) -> Vec<u8> {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        body();
        // Reached only when `body` did not abort.
        std::process::exit(0);
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("re-run the test binary");
    #[cfg(unix)]
    assert_eq!(
        output.status.signal(),
        Some(6),
        "{name} must die of SIGABRT"
    );
    #[cfg(windows)]
    // Rust 1.99.1, library/std/src/sys/pal/windows/mod.rs:269, implements
    // __fastfail(FAST_FAIL_FATAL_APP_EXIT) with int 0x29 (arm64:275, brk).
    // https://github.com/rust-lang/rust/blob/1.99.1/library/std/src/sys/pal/windows/mod.rs#L269
    // User-mode __fastfail raises 0xC0000409 (STATUS_STACK_BUFFER_OVERRUN):
    // https://learn.microsoft.com/en-us/cpp/intrinsics/fastfail
    // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-erref/596a1078-e883-4972-9bbc-49e60bebca55
    assert_eq!(
        output.status.code(),
        Some(i32::from_ne_bytes(0xC000_0409_u32.to_ne_bytes())),
        "{name} must die of STATUS_STACK_BUFFER_OVERRUN"
    );
    output.stderr
}
