//! The contract runner's own tests: probe replays and nested-child deadlines.

use crate::support::{Bounded, Expect, isolated, run};
use std::time::{Duration, Instant};

/// A probe that aborts, then one that answers. The second's replay returns the
/// first's recorded `None` instead of running it again, so it reaches the
/// second probe and answers.
#[test]
fn a_probe_after_an_aborting_probe_answers() {
    run(
        "harness_tests::a_probe_after_an_aborting_probe_answers",
        Expect::RESULT,
        |_| {
            let aborting = isolated(|| std::process::abort());
            let answering = isolated(|| b"answered".to_vec());
            (aborting, answering)
        },
        (None, Some(b"answered".to_vec())),
        None,
    );
}

/// Set in the child of [`a_nested_child_past_its_deadline_is_killed_and_reaped`]:
/// that child never exits.
const HANG: &str = "OXPINYIN_CONTRACT_HARNESS_HANG";

/// A nested child that never exits is killed at its deadline and reaped.
#[test]
fn a_nested_child_past_its_deadline_is_killed_and_reaped() {
    if std::env::var_os(HANG).is_some() {
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args([
            "--exact",
            "harness_tests::a_nested_child_past_its_deadline_is_killed_and_reaped",
            "--test-threads=1",
        ])
        .env(HANG, "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let started = Instant::now();
    let overdue = Bounded::spawn("the hanging child", &mut command, Duration::from_secs(2))
        .wait()
        .expect_err("the child never exits");
    assert!(
        overdue.error.is_none(),
        "the deadline ended the wait: {overdue}"
    );
    let status = overdue.reaped.expect("the killed child was reaped");
    assert!(!status.success(), "the child was killed: {status:?}");
    assert!(started.elapsed() < Duration::from_secs(60));
}
