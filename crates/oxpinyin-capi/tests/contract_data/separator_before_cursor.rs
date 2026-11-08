//! Separators, the before-cursor guess and zero keys (#1175, #1616). Generated from the frozen pin observations.

use crate::bodies::separator_before_cursor as b;
use crate::support::{Expect, run_aborting, run_j};
// separator/pinyin/0/character/0: the pin's abort, reproduced.
#[test]
fn separator_pinyin_0_character_0() {
    run_aborting(
        "separator_before_cursor::separator_pinyin_0_character_0",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/pinyin/0/character/0");
        },
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
// separator/pinyin/3/left/4: the pin's abort, reproduced.
#[test]
fn separator_pinyin_3_left_4() {
    run_aborting(
        "separator_before_cursor::separator_pinyin_3_left_4",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "separator/pinyin/3/left/4");
        },
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
