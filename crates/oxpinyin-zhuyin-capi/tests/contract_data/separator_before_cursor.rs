//! Separators, the before-cursor guess and zero keys (#1175, #1616). Generated from the frozen pin observations.

use crate::bodies::separator_before_cursor as b;
use crate::support::{Expect, run_aborting, run_j};
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
