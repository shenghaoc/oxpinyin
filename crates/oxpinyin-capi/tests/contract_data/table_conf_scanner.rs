//! table.conf rows, header scanning and file paths (#1175, #1622). Generated from the frozen pin observations.

use crate::bodies::table_conf_scanner as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};
// hash-path/system-filename-pinyin: MATCH.
#[test]
fn hash_path_system_filename_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/1b1c9abae2956469.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/77874941c01c662a.rs"));
    run_j(
        "table_conf_scanner::hash_path_system_filename_pinyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "hash-path/system-filename-pinyin"),
        expected,
        None,
    );
}
// table-conf-1378-unigram-lambda-one: MATCH.
#[test]
fn table_conf_1378_unigram_lambda_one() {
    run_j(
        "table_conf_scanner::table_conf_1378_unigram_lambda_one",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-1378-unigram-lambda-one"),
        raw!(M(&[
            ("guessed", B(true)),
            ("has_sentence", B(true)),
            ("logs", L(&[])),
            ("parsed", I(2))
        ])),
        None,
    );
}
// table-conf-abort-database-format-unknown: the pin's abort, reproduced.
#[test]
fn table_conf_abort_database_format_unknown() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_database_format_unknown",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-database-format-unknown");
        },
    );
}
// table-conf-addon-cleanup: MATCH.
#[test]
fn table_conf_addon_cleanup() {
    run_j(
        "table_conf_scanner::table_conf_addon_cleanup",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-addon-cleanup"),
        raw!(M(&[
            ("addon", B(false)),
            ("ctx", B(true)),
            ("logs", L(&[]))
        ])),
        None,
    );
}
// table-conf-missing-with-dbms: MATCH.
#[test]
fn table_conf_missing_with_dbms() {
    run_j(
        "table_conf_scanner::table_conf_missing_with_dbms",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\sys-X\\table.conf failed.\r\nload SCRATCH\\sys-X\\table.conf failed!\r\n"
        } else {
            b"open SCRATCH/sys-X/table.conf failed.\nload SCRATCH/sys-X/table.conf failed!\n"
        }),
        |k| b::case(k, "table-conf-missing-with-dbms"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}
// table-conf-row-removed-gb: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn table_conf_row_removed_gb() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/6ce775a48c590ef8.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/1e687a68863d8a1d.rs"));
    run_j(
        "table_conf_scanner::table_conf_row_removed_gb",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-row-removed-gb"),
        expected,
        None,
    );
}
// table-conf-user-file-absolute: MATCH.
// Unix only: the table.conf row spells a POSIX absolute path.
#[cfg(unix)]
#[test]
fn table_conf_user_file_absolute() {
    run_j(
        "table_conf_scanner::table_conf_user_file_absolute",
        Expect::RESULT,
        |k| b::case(k, "table-conf-user-file-absolute"),
        raw!(include!("fixtures/table_conf_scanner/2790d49166bce1aa.rs")),
        None,
    );
}

// user-conf-abort-database-format-unknown: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn user_conf_abort_database_format_unknown() {
    run_aborting(
        "table_conf_scanner::user_conf_abort_database_format_unknown",
        Expect::aborts(b"").with_files(&[("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:LMDB|open counter:0|"))]),
        |k| {
            b::case(k, "user-conf-abort-database-format-unknown");
        },
    );
}
