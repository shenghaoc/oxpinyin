//! Save failures and the pin's stderr: blocked, removed and unwritable user files, interrupted saves and saved-profile reloads (#525, #545, #929). Generated from the frozen pin observations.

use crate::bodies::save_failures_stderr as b;
use crate::support::{Expect, run_j};

// stderr-init-missing-system-dir: MATCH.
#[test]
fn stderr_init_missing_system_dir() {
    run_j(
        "save_failures_stderr::stderr_init_missing_system_dir",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open /nonexistent-system-dir/table.conf failed.\r\nload /nonexistent-system-dir/table.conf failed!\r\n"
        } else {
            b"open /nonexistent-system-dir/table.conf failed.\nload /nonexistent-system-dir/table.conf failed!\n"
        }),
        |k| b::case(k, "stderr-init-missing-system-dir"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// stderr-init-system-dir-without-table-conf: MATCH.
#[test]
fn stderr_init_system_dir_without_table_conf() {
    run_j(
        "save_failures_stderr::stderr_init_system_dir_without_table_conf",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\sys-X/table.conf failed.\r\nload SCRATCH\\sys-X/table.conf failed!\r\n"
        } else {
            b"open SCRATCH/sys-X/table.conf failed.\nload SCRATCH/sys-X/table.conf failed!\n"
        }),
        |k| b::case(k, "stderr-init-system-dir-without-table-conf"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}
