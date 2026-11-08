//! Save failures and the pin's stderr: blocked, removed and unwritable user files, interrupted saves and saved-profile reloads (#525, #545, #929). Generated from the frozen pin observations.

use crate::bodies::save_failures_stderr as b;
use crate::support::{Expect, run_j};

// stderr-init-missing-system-dir-zhuyin: MATCH.
#[test]
fn stderr_init_missing_system_dir_zhuyin() {
    run_j(
        "save_failures_stderr::stderr_init_missing_system_dir_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open /nonexistent-system-dir/table.conf failed.\r\nload /nonexistent-system-dir/table.conf failed!\r\n"
        } else {
            b"open /nonexistent-system-dir/table.conf failed.\nload /nonexistent-system-dir/table.conf failed!\n"
        }),
        |k| b::case(k, "stderr-init-missing-system-dir-zhuyin"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}
