//! table.conf rows, header scanning and file paths (#1175, #1622). Generated from the frozen pin observations.

use crate::bodies::table_conf_scanner as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};

// hash-path/system-directory-pinyin: MATCH.
#[test]
fn hash_path_system_directory_pinyin() {
    run_j(
        "table_conf_scanner::hash_path_system_directory_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "hash-path/system-directory-pinyin"),
        raw!(include!("fixtures/table_conf_scanner/77874941c01c662a.rs")),
        None,
    );
}

// hash-path/system-filename-pinyin: MATCH.
#[test]
fn hash_path_system_filename_pinyin() {
    run_j(
        "table_conf_scanner::hash_path_system_filename_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "hash-path/system-filename-pinyin"),
        raw!(include!("fixtures/table_conf_scanner/77874941c01c662a.rs")),
        None,
    );
}

// hash-path/user-directory-pinyin: MATCH.
#[test]
fn hash_path_user_directory_pinyin() {
    run_j(
        "table_conf_scanner::hash_path_user_directory_pinyin",
        Expect::stderr(b"open SCRATCH/user-X#type=kct/user.conf failed."),
        |k| b::case(k, "hash-path/user-directory-pinyin"),
        raw!(include!("fixtures/table_conf_scanner/77874941c01c662a.rs")),
        None,
    );
}

// hash-path/user-filename-pinyin: MATCH.
#[test]
fn hash_path_user_filename_pinyin() {
    run_j(
        "table_conf_scanner::hash_path_user_filename_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "hash-path/user-filename-pinyin"),
        raw!(include!("fixtures/table_conf_scanner/9c44b04c27eadc62.rs")),
        None,
    );
}

// table-conf-1378-unigram-lambda-one: MATCH.
#[test]
fn table_conf_1378_unigram_lambda_one() {
    run_j(
        "table_conf_scanner::table_conf_1378_unigram_lambda_one",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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
        Expect::stderr(b"load SCRATCH/sys-X/table.conf failed!\n"),
        |k| b::case(k, "table-conf-919-binary-version-line-missing"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-919-database-token-nul: MATCH.
#[test]
fn table_conf_919_database_token_nul() {
    run_j(
        "table_conf_scanner::table_conf_919_database_token_nul",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-database-token-nul"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
        None,
    );
}

// table-conf-919-header-no-final-newline: MATCH.
#[test]
fn table_conf_919_header_no_final_newline() {
    run_j(
        "table_conf_scanner::table_conf_919_header_no_final_newline",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-header-no-final-newline"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
        None,
    );
}

// table-conf-919-header-one-line: MATCH.
#[test]
fn table_conf_919_header_one_line() {
    run_j(
        "table_conf_scanner::table_conf_919_header_one_line",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-header-one-line"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
        None,
    );
}

// table-conf-919-model-version-line-missing: MATCH.
#[test]
fn table_conf_919_model_version_line_missing() {
    run_j(
        "table_conf_scanner::table_conf_919_model_version_line_missing",
        Expect::stderr(b"load SCRATCH/sys-X/table.conf failed!\n"),
        |k| b::case(k, "table-conf-919-model-version-line-missing"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-919-source-token-nul: MATCH.
#[test]
fn table_conf_919_source_token_nul() {
    run_j(
        "table_conf_scanner::table_conf_919_source_token_nul",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-source-token-nul"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
        None,
    );
}

// table-conf-919-versions-int-wrap: MATCH.
#[test]
fn table_conf_919_versions_int_wrap() {
    run_j(
        "table_conf_scanner::table_conf_919_versions_int_wrap",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-versions-int-wrap"),
        raw!(include!("fixtures/table_conf_scanner/c27d4a89690d8276.rs")),
        None,
    );
}

// table-conf-919-versions-negative: MATCH.
#[test]
fn table_conf_919_versions_negative() {
    run_j(
        "table_conf_scanner::table_conf_919_versions_negative",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-versions-negative"),
        raw!(include!("fixtures/table_conf_scanner/c11c4f25f11b909a.rs")),
        None,
    );
}

// table-conf-919-versions-positive-overflow: MATCH.
#[test]
fn table_conf_919_versions_positive_overflow() {
    run_j(
        "table_conf_scanner::table_conf_919_versions_positive_overflow",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-versions-positive-overflow"),
        raw!(include!("fixtures/table_conf_scanner/8d300a4a7438ef03.rs")),
        None,
    );
}

// table-conf-919-word-nul-truncates: MATCH.
#[test]
fn table_conf_919_word_nul_truncates() {
    run_j(
        "table_conf_scanner::table_conf_919_word_nul_truncates",
        Expect::RESULT,
        |k| b::case(k, "table-conf-919-word-nul-truncates"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
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
    run_aborting(
        "table_conf_scanner::table_conf_abort_default_dictionary",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|"))]),
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
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
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
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
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
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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
    run_j(
        "table_conf_scanner::table_conf_colliding_library_files",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/shared.bin.tmp to SCRATCH/user-X/shared.bin failed.\n"),
        |k| b::case(k, "table-conf-colliding-library-files"),
        raw!(include!("fixtures/table_conf_scanner/bc59a5bf255b96c1.rs")),
        None,
    );
}

// table-conf-database-format-other: MATCH.
#[test]
fn table_conf_database_format_other() {
    run_j(
        "table_conf_scanner::table_conf_database_format_other",
        Expect::RESULT,
        |k| b::case(k, "table-conf-database-format-other"),
        raw!(include!("fixtures/table_conf_scanner/b8e33615dac4c123.rs")),
        None,
    );
}

// table-conf-extra-word-dropped: MATCH.
#[test]
fn table_conf_extra_word_dropped() {
    run_j(
        "table_conf_scanner::table_conf_extra_word_dropped",
        Expect::RESULT,
        |k| b::case(k, "table-conf-extra-word-dropped"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
        None,
    );
}

// table-conf-lambda-half: MATCH.
#[test]
fn table_conf_lambda_half() {
    run_j(
        "table_conf_scanner::table_conf_lambda_half",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-half"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
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
        Expect::stderr(b"load SCRATCH/sys-X/table.conf failed!\n"),
        |k| b::case(k, "table-conf-lambda-line-missing"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-lambda-negative-half: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn table_conf_lambda_negative_half() {
    run_j(
        "table_conf_scanner::table_conf_lambda_negative_half",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "table-conf-lambda-negative-half"),
        raw!(include!("fixtures/table_conf_scanner/821f1bf1acdaa4a1.rs")),
        None,
    );
}

// table-conf-lambda-negative-two: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn table_conf_lambda_negative_two() {
    run_j(
        "table_conf_scanner::table_conf_lambda_negative_two",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "table-conf-lambda-negative-two"),
        raw!(include!("fixtures/table_conf_scanner/821f1bf1acdaa4a1.rs")),
        None,
    );
}

// table-conf-lambda-one: MATCH.
#[test]
fn table_conf_lambda_one() {
    run_j(
        "table_conf_scanner::table_conf_lambda_one",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-one"),
        raw!(include!("fixtures/table_conf_scanner/cc95bfda7f67135a.rs")),
        None,
    );
}

// table-conf-lambda-one-and-a-half: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn table_conf_lambda_one_and_a_half() {
    run_j(
        "table_conf_scanner::table_conf_lambda_one_and_a_half",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "table-conf-lambda-one-and-a-half"),
        raw!(include!("fixtures/table_conf_scanner/7926092709975925.rs")),
        None,
    );
}

// table-conf-lambda-zero: MATCH.
#[test]
fn table_conf_lambda_zero() {
    run_j(
        "table_conf_scanner::table_conf_lambda_zero",
        Expect::RESULT,
        |k| b::case(k, "table-conf-lambda-zero"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
        None,
    );
}

// table-conf-later-row-wins: MATCH.
#[test]
fn table_conf_later_row_wins() {
    run_j(
        "table_conf_scanner::table_conf_later_row_wins",
        Expect::RESULT,
        |k| b::case(k, "table-conf-later-row-wins"),
        raw!(include!("fixtures/table_conf_scanner/c670922c818ced15.rs")),
        None,
    );
}

// table-conf-library-named-like-an-index: MATCH.
#[test]
fn table_conf_library_named_like_an_index() {
    run_j(
        "table_conf_scanner::table_conf_library_named_like_an_index",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\n"),
        |k| b::case(k, "table-conf-library-named-like-an-index"),
        raw!(include!("fixtures/table_conf_scanner/6931f9962356b0ce.rs")),
        None,
    );
}

// table-conf-log-named-like-a-library: MATCH.
#[test]
fn table_conf_log_named_like_a_library() {
    run_j(
        "table_conf_scanner::table_conf_log_named_like_a_library",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user.bin.tmp to SCRATCH/user-X/user.bin failed.\n"),
        |k| b::case(k, "table-conf-log-named-like-a-library"),
        raw!(include!("fixtures/table_conf_scanner/12f2784d16523164.rs")),
        None,
    );
}

// table-conf-missing-with-dbms: MATCH.
#[test]
fn table_conf_missing_with_dbms() {
    run_j(
        "table_conf_scanner::table_conf_missing_with_dbms",
        Expect::stderr(
            b"open SCRATCH/sys-X/table.conf failed.\nload SCRATCH/sys-X/table.conf failed!\n",
        ),
        |k| b::case(k, "table-conf-missing-with-dbms"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// table-conf-network-as-system-file: MATCH.
#[test]
fn table_conf_network_as_system_file() {
    run_j(
        "table_conf_scanner::table_conf_network_as_system_file",
        Expect::RESULT,
        |k| b::case(k, "table-conf-network-as-system-file"),
        raw!(include!("fixtures/table_conf_scanner/ba2de8e440284a87.rs")),
        None,
    );
}

// table-conf-not-used-merged: MATCH.
#[test]
fn table_conf_not_used_merged() {
    run_j(
        "table_conf_scanner::table_conf_not_used_merged",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-merged"),
        raw!(include!("fixtures/table_conf_scanner/618d72a6c50419c7.rs")),
        None,
    );
}

// table-conf-not-used-network-dictionary: MATCH.
#[test]
fn table_conf_not_used_network_dictionary() {
    run_j(
        "table_conf_scanner::table_conf_not_used_network_dictionary",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-network-dictionary"),
        raw!(include!("fixtures/table_conf_scanner/29b08dab70b61b23.rs")),
        None,
    );
}

// table-conf-not-used-opengram: MATCH.
#[test]
fn table_conf_not_used_opengram() {
    run_j(
        "table_conf_scanner::table_conf_not_used_opengram",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-opengram"),
        raw!(include!("fixtures/table_conf_scanner/c670922c818ced15.rs")),
        None,
    );
}

// table-conf-not-used-user-dictionary: MATCH.
#[test]
fn table_conf_not_used_user_dictionary() {
    run_j(
        "table_conf_scanner::table_conf_not_used_user_dictionary",
        Expect::RESULT,
        |k| b::case(k, "table-conf-not-used-user-dictionary"),
        raw!(include!("fixtures/table_conf_scanner/620be2b35a131400.rs")),
        None,
    );
}

// table-conf-reserved-as-system-file: MATCH.
#[test]
fn table_conf_reserved_as_system_file() {
    run_j(
        "table_conf_scanner::table_conf_reserved_as_system_file",
        Expect::RESULT,
        |k| b::case(k, "table-conf-reserved-as-system-file"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
        None,
    );
}

// table-conf-row-removed-gb: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn table_conf_row_removed_gb() {
    run_j(
        "table_conf_scanner::table_conf_row_removed_gb",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "table-conf-row-removed-gb"),
        raw!(include!("fixtures/table_conf_scanner/1e687a68863d8a1d.rs")),
        None,
    );
}

// table-conf-row-removed-gbk: MATCH.
#[test]
fn table_conf_row_removed_gbk() {
    run_j(
        "table_conf_scanner::table_conf_row_removed_gbk",
        Expect::RESULT,
        |k| b::case(k, "table-conf-row-removed-gbk"),
        raw!(include!("fixtures/table_conf_scanner/22f614f9cf6c02ed.rs")),
        None,
    );
}

// table-conf-short-row-dropped: MATCH.
#[test]
fn table_conf_short_row_dropped() {
    run_j(
        "table_conf_scanner::table_conf_short_row_dropped",
        Expect::RESULT,
        |k| b::case(k, "table-conf-short-row-dropped"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
        None,
    );
}

// table-conf-source-format-zhuyin: MATCH.
#[test]
fn table_conf_source_format_zhuyin() {
    run_j(
        "table_conf_scanner::table_conf_source_format_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "table-conf-source-format-zhuyin"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
        None,
    );
}

// table-conf-tsi-is-gb: MATCH.
#[test]
fn table_conf_tsi_is_gb() {
    run_j(
        "table_conf_scanner::table_conf_tsi_is_gb",
        Expect::RESULT,
        |k| b::case(k, "table-conf-tsi-is-gb"),
        raw!(include!("fixtures/table_conf_scanner/fef6901043562142.rs")),
        None,
    );
}

// table-conf-user-file-absolute: MATCH.
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
    run_j(
        "table_conf_scanner::table_conf_user_files_renamed",
        Expect::RESULT,
        |k| b::case(k, "table-conf-user-files-renamed"),
        raw!(include!("fixtures/table_conf_scanner/6b2a437e199e51c0.rs")),
        None,
    );
}

// table-conf-user-library-named-user-conf: MATCH.
#[test]
fn table_conf_user_library_named_user_conf() {
    run_j(
        "table_conf_scanner::table_conf_user_library_named_user_conf",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "table-conf-user-library-named-user-conf"),
        raw!(include!("fixtures/table_conf_scanner/6931f9962356b0ce.rs")),
        None,
    );
}

// table-conf-versions-negative-overflow: MATCH.
#[test]
fn table_conf_versions_negative_overflow() {
    run_j(
        "table_conf_scanner::table_conf_versions_negative_overflow",
        Expect::RESULT,
        |k| b::case(k, "table-conf-versions-negative-overflow"),
        raw!(include!("fixtures/table_conf_scanner/00ddb016d281516f.rs")),
        None,
    );
}

// table-conf-versions-other: MATCH.
#[test]
fn table_conf_versions_other() {
    run_j(
        "table_conf_scanner::table_conf_versions_other",
        Expect::RESULT,
        |k| b::case(k, "table-conf-versions-other"),
        raw!(include!("fixtures/table_conf_scanner/1bbee02b83db3225.rs")),
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
