//! Re-runs one test of this test binary in a child process.
//!
//! A test runs a child only where its parent observes what the child does:
//! an abort, an exit status, the child's output, or the files it leaves.

use std::ffi::OsStr;
use std::process::{Command, Output};

/// Runs the test `name` of this binary alone in a child with `env` set, and
/// returns the child's output.
///
/// `name` is the test's full path, as libtest names it. A name that matches
/// no test runs nothing and exits 0, so a parent whose checks are all
/// negative would pass without the child's body running; the child must
/// report that it ran exactly one test.
pub(crate) fn run_exact<K, V>(name: &str, env: impl IntoIterator<Item = (K, V)>) -> Output
where
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--test-threads=1"])
        .envs(env)
        .output()
        .expect("re-run the test binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().any(|line| line == "running 1 test"),
        "the child ran no test named {name}: {stdout}"
    );
    output
}
