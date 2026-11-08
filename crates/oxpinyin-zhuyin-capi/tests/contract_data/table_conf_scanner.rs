//! table.conf rows, header scanning and file paths (#1175, #1622). Generated from the frozen pin observations.

use crate::bodies::table_conf_scanner as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};

// hash-path/system-directory-zhuyin: MATCH.
#[test]
fn hash_path_system_directory_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/a3c8780529a35ce4.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/bf6473203b51a7f2.rs"));
    run_j(
        "table_conf_scanner::hash_path_system_directory_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "hash-path/system-directory-zhuyin"),
        expected,
        None,
    );
}
// table-conf-abort-database-format-unknown-zhuyin: the pin's abort, reproduced.
#[test]
fn table_conf_abort_database_format_unknown_zhuyin() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_database_format_unknown_zhuyin",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-database-format-unknown-zhuyin");
        },
    );
}
// table-conf-missing-with-dbms-zhuyin: MATCH.
#[test]
fn table_conf_missing_with_dbms_zhuyin() {
    run_j(
        "table_conf_scanner::table_conf_missing_with_dbms_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\sys-X\\table.conf failed.\r\nload SCRATCH\\sys-X\\table.conf failed!\r\n"
        } else {
            b"open SCRATCH/sys-X/table.conf failed.\nload SCRATCH/sys-X/table.conf failed!\n"
        }),
        |k| b::case(k, "table-conf-missing-with-dbms-zhuyin"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}
// table-conf-row-removed-gb-zhuyin: MATCH.
#[test]
fn table_conf_row_removed_gb_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/da09f591808c04d6.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/321ce9cc02dfafc8.rs"));
    run_j(
        "table_conf_scanner::table_conf_row_removed_gb_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-row-removed-gb-zhuyin"),
        expected,
        None,
    );
}

// user-conf-abort-database-format-unknown: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn user_conf_abort_database_format_unknown_zhuyin() {
    run_aborting(
        "table_conf_scanner::user_conf_abort_database_format_unknown_zhuyin",
        Expect::aborts(b"").with_files(&[("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:LMDB|open counter:0|"))]),
        |k| {
            b::case(k, "user-conf-abort-database-format-unknown-zhuyin");
        },
    );
}
