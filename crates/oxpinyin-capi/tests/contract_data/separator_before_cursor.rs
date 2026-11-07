//! Separators, the before-cursor guess and zero keys (#1175, #1616). Generated from the frozen pin observations.

use crate::bodies::separator_before_cursor as b;
use crate::support::{Expect, run_aborting, run_j};

// allocator-zero/pinyin/xi''/render/3: MATCH.
#[test]
fn allocator_zero_pinyin_xi_q_q_render_3() {
    run_j(
        "separator_before_cursor::allocator_zero_pinyin_xi_q_q_render_3",
        Expect::RESULT,
        |k| b::case(k, "allocator-zero/pinyin/xi''/render/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/cae84c21ba45ceb9.rs"
        )),
        None,
    );
}

// separator-free-control/beijin/key/5: MATCH.
#[test]
fn separator_free_control_beijin_key_5() {
    run_j(
        "separator_before_cursor::separator_free_control_beijin_key_5",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/beijin/key/5"),
        raw!(include!(
            "fixtures/separator_before_cursor/480a061cd56c3368.rs"
        )),
        None,
    );
}

// separator-free-control/beijingdaxue/candidates/0: MATCH.
#[test]
fn separator_free_control_beijingdaxue_candidates_0() {
    run_j(
        "separator_before_cursor::separator_free_control_beijingdaxue_candidates_0",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/beijingdaxue/candidates/0"),
        raw!(include!(
            "fixtures/separator_before_cursor/96a170e42fe4f31e.rs"
        )),
        None,
    );
}

// separator-free-control/beijingdaxue/candidates/12: MATCH.
#[test]
fn separator_free_control_beijingdaxue_candidates_12() {
    run_j(
        "separator_before_cursor::separator_free_control_beijingdaxue_candidates_12",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/beijingdaxue/candidates/12"),
        raw!(include!(
            "fixtures/separator_before_cursor/292decd111f3eaae.rs"
        )),
        None,
    );
}

// separator-free-control/beijingdaxue/key/0: MATCH.
#[test]
fn separator_free_control_beijingdaxue_key_0() {
    run_j(
        "separator_before_cursor::separator_free_control_beijingdaxue_key_0",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/beijingdaxue/key/0"),
        raw!(M(&[
            ("bytes", S("0104")),
            ("get_parsed_input_length", I(12)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(12)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator-free-control/beijingdaxue/key/1: MATCH.
#[test]
fn separator_free_control_beijingdaxue_key_1() {
    run_j(
        "separator_before_cursor::separator_free_control_beijingdaxue_key_1",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/beijingdaxue/key/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/7db754513df5db7a.rs"
        )),
        None,
    );
}

// separator-free-control/beijingdaxue/key/11: MATCH.
#[test]
fn separator_free_control_beijingdaxue_key_11() {
    run_j(
        "separator_before_cursor::separator_free_control_beijingdaxue_key_11",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/beijingdaxue/key/11"),
        raw!(include!(
            "fixtures/separator_before_cursor/7db754513df5db7a.rs"
        )),
        None,
    );
}

// separator-free-control/beijingdaxue/key/12: MATCH.
#[test]
fn separator_free_control_beijingdaxue_key_12() {
    run_j(
        "separator_before_cursor::separator_free_control_beijingdaxue_key_12",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/beijingdaxue/key/12"),
        raw!(include!(
            "fixtures/separator_before_cursor/7db754513df5db7a.rs"
        )),
        None,
    );
}

// separator-free-control/beijingdaxue/sentence: MATCH.
#[test]
fn separator_free_control_beijingdaxue_sentence() {
    run_j(
        "separator_before_cursor::separator_free_control_beijingdaxue_sentence",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/beijingdaxue/sentence"),
        raw!(M(&[
            ("get_parsed_input_length", I(12)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(12)),
            ("ret", B(true)),
            ("sentence", S("北京大学"))
        ])),
        None,
    );
}

// separator-free-control/ni3/candidates/0: MATCH.
#[test]
fn separator_free_control_ni3_candidates_0() {
    run_j(
        "separator_before_cursor::separator_free_control_ni3_candidates_0",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/ni3/candidates/0"),
        raw!(include!(
            "fixtures/separator_before_cursor/27883741f03bf5e2.rs"
        )),
        None,
    );
}

// separator-free-control/ni3/candidates/1: MATCH.
#[test]
fn separator_free_control_ni3_candidates_1() {
    run_j(
        "separator_before_cursor::separator_free_control_ni3_candidates_1",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/ni3/candidates/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/d139cede3ca01a6b.rs"
        )),
        None,
    );
}

// separator-free-control/ni3/candidates/2: MATCH.
#[test]
fn separator_free_control_ni3_candidates_2() {
    run_j(
        "separator_before_cursor::separator_free_control_ni3_candidates_2",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/ni3/candidates/2"),
        raw!(include!(
            "fixtures/separator_before_cursor/d139cede3ca01a6b.rs"
        )),
        None,
    );
}

// separator-free-control/ni3/key/0: MATCH.
#[test]
fn separator_free_control_ni3_key_0() {
    run_j(
        "separator_before_cursor::separator_free_control_ni3_key_0",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/ni3/key/0"),
        raw!(M(&[
            ("bytes", S("2b30")),
            ("get_parsed_input_length", I(3)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(3)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator-free-control/ni3/key/1: MATCH.
#[test]
fn separator_free_control_ni3_key_1() {
    run_j(
        "separator_before_cursor::separator_free_control_ni3_key_1",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/ni3/key/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/c5d29ea3c1f34a77.rs"
        )),
        None,
    );
}

// separator-free-control/ni3/key/2: MATCH.
#[test]
fn separator_free_control_ni3_key_2() {
    run_j(
        "separator_before_cursor::separator_free_control_ni3_key_2",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/ni3/key/2"),
        raw!(include!(
            "fixtures/separator_before_cursor/c5d29ea3c1f34a77.rs"
        )),
        None,
    );
}

// separator-free-control/ni3/key/3: MATCH.
#[test]
fn separator_free_control_ni3_key_3() {
    run_j(
        "separator_before_cursor::separator_free_control_ni3_key_3",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/ni3/key/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/c5d29ea3c1f34a77.rs"
        )),
        None,
    );
}

// separator-free-control/ni3/sentence: MATCH.
#[test]
fn separator_free_control_ni3_sentence() {
    run_j(
        "separator_before_cursor::separator_free_control_ni3_sentence",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/ni3/sentence"),
        raw!(M(&[
            ("get_parsed_input_length", I(3)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(3)),
            ("ret", B(true)),
            ("sentence", S("你"))
        ])),
        None,
    );
}

// separator-free-control/ni3hao3/candidates/7: MATCH.
#[test]
fn separator_free_control_ni3hao3_candidates_7() {
    run_j(
        "separator_before_cursor::separator_free_control_ni3hao3_candidates_7",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/ni3hao3/candidates/7"),
        raw!(include!(
            "fixtures/separator_before_cursor/0ffea0e7adfa8705.rs"
        )),
        None,
    );
}

// separator-free-control/ni3hao3/key/3: MATCH.
#[test]
fn separator_free_control_ni3hao3_key_3() {
    run_j(
        "separator_before_cursor::separator_free_control_ni3hao3_key_3",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/ni3hao3/key/3"),
        raw!(M(&[
            ("bytes", S("8632")),
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(7)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator-free-control/nih/candidates/0: MATCH.
#[test]
fn separator_free_control_nih_candidates_0() {
    run_j(
        "separator_before_cursor::separator_free_control_nih_candidates_0",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/nih/candidates/0"),
        raw!(include!(
            "fixtures/separator_before_cursor/e2db748e68ed5528.rs"
        )),
        None,
    );
}

// separator-free-control/nih/candidates/1: MATCH.
#[test]
fn separator_free_control_nih_candidates_1() {
    run_j(
        "separator_before_cursor::separator_free_control_nih_candidates_1",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/nih/candidates/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/09c111ea4a34653b.rs"
        )),
        None,
    );
}

// separator-free-control/nih/key/0: MATCH.
#[test]
fn separator_free_control_nih_key_0() {
    run_j(
        "separator_before_cursor::separator_free_control_nih_key_0",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/nih/key/0"),
        raw!(M(&[
            ("bytes", S("2b00")),
            ("get_parsed_input_length", I(3)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(3)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator-free-control/nih/key/1: MATCH.
#[test]
fn separator_free_control_nih_key_1() {
    run_j(
        "separator_before_cursor::separator_free_control_nih_key_1",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/nih/key/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/c5d29ea3c1f34a77.rs"
        )),
        None,
    );
}

// separator-free-control/nih/key/2: MATCH.
#[test]
fn separator_free_control_nih_key_2() {
    run_j(
        "separator_before_cursor::separator_free_control_nih_key_2",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/nih/key/2"),
        raw!(M(&[
            ("bytes", S("0600")),
            ("get_parsed_input_length", I(3)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(3)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator-free-control/nih/key/3: MATCH.
#[test]
fn separator_free_control_nih_key_3() {
    run_j(
        "separator_before_cursor::separator_free_control_nih_key_3",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/nih/key/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/c5d29ea3c1f34a77.rs"
        )),
        None,
    );
}

// separator-free-control/nih/sentence: MATCH.
#[test]
fn separator_free_control_nih_sentence() {
    run_j(
        "separator_before_cursor::separator_free_control_nih_sentence",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/nih/sentence"),
        raw!(M(&[
            ("get_parsed_input_length", I(3)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(3)),
            ("ret", B(true)),
            ("sentence", S("你和"))
        ])),
        None,
    );
}

// separator-free-control/nihaoshij/candidates/1: MATCH.
#[test]
fn separator_free_control_nihaoshij_candidates_1() {
    run_j(
        "separator_before_cursor::separator_free_control_nihaoshij_candidates_1",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/nihaoshij/candidates/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/ba0d2e7cc82d7378.rs"
        )),
        None,
    );
}

// separator-free-control/nihaoshij/key/8: MATCH.
#[test]
fn separator_free_control_nihaoshij_key_8() {
    run_j(
        "separator_before_cursor::separator_free_control_nihaoshij_key_8",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/nihaoshij/key/8"),
        raw!(M(&[
            ("bytes", S("0900")),
            ("get_parsed_input_length", I(9)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(9)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator-free-control/nihaoshijie/key/10: MATCH.
#[test]
fn separator_free_control_nihaoshijie_key_10() {
    run_j(
        "separator_before_cursor::separator_free_control_nihaoshijie_key_10",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/nihaoshijie/key/10"),
        raw!(M(&[
            ("bytes", S("0003")),
            ("get_parsed_input_length", I(11)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(11)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator-free-control/tian/key/2: MATCH.
#[test]
fn separator_free_control_tian_key_2() {
    run_j(
        "separator_before_cursor::separator_free_control_tian_key_2",
        Expect::RESULT,
        |k| b::case(k, "separator-free-control/tian/key/2"),
        raw!(M(&[
            ("bytes", S("8001")),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(4)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator/pinyin/0/character/0: the pin's abort, reproduced.
#[test]
fn separator_pinyin_0_character_0() {
    run_aborting(
        "separator_before_cursor::separator_pinyin_0_character_0",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "separator/pinyin/0/character/0");
        },
    );
}

// separator/pinyin/0/character/1: the pin's abort, reproduced.
#[test]
fn separator_pinyin_0_character_1() {
    run_aborting(
        "separator_before_cursor::separator_pinyin_0_character_1",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "separator/pinyin/0/character/1");
        },
    );
}

// separator/pinyin/0/character/2: the pin's abort, reproduced.
#[test]
fn separator_pinyin_0_character_2() {
    run_aborting(
        "separator_before_cursor::separator_pinyin_0_character_2",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "separator/pinyin/0/character/2");
        },
    );
}

// separator/pinyin/0/character/3: the pin's abort, reproduced.
#[test]
fn separator_pinyin_0_character_3() {
    run_aborting(
        "separator_before_cursor::separator_pinyin_0_character_3",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "separator/pinyin/0/character/3");
        },
    );
}

// separator/pinyin/0/right/3: the pin's abort, reproduced.
#[test]
fn separator_pinyin_0_right_3() {
    run_aborting(
        "separator_before_cursor::separator_pinyin_0_right_3",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "separator/pinyin/0/right/3");
        },
    );
}

// separator/pinyin/1/candidates/0: MATCH.
#[test]
fn separator_pinyin_1_candidates_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_candidates_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/candidates/0"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/1/candidates/1: MATCH.
#[test]
fn separator_pinyin_1_candidates_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_candidates_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/candidates/1"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/1/candidates/2: MATCH.
#[test]
fn separator_pinyin_1_candidates_2() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_candidates_2",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/candidates/2"),
        raw!(include!(
            "fixtures/separator_before_cursor/63204e6d9a5fcb52.rs"
        )),
        None,
    );
}

// separator/pinyin/1/candidates/3: MATCH.
#[test]
fn separator_pinyin_1_candidates_3() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_candidates_3",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/candidates/3"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/1/candidates/4: MATCH.
#[test]
fn separator_pinyin_1_candidates_4() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_candidates_4",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/candidates/4"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/1/key/0: MATCH.
#[test]
fn separator_pinyin_1_key_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_key_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/key/0"),
        raw!(include!(
            "fixtures/separator_before_cursor/e6dd594be807928f.rs"
        )),
        None,
    );
}

// separator/pinyin/1/key/1: MATCH.
#[test]
fn separator_pinyin_1_key_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_key_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/key/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/e6dd594be807928f.rs"
        )),
        None,
    );
}

// separator/pinyin/1/key/2: MATCH.
#[test]
fn separator_pinyin_1_key_2() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_key_2",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/key/2"),
        raw!(M(&[
            ("bytes", S("2b00")),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(4)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator/pinyin/1/key/3: MATCH.
#[test]
fn separator_pinyin_1_key_3() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_key_3",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/key/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/e6dd594be807928f.rs"
        )),
        None,
    );
}

// separator/pinyin/1/key/4: MATCH.
#[test]
fn separator_pinyin_1_key_4() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_key_4",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/key/4"),
        raw!(include!(
            "fixtures/separator_before_cursor/e6dd594be807928f.rs"
        )),
        None,
    );
}

// separator/pinyin/1/left/0: MATCH.
#[test]
fn separator_pinyin_1_left_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_left_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/left/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/1/left/1: MATCH.
#[test]
fn separator_pinyin_1_left_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_left_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/left/1"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/1/left/2: MATCH.
#[test]
fn separator_pinyin_1_left_2() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_left_2",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/left/2"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/1/left/3: MATCH.
#[test]
fn separator_pinyin_1_left_3() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_left_3",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/left/3"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/1/left/4: MATCH.
#[test]
fn separator_pinyin_1_left_4() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_left_4",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/left/4"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(2)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/1/right/0: MATCH.
#[test]
fn separator_pinyin_1_right_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_right_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/right/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(11259375)),
            ("parse_return_consumed", I(4)),
            ("ret", B(false)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/1/right/1: MATCH.
#[test]
fn separator_pinyin_1_right_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_right_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/right/1"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(11259375)),
            ("parse_return_consumed", I(4)),
            ("ret", B(false)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/1/right/3: MATCH.
#[test]
fn separator_pinyin_1_right_3() {
    run_j(
        "separator_before_cursor::separator_pinyin_1_right_3",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/1/right/3"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(11259375)),
            ("parse_return_consumed", I(4)),
            ("ret", B(false)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/3/candidates/1: MATCH.
#[test]
fn separator_pinyin_3_candidates_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_candidates_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/candidates/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/51347833bc75e720.rs"
        )),
        None,
    );
}

// separator/pinyin/3/candidates/3: MATCH.
#[test]
fn separator_pinyin_3_candidates_3() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_candidates_3",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/candidates/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/4f688fd6e0189b7a.rs"
        )),
        None,
    );
}

// separator/pinyin/3/candidates/6: MATCH.
#[test]
fn separator_pinyin_3_candidates_6() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_candidates_6",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/candidates/6"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(7)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/3/candidates/7: MATCH.
#[test]
fn separator_pinyin_3_candidates_7() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_candidates_7",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/candidates/7"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(7)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/3/key/0: MATCH.
#[test]
fn separator_pinyin_3_key_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_key_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/key/0"),
        raw!(include!(
            "fixtures/separator_before_cursor/7eed5ada9ae82976.rs"
        )),
        None,
    );
}

// separator/pinyin/3/key/3: MATCH.
#[test]
fn separator_pinyin_3_key_3() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_key_3",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/key/3"),
        raw!(M(&[
            ("bytes", S("8602")),
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(7)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator/pinyin/3/key/6: MATCH.
#[test]
fn separator_pinyin_3_key_6() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_key_6",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/key/6"),
        raw!(include!(
            "fixtures/separator_before_cursor/7eed5ada9ae82976.rs"
        )),
        None,
    );
}

// separator/pinyin/3/left/1: MATCH.
#[test]
fn separator_pinyin_3_left_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_left_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/left/1"),
        raw!(M(&[
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(7)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/3/left/4: the pin's abort, reproduced.
#[test]
fn separator_pinyin_3_left_4() {
    run_aborting(
        "separator_before_cursor::separator_pinyin_3_left_4",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "separator/pinyin/3/left/4");
        },
    );
}

// separator/pinyin/3/left/7: MATCH.
#[test]
fn separator_pinyin_3_left_7() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_left_7",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/left/7"),
        raw!(M(&[
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(3)),
            ("parse_return_consumed", I(7)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/3/offset/0: MATCH.
#[test]
fn separator_pinyin_3_offset_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_offset_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/offset/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(7)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/3/parse/0: MATCH.
#[test]
fn separator_pinyin_3_parse_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_parse_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/parse/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(7)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(7))
        ])),
        None,
    );
}

// separator/pinyin/3/rest/1: MATCH.
#[test]
fn separator_pinyin_3_rest_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_rest_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/rest/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/6638a64f7410ba7f.rs"
        )),
        None,
    );
}

// separator/pinyin/3/right/0: MATCH.
#[test]
fn separator_pinyin_3_right_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_right_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/right/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(11259375)),
            ("parse_return_consumed", I(7)),
            ("ret", B(false)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/3/right/4: the pin's abort, reproduced.
#[test]
fn separator_pinyin_3_right_4() {
    run_aborting(
        "separator_before_cursor::separator_pinyin_3_right_4",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "separator/pinyin/3/right/4");
        },
    );
}

// separator/pinyin/3/sentence/0: MATCH.
#[test]
fn separator_pinyin_3_sentence_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_3_sentence_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/3/sentence/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(7)),
            ("ret", B(false)),
            ("sentence", S("untouched"))
        ])),
        None,
    );
}

// separator/pinyin/4/candidates/0: MATCH.
#[test]
fn separator_pinyin_4_candidates_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_candidates_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/candidates/0"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/4/candidates/1: MATCH.
#[test]
fn separator_pinyin_4_candidates_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_candidates_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/candidates/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/63204e6d9a5fcb52.rs"
        )),
        None,
    );
}

// separator/pinyin/4/candidates/2: MATCH.
#[test]
fn separator_pinyin_4_candidates_2() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_candidates_2",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/candidates/2"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/4/candidates/3: MATCH.
#[test]
fn separator_pinyin_4_candidates_3() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_candidates_3",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/candidates/3"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/4/candidates/4: MATCH.
#[test]
fn separator_pinyin_4_candidates_4() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_candidates_4",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/candidates/4"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/4/key/0: MATCH.
#[test]
fn separator_pinyin_4_key_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_key_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/key/0"),
        raw!(include!(
            "fixtures/separator_before_cursor/e6dd594be807928f.rs"
        )),
        None,
    );
}

// separator/pinyin/4/key/1: MATCH.
#[test]
fn separator_pinyin_4_key_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_key_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/key/1"),
        raw!(M(&[
            ("bytes", S("2b00")),
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(4)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator/pinyin/4/left/0: MATCH.
#[test]
fn separator_pinyin_4_left_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_left_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/left/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/4/left/1: MATCH.
#[test]
fn separator_pinyin_4_left_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_left_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/left/1"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/4/left/2: MATCH.
#[test]
fn separator_pinyin_4_left_2() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_left_2",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/left/2"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/4/left/3: MATCH.
#[test]
fn separator_pinyin_4_left_3() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_left_3",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/left/3"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(1)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/4/parse/0: MATCH.
#[test]
fn separator_pinyin_4_parse_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_parse_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/parse/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(4))
        ])),
        None,
    );
}

// separator/pinyin/4/right/0: MATCH.
#[test]
fn separator_pinyin_4_right_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_right_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/right/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(11259375)),
            ("parse_return_consumed", I(4)),
            ("ret", B(false)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/4/right/2: MATCH.
#[test]
fn separator_pinyin_4_right_2() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_right_2",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/right/2"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(11259375)),
            ("parse_return_consumed", I(4)),
            ("ret", B(false)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/4/right/3: the pin's abort, reproduced.
#[test]
fn separator_pinyin_4_right_3() {
    run_aborting(
        "separator_before_cursor::separator_pinyin_4_right_3",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed."),
        |k| {
            b::case(k, "separator/pinyin/4/right/3");
        },
    );
}

// separator/pinyin/4/sentence/0: MATCH.
#[test]
fn separator_pinyin_4_sentence_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_4_sentence_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/4/sentence/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(4)),
            ("ret", B(false)),
            ("sentence", S("untouched"))
        ])),
        None,
    );
}

// separator/pinyin/5/candidates/0: MATCH.
#[test]
fn separator_pinyin_5_candidates_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_candidates_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/candidates/0"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(2)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(2)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/5/candidates/1: MATCH.
#[test]
fn separator_pinyin_5_candidates_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_candidates_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/candidates/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/cd41b868c73572e6.rs"
        )),
        None,
    );
}

// separator/pinyin/5/candidates/2: MATCH.
#[test]
fn separator_pinyin_5_candidates_2() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_candidates_2",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/candidates/2"),
        raw!(M(&[
            ("get_n", B(true)),
            ("get_parsed_input_length", I(2)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("n", I(0)),
            ("parse_return_consumed", I(2)),
            ("ret", B(true)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// separator/pinyin/5/key/0: MATCH.
#[test]
fn separator_pinyin_5_key_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_key_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/key/0"),
        raw!(include!(
            "fixtures/separator_before_cursor/9be472cce51ab54f.rs"
        )),
        None,
    );
}

// separator/pinyin/5/key/1: MATCH.
#[test]
fn separator_pinyin_5_key_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_key_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/key/1"),
        raw!(M(&[
            ("bytes", S("8000")),
            ("get_parsed_input_length", I(2)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(2)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// separator/pinyin/5/key/2: MATCH.
#[test]
fn separator_pinyin_5_key_2() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_key_2",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/key/2"),
        raw!(include!(
            "fixtures/separator_before_cursor/9be472cce51ab54f.rs"
        )),
        None,
    );
}

// separator/pinyin/5/left/0: MATCH.
#[test]
fn separator_pinyin_5_left_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_left_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/left/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(2)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(2)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/5/left/1: MATCH.
#[test]
fn separator_pinyin_5_left_1() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_left_1",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/left/1"),
        raw!(M(&[
            ("get_parsed_input_length", I(2)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(0)),
            ("parse_return_consumed", I(2)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/5/left/2: MATCH.
#[test]
fn separator_pinyin_5_left_2() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_left_2",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/left/2"),
        raw!(M(&[
            ("get_parsed_input_length", I(2)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(1)),
            ("parse_return_consumed", I(2)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/5/parse/0: MATCH.
#[test]
fn separator_pinyin_5_parse_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_parse_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/parse/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(2)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(2))
        ])),
        None,
    );
}

// separator/pinyin/5/right/0: MATCH.
#[test]
fn separator_pinyin_5_right_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_right_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/right/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(2)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(11259375)),
            ("parse_return_consumed", I(2)),
            ("ret", B(false)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/pinyin/5/sentence/0: MATCH.
#[test]
fn separator_pinyin_5_sentence_0() {
    run_j(
        "separator_before_cursor::separator_pinyin_5_sentence_0",
        Expect::RESULT,
        |k| b::case(k, "separator/pinyin/5/sentence/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(2)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(2)),
            ("ret", B(false)),
            ("sentence", S("untouched"))
        ])),
        None,
    );
}

// zero-key/pinyin/ni''hao/key/0: MATCH.
#[test]
fn zero_key_pinyin_ni_q_q_hao_key_0() {
    run_j(
        "separator_before_cursor::zero_key_pinyin_ni_q_q_hao_key_0",
        Expect::RESULT,
        |k| b::case(k, "zero-key/pinyin/ni''hao/key/0"),
        raw!(M(&[
            ("bytes", S("2b00")),
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(7)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// zero-key/pinyin/ni''hao/key/1: MATCH.
#[test]
fn zero_key_pinyin_ni_q_q_hao_key_1() {
    run_j(
        "separator_before_cursor::zero_key_pinyin_ni_q_q_hao_key_1",
        Expect::RESULT,
        |k| b::case(k, "zero-key/pinyin/ni''hao/key/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/7eed5ada9ae82976.rs"
        )),
        None,
    );
}

// zero-key/pinyin/ni''hao/key/3: MATCH.
#[test]
fn zero_key_pinyin_ni_q_q_hao_key_3() {
    run_j(
        "separator_before_cursor::zero_key_pinyin_ni_q_q_hao_key_3",
        Expect::RESULT,
        |k| b::case(k, "zero-key/pinyin/ni''hao/key/3"),
        raw!(M(&[
            ("bytes", S("8602")),
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(7)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// zero-key/pinyin/ni''hao/key/4: MATCH.
#[test]
fn zero_key_pinyin_ni_q_q_hao_key_4() {
    run_j(
        "separator_before_cursor::zero_key_pinyin_ni_q_q_hao_key_4",
        Expect::RESULT,
        |k| b::case(k, "zero-key/pinyin/ni''hao/key/4"),
        raw!(M(&[
            ("bytes", S("8602")),
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(7)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}

// zero-key/pinyin/ni''hao/key/6: MATCH.
#[test]
fn zero_key_pinyin_ni_q_q_hao_key_6() {
    run_j(
        "separator_before_cursor::zero_key_pinyin_ni_q_q_hao_key_6",
        Expect::RESULT,
        |k| b::case(k, "zero-key/pinyin/ni''hao/key/6"),
        raw!(include!(
            "fixtures/separator_before_cursor/7eed5ada9ae82976.rs"
        )),
        None,
    );
}

// zero-key/pinyin/ni''hao/key/7: MATCH.
#[test]
fn zero_key_pinyin_ni_q_q_hao_key_7() {
    run_j(
        "separator_before_cursor::zero_key_pinyin_ni_q_q_hao_key_7",
        Expect::RESULT,
        |k| b::case(k, "zero-key/pinyin/ni''hao/key/7"),
        raw!(include!(
            "fixtures/separator_before_cursor/7eed5ada9ae82976.rs"
        )),
        None,
    );
}

// zero-key/pinyin/ni'hao'/key/6: MATCH.
#[test]
fn zero_key_pinyin_ni_q_hao_q_key_6() {
    run_j(
        "separator_before_cursor::zero_key_pinyin_ni_q_hao_q_key_6",
        Expect::RESULT,
        |k| b::case(k, "zero-key/pinyin/ni'hao'/key/6"),
        raw!(M(&[
            ("bytes", S("0000")),
            ("get_parsed_input_length", I(7)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(7)),
            ("pointer", S("key")),
            ("ret", B(true))
        ])),
        None,
    );
}
