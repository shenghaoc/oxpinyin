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

// hash-path/user-directory-zhuyin: MATCH.
#[test]
fn hash_path_user_directory_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/a3c8780529a35ce4.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/bf6473203b51a7f2.rs"));
    run_j(
        "table_conf_scanner::hash_path_user_directory_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X#type=kct\\user.conf failed."
        } else {
            b"open SCRATCH/user-X#type=kct/user.conf failed."
        }),
        |k| b::case(k, "hash-path/user-directory-zhuyin"),
        expected,
        None,
    );
}

// hash-path/user-filename-zhuyin: MATCH.
#[test]
fn hash_path_user_filename_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/aedf6d312934b96e.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/053650cb75fa30e1.rs"));
    run_j(
        "table_conf_scanner::hash_path_user_filename_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "hash-path/user-filename-zhuyin"),
        expected,
        None,
    );
}

// table-conf-1378-unigram-lambda-one-zhuyin: MATCH.
#[test]
fn table_conf_1378_unigram_lambda_one_zhuyin() {
    run_j(
        "table_conf_scanner::table_conf_1378_unigram_lambda_one_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-1378-unigram-lambda-one-zhuyin"),
        raw!(M(&[
            ("guessed", B(true)),
            ("has_sentence", B(true)),
            ("logs", L(&[])),
            ("parsed", I(3))
        ])),
        None,
    );
}

// table-conf-919-versions-int-wrap-zhuyin: MATCH.
#[test]
fn table_conf_919_versions_int_wrap_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/d1dd4194020e7862.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/0fd295ac1b22781c.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_versions_int_wrap_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-versions-int-wrap-zhuyin"),
        expected,
        None,
    );
}

// table-conf-919-versions-negative-zhuyin: MATCH.
#[test]
fn table_conf_919_versions_negative_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/c80896bba6dd010c.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/ea043331bc9fa871.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_versions_negative_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-versions-negative-zhuyin"),
        expected,
        None,
    );
}

// table-conf-919-versions-positive-overflow-zhuyin: MATCH.
#[test]
fn table_conf_919_versions_positive_overflow_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/60270c408b750b7a.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/f067034f83a6df1f.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_versions_positive_overflow_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-versions-positive-overflow-zhuyin"),
        expected,
        None,
    );
}

// table-conf-abort-919-word-width-overlong-zhuyin: the pin's abort, reproduced.
#[test]
fn table_conf_abort_919_word_width_overlong_zhuyin() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_919_word_width_overlong_zhuyin",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-919-word-width-overlong-zhuyin");
        },
    );
}

// table-conf-abort-addon-index-16-zhuyin: the pin's abort, reproduced.
#[test]
fn table_conf_abort_addon_index_16_zhuyin() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_addon_index_16_zhuyin",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-addon-index-16-zhuyin");
        },
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

// table-conf-abort-default-dictionary-zhuyin: the pin's abort, reproduced.
#[test]
fn table_conf_abort_default_dictionary_zhuyin() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_default_dictionary_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-default-dictionary-zhuyin");
        },
    );
}

// table-conf-abort-default-name-unknown-zhuyin: the pin's abort, reproduced.
#[test]
fn table_conf_abort_default_name_unknown_zhuyin() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_default_name_unknown_zhuyin",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-default-name-unknown-zhuyin");
        },
    );
}

// table-conf-abort-file-type-unknown-zhuyin: the pin's abort, reproduced.
#[test]
fn table_conf_abort_file_type_unknown_zhuyin() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_file_type_unknown_zhuyin",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-file-type-unknown-zhuyin");
        },
    );
}

// table-conf-abort-load-library-not-used-zhuyin: the pin's abort, reproduced.
#[test]
fn table_conf_abort_load_library_not_used_zhuyin() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_load_library_not_used_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "table-conf-abort-load-library-not-used-zhuyin");
        },
    );
}

// table-conf-abort-source-format-line-missing-zhuyin: the pin aborts; oxpinyin refuses with the pin's warning and answers False.
#[test]
fn table_conf_abort_source_format_line_missing_zhuyin() {
    run_j(
        "table_conf_scanner::table_conf_abort_source_format_line_missing_zhuyin",
        Expect::RESULT.with_files(&[]),
        |k| b::case(k, "table-conf-abort-source-format-line-missing-zhuyin"),
        raw!(M(&[
            ("logs", L(&[L(&[S("libzhuyin"), I(16)])])),
            ("ret", B(false))
        ])),
        None,
    );
}

// table-conf-abort-source-format-unknown-zhuyin: the pin's abort, reproduced.
#[test]
fn table_conf_abort_source_format_unknown_zhuyin() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_source_format_unknown_zhuyin",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-source-format-unknown-zhuyin");
        },
    );
}

// table-conf-addon-cleanup-zhuyin: MATCH.
#[test]
fn table_conf_addon_cleanup_zhuyin() {
    run_j(
        "table_conf_scanner::table_conf_addon_cleanup_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-addon-cleanup-zhuyin"),
        raw!(M(&[("addon", B(true)), ("ctx", B(true)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-candidates-files-swapped-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn table_conf_candidates_files_swapped_zhuyin() {
    run_j(
        "table_conf_scanner::table_conf_candidates_files_swapped_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-candidates-files-swapped-zhuyin"),
        raw!(M(&[
            ("logs", L(&[L(&[S("libzhuyin"), I(16)])])),
            ("ret", B(false))
        ])),
        Some(&["logs", "ret"]),
    );
}

// table-conf-colliding-library-files-zhuyin: MATCH.
#[test]
fn table_conf_colliding_library_files_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/3ec0311d373d22a8.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/a3c3f10d4ea5365b.rs"));
    run_j(
        "table_conf_scanner::table_conf_colliding_library_files_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-colliding-library-files-zhuyin"),
        expected,
        None,
    );
}

// table-conf-database-format-other-zhuyin: MATCH.
#[test]
fn table_conf_database_format_other_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/5ccbdc20ad98b818.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/782d620e4ff659bf.rs"));
    run_j(
        "table_conf_scanner::table_conf_database_format_other_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-database-format-other-zhuyin"),
        expected,
        None,
    );
}

// table-conf-lambda-half-zhuyin: MATCH.
#[test]
fn table_conf_lambda_half_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/844aa605ed333b63.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/a77412697ca68a88.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_half_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-half-zhuyin"),
        expected,
        None,
    );
}

// table-conf-lambda-hex-zhuyin: MATCH.
#[test]
fn table_conf_lambda_hex_zhuyin() {
    run_j(
        "table_conf_scanner::table_conf_lambda_hex_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-hex-zhuyin"),
        raw!(M(&[("ctx", B(true)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-lambda-line-missing-zhuyin: MATCH.
#[test]
fn table_conf_lambda_line_missing_zhuyin() {
    run_j(
        "table_conf_scanner::table_conf_lambda_line_missing_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"load SCRATCH\\sys-X\\table.conf failed!\r\n"
        } else {
            b"load SCRATCH/sys-X/table.conf failed!\n"
        }),
        |k| b::case(k, "table-conf-lambda-line-missing-zhuyin"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-lambda-million-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1378-zhuyin-selection.md).
#[test]
fn table_conf_lambda_million_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/99630fd4560fe332.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/d2727a80a07fcb49.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_million_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-lambda-million-zhuyin"),
        expected,
        None,
    );
}

// table-conf-lambda-negative-half-zhuyin: MATCH.
#[test]
fn table_conf_lambda_negative_half_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/0e6451ed743b18ab.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/6ae16321b6a22ca6.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_negative_half_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-negative-half-zhuyin"),
        expected,
        None,
    );
}

// table-conf-lambda-negative-two-zhuyin: MATCH.
#[test]
fn table_conf_lambda_negative_two_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/022b3fb05c551b39.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/7357674e3734f89c.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_negative_two_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-negative-two-zhuyin"),
        expected,
        None,
    );
}

// table-conf-lambda-one-and-a-half-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1378-zhuyin-selection.md).
#[test]
fn table_conf_lambda_one_and_a_half_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/a79372a8478ab73f.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/bb00529e71d62b24.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_one_and_a_half_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-lambda-one-and-a-half-zhuyin"),
        expected,
        None,
    );
}

// table-conf-lambda-one-zhuyin: MATCH.
#[test]
fn table_conf_lambda_one_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/a79372a8478ab73f.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/bb00529e71d62b24.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_one_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-one-zhuyin"),
        expected,
        None,
    );
}

// table-conf-lambda-two-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1378-zhuyin-selection.md).
#[test]
fn table_conf_lambda_two_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/daf712b4b8a8ff39.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/aacc75bc2a1b2fcd.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_two_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-lambda-two-zhuyin"),
        expected,
        None,
    );
}

// table-conf-lambda-zero-zhuyin: MATCH.
#[test]
fn table_conf_lambda_zero_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/b14d1440197b71f4.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/a07e55f519f47535.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_zero_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-zero-zhuyin"),
        expected,
        None,
    );
}

// table-conf-log-named-like-a-library-zhuyin: MATCH.
#[test]
fn table_conf_log_named_like_a_library_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/e8c4fa7a90ff5c3a.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/2534532dd1de6c3b.rs"));
    run_j(
        "table_conf_scanner::table_conf_log_named_like_a_library_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-log-named-like-a-library-zhuyin"),
        expected,
        None,
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

// table-conf-network-as-system-file-zhuyin: MATCH.
#[test]
fn table_conf_network_as_system_file_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/41a527f1595323ab.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/5a72c953ac280312.rs"));
    run_j(
        "table_conf_scanner::table_conf_network_as_system_file_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-network-as-system-file-zhuyin"),
        expected,
        None,
    );
}

// table-conf-not-used-merged-zhuyin: MATCH.
#[test]
fn table_conf_not_used_merged_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/4587bbb188353b9b.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/4838fb71f8839c5f.rs"));
    run_j(
        "table_conf_scanner::table_conf_not_used_merged_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-merged-zhuyin"),
        expected,
        None,
    );
}

// table-conf-not-used-network-dictionary-zhuyin: MATCH.
#[test]
fn table_conf_not_used_network_dictionary_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/698a9271d3fc2a5b.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/d206b2eb6d6c1e47.rs"));
    run_j(
        "table_conf_scanner::table_conf_not_used_network_dictionary_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-network-dictionary-zhuyin"),
        expected,
        None,
    );
}

// table-conf-not-used-opengram-zhuyin: MATCH.
#[test]
fn table_conf_not_used_opengram_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/1433656d4dabf9ee.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/c928862662f87d2c.rs"));
    run_j(
        "table_conf_scanner::table_conf_not_used_opengram_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-opengram-zhuyin"),
        expected,
        None,
    );
}

// table-conf-not-used-user-dictionary-zhuyin: MATCH.
#[test]
fn table_conf_not_used_user_dictionary_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/fff9731fdaaca6b3.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/f8b1dd9d0bf458d2.rs"));
    run_j(
        "table_conf_scanner::table_conf_not_used_user_dictionary_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-user-dictionary-zhuyin"),
        expected,
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

// table-conf-row-removed-gbk-zhuyin: MATCH.
#[test]
fn table_conf_row_removed_gbk_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/433fa2dd8ed0265b.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/d0b93f4ede6c1fae.rs"));
    run_j(
        "table_conf_scanner::table_conf_row_removed_gbk_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-row-removed-gbk-zhuyin"),
        expected,
        None,
    );
}

// table-conf-user-file-absolute-zhuyin: MATCH.
// Unix only: the table.conf row spells a POSIX absolute path.
#[cfg(unix)]
#[test]
fn table_conf_user_file_absolute_zhuyin() {
    run_j(
        "table_conf_scanner::table_conf_user_file_absolute_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-user-file-absolute-zhuyin"),
        raw!(include!("fixtures/table_conf_scanner/4e92520c71a48168.rs")),
        None,
    );
}

// table-conf-user-file-not-utf8-zhuyin: MATCH.
// Unix only: the user file name holds a byte that is not UTF-8, which only a byte-string file name can spell.
#[cfg(unix)]
#[test]
fn table_conf_user_file_not_utf8_zhuyin() {
    // APFS refuses a file name that is not UTF-8.
    #[cfg(target_os = "macos")]
    let (expect, expected) = (
        Expect::RESULT,
        raw!(include!("fixtures/table_conf_scanner/76cd3e404d2193eb.rs")),
    );
    #[cfg(not(target_os = "macos"))]
    let (expect, expected) = (
        Expect::RESULT,
        raw!(include!("fixtures/table_conf_scanner/a882023e27cc8c5c.rs")),
    );
    run_j(
        "table_conf_scanner::table_conf_user_file_not_utf8_zhuyin",
        expect,
        |k| b::case(k, "table-conf-user-file-not-utf8-zhuyin"),
        expected,
        None,
    );
}

// table-conf-user-files-renamed-zhuyin: MATCH.
#[test]
fn table_conf_user_files_renamed_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/0a7421145d58754b.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/59bf82a58728ab2f.rs"));
    run_j(
        "table_conf_scanner::table_conf_user_files_renamed_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-user-files-renamed-zhuyin"),
        expected,
        None,
    );
}

// table-conf-user-library-named-user-conf-zhuyin: MATCH.
#[test]
fn table_conf_user_library_named_user_conf_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/1feb6fa3774f6c5e.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/76cd3e404d2193eb.rs"));
    run_j(
        "table_conf_scanner::table_conf_user_library_named_user_conf_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-user-library-named-user-conf-zhuyin"),
        expected,
        None,
    );
}

// table-conf-versions-negative-overflow-zhuyin: MATCH.
#[test]
fn table_conf_versions_negative_overflow_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/22d43fcd64d47767.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/82ba0c93d6390196.rs"));
    run_j(
        "table_conf_scanner::table_conf_versions_negative_overflow_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-versions-negative-overflow-zhuyin"),
        expected,
        None,
    );
}

// table-conf-versions-other-zhuyin: MATCH.
#[test]
fn table_conf_versions_other_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/57a2d271a1235df6.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/2817158461210b43.rs"));
    run_j(
        "table_conf_scanner::table_conf_versions_other_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-versions-other-zhuyin"),
        expected,
        None,
    );
}

// user-conf-abort-database-format-unknown-zhuyin: the pin's abort, reproduced.
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
