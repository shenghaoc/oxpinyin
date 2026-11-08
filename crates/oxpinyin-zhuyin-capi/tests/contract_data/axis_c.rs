//! Axis C: the option word, the input encodings and the zhuyin layouts (#1175, #1620). Generated from the frozen pin observations.

use crate::bodies::axis_c as b;
use crate::support::{Expect, run_j};

// axis-c-encoding-zhuyin-import-phrase-empty: MATCH.
#[test]
fn axis_c_encoding_zhuyin_import_phrase_empty() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_import_phrase_empty",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-zhuyin-import-phrase-empty"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/axis_c/0585432bc9d11738.rs"
        )),
        None,
    );
}
// axis-c-encoding-zhuyin-import-reading-bopomofo: MATCH.
#[test]
fn axis_c_encoding_zhuyin_import_reading_bopomofo() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_import_reading_bopomofo",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-zhuyin-import-reading-bopomofo"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/axis_c/209f46dcea94ace3.rs"
        )),
        None,
    );
}
// axis-c-encoding-zhuyin-lookup_tokens-hanzi: MATCH.
#[test]
fn axis_c_encoding_zhuyin_lookup_tokens_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_lookup_tokens_hanzi",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-zhuyin-lookup_tokens-hanzi"),
        raw!(M(&[
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("ret", B(true)),
            ("tokens", L(&[I(16802309)]))
        ])),
        None,
    );
}
// axis-c-encoding-zhuyin-parse_more_chewings-nfc: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_chewings_nfc() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_chewings_nfc",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-zhuyin-parse_more_chewings-nfc"),
        raw!(include!("fixtures/axis_c/42044682cd282da5.rs")),
        None,
    );
}
// axis-c-encoding-zhuyin-parse_more_full_pinyins-fullwidth: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_full_pinyins_fullwidth() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_full_pinyins_fullwidth",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(
                k,
                "axis-c-encoding-zhuyin-parse_more_full_pinyins-fullwidth",
            )
        },
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/axis_c/d8171b40697b7226.rs"
        )),
        None,
    );
}
// axis-c-encoding-zhuyin-phrase_segment-nul-middle: MATCH.
#[test]
fn axis_c_encoding_zhuyin_phrase_segment_nul_middle() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_phrase_segment_nul_middle",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-zhuyin-phrase_segment-nul-middle"),
        raw!(M(&[
            ("count", I(2)),
            ("get_n_phrase", B(true)),
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("ret", B(false))
        ])),
        None,
    );
}
// axis-c-encoding-zhuyin-phrase_segment-tone: MATCH.
#[test]
fn axis_c_encoding_zhuyin_phrase_segment_tone() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_phrase_segment_tone",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-zhuyin-phrase_segment-tone"),
        raw!(M(&[
            ("count", I(7)),
            ("get_n_phrase", B(true)),
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("ret", B(false))
        ])),
        None,
    );
}
// axis-c-layout-zhuyin-1-00000000: MATCH.
#[test]
fn axis_c_layout_zhuyin_1_00000000() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_1_00000000",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-layout-zhuyin-1-00000000"),
        raw!(include!("fixtures/axis_c/09b0ccf139077a47.rs")),
        None,
    );
}

// axis-c-layout-zhuyin-1-00000060: MATCH.
#[test]
fn axis_c_layout_zhuyin_1_00000060() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_1_00000060",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-layout-zhuyin-1-00000060"),
        raw!(include!("fixtures/axis_c/0660c7fe0834efbe.rs")),
        None,
    );
}
// axis-c-layout-zhuyin-2-e0000070: MATCH.
#[test]
fn axis_c_layout_zhuyin_2_e0000070() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_2_e0000070",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-layout-zhuyin-2-e0000070"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/axis_c/77cbdd7f221e05c3.rs"
        )),
        None,
    );
}
// axis-c-layout-zhuyin-5-00000000: MATCH.
#[test]
fn axis_c_layout_zhuyin_5_00000000() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_5_00000000",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-layout-zhuyin-5-00000000"),
        raw!(include!("fixtures/axis_c/c6b93997bc706f09.rs")),
        None,
    );
}
