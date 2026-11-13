//! Death tests: a test that must end the process re-runs itself as a child
//! and checks how the child died, as the C adapters' `assert_aborts` does.

/// The variable that names the one test a child runs for real.
const DEATH_TEST: &str = "OXPINYIN_STORE_DEATH_TEST";

/// The exit status of a death-test child whose operation panicked. A panic
/// that reaches a non-unwinding `extern "C"` function or cxx's unwind guard
/// aborts the process, so without this status an unrelated panic passes as
/// the pin's abort.
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

/// Run `body` in a child copy of this test binary that runs only the test
/// `name` (its full path, as `--exact` takes it), and require the child to
/// abort (SIGABRT on Unix, fast-fail on Windows). Answers the child's stderr.
///
/// # Panics
///
/// When the child does not abort as required by the host platform, and
/// when `body` panics.
pub(crate) fn assert_aborts(name: &str, body: impl FnOnce()) -> Vec<u8> {
    assert_aborts_after_setup(name, || (), |()| body())
}

/// [`assert_aborts`] for an operation that needs setup which may panic:
/// `setup` runs before the strict check is armed, then `operation` runs on
/// its value and must abort.
///
/// # Panics
///
/// As [`assert_aborts`], for a panic in `operation`.
pub(crate) fn assert_aborts_after_setup<T>(
    name: &str,
    setup: impl FnOnce() -> T,
    operation: impl FnOnce(T),
) -> Vec<u8> {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let value = setup();
        exit_on_panic();
        operation(value);
        // Reached only when `operation` did not abort.
        std::process::exit(0);
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("re-run the test binary");
    assert_ne!(
        output.status.code(),
        Some(OPERATION_PANICKED),
        "{name}: the operation panicked: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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

mod tests {
    /// A panic that reaches this non-unwinding function aborts the process,
    /// as a panic inside a native callback would.
    extern "C" fn panic_in_a_c_boundary() {
        panic!("an unrelated panic");
    }

    /// The witness: a death test whose operation panics at a C boundary
    /// fails with its own message, not as the pin's abort.
    #[test]
    fn a_panic_at_the_c_boundary_is_not_the_pins_abort() {
        const NAME: &str = "death_test::tests::a_panic_at_the_c_boundary_is_not_the_pins_abort";
        let parent = std::panic::catch_unwind(|| {
            super::assert_aborts(NAME, || panic_in_a_c_boundary());
        });
        let message = parent.expect_err("a panic must not pass as an abort");
        let message = message
            .downcast_ref::<String>()
            .expect("the helper's assertion message");
        assert!(message.contains("the operation panicked"), "{message}");
        assert!(message.contains("an unrelated panic"), "{message}");
    }
}
