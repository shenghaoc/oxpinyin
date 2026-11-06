//! Death tests: a test that must end the process re-runs itself as a child
//! and checks how the child died, as the C adapters' `assert_aborts` does.

/// The variable that names the one test a child runs for real.
const DEATH_TEST: &str = "OXPINYIN_STORE_DEATH_TEST";

/// Run `body` in a child copy of this test binary that runs only the test
/// `name` (its full path, as `--exact` takes it), and require the child to
/// die of SIGABRT. Answers the child's stderr.
///
/// # Panics
///
/// When the child does not die of SIGABRT.
#[cfg(unix)]
pub(crate) fn assert_aborts(name: &str, body: impl FnOnce()) -> Vec<u8> {
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
    assert_eq!(
        output.status.signal(),
        Some(6),
        "{name} must die of SIGABRT"
    );
    output.stderr
}
