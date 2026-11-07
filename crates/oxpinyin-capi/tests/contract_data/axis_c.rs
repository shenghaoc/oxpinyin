//! Axis C: the option word, the input encodings and the zhuyin layouts (#1175, #1620). Generated from the frozen pin observations.

use crate::bodies::axis_c as b;
use crate::support::{Expect, run_aborting, run_j};

// axis-c-encoding-pinyin-import-phrase-empty: MATCH.
#[test]
fn axis_c_encoding_pinyin_import_phrase_empty() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_import_phrase_empty",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-import-phrase-empty"),
        raw!(include!("fixtures/axis_c/0585432bc9d11738.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-import-phrase-hanzi: MATCH.
#[test]
fn axis_c_encoding_pinyin_import_phrase_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_import_phrase_hanzi",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-import-phrase-hanzi"),
        raw!(include!("fixtures/axis_c/74a991a48b589cf6.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-import-phrase-nul-middle: MATCH.
#[test]
fn axis_c_encoding_pinyin_import_phrase_nul_middle() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_import_phrase_nul_middle",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-import-phrase-nul-middle"),
        raw!(include!("fixtures/axis_c/300a27d3c65dd321.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-import-reading-empty: MATCH.
#[test]
fn axis_c_encoding_pinyin_import_reading_empty() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_import_reading_empty",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-import-reading-empty"),
        raw!(include!("fixtures/axis_c/dfe24221e6b513be.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-import-reading-pinyin: MATCH.
#[test]
fn axis_c_encoding_pinyin_import_reading_pinyin() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_import_reading_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-import-reading-pinyin"),
        raw!(include!("fixtures/axis_c/74a991a48b589cf6.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-import-reading-tone: MATCH.
#[test]
fn axis_c_encoding_pinyin_import_reading_tone() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_import_reading_tone",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-import-reading-tone"),
        raw!(include!("fixtures/axis_c/209f46dcea94ace3.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-lookup_tokens-hanzi: MATCH.
#[test]
fn axis_c_encoding_pinyin_lookup_tokens_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_lookup_tokens_hanzi",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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

// axis-c-encoding-pinyin-lookup_tokens-overlong-start: MATCH.
#[test]
fn axis_c_encoding_pinyin_lookup_tokens_overlong_start() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_lookup_tokens_overlong_start",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-lookup_tokens-overlong-start"),
        raw!(M(&[
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[]))
        ])),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_chewings-bopomofo: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_chewings_bopomofo() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_chewings_bopomofo",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_chewings-bopomofo"),
        raw!(include!("fixtures/axis_c/d8171b40697b7226.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_chewings-nfc: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_chewings_nfc() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_chewings_nfc",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_chewings-nfc"),
        raw!(include!("fixtures/axis_c/09853e1b98556bdb.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_chewings-overlong-end: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_chewings_overlong_end() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_chewings_overlong_end",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_chewings-overlong-end"),
        raw!(include!("fixtures/axis_c/f0ff563973c843d8.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_chewings-overlong-middle: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_chewings_overlong_middle() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_chewings_overlong_middle",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(
                k,
                "axis-c-encoding-pinyin-parse_more_chewings-overlong-middle",
            )
        },
        raw!(include!("fixtures/axis_c/e54782117646e745.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_chewings-tone: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_chewings_tone() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_chewings_tone",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_chewings-tone"),
        raw!(include!("fixtures/axis_c/795c8f4000252ebf.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_double_pinyins-ff-end: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_double_pinyins_ff_end() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_double_pinyins_ff_end",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_double_pinyins-ff-end"),
        raw!(include!("fixtures/axis_c/efd00e1b88cb0d96.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_double_pinyins-ff-middle: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_double_pinyins_ff_middle() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_double_pinyins_ff_middle",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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

// axis-c-encoding-pinyin-parse_more_double_pinyins-hanzi: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_double_pinyins_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_double_pinyins_hanzi",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_double_pinyins-hanzi"),
        raw!(include!("fixtures/axis_c/d8171b40697b7226.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_double_pinyins-nfc: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_double_pinyins_nfc() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_double_pinyins_nfc",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_double_pinyins-nfc"),
        raw!(include!("fixtures/axis_c/dafdb1158e419e19.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_double_pinyins-tone: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_double_pinyins_tone() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_double_pinyins_tone",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_double_pinyins-tone"),
        raw!(include!("fixtures/axis_c/75ce71a17fdea0cd.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_full_pinyins-empty: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_full_pinyins_empty() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_full_pinyins_empty",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_full_pinyins-empty"),
        raw!(include!("fixtures/axis_c/d8171b40697b7226.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_full_pinyins-nfc: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_full_pinyins_nfc() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_full_pinyins_nfc",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_full_pinyins-nfc"),
        raw!(include!("fixtures/axis_c/dafdb1158e419e19.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_full_pinyins-nfd: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_full_pinyins_nfd() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_full_pinyins_nfd",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_full_pinyins-nfd"),
        raw!(include!("fixtures/axis_c/113eef27716ef021.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_full_pinyins-pinyin: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_full_pinyins_pinyin() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_full_pinyins_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_full_pinyins-pinyin"),
        raw!(include!("fixtures/axis_c/a0a82f7b9c12b4b4.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-parse_more_full_pinyins-tone: MATCH.
#[test]
fn axis_c_encoding_pinyin_parse_more_full_pinyins_tone() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_parse_more_full_pinyins_tone",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-parse_more_full_pinyins-tone"),
        raw!(include!("fixtures/axis_c/46022603cac07f21.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-phrase_segment-ff-start: POLICY-ACCEPTED, oxpinyin's value (1297-segment-critical.md).
#[test]
fn axis_c_encoding_pinyin_phrase_segment_ff_start() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_phrase_segment_ff_start",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-phrase_segment-ff-start"),
        raw!(M(&[
            ("count", I(0)),
            ("get_n_phrase", B(true)),
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("ret", B(false))
        ])),
        None,
    );
}

// axis-c-encoding-pinyin-phrase_segment-hanzi: MATCH.
#[test]
fn axis_c_encoding_pinyin_phrase_segment_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_phrase_segment_hanzi",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-phrase_segment-hanzi"),
        raw!(M(&[
            ("count", I(2)),
            ("get_n_phrase", B(true)),
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("ret", B(true))
        ])),
        None,
    );
}

// axis-c-encoding-pinyin-phrase_segment-nfd: MATCH.
#[test]
fn axis_c_encoding_pinyin_phrase_segment_nfd() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_phrase_segment_nfd",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-phrase_segment-nfd"),
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

// axis-c-encoding-pinyin-phrase_segment-nul-end: MATCH.
#[test]
fn axis_c_encoding_pinyin_phrase_segment_nul_end() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_phrase_segment_nul_end",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-phrase_segment-nul-end"),
        raw!(M(&[
            ("count", I(5)),
            ("get_n_phrase", B(true)),
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("ret", B(false))
        ])),
        None,
    );
}

// axis-c-encoding-pinyin-phrase_segment-nul-middle: MATCH.
#[test]
fn axis_c_encoding_pinyin_phrase_segment_nul_middle() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_phrase_segment_nul_middle",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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

// axis-c-encoding-pinyin-phrase_segment-nul-start: MATCH.
#[test]
fn axis_c_encoding_pinyin_phrase_segment_nul_start() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_phrase_segment_nul_start",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-phrase_segment-nul-start"),
        raw!(M(&[
            ("count", I(0)),
            ("get_n_phrase", B(true)),
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("ret", B(true))
        ])),
        None,
    );
}

// axis-c-encoding-pinyin-phrase_segment-surrogate-start: MATCH.
#[test]
fn axis_c_encoding_pinyin_phrase_segment_surrogate_start() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_phrase_segment_surrogate_start",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-pinyin-phrase_segment-surrogate-start"),
        raw!(include!("fixtures/axis_c/868f2e103465ad96.rs")),
        None,
    );
}

// axis-c-encoding-pinyin-remember_user_input-hanzi: MATCH.
#[test]
fn axis_c_encoding_pinyin_remember_user_input_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_remember_user_input_hanzi",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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

// axis-c-encoding-pinyin-remember_user_input-truncated-start: MATCH.
#[test]
fn axis_c_encoding_pinyin_remember_user_input_truncated_start() {
    run_j(
        "axis_c::axis_c_encoding_pinyin_remember_user_input_truncated_start",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(
                k,
                "axis-c-encoding-pinyin-remember_user_input-truncated-start",
            )
        },
        raw!(M(&[
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("parsed", I(5)),
            ("ret", B(false)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// axis-c-layout-pinyin-0-00000000: the pin aborts in the scheme setter, before any input is read.
#[test]
fn axis_c_layout_pinyin_0_00000000() {
    run_aborting(
        "axis_c::axis_c_layout_pinyin_0_00000000",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "axis-c-layout-pinyin-0-00000000");
        },
    );
}

// axis-c-layout-pinyin-1-00000000: MATCH.
#[test]
fn axis_c_layout_pinyin_1_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_1_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-1-00000060"),
        raw!(include!("fixtures/axis_c/22ab9b8cb09ace33.rs")),
        None,
    );
}

// axis-c-layout-pinyin-2-00000000: MATCH.
#[test]
fn axis_c_layout_pinyin_2_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_2_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-2-00000000"),
        raw!(include!("fixtures/axis_c/3e395128cbe8ffd8.rs")),
        None,
    );
}

// axis-c-layout-pinyin-2-00000060: MATCH.
#[test]
fn axis_c_layout_pinyin_2_00000060() {
    run_j(
        "axis_c::axis_c_layout_pinyin_2_00000060",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-2-00000060"),
        raw!(include!("fixtures/axis_c/77cbdd7f221e05c3.rs")),
        None,
    );
}

// axis-c-layout-pinyin-3-00000000: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn axis_c_layout_pinyin_3_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_3_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-3-00000000"),
        raw!(include!("fixtures/axis_c/b77fc1a870f2749f.rs")),
        None,
    );
}

// axis-c-layout-pinyin-3-00000060: MATCH.
#[test]
fn axis_c_layout_pinyin_3_00000060() {
    run_j(
        "axis_c::axis_c_layout_pinyin_3_00000060",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-3-00000060"),
        raw!(include!("fixtures/axis_c/72079bb9310eed99.rs")),
        None,
    );
}

// axis-c-layout-pinyin-4-00000000: POLICY-ACCEPTED, oxpinyin's value (trellis-adjusted-cost-s1-s2.md).
#[test]
fn axis_c_layout_pinyin_4_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_4_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-4-00000000"),
        raw!(include!("fixtures/axis_c/9d685e50b83bb88b.rs")),
        None,
    );
}

// axis-c-layout-pinyin-4-00000060: MATCH.
#[test]
fn axis_c_layout_pinyin_4_00000060() {
    run_j(
        "axis_c::axis_c_layout_pinyin_4_00000060",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-4-00000060"),
        raw!(include!("fixtures/axis_c/c61cc9617b24be74.rs")),
        None,
    );
}

// axis-c-layout-pinyin-5-00000000: MATCH.
#[test]
fn axis_c_layout_pinyin_5_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_5_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-5-00000000"),
        raw!(include!("fixtures/axis_c/611df762277de397.rs")),
        None,
    );
}

// axis-c-layout-pinyin-5-00000060: MATCH.
#[test]
fn axis_c_layout_pinyin_5_00000060() {
    run_j(
        "axis_c::axis_c_layout_pinyin_5_00000060",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-5-00000060"),
        raw!(include!("fixtures/axis_c/6bb96a2ddbb60adb.rs")),
        None,
    );
}

// axis-c-layout-pinyin-6-00000000: MATCH.
#[test]
fn axis_c_layout_pinyin_6_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_6_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-6-00000000"),
        raw!(include!("fixtures/axis_c/cdacdf01b0d77976.rs")),
        None,
    );
}

// axis-c-layout-pinyin-6-00000060: MATCH.
#[test]
fn axis_c_layout_pinyin_6_00000060() {
    run_j(
        "axis_c::axis_c_layout_pinyin_6_00000060",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-6-00000060"),
        raw!(include!("fixtures/axis_c/192ff5b77355ceb6.rs")),
        None,
    );
}

// axis-c-layout-pinyin-7-00000000: the pin aborts in the scheme setter, before any input is read.
#[test]
fn axis_c_layout_pinyin_7_00000000() {
    run_aborting(
        "axis_c::axis_c_layout_pinyin_7_00000000",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "axis-c-layout-pinyin-7-00000000");
        },
    );
}

// axis-c-layout-pinyin-8-00000000: MATCH.
#[test]
fn axis_c_layout_pinyin_8_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_8_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-8-00000000"),
        raw!(include!("fixtures/axis_c/3e395128cbe8ffd8.rs")),
        None,
    );
}

// axis-c-layout-pinyin-9-00000000: MATCH.
#[test]
fn axis_c_layout_pinyin_9_00000000() {
    run_j(
        "axis_c::axis_c_layout_pinyin_9_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-9-00000000"),
        raw!(include!("fixtures/axis_c/05402d314ce38220.rs")),
        None,
    );
}

// axis-c-layout-pinyin-9-00000060: MATCH.
#[test]
fn axis_c_layout_pinyin_9_00000060() {
    run_j(
        "axis_c::axis_c_layout_pinyin_9_00000060",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-pinyin-9-00000060"),
        raw!(include!("fixtures/axis_c/9faec9dda267a3dd.rs")),
        None,
    );
}

// axis-c-options-boundary-00000000: MATCH.
#[test]
fn axis_c_options_boundary_00000000() {
    run_j(
        "axis_c::axis_c_options_boundary_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-boundary-00000000"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-boundary-00000001: MATCH.
#[test]
fn axis_c_options_boundary_00000001() {
    run_j(
        "axis_c::axis_c_options_boundary_00000001",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-boundary-00000001"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-boundary-00100000: MATCH.
#[test]
fn axis_c_options_boundary_00100000() {
    run_j(
        "axis_c::axis_c_options_boundary_00100000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-boundary-00100000"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-boundary-fffffffe: MATCH.
#[test]
fn axis_c_options_boundary_fffffffe() {
    run_j(
        "axis_c::axis_c_options_boundary_fffffffe",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-boundary-fffffffe"),
        raw!(include!("fixtures/axis_c/c93ff4ac9e000439.rs")),
        None,
    );
}

// axis-c-options-boundary-ffffffff: MATCH.
#[test]
fn axis_c_options_boundary_ffffffff() {
    run_j(
        "axis_c::axis_c_options_boundary_ffffffff",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00000060"),
        raw!(include!("fixtures/axis_c/c93ff4ac9e000439.rs")),
        None,
    );
}

// axis-c-options-pair-00000208: MATCH.
#[test]
fn axis_c_options_pair_00000208() {
    run_j(
        "axis_c::axis_c_options_pair_00000208",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00000208"),
        raw!(include!("fixtures/axis_c/58ae716566fe23f2.rs")),
        None,
    );
}

// axis-c-options-pair-00000280: MATCH.
#[test]
fn axis_c_options_pair_00000280() {
    run_j(
        "axis_c::axis_c_options_pair_00000280",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00000280"),
        raw!(include!("fixtures/axis_c/87e1a41f62bd9c6a.rs")),
        None,
    );
}

// axis-c-options-pair-00000408: MATCH.
#[test]
fn axis_c_options_pair_00000408() {
    run_j(
        "axis_c::axis_c_options_pair_00000408",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00000408"),
        raw!(include!("fixtures/axis_c/5d6089550a7aa846.rs")),
        None,
    );
}

// axis-c-options-pair-00000a00: MATCH.
#[test]
fn axis_c_options_pair_00000a00() {
    run_j(
        "axis_c::axis_c_options_pair_00000a00",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00000a00"),
        raw!(include!("fixtures/axis_c/d0f6bc640941481f.rs")),
        None,
    );
}

// axis-c-options-pair-00001200: MATCH.
#[test]
fn axis_c_options_pair_00001200() {
    run_j(
        "axis_c::axis_c_options_pair_00001200",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00001200"),
        raw!(include!("fixtures/axis_c/18203ac513dbaad1.rs")),
        None,
    );
}

// axis-c-options-pair-00002800: MATCH.
#[test]
fn axis_c_options_pair_00002800() {
    run_j(
        "axis_c::axis_c_options_pair_00002800",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00002800"),
        raw!(include!("fixtures/axis_c/33c836a18d1dca14.rs")),
        None,
    );
}

// axis-c-options-pair-00004080: MATCH.
#[test]
fn axis_c_options_pair_00004080() {
    run_j(
        "axis_c::axis_c_options_pair_00004080",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00004080"),
        raw!(include!("fixtures/axis_c/97f59d55de758ebb.rs")),
        None,
    );
}

// axis-c-options-pair-00004200: MATCH.
#[test]
fn axis_c_options_pair_00004200() {
    run_j(
        "axis_c::axis_c_options_pair_00004200",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00004200"),
        raw!(include!("fixtures/axis_c/09b2a1ffe985b203.rs")),
        None,
    );
}

// axis-c-options-pair-00004800: MATCH.
#[test]
fn axis_c_options_pair_00004800() {
    run_j(
        "axis_c::axis_c_options_pair_00004800",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00004800"),
        raw!(include!("fixtures/axis_c/1e876aed42231bb7.rs")),
        None,
    );
}

// axis-c-options-pair-00008008: MATCH.
#[test]
fn axis_c_options_pair_00008008() {
    run_j(
        "axis_c::axis_c_options_pair_00008008",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00008008"),
        raw!(include!("fixtures/axis_c/2f5b5a79371dfece.rs")),
        None,
    );
}

// axis-c-options-pair-00008080: MATCH.
#[test]
fn axis_c_options_pair_00008080() {
    run_j(
        "axis_c::axis_c_options_pair_00008080",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00008080"),
        raw!(include!("fixtures/axis_c/08400afef8c6291f.rs")),
        None,
    );
}

// axis-c-options-pair-00008200: MATCH.
#[test]
fn axis_c_options_pair_00008200() {
    run_j(
        "axis_c::axis_c_options_pair_00008200",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00008200"),
        raw!(include!("fixtures/axis_c/443e5f6a8dbf7f08.rs")),
        None,
    );
}

// axis-c-options-pair-00008800: MATCH.
#[test]
fn axis_c_options_pair_00008800() {
    run_j(
        "axis_c::axis_c_options_pair_00008800",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00008800"),
        raw!(include!("fixtures/axis_c/84c4061a30d7e665.rs")),
        None,
    );
}

// axis-c-options-pair-0000c000: MATCH.
#[test]
fn axis_c_options_pair_0000c000() {
    run_j(
        "axis_c::axis_c_options_pair_0000c000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-0000c000"),
        raw!(include!("fixtures/axis_c/f4dfc1437013d827.rs")),
        None,
    );
}

// axis-c-options-pair-00010080: MATCH.
#[test]
fn axis_c_options_pair_00010080() {
    run_j(
        "axis_c::axis_c_options_pair_00010080",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00010080"),
        raw!(include!("fixtures/axis_c/b943badef6661504.rs")),
        None,
    );
}

// axis-c-options-pair-00010200: MATCH.
#[test]
fn axis_c_options_pair_00010200() {
    run_j(
        "axis_c::axis_c_options_pair_00010200",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00010200"),
        raw!(include!("fixtures/axis_c/74539dde61b3baef.rs")),
        None,
    );
}

// axis-c-options-pair-00010800: MATCH.
#[test]
fn axis_c_options_pair_00010800() {
    run_j(
        "axis_c::axis_c_options_pair_00010800",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00010800"),
        raw!(include!("fixtures/axis_c/807d25bda8e3498e.rs")),
        None,
    );
}

// axis-c-options-pair-00014000: MATCH.
#[test]
fn axis_c_options_pair_00014000() {
    run_j(
        "axis_c::axis_c_options_pair_00014000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00014000"),
        raw!(include!("fixtures/axis_c/cc653bada960ae56.rs")),
        None,
    );
}

// axis-c-options-pair-00018000: MATCH.
#[test]
fn axis_c_options_pair_00018000() {
    run_j(
        "axis_c::axis_c_options_pair_00018000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00018000"),
        raw!(include!("fixtures/axis_c/25e5f1fcb8afb829.rs")),
        None,
    );
}

// axis-c-options-pair-00020080: MATCH.
#[test]
fn axis_c_options_pair_00020080() {
    run_j(
        "axis_c::axis_c_options_pair_00020080",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00020080"),
        raw!(include!("fixtures/axis_c/5eb51a94449eb6be.rs")),
        None,
    );
}

// axis-c-options-pair-00020200: MATCH.
#[test]
fn axis_c_options_pair_00020200() {
    run_j(
        "axis_c::axis_c_options_pair_00020200",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00020200"),
        raw!(include!("fixtures/axis_c/a7c2c3a50d8cb114.rs")),
        None,
    );
}

// axis-c-options-pair-00020800: MATCH.
#[test]
fn axis_c_options_pair_00020800() {
    run_j(
        "axis_c::axis_c_options_pair_00020800",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00020800"),
        raw!(include!("fixtures/axis_c/f4852cb3fbe5820e.rs")),
        None,
    );
}

// axis-c-options-pair-00021000: MATCH.
#[test]
fn axis_c_options_pair_00021000() {
    run_j(
        "axis_c::axis_c_options_pair_00021000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00021000"),
        raw!(include!("fixtures/axis_c/69fdbfa180bd0438.rs")),
        None,
    );
}

// axis-c-options-pair-00024000: MATCH.
#[test]
fn axis_c_options_pair_00024000() {
    run_j(
        "axis_c::axis_c_options_pair_00024000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00024000"),
        raw!(include!("fixtures/axis_c/df155e1a9d1b8551.rs")),
        None,
    );
}

// axis-c-options-pair-00028000: MATCH.
#[test]
fn axis_c_options_pair_00028000() {
    run_j(
        "axis_c::axis_c_options_pair_00028000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00028000"),
        raw!(include!("fixtures/axis_c/b54500df6a40a9df.rs")),
        None,
    );
}

// axis-c-options-pair-00030000: MATCH.
#[test]
fn axis_c_options_pair_00030000() {
    run_j(
        "axis_c::axis_c_options_pair_00030000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00030000"),
        raw!(include!("fixtures/axis_c/674af2d882729934.rs")),
        None,
    );
}

// axis-c-options-pair-00040200: MATCH.
#[test]
fn axis_c_options_pair_00040200() {
    run_j(
        "axis_c::axis_c_options_pair_00040200",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00040200"),
        raw!(include!("fixtures/axis_c/b90fc762721a9b91.rs")),
        None,
    );
}

// axis-c-options-pair-00040800: MATCH.
#[test]
fn axis_c_options_pair_00040800() {
    run_j(
        "axis_c::axis_c_options_pair_00040800",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00040800"),
        raw!(include!("fixtures/axis_c/dbf93e58ba30514b.rs")),
        None,
    );
}

// axis-c-options-pair-00044000: MATCH.
#[test]
fn axis_c_options_pair_00044000() {
    run_j(
        "axis_c::axis_c_options_pair_00044000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00044000"),
        raw!(include!("fixtures/axis_c/dfde4fd9defab4dc.rs")),
        None,
    );
}

// axis-c-options-pair-00048000: MATCH.
#[test]
fn axis_c_options_pair_00048000() {
    run_j(
        "axis_c::axis_c_options_pair_00048000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00048000"),
        raw!(include!("fixtures/axis_c/e4f0d2e83edbcbc0.rs")),
        None,
    );
}

// axis-c-options-pair-00050000: MATCH.
#[test]
fn axis_c_options_pair_00050000() {
    run_j(
        "axis_c::axis_c_options_pair_00050000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00050000"),
        raw!(include!("fixtures/axis_c/caa3a4e47ae7699d.rs")),
        None,
    );
}

// axis-c-options-pair-00060000: MATCH.
#[test]
fn axis_c_options_pair_00060000() {
    run_j(
        "axis_c::axis_c_options_pair_00060000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00060000"),
        raw!(include!("fixtures/axis_c/f466ff1cc84e479f.rs")),
        None,
    );
}

// axis-c-options-pair-00080200: MATCH.
#[test]
fn axis_c_options_pair_00080200() {
    run_j(
        "axis_c::axis_c_options_pair_00080200",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00080200"),
        raw!(include!("fixtures/axis_c/9a1deff85590429a.rs")),
        None,
    );
}

// axis-c-options-pair-00080800: MATCH.
#[test]
fn axis_c_options_pair_00080800() {
    run_j(
        "axis_c::axis_c_options_pair_00080800",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00080800"),
        raw!(include!("fixtures/axis_c/e1c915d2fc413594.rs")),
        None,
    );
}

// axis-c-options-pair-00082000: MATCH.
#[test]
fn axis_c_options_pair_00082000() {
    run_j(
        "axis_c::axis_c_options_pair_00082000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00082000"),
        raw!(include!("fixtures/axis_c/01b6fd6f7b507b5e.rs")),
        None,
    );
}

// axis-c-options-pair-00084000: MATCH.
#[test]
fn axis_c_options_pair_00084000() {
    run_j(
        "axis_c::axis_c_options_pair_00084000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00084000"),
        raw!(include!("fixtures/axis_c/4a488bf16cec6416.rs")),
        None,
    );
}

// axis-c-options-pair-00088000: MATCH.
#[test]
fn axis_c_options_pair_00088000() {
    run_j(
        "axis_c::axis_c_options_pair_00088000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-00088000"),
        raw!(include!("fixtures/axis_c/ae71eb61f35df3ec.rs")),
        None,
    );
}

// axis-c-options-pair-000a0000: MATCH.
#[test]
fn axis_c_options_pair_000a0000() {
    run_j(
        "axis_c::axis_c_options_pair_000a0000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-000a0000"),
        raw!(include!("fixtures/axis_c/df235b90e17c1158.rs")),
        None,
    );
}

// axis-c-options-pair-000c0000: MATCH.
#[test]
fn axis_c_options_pair_000c0000() {
    run_j(
        "axis_c::axis_c_options_pair_000c0000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-000c0000"),
        raw!(include!("fixtures/axis_c/6660c65b4951bce9.rs")),
        None,
    );
}

// axis-c-options-pair-10000008: MATCH.
#[test]
fn axis_c_options_pair_10000008() {
    run_j(
        "axis_c::axis_c_options_pair_10000008",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-10000008"),
        raw!(include!("fixtures/axis_c/25fff8c939ce18a4.rs")),
        None,
    );
}

// axis-c-options-pair-10000200: MATCH.
#[test]
fn axis_c_options_pair_10000200() {
    run_j(
        "axis_c::axis_c_options_pair_10000200",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-10000200"),
        raw!(include!("fixtures/axis_c/a7b45aa86e01dc49.rs")),
        None,
    );
}

// axis-c-options-pair-10008000: MATCH.
#[test]
fn axis_c_options_pair_10008000() {
    run_j(
        "axis_c::axis_c_options_pair_10008000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-pair-10008000"),
        raw!(include!("fixtures/axis_c/58f9cba800ee3464.rs")),
        None,
    );
}

// axis-c-options-single-00000002: MATCH.
#[test]
fn axis_c_options_single_00000002() {
    run_j(
        "axis_c::axis_c_options_single_00000002",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00000002"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-00000008: MATCH.
#[test]
fn axis_c_options_single_00000008() {
    run_j(
        "axis_c::axis_c_options_single_00000008",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00000008"),
        raw!(include!("fixtures/axis_c/d46eb6ddc3ee831f.rs")),
        None,
    );
}

// axis-c-options-single-00000010: MATCH.
#[test]
fn axis_c_options_single_00000010() {
    run_j(
        "axis_c::axis_c_options_single_00000010",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00000010"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-00000020: MATCH.
#[test]
fn axis_c_options_single_00000020() {
    run_j(
        "axis_c::axis_c_options_single_00000020",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00000020"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-00000040: MATCH.
#[test]
fn axis_c_options_single_00000040() {
    run_j(
        "axis_c::axis_c_options_single_00000040",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00000040"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-00000080: MATCH.
#[test]
fn axis_c_options_single_00000080() {
    run_j(
        "axis_c::axis_c_options_single_00000080",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00000080"),
        raw!(include!("fixtures/axis_c/63154fa3b33c165f.rs")),
        None,
    );
}

// axis-c-options-single-00000100: MATCH.
#[test]
fn axis_c_options_single_00000100() {
    run_j(
        "axis_c::axis_c_options_single_00000100",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00000100"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-00000200: MATCH.
#[test]
fn axis_c_options_single_00000200() {
    run_j(
        "axis_c::axis_c_options_single_00000200",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00000200"),
        raw!(include!("fixtures/axis_c/2f8d08a183912b5e.rs")),
        None,
    );
}

// axis-c-options-single-00000400: MATCH.
#[test]
fn axis_c_options_single_00000400() {
    run_j(
        "axis_c::axis_c_options_single_00000400",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00000400"),
        raw!(include!("fixtures/axis_c/f31357190ddf7651.rs")),
        None,
    );
}

// axis-c-options-single-00000800: MATCH.
#[test]
fn axis_c_options_single_00000800() {
    run_j(
        "axis_c::axis_c_options_single_00000800",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00000800"),
        raw!(include!("fixtures/axis_c/06271c092a898bc4.rs")),
        None,
    );
}

// axis-c-options-single-00001000: MATCH.
#[test]
fn axis_c_options_single_00001000() {
    run_j(
        "axis_c::axis_c_options_single_00001000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00001000"),
        raw!(include!("fixtures/axis_c/99865f2dd355247f.rs")),
        None,
    );
}

// axis-c-options-single-00002000: MATCH.
#[test]
fn axis_c_options_single_00002000() {
    run_j(
        "axis_c::axis_c_options_single_00002000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00002000"),
        raw!(include!("fixtures/axis_c/e2c969ed06f3365e.rs")),
        None,
    );
}

// axis-c-options-single-00004000: MATCH.
#[test]
fn axis_c_options_single_00004000() {
    run_j(
        "axis_c::axis_c_options_single_00004000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00004000"),
        raw!(include!("fixtures/axis_c/0f85a79ccd1de90d.rs")),
        None,
    );
}

// axis-c-options-single-00008000: MATCH.
#[test]
fn axis_c_options_single_00008000() {
    run_j(
        "axis_c::axis_c_options_single_00008000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00008000"),
        raw!(include!("fixtures/axis_c/acc869f9c28c47aa.rs")),
        None,
    );
}

// axis-c-options-single-00010000: MATCH.
#[test]
fn axis_c_options_single_00010000() {
    run_j(
        "axis_c::axis_c_options_single_00010000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00010000"),
        raw!(include!("fixtures/axis_c/f02c070f368c7312.rs")),
        None,
    );
}

// axis-c-options-single-00020000: MATCH.
#[test]
fn axis_c_options_single_00020000() {
    run_j(
        "axis_c::axis_c_options_single_00020000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00020000"),
        raw!(include!("fixtures/axis_c/591e4eddc5c177bf.rs")),
        None,
    );
}

// axis-c-options-single-00040000: MATCH.
#[test]
fn axis_c_options_single_00040000() {
    run_j(
        "axis_c::axis_c_options_single_00040000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00040000"),
        raw!(include!("fixtures/axis_c/5b90b1d994e403b6.rs")),
        None,
    );
}

// axis-c-options-single-00080000: MATCH.
#[test]
fn axis_c_options_single_00080000() {
    run_j(
        "axis_c::axis_c_options_single_00080000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00080000"),
        raw!(include!("fixtures/axis_c/89b50d7126dfecfb.rs")),
        None,
    );
}

// axis-c-options-single-00200000: MATCH.
#[test]
fn axis_c_options_single_00200000() {
    run_j(
        "axis_c::axis_c_options_single_00200000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00200000"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-00400000: MATCH.
#[test]
fn axis_c_options_single_00400000() {
    run_j(
        "axis_c::axis_c_options_single_00400000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00400000"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-00800000: MATCH.
#[test]
fn axis_c_options_single_00800000() {
    run_j(
        "axis_c::axis_c_options_single_00800000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-00800000"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-01000000: MATCH.
#[test]
fn axis_c_options_single_01000000() {
    run_j(
        "axis_c::axis_c_options_single_01000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-01000000"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-02000000: MATCH.
#[test]
fn axis_c_options_single_02000000() {
    run_j(
        "axis_c::axis_c_options_single_02000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-02000000"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-04000000: MATCH.
#[test]
fn axis_c_options_single_04000000() {
    run_j(
        "axis_c::axis_c_options_single_04000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-04000000"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-08000000: MATCH.
#[test]
fn axis_c_options_single_08000000() {
    run_j(
        "axis_c::axis_c_options_single_08000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-08000000"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}

// axis-c-options-single-10000000: MATCH.
#[test]
fn axis_c_options_single_10000000() {
    run_j(
        "axis_c::axis_c_options_single_10000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-10000000"),
        raw!(include!("fixtures/axis_c/6eebc39ea40c654f.rs")),
        None,
    );
}

// axis-c-options-single-20000000: MATCH.
#[test]
fn axis_c_options_single_20000000() {
    run_j(
        "axis_c::axis_c_options_single_20000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-options-single-20000000"),
        raw!(include!("fixtures/axis_c/976b572933756dc6.rs")),
        None,
    );
}
