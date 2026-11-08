//! Separators, the before-cursor guess and zero keys (#1175, #1616). Generated from the frozen pin observations.

use crate::bodies::separator_before_cursor as b;
use crate::support::{Expect, run_aborting, run_j};

// allocator-zero/zhuyin/xi1'/key/3: MATCH.
#[test]
fn allocator_zero_zhuyin_xi1_q_key_3() {
    run_j(
        "separator_before_cursor::allocator_zero_zhuyin_xi1_q_key_3",
        Expect::RESULT,
        |k| b::case(k, "allocator-zero/zhuyin/xi1'/key/3"),
        raw!(M(&[
            ("bytes", S("0000")),
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

// allocator-zero/zhuyin/xi1'an1/key/4: the pin's abort, reproduced.
#[test]
fn allocator_zero_zhuyin_xi1_q_an1_key_4() {
    run_aborting(
        "separator_before_cursor::allocator_zero_zhuyin_xi1_q_an1_key_4",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "allocator-zero/zhuyin/xi1'an1/key/4");
        },
    );
}

// allocator-zero/zhuyin/xi1'an1/render/3: MATCH.
#[test]
fn allocator_zero_zhuyin_xi1_q_an1_render_3() {
    run_j(
        "separator_before_cursor::allocator_zero_zhuyin_xi1_q_an1_render_3",
        Expect::RESULT,
        |k| b::case(k, "allocator-zero/zhuyin/xi1'an1/render/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/ae8353dc6f4aebb4.rs"
        )),
        None,
    );
}

// before-cursor/beijin/1: MATCH.
#[test]
fn before_cursor_beijin_1() {
    run_j(
        "separator_before_cursor::before_cursor_beijin_1",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/beijin/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/d2d4d9889fe8238d.rs"
        )),
        None,
    );
}

// before-cursor/beijingdaxue/0: MATCH.
#[test]
fn before_cursor_beijingdaxue_0() {
    run_j(
        "separator_before_cursor::before_cursor_beijingdaxue_0",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/beijingdaxue/0"),
        raw!(include!(
            "fixtures/separator_before_cursor/c92640b1bb2b4bd3.rs"
        )),
        None,
    );
}

// before-cursor/beijingdaxue/11: MATCH.
#[test]
fn before_cursor_beijingdaxue_11() {
    run_j(
        "separator_before_cursor::before_cursor_beijingdaxue_11",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/beijingdaxue/11"),
        raw!(include!(
            "fixtures/separator_before_cursor/c92640b1bb2b4bd3.rs"
        )),
        None,
    );
}

// before-cursor/beijingdaxue/12: MATCH.
#[test]
fn before_cursor_beijingdaxue_12() {
    run_j(
        "separator_before_cursor::before_cursor_beijingdaxue_12",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/beijingdaxue/12"),
        raw!(include!(
            "fixtures/separator_before_cursor/f48e8f19bb2e3459.rs"
        )),
        None,
    );
}

// before-cursor/guo/2: MATCH.
#[test]
fn before_cursor_guo_2() {
    run_j(
        "separator_before_cursor::before_cursor_guo_2",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/guo/2"),
        raw!(include!(
            "fixtures/separator_before_cursor/c4087ad33300b2af.rs"
        )),
        None,
    );
}

// before-cursor/ni3/0: MATCH.
#[test]
fn before_cursor_ni3_0() {
    run_j(
        "separator_before_cursor::before_cursor_ni3_0",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/ni3/0"),
        raw!(include!(
            "fixtures/separator_before_cursor/bff32c219b658c93.rs"
        )),
        None,
    );
}

// before-cursor/ni3/1: MATCH.
#[test]
fn before_cursor_ni3_1() {
    run_j(
        "separator_before_cursor::before_cursor_ni3_1",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/ni3/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/bff32c219b658c93.rs"
        )),
        None,
    );
}

// before-cursor/ni3/3: MATCH.
#[test]
fn before_cursor_ni3_3() {
    run_j(
        "separator_before_cursor::before_cursor_ni3_3",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/ni3/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/05d682487c90dcd8.rs"
        )),
        None,
    );
}

// before-cursor/ni3hao3/3: MATCH.
#[test]
fn before_cursor_ni3hao3_3() {
    run_j(
        "separator_before_cursor::before_cursor_ni3hao3_3",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/ni3hao3/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/6716e59f1ca7829f.rs"
        )),
        None,
    );
}

// before-cursor/ni3hao3/6: MATCH.
#[test]
fn before_cursor_ni3hao3_6() {
    run_j(
        "separator_before_cursor::before_cursor_ni3hao3_6",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/ni3hao3/6"),
        raw!(include!(
            "fixtures/separator_before_cursor/b75ad4c04215d4bb.rs"
        )),
        None,
    );
}

// before-cursor/nih/0: MATCH.
#[test]
fn before_cursor_nih_0() {
    run_j(
        "separator_before_cursor::before_cursor_nih_0",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/nih/0"),
        raw!(include!(
            "fixtures/separator_before_cursor/5093245a9ccb8f2b.rs"
        )),
        None,
    );
}

// before-cursor/nih/2: MATCH.
#[test]
fn before_cursor_nih_2() {
    run_j(
        "separator_before_cursor::before_cursor_nih_2",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/nih/2"),
        raw!(include!(
            "fixtures/separator_before_cursor/fd0f0bc51c2f47c6.rs"
        )),
        None,
    );
}

// before-cursor/nih/3: MATCH.
#[test]
fn before_cursor_nih_3() {
    run_j(
        "separator_before_cursor::before_cursor_nih_3",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/nih/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/1b335ea2ac88b19a.rs"
        )),
        None,
    );
}

// before-cursor/nihaoshij/1: MATCH.
#[test]
fn before_cursor_nihaoshij_1() {
    run_j(
        "separator_before_cursor::before_cursor_nihaoshij_1",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/nihaoshij/1"),
        raw!(include!(
            "fixtures/separator_before_cursor/cb52cb61bfaa0aa0.rs"
        )),
        None,
    );
}

// before-cursor/nihaoshij/8: MATCH.
#[test]
fn before_cursor_nihaoshij_8() {
    run_j(
        "separator_before_cursor::before_cursor_nihaoshij_8",
        Expect::RESULT,
        |k| b::case(k, "before-cursor/nihaoshij/8"),
        raw!(include!(
            "fixtures/separator_before_cursor/d67265c805499c65.rs"
        )),
        None,
    );
}

// separator/zhuyin/0/character/0: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_0_character_0() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_0_character_0",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/0/character/0");
        },
    );
}

// separator/zhuyin/0/character/1: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_0_character_1() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_0_character_1",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/0/character/1");
        },
    );
}

// separator/zhuyin/0/character/2: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_0_character_2() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_0_character_2",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/0/character/2");
        },
    );
}

// separator/zhuyin/0/character/3: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_0_character_3() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_0_character_3",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/0/character/3");
        },
    );
}

// separator/zhuyin/0/right/3: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_0_right_3() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_0_right_3",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/0/right/3");
        },
    );
}

// separator/zhuyin/1/before/2: MATCH.
#[test]
fn separator_zhuyin_1_before_2() {
    run_j(
        "separator_before_cursor::separator_zhuyin_1_before_2",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/1/before/2"),
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

// separator/zhuyin/1/before/4: MATCH.
#[test]
fn separator_zhuyin_1_before_4() {
    run_j(
        "separator_before_cursor::separator_zhuyin_1_before_4",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/1/before/4"),
        raw!(include!(
            "fixtures/separator_before_cursor/c3c642936f76917e.rs"
        )),
        None,
    );
}

// separator/zhuyin/1/parse/0: MATCH.
#[test]
fn separator_zhuyin_1_parse_0() {
    run_j(
        "separator_before_cursor::separator_zhuyin_1_parse_0",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/1/parse/0"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("logs", L(&[])),
            ("parse_return_consumed", I(4))
        ])),
        None,
    );
}

// separator/zhuyin/1/sentence/0: MATCH.
#[test]
fn separator_zhuyin_1_sentence_0() {
    run_j(
        "separator_before_cursor::separator_zhuyin_1_sentence_0",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/1/sentence/0"),
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

// separator/zhuyin/3/before/1: MATCH.
#[test]
fn separator_zhuyin_3_before_1() {
    run_j(
        "separator_before_cursor::separator_zhuyin_3_before_1",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/3/before/1"),
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

// separator/zhuyin/3/before/4: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_3_before_4() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_3_before_4",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/3/before/4");
        },
    );
}

// separator/zhuyin/3/before/7: MATCH.
#[test]
fn separator_zhuyin_3_before_7() {
    run_j(
        "separator_before_cursor::separator_zhuyin_3_before_7",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/3/before/7"),
        raw!(include!(
            "fixtures/separator_before_cursor/409ef4894c5291f9.rs"
        )),
        None,
    );
}

// separator/zhuyin/3/candidates/0: MATCH.
#[test]
fn separator_zhuyin_3_candidates_0() {
    run_j(
        "separator_before_cursor::separator_zhuyin_3_candidates_0",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/3/candidates/0"),
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

// separator/zhuyin/3/candidates/4: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_3_candidates_4() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_3_candidates_4",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/3/candidates/4");
        },
    );
}

// separator/zhuyin/3/key/3: MATCH.
#[test]
fn separator_zhuyin_3_key_3() {
    run_j(
        "separator_before_cursor::separator_zhuyin_3_key_3",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/3/key/3"),
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

// separator/zhuyin/3/left/4: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_3_left_4() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_3_left_4",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/3/left/4");
        },
    );
}

// separator/zhuyin/3/left/6: MATCH.
#[test]
fn separator_zhuyin_3_left_6() {
    run_j(
        "separator_before_cursor::separator_zhuyin_3_left_6",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/3/left/6"),
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

// separator/zhuyin/3/left/7: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_3_left_7() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_3_left_7",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/3/left/7");
        },
    );
}

// separator/zhuyin/3/rest/3: MATCH.
#[test]
fn separator_zhuyin_3_rest_3() {
    run_j(
        "separator_before_cursor::separator_zhuyin_3_rest_3",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/3/rest/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/c4824e366c279ea3.rs"
        )),
        None,
    );
}

// separator/zhuyin/3/rest/4: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_3_rest_4() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_3_rest_4",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/3/rest/4");
        },
    );
}

// separator/zhuyin/3/right/4: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_3_right_4() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_3_right_4",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/3/right/4");
        },
    );
}

// separator/zhuyin/3/right/6: MATCH.
#[test]
fn separator_zhuyin_3_right_6() {
    run_j(
        "separator_before_cursor::separator_zhuyin_3_right_6",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/3/right/6"),
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

// separator/zhuyin/4/before/1: MATCH.
#[test]
fn separator_zhuyin_4_before_1() {
    run_j(
        "separator_before_cursor::separator_zhuyin_4_before_1",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/4/before/1"),
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

// separator/zhuyin/4/before/3: MATCH.
#[test]
fn separator_zhuyin_4_before_3() {
    run_j(
        "separator_before_cursor::separator_zhuyin_4_before_3",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/4/before/3"),
        raw!(include!(
            "fixtures/separator_before_cursor/c3c642936f76917e.rs"
        )),
        None,
    );
}

// separator/zhuyin/4/offset/4: MATCH.
#[test]
fn separator_zhuyin_4_offset_4() {
    run_j(
        "separator_before_cursor::separator_zhuyin_4_offset_4",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/4/offset/4"),
        raw!(M(&[
            ("get_parsed_input_length", I(4)),
            ("guess_sentence", B(true)),
            ("logs", L(&[])),
            ("out", I(3)),
            ("parse_return_consumed", I(4)),
            ("ret", B(true)),
            ("untouched_output", B(true))
        ])),
        None,
    );
}

// separator/zhuyin/4/rest/4: MATCH.
#[test]
fn separator_zhuyin_4_rest_4() {
    run_j(
        "separator_before_cursor::separator_zhuyin_4_rest_4",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/4/rest/4"),
        raw!(include!(
            "fixtures/separator_before_cursor/e6dd594be807928f.rs"
        )),
        None,
    );
}

// separator/zhuyin/4/right/3: the pin's abort, reproduced.
#[test]
fn separator_zhuyin_4_right_3() {
    run_aborting(
        "separator_before_cursor::separator_zhuyin_4_right_3",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/zhuyin/4/right/3");
        },
    );
}

// separator/zhuyin/5/before/1: MATCH.
#[test]
fn separator_zhuyin_5_before_1() {
    run_j(
        "separator_before_cursor::separator_zhuyin_5_before_1",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/5/before/1"),
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

// separator/zhuyin/5/before/2: MATCH.
#[test]
fn separator_zhuyin_5_before_2() {
    run_j(
        "separator_before_cursor::separator_zhuyin_5_before_2",
        Expect::RESULT,
        |k| b::case(k, "separator/zhuyin/5/before/2"),
        raw!(include!(
            "fixtures/separator_before_cursor/6e27500a125189b4.rs"
        )),
        None,
    );
}

// zero-key/zhuyin/ni''hao/key/2: MATCH.
#[test]
fn zero_key_zhuyin_ni_q_q_hao_key_2() {
    run_j(
        "separator_before_cursor::zero_key_zhuyin_ni_q_q_hao_key_2",
        Expect::RESULT,
        |k| b::case(k, "zero-key/zhuyin/ni''hao/key/2"),
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

// zero-key/zhuyin/ni''hao/rest/2: MATCH.
#[test]
fn zero_key_zhuyin_ni_q_q_hao_rest_2() {
    run_j(
        "separator_before_cursor::zero_key_zhuyin_ni_q_q_hao_rest_2",
        Expect::RESULT,
        |k| b::case(k, "zero-key/zhuyin/ni''hao/rest/2"),
        raw!(include!(
            "fixtures/separator_before_cursor/463c2922aeaee769.rs"
        )),
        None,
    );
}
