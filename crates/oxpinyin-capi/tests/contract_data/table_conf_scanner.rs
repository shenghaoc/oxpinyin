//! table.conf rows, header scanning and file paths (#1175, #1622). Generated from the frozen pin observations.

use crate::bodies::table_conf_scanner as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};

// hash-path/system-directory-pinyin: MATCH.
#[test]
fn hash_path_system_directory_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/1b1c9abae2956469.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/77874941c01c662a.rs"));
    run_j(
        "table_conf_scanner::hash_path_system_directory_pinyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "hash-path/system-directory-pinyin"),
        expected,
        None,
    );
}

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

// hash-path/user-directory-pinyin: MATCH.
#[test]
fn hash_path_user_directory_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/1b1c9abae2956469.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/77874941c01c662a.rs"));
    run_j(
        "table_conf_scanner::hash_path_user_directory_pinyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X#type=kct\\user.conf failed."
        } else {
            b"open SCRATCH/user-X#type=kct/user.conf failed."
        }),
        |k| b::case(k, "hash-path/user-directory-pinyin"),
        expected,
        None,
    );
}

// hash-path/user-filename-pinyin: MATCH.
#[test]
fn hash_path_user_filename_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/b88e9130115b2e3c.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/9c44b04c27eadc62.rs"));
    run_j(
        "table_conf_scanner::hash_path_user_filename_pinyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "hash-path/user-filename-pinyin"),
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

// table-conf-1378-unigram-lambda-zero-control: MATCH.
#[test]
fn table_conf_1378_unigram_lambda_zero_control() {
    run_j(
        "table_conf_scanner::table_conf_1378_unigram_lambda_zero_control",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-1378-unigram-lambda-zero-control"),
        raw!(M(&[
            ("guessed", B(true)),
            ("has_sentence", B(true)),
            ("logs", L(&[])),
            ("parsed", I(2))
        ])),
        None,
    );
}

// table-conf-919-binary-version-line-missing: MATCH.
#[test]
fn table_conf_919_binary_version_line_missing() {
    run_j(
        "table_conf_scanner::table_conf_919_binary_version_line_missing",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"load SCRATCH\\sys-X\\table.conf failed!\r\n"
        } else {
            b"load SCRATCH/sys-X/table.conf failed!\n"
        }),
        |k| b::case(k, "table-conf-919-binary-version-line-missing"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-919-database-token-nul: MATCH.
#[test]
fn table_conf_919_database_token_nul() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_database_token_nul",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-database-token-nul"),
        expected,
        None,
    );
}

// table-conf-919-header-no-final-newline: MATCH.
#[test]
fn table_conf_919_header_no_final_newline() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_header_no_final_newline",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-header-no-final-newline"),
        expected,
        None,
    );
}

// table-conf-919-header-one-line: MATCH.
#[test]
fn table_conf_919_header_one_line() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_header_one_line",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-header-one-line"),
        expected,
        None,
    );
}

// table-conf-919-model-version-line-missing: MATCH.
#[test]
fn table_conf_919_model_version_line_missing() {
    run_j(
        "table_conf_scanner::table_conf_919_model_version_line_missing",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"load SCRATCH\\sys-X\\table.conf failed!\r\n"
        } else {
            b"load SCRATCH/sys-X/table.conf failed!\n"
        }),
        |k| b::case(k, "table-conf-919-model-version-line-missing"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-919-source-token-nul: MATCH.
#[test]
fn table_conf_919_source_token_nul() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_source_token_nul",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-source-token-nul"),
        expected,
        None,
    );
}

// table-conf-919-versions-int-wrap: MATCH.
#[test]
fn table_conf_919_versions_int_wrap() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/73721920a3edc88a.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/c27d4a89690d8276.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_versions_int_wrap",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-versions-int-wrap"),
        expected,
        None,
    );
}

// table-conf-919-versions-negative: MATCH.
#[test]
fn table_conf_919_versions_negative() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/44c9bc18d4db4223.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/c11c4f25f11b909a.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_versions_negative",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-versions-negative"),
        expected,
        None,
    );
}

// table-conf-919-versions-positive-overflow: MATCH.
#[test]
fn table_conf_919_versions_positive_overflow() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/f63be9df1f3cc939.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/8d300a4a7438ef03.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_versions_positive_overflow",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-versions-positive-overflow"),
        expected,
        None,
    );
}

// table-conf-919-word-nul-truncates: MATCH.
#[test]
fn table_conf_919_word_nul_truncates() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_919_word_nul_truncates",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-word-nul-truncates"),
        expected,
        None,
    );
}

// table-conf-abort-919-word-width-overlong: the pin's abort, reproduced.
#[test]
fn table_conf_abort_919_word_width_overlong() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_919_word_width_overlong",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-919-word-width-overlong");
        },
    );
}

// table-conf-abort-addon-index-16: the pin's abort, reproduced.
#[test]
fn table_conf_abort_addon_index_16() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_addon_index_16",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-addon-index-16");
        },
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

// table-conf-abort-default-dictionary: the pin's abort, reproduced.
#[test]
fn table_conf_abort_default_dictionary() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "table_conf_scanner::table_conf_abort_default_dictionary",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[("user.conf", UserFile::Conf(USER_CONF))]),
        |k| {
            b::case(k, "table-conf-abort-default-dictionary");
        },
    );
}

// table-conf-abort-default-name-unknown: the pin's abort, reproduced.
#[test]
fn table_conf_abort_default_name_unknown() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_default_name_unknown",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-default-name-unknown");
        },
    );
}

// table-conf-abort-file-type-unknown: the pin's abort, reproduced.
#[test]
fn table_conf_abort_file_type_unknown() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_file_type_unknown",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-file-type-unknown");
        },
    );
}

// table-conf-abort-load-addon-system-file: the pin's abort, reproduced.
#[test]
fn table_conf_abort_load_addon_system_file() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_load_addon_system_file",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "table-conf-abort-load-addon-system-file");
        },
    );
}

// table-conf-abort-load-library-not-used: the pin's abort, reproduced.
#[test]
fn table_conf_abort_load_library_not_used() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_load_library_not_used",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "table-conf-abort-load-library-not-used");
        },
    );
}

// table-conf-abort-source-format-line-missing: the pin aborts; oxpinyin refuses with the pin's warning and answers False.
#[test]
fn table_conf_abort_source_format_line_missing() {
    run_j(
        "table_conf_scanner::table_conf_abort_source_format_line_missing",
        Expect::RESULT.with_files(&[]),
        |k| b::case(k, "table-conf-abort-source-format-line-missing"),
        raw!(M(&[
            ("logs", L(&[L(&[S("libpinyin"), I(16)])])),
            ("ret", B(false))
        ])),
        None,
    );
}

// table-conf-abort-source-format-unknown: the pin's abort, reproduced.
#[test]
fn table_conf_abort_source_format_unknown() {
    run_aborting(
        "table_conf_scanner::table_conf_abort_source_format_unknown",
        Expect::aborts(b"").with_files(&[]),
        |k| {
            b::case(k, "table-conf-abort-source-format-unknown");
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

// table-conf-addon-files-swapped: MATCH.
#[test]
fn table_conf_addon_files_swapped() {
    run_j(
        "table_conf_scanner::table_conf_addon_files_swapped",
        Expect::RESULT,
        |k| b::case(k, "table-conf-addon-files-swapped"),
        raw!(include!("fixtures/table_conf_scanner/a24150bf2e491537.rs")),
        None,
    );
}

// table-conf-addon-index-wraps: MATCH.
#[test]
fn table_conf_addon_index_wraps() {
    run_j(
        "table_conf_scanner::table_conf_addon_index_wraps",
        Expect::RESULT,
        |k| b::case(k, "table-conf-addon-index-wraps"),
        raw!(include!("fixtures/table_conf_scanner/e845483546a60845.rs")),
        None,
    );
}

// table-conf-addon-index-zero: MATCH.
#[test]
fn table_conf_addon_index_zero() {
    run_j(
        "table_conf_scanner::table_conf_addon_index_zero",
        Expect::RESULT,
        |k| b::case(k, "table-conf-addon-index-zero"),
        raw!(include!("fixtures/table_conf_scanner/9324712e67acdfdf.rs")),
        None,
    );
}

// table-conf-addon-moved: MATCH.
#[test]
fn table_conf_addon_moved() {
    run_j(
        "table_conf_scanner::table_conf_addon_moved",
        Expect::RESULT,
        |k| b::case(k, "table-conf-addon-moved"),
        raw!(include!("fixtures/table_conf_scanner/f2bcea360c5fc7a3.rs")),
        None,
    );
}

// table-conf-addon-not-used: MATCH.
#[test]
fn table_conf_addon_not_used() {
    run_j(
        "table_conf_scanner::table_conf_addon_not_used",
        Expect::RESULT,
        |k| b::case(k, "table-conf-addon-not-used"),
        raw!(include!("fixtures/table_conf_scanner/a9f17f014a1b7a3e.rs")),
        None,
    );
}

// table-conf-candidates-files-swapped: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn table_conf_candidates_files_swapped() {
    run_j(
        "table_conf_scanner::table_conf_candidates_files_swapped",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-candidates-files-swapped"),
        raw!(M(&[
            ("logs", L(&[L(&[S("libpinyin"), I(16)])])),
            ("ret", B(false))
        ])),
        Some(&["logs", "ret"]),
    );
}

// table-conf-colliding-library-files: MATCH.
#[test]
fn table_conf_colliding_library_files() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/917f76420db5d74c.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/bc59a5bf255b96c1.rs"));
    run_j(
        "table_conf_scanner::table_conf_colliding_library_files",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\shared.bin.tmp to SCRATCH\\user-X\\shared.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/shared.bin.tmp to SCRATCH/user-X/shared.bin failed.\n"
        }),
        |k| b::case(k, "table-conf-colliding-library-files"),
        expected,
        None,
    );
}

// table-conf-database-format-other: MATCH.
#[test]
fn table_conf_database_format_other() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/1aa5ee96e3cc22ae.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/b8e33615dac4c123.rs"));
    run_j(
        "table_conf_scanner::table_conf_database_format_other",
        Expect::RESULT,
        |k| b::case(k, "table-conf-database-format-other"),
        expected,
        None,
    );
}

// table-conf-extra-word-dropped: MATCH.
#[test]
fn table_conf_extra_word_dropped() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_extra_word_dropped",
        Expect::RESULT,
        |k| b::case(k, "table-conf-extra-word-dropped"),
        expected,
        None,
    );
}

// table-conf-lambda-half: MATCH.
#[test]
fn table_conf_lambda_half() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_half",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-half"),
        expected,
        None,
    );
}

// table-conf-lambda-hex: MATCH.
#[test]
fn table_conf_lambda_hex() {
    run_j(
        "table_conf_scanner::table_conf_lambda_hex",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-hex"),
        raw!(M(&[("ctx", B(true)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-lambda-line-missing: MATCH.
#[test]
fn table_conf_lambda_line_missing() {
    run_j(
        "table_conf_scanner::table_conf_lambda_line_missing",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"load SCRATCH\\sys-X\\table.conf failed!\r\n"
        } else {
            b"load SCRATCH/sys-X/table.conf failed!\n"
        }),
        |k| b::case(k, "table-conf-lambda-line-missing"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-lambda-negative-half: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn table_conf_lambda_negative_half() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/6410d71bd3ae9cfe.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/821f1bf1acdaa4a1.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_negative_half",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-lambda-negative-half"),
        expected,
        None,
    );
}

// table-conf-lambda-negative-two: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn table_conf_lambda_negative_two() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/6410d71bd3ae9cfe.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/821f1bf1acdaa4a1.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_negative_two",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-lambda-negative-two"),
        expected,
        None,
    );
}

// table-conf-lambda-one: MATCH.
#[test]
fn table_conf_lambda_one() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/657f5563367c7c84.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/cc95bfda7f67135a.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_one",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-one"),
        expected,
        None,
    );
}

// table-conf-lambda-one-and-a-half: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn table_conf_lambda_one_and_a_half() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/6db4a4f00df6ea2d.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/7926092709975925.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_one_and_a_half",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-lambda-one-and-a-half"),
        expected,
        None,
    );
}

// table-conf-lambda-zero: MATCH.
#[test]
fn table_conf_lambda_zero() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_lambda_zero",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-zero"),
        expected,
        None,
    );
}

// table-conf-later-row-wins: MATCH.
#[test]
fn table_conf_later_row_wins() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/ca3a35e8b970b4f6.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/c670922c818ced15.rs"));
    run_j(
        "table_conf_scanner::table_conf_later_row_wins",
        Expect::RESULT,
        |k| b::case(k, "table-conf-later-row-wins"),
        expected,
        None,
    );
}

// table-conf-library-named-like-an-index: MATCH.
#[test]
fn table_conf_library_named_like_an_index() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/845ebf53415e85e7.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/6931f9962356b0ce.rs"));
    run_j(
        "table_conf_scanner::table_conf_library_named_like_an_index",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_pinyin_index.bin.tmp to SCRATCH\\user-X\\user_pinyin_index.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\n"
        }),
        |k| b::case(k, "table-conf-library-named-like-an-index"),
        expected,
        None,
    );
}

// table-conf-log-named-like-a-library: MATCH.
#[test]
fn table_conf_log_named_like_a_library() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/bf83f316fd0c8d0a.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/12f2784d16523164.rs"));
    run_j(
        "table_conf_scanner::table_conf_log_named_like_a_library",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user.bin.tmp to SCRATCH\\user-X\\user.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user.bin.tmp to SCRATCH/user-X/user.bin failed.\n"
        }),
        |k| b::case(k, "table-conf-log-named-like-a-library"),
        expected,
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

// table-conf-network-as-system-file: MATCH.
#[test]
fn table_conf_network_as_system_file() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/464f6eddf219ff71.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/ba2de8e440284a87.rs"));
    run_j(
        "table_conf_scanner::table_conf_network_as_system_file",
        Expect::RESULT,
        |k| b::case(k, "table-conf-network-as-system-file"),
        expected,
        None,
    );
}

// table-conf-not-used-merged: MATCH.
#[test]
fn table_conf_not_used_merged() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/e9d741571b449549.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/618d72a6c50419c7.rs"));
    run_j(
        "table_conf_scanner::table_conf_not_used_merged",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-merged"),
        expected,
        None,
    );
}

// table-conf-not-used-network-dictionary: MATCH.
#[test]
fn table_conf_not_used_network_dictionary() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/98d65e9157cd7d8b.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/29b08dab70b61b23.rs"));
    run_j(
        "table_conf_scanner::table_conf_not_used_network_dictionary",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-network-dictionary"),
        expected,
        None,
    );
}

// table-conf-not-used-opengram: MATCH.
#[test]
fn table_conf_not_used_opengram() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/ca3a35e8b970b4f6.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/c670922c818ced15.rs"));
    run_j(
        "table_conf_scanner::table_conf_not_used_opengram",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-opengram"),
        expected,
        None,
    );
}

// table-conf-not-used-user-dictionary: MATCH.
#[test]
fn table_conf_not_used_user_dictionary() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/a5dbfe722390bac4.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/620be2b35a131400.rs"));
    run_j(
        "table_conf_scanner::table_conf_not_used_user_dictionary",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-user-dictionary"),
        expected,
        None,
    );
}

// table-conf-reserved-as-system-file: MATCH.
#[test]
fn table_conf_reserved_as_system_file() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_reserved_as_system_file",
        Expect::RESULT,
        |k| b::case(k, "table-conf-reserved-as-system-file"),
        expected,
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

// table-conf-row-removed-gbk: MATCH.
#[test]
fn table_conf_row_removed_gbk() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/c7059436defa2137.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/22f614f9cf6c02ed.rs"));
    run_j(
        "table_conf_scanner::table_conf_row_removed_gbk",
        Expect::RESULT,
        |k| b::case(k, "table-conf-row-removed-gbk"),
        expected,
        None,
    );
}

// table-conf-short-row-dropped: MATCH.
#[test]
fn table_conf_short_row_dropped() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_short_row_dropped",
        Expect::RESULT,
        |k| b::case(k, "table-conf-short-row-dropped"),
        expected,
        None,
    );
}

// table-conf-source-format-zhuyin: MATCH.
#[test]
fn table_conf_source_format_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_source_format_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-source-format-zhuyin"),
        expected,
        None,
    );
}

// table-conf-tsi-is-gb: MATCH.
#[test]
fn table_conf_tsi_is_gb() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/817b946376ddef03.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs"));
    run_j(
        "table_conf_scanner::table_conf_tsi_is_gb",
        Expect::RESULT,
        |k| b::case(k, "table-conf-tsi-is-gb"),
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

// table-conf-user-file-not-utf8: MATCH.
// Unix only: the user file name holds a byte that is not UTF-8, which only a byte-string file name can spell.
#[cfg(unix)]
#[test]
fn table_conf_user_file_not_utf8() {
    // APFS refuses a file name that is not UTF-8.
    #[cfg(target_os = "macos")]
    let (expect, expected) = (
        Expect::RESULT,
        raw!(include!("fixtures/table_conf_scanner/6931f9962356b0ce.rs")),
    );
    #[cfg(not(target_os = "macos"))]
    let (expect, expected) = (
        Expect::RESULT,
        raw!(include!("fixtures/table_conf_scanner/78872818fec4de04.rs")),
    );
    run_j(
        "table_conf_scanner::table_conf_user_file_not_utf8",
        expect,
        |k| b::case(k, "table-conf-user-file-not-utf8"),
        expected,
        None,
    );
}

// table-conf-user-files-renamed: MATCH.
#[test]
fn table_conf_user_files_renamed() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/47cb84fa34eaba61.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/6b2a437e199e51c0.rs"));
    run_j(
        "table_conf_scanner::table_conf_user_files_renamed",
        Expect::RESULT,
        |k| b::case(k, "table-conf-user-files-renamed"),
        expected,
        None,
    );
}

// table-conf-user-library-named-user-conf: MATCH.
#[test]
fn table_conf_user_library_named_user_conf() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/845ebf53415e85e7.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/6931f9962356b0ce.rs"));
    run_j(
        "table_conf_scanner::table_conf_user_library_named_user_conf",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "table-conf-user-library-named-user-conf"),
        expected,
        None,
    );
}

// table-conf-versions-negative-overflow: MATCH.
#[test]
fn table_conf_versions_negative_overflow() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/62cbed6da03e174a.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/00ddb016d281516f.rs"));
    run_j(
        "table_conf_scanner::table_conf_versions_negative_overflow",
        Expect::RESULT,
        |k| b::case(k, "table-conf-versions-negative-overflow"),
        expected,
        None,
    );
}

// table-conf-versions-other: MATCH.
#[test]
fn table_conf_versions_other() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/table_conf_scanner/ff7c4bc70e299b9f.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/table_conf_scanner/1bbee02b83db3225.rs"));
    run_j(
        "table_conf_scanner::table_conf_versions_other",
        Expect::RESULT,
        |k| b::case(k, "table-conf-versions-other"),
        expected,
        None,
    );
}

// user-conf-abort-database-format-unknown: the pin's abort, reproduced.
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
