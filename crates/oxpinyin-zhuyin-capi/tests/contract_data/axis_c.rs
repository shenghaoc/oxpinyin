//! Axis C: the option word, the input encodings and the zhuyin layouts (#1175, #1620). Generated from the frozen pin observations.

use crate::bodies::axis_c as b;
use crate::support::{Expect, run_aborting, run_j};

// axis-c-encoding-zhuyin-import-phrase-empty: MATCH.
#[test]
fn axis_c_encoding_zhuyin_import_phrase_empty() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_import_phrase_empty",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-import-phrase-empty"),
        raw!(include!("fixtures/axis_c/0585432bc9d11738.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-import-phrase-hanzi: MATCH.
#[test]
fn axis_c_encoding_zhuyin_import_phrase_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_import_phrase_hanzi",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-import-phrase-hanzi"),
        raw!(include!("fixtures/axis_c/209f46dcea94ace3.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-import-phrase-nul-middle: MATCH.
#[test]
fn axis_c_encoding_zhuyin_import_phrase_nul_middle() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_import_phrase_nul_middle",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-import-phrase-nul-middle"),
        raw!(include!("fixtures/axis_c/db99484272e202f7.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-import-reading-bopomofo: MATCH.
#[test]
fn axis_c_encoding_zhuyin_import_reading_bopomofo() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_import_reading_bopomofo",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-import-reading-bopomofo"),
        raw!(include!("fixtures/axis_c/209f46dcea94ace3.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-import-reading-empty: MATCH.
#[test]
fn axis_c_encoding_zhuyin_import_reading_empty() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_import_reading_empty",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-import-reading-empty"),
        raw!(include!("fixtures/axis_c/dfe24221e6b513be.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-lookup_tokens-hanzi: MATCH.
#[test]
fn axis_c_encoding_zhuyin_lookup_tokens_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_lookup_tokens_hanzi",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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

// axis-c-encoding-zhuyin-lookup_tokens-surrogate-start: MATCH.
#[test]
fn axis_c_encoding_zhuyin_lookup_tokens_surrogate_start() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_lookup_tokens_surrogate_start",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-lookup_tokens-surrogate-start"),
        raw!(M(&[
            ("log_messages", L(&[])),
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[]))
        ])),
        None,
    );
}

// axis-c-encoding-zhuyin-parse_more_chewings-ff-start: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_chewings_ff_start() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_chewings_ff_start",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-parse_more_chewings-ff-start"),
        raw!(include!("fixtures/axis_c/d8171b40697b7226.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-parse_more_chewings-nfc: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_chewings_nfc() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_chewings_nfc",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-parse_more_chewings-nfc"),
        raw!(include!("fixtures/axis_c/42044682cd282da5.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-parse_more_chewings-tone: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_chewings_tone() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_chewings_tone",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-parse_more_chewings-tone"),
        raw!(include!("fixtures/axis_c/3db045de3f410331.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-parse_more_chewings-truncated-end: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_chewings_truncated_end() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_chewings_truncated_end",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(
                k,
                "axis-c-encoding-zhuyin-parse_more_chewings-truncated-end",
            )
        },
        raw!(include!("fixtures/axis_c/b4e02d0e5ceef24d.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-parse_more_chewings-truncated-middle: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_chewings_truncated_middle() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_chewings_truncated_middle",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(
                k,
                "axis-c-encoding-zhuyin-parse_more_chewings-truncated-middle",
            )
        },
        raw!(include!("fixtures/axis_c/9052062091eb364b.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-parse_more_full_pinyins-fullwidth: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_full_pinyins_fullwidth() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_full_pinyins_fullwidth",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(
                k,
                "axis-c-encoding-zhuyin-parse_more_full_pinyins-fullwidth",
            )
        },
        raw!(include!("fixtures/axis_c/d8171b40697b7226.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-parse_more_full_pinyins-nfc: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_full_pinyins_nfc() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_full_pinyins_nfc",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-parse_more_full_pinyins-nfc"),
        raw!(include!("fixtures/axis_c/8bb17bccd6a764ce.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-parse_more_full_pinyins-surrogate-end: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_full_pinyins_surrogate_end() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_full_pinyins_surrogate_end",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(
                k,
                "axis-c-encoding-zhuyin-parse_more_full_pinyins-surrogate-end",
            )
        },
        raw!(include!("fixtures/axis_c/a0a82f7b9c12b4b4.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-parse_more_full_pinyins-surrogate-middle: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_full_pinyins_surrogate_middle() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_full_pinyins_surrogate_middle",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(
                k,
                "axis-c-encoding-zhuyin-parse_more_full_pinyins-surrogate-middle",
            )
        },
        raw!(include!("fixtures/axis_c/c7f6c4d3d2105c5a.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-parse_more_full_pinyins-tone: MATCH.
#[test]
fn axis_c_encoding_zhuyin_parse_more_full_pinyins_tone() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_parse_more_full_pinyins_tone",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-parse_more_full_pinyins-tone"),
        raw!(include!("fixtures/axis_c/46022603cac07f21.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-phrase_segment-empty: MATCH.
#[test]
fn axis_c_encoding_zhuyin_phrase_segment_empty() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_phrase_segment_empty",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-phrase_segment-empty"),
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

// axis-c-encoding-zhuyin-phrase_segment-ff-start: POLICY-ACCEPTED, oxpinyin's value (1297-segment-critical.md).
#[test]
fn axis_c_encoding_zhuyin_phrase_segment_ff_start() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_phrase_segment_ff_start",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-phrase_segment-ff-start"),
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

// axis-c-encoding-zhuyin-phrase_segment-hanzi: MATCH.
#[test]
fn axis_c_encoding_zhuyin_phrase_segment_hanzi() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_phrase_segment_hanzi",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-phrase_segment-hanzi"),
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

// axis-c-encoding-zhuyin-phrase_segment-nul-middle: MATCH.
#[test]
fn axis_c_encoding_zhuyin_phrase_segment_nul_middle() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_phrase_segment_nul_middle",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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

// axis-c-encoding-zhuyin-phrase_segment-pinyin: MATCH.
#[test]
fn axis_c_encoding_zhuyin_phrase_segment_pinyin() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_phrase_segment_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-phrase_segment-pinyin"),
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

// axis-c-encoding-zhuyin-phrase_segment-surrogate-start: MATCH.
#[test]
fn axis_c_encoding_zhuyin_phrase_segment_surrogate_start() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_phrase_segment_surrogate_start",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-encoding-zhuyin-phrase_segment-surrogate-start"),
        raw!(include!("fixtures/axis_c/868f2e103465ad96.rs")),
        None,
    );
}

// axis-c-encoding-zhuyin-phrase_segment-tone: MATCH.
#[test]
fn axis_c_encoding_zhuyin_phrase_segment_tone() {
    run_j(
        "axis_c::axis_c_encoding_zhuyin_phrase_segment_tone",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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

// axis-c-layout-zhuyin-0-00000000: the pin aborts in the scheme setter, before any input is read.
#[test]
fn axis_c_layout_zhuyin_0_00000000() {
    run_aborting(
        "axis_c::axis_c_layout_zhuyin_0_00000000",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "axis-c-layout-zhuyin-0-00000000");
        },
    );
}

// axis-c-layout-zhuyin-1-00000000: MATCH.
#[test]
fn axis_c_layout_zhuyin_1_00000000() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_1_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-zhuyin-1-00000060"),
        raw!(include!("fixtures/axis_c/0660c7fe0834efbe.rs")),
        None,
    );
}

// axis-c-layout-zhuyin-2-00000000: MATCH.
#[test]
fn axis_c_layout_zhuyin_2_00000000() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_2_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-zhuyin-2-00000000"),
        raw!(include!("fixtures/axis_c/29f0336ed7afc022.rs")),
        None,
    );
}

// axis-c-layout-zhuyin-2-e0000070: MATCH.
#[test]
fn axis_c_layout_zhuyin_2_e0000070() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_2_e0000070",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-zhuyin-2-e0000070"),
        raw!(include!("fixtures/axis_c/77cbdd7f221e05c3.rs")),
        None,
    );
}

// axis-c-layout-zhuyin-3-00000000: MATCH.
#[test]
fn axis_c_layout_zhuyin_3_00000000() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_3_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-zhuyin-3-00000000"),
        raw!(include!("fixtures/axis_c/eae575ae961c7062.rs")),
        None,
    );
}

// axis-c-layout-zhuyin-4-00000000: MATCH.
#[test]
fn axis_c_layout_zhuyin_4_00000000() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_4_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-zhuyin-4-00000000"),
        raw!(include!("fixtures/axis_c/6835f96efe11fc1a.rs")),
        None,
    );
}

// axis-c-layout-zhuyin-5-00000000: MATCH.
#[test]
fn axis_c_layout_zhuyin_5_00000000() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_5_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-zhuyin-5-00000000"),
        raw!(include!("fixtures/axis_c/c6b93997bc706f09.rs")),
        None,
    );
}

// axis-c-layout-zhuyin-6-00000000: MATCH.
#[test]
fn axis_c_layout_zhuyin_6_00000000() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_6_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-zhuyin-6-00000000"),
        raw!(include!("fixtures/axis_c/6a5374dae987e46d.rs")),
        None,
    );
}

// axis-c-layout-zhuyin-7-00000000: the pin aborts in the scheme setter, before any input is read.
#[test]
fn axis_c_layout_zhuyin_7_00000000() {
    run_aborting(
        "axis_c::axis_c_layout_zhuyin_7_00000000",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "axis-c-layout-zhuyin-7-00000000");
        },
    );
}

// axis-c-layout-zhuyin-8-00000000: MATCH.
#[test]
fn axis_c_layout_zhuyin_8_00000000() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_8_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-zhuyin-8-00000000"),
        raw!(include!("fixtures/axis_c/29f0336ed7afc022.rs")),
        None,
    );
}

// axis-c-layout-zhuyin-9-00000000: MATCH.
#[test]
fn axis_c_layout_zhuyin_9_00000000() {
    run_j(
        "axis_c::axis_c_layout_zhuyin_9_00000000",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "axis-c-layout-zhuyin-9-00000000"),
        raw!(include!("fixtures/axis_c/d0121f2416d9d845.rs")),
        None,
    );
}
