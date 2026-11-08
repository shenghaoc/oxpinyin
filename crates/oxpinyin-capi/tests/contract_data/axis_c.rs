//! Axis C: the option word, the input encodings and the zhuyin layouts (#1175, #1620). Generated from the frozen pin observations.

use crate::bodies::axis_c as b;
use crate::support::{Expect, run_j};

// axis-c-encoding-pinyin-import-phrase-empty: MATCH.
#[test]
fn axis_c_encoding_pinyin_import_phrase_empty() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_import_phrase_empty",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-pinyin-import-phrase-empty"),
        raw!(include!("fixtures/axis_c/0585432bc9d11738.rs")),
        None,
    );
}
// axis-c-encoding-pinyin-import-reading-pinyin: MATCH.
#[test]
fn axis_c_encoding_pinyin_import_reading_pinyin() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_import_reading_pinyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-pinyin-import-reading-pinyin"),
        raw!(include!("fixtures/axis_c/74a991a48b589cf6.rs")),
        None,
    );
}
// axis-c-encoding-pinyin-lookup_tokens-hanzi: MATCH.
#[test]
fn axis_c_encoding_pinyin_lookup_tokens_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_lookup_tokens_hanzi",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-pinyin-lookup_tokens-hanzi"),
        raw!(M(&[
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("ret", B(true)),
            ("tokens", L(&[I(16802309)]))
        ])),
        None,
    );
}
// axis-c-encoding-pinyin-parse_more_double_pinyins-ff-middle: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_double_pinyins_ff_middle() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_double_pinyins_ff_middle",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(
                k,
                "axis-c-encoding-pinyin-parse_more_double_pinyins-ff-middle",
            )
        },
        raw!(include!("fixtures/axis_c/113eef27716ef021.rs")),
        None,
    );
}
// axis-c-encoding-pinyin-parse_more_full_pinyins-nfc: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_full_pinyins_nfc() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_full_pinyins_nfc",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_full_pinyins-nfc"),
        raw!(include!("fixtures/axis_c/dafdb1158e419e19.rs")),
        None,
    );
}
// axis-c-encoding-pinyin-phrase_segment-nul-middle: MATCH.
#[test]
fn axis_c_encoding_pinyin_phrase_segment_nul_middle() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_phrase_segment_nul_middle",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-pinyin-phrase_segment-nul-middle"),
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
// axis-c-encoding-pinyin-remember_user_input-hanzi: MATCH.
#[test]
fn axis_c_encoding_pinyin_remember_user_input_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_remember_user_input_hanzi",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-encoding-pinyin-remember_user_input-hanzi"),
        raw!(M(&[
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("parsed", I(5)),
            ("ret", B(true)),
            ("rows", L(&[L(&[B(true), S("你好"), S("ni'hao"), I(1)])]))
        ])),
        None,
    );
}
// axis-c-layout-pinyin-1-00000000: MATCH.
#[test]
fn axis_c_layout_pinyin_1_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_1_00000000",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-layout-pinyin-1-00000000"),
        raw!(include!("fixtures/axis_c/14f05efcfaac2d73.rs")),
        None,
    );
}

// axis-c-layout-pinyin-1-00000060: MATCH.
#[test]
fn axis_c_layout_pinyin_1_00000060() {
    run_j(
        "axis_c::axis_c_layout_pinyin_1_00000060",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-layout-pinyin-1-00000060"),
        raw!(include!("fixtures/axis_c/22ab9b8cb09ace33.rs")),
        None,
    );
}
// axis-c-layout-pinyin-4-00000000: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn axis_c_layout_pinyin_4_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_4_00000000",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-layout-pinyin-4-00000000"),
        raw!(include!("fixtures/axis_c/9d685e50b83bb88b.rs")),
        None,
    );
}
// axis-c-layout-pinyin-9-00000000: MATCH.
#[test]
fn axis_c_layout_pinyin_9_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_9_00000000",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-layout-pinyin-9-00000000"),
        raw!(include!("fixtures/axis_c/05402d314ce38220.rs")),
        None,
    );
}
// axis-c-options-boundary-ffffffff: MATCH.
#[test]
fn axis_c_options_boundary_ffffffff() {
    run_j(
        "axis_c::axis_c_options_boundary_ffffffff",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-options-boundary-ffffffff"),
        raw!(include!("fixtures/axis_c/c93ff4ac9e000439.rs")),
        None,
    );
}

// axis-c-options-pair-00000018: MATCH.
#[test]
fn axis_c_options_pair_00000018() {
    run_j(
        "axis_c::axis_c_options_pair_00000018",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-options-pair-00000018"),
        raw!(include!("fixtures/axis_c/d46eb6ddc3ee831f.rs")),
        None,
    );
}

// axis-c-options-pair-00000060: MATCH.
#[test]
fn axis_c_options_pair_00000060() {
    run_j(
        "axis_c::axis_c_options_pair_00000060",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-options-pair-00000060"),
        raw!(include!("fixtures/axis_c/c93ff4ac9e000439.rs")),
        None,
    );
}
// axis-c-options-single-00000008: MATCH.
#[test]
fn axis_c_options_single_00000008() {
    run_j(
        "axis_c::axis_c_options_single_00000008",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "axis-c-options-single-00000008"),
        raw!(include!("fixtures/axis_c/d46eb6ddc3ee831f.rs")),
        None,
    );
}
