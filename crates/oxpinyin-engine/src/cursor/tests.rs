//! Pin-table tests for the cursor laws.
//!
//! Every table below was measured FIRST-HAND on the rebuilt pin
//! (`tools/oracle/build-oracle.sh` prefix; libpinyin 2.11.91 @ `0c5e80e`,
//! parity word `0x18a`) with a fork-per-probe driver: each offset ran in
//! its own child process so the pin's `assert`-abort is a datum, not a
//! dead driver. Legend: values are the pin's answers; `ABORT` is the
//! pin's `_check_offset` SIGABRT (the engine answers
//! [`EngineError::ZeroKeyOffsetCheck`]); `false` is the pin's one graceful
//! false (`pinyin.cpp:3085-3086`), encoded `Ok(None)` on the right move.

use oxpinyin_core::OptionBits;

use super::*;
use crate::error::EngineError;

/// The parity word: `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
/// USE_RESPLIT_TABLE` plus the harness's `0x2` bit.
const PARITY: OptionBits = OptionBits::from_bits(0x18a);

/// Expected pin answer for one probe: non-negative = value, -1 = the
/// `_check_offset` abort shape, -2 = the graceful false (right move only).
fn off(input: &[u8], probes: &[(usize, i64)]) {
    for &(cursor, want) in probes {
        let got = lookup_offset_for_cursor(input, PARITY, cursor);
        assert_eq!(
            got,
            Ok(usize::try_from(want).expect("non-negative probe")),
            "{:?} OFF c={cursor}: pin {want}",
            std::str::from_utf8(input)
        );
    }
}

fn left(input: &[u8], probes: &[(usize, i64)]) {
    for &(offset, want) in probes {
        let got = left_word_offset(input, PARITY, offset);
        match want {
            v if v >= 0 => assert_eq!(
                got,
                Ok(usize::try_from(v).expect("non-negative probe")),
                "{:?} LEFT off={offset}: pin {v}",
                std::str::from_utf8(input)
            ),
            -1 => assert!(
                matches!(got, Err(EngineError::ZeroKeyOffsetCheck { .. })),
                "{:?} LEFT off={offset}: pin aborts, got {got:?}",
                std::str::from_utf8(input)
            ),
            _ => panic!("left has no graceful false"),
        }
    }
}

fn right(input: &[u8], probes: &[(usize, i64)]) {
    for &(offset, want) in probes {
        let got = right_word_offset(input, PARITY, offset);
        match want {
            v if v >= 0 => assert_eq!(
                got,
                Ok(Some(usize::try_from(v).expect("non-negative probe"))),
                "{:?} RIGHT off={offset}: pin {v}",
                std::str::from_utf8(input)
            ),
            -1 => assert!(
                matches!(got, Err(EngineError::ZeroKeyOffsetCheck { .. })),
                "{:?} RIGHT off={offset}: pin aborts, got {got:?}",
                std::str::from_utf8(input)
            ),
            -2 => assert_eq!(
                got,
                Ok(None),
                "{:?} RIGHT off={offset}: pin false",
                std::str::from_utf8(input)
            ),
            _ => unreachable!(),
        }
    }
}

#[test]
fn nihaoshijie_matches_the_pin_tables() {
    let input = b"nihaoshijie";
    off(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 2),
            (3, 2),
            (4, 2),
            (5, 5),
            (6, 5),
            (7, 5),
            (8, 8),
            (9, 8),
            (10, 10),
            (11, 11),
        ],
    );
    left(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 2),
            (6, 0),
            (7, 0),
            (8, 5),
            (9, 0),
            (10, 8),
            (11, 10),
        ],
    );
    right(
        input,
        &[
            (0, 2),
            (1, -2),
            (2, 5),
            (3, -2),
            (4, -2),
            (5, 8),
            (6, -2),
            (7, -2),
            (8, 11),
            (9, -2),
            (10, 11),
            (11, -1),
        ],
    );
}

#[test]
fn separator_inputs_match_the_pin_tables() {
    // ni'hao: the apostrophe at 2 is a lone zero key; offset 3 sits one
    // past it (the abort shape), and the zero-skip steps 2 -> 6 over it.
    let input = b"ni'hao";
    off(
        input,
        &[(0, 0), (1, 0), (2, 2), (3, 2), (4, 2), (5, 2), (6, 6)],
    );
    left(
        input,
        &[(0, 0), (1, 0), (2, 0), (3, -1), (4, 0), (5, 0), (6, 2)],
    );
    right(
        input,
        &[(0, 2), (1, -2), (2, 6), (3, -1), (4, -2), (5, -2), (6, -1)],
    );

    // xiang'a: zero-start pulls cursor 6 back over the apostrophe to 5.
    let input = b"xiang'a";
    off(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 5),
            (6, 5),
            (7, 7),
        ],
    );
    left(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, -1),
            (7, 5),
        ],
    );
    right(
        input,
        &[
            (0, 5),
            (1, -2),
            (2, -2),
            (3, -2),
            (4, -2),
            (5, 7),
            (6, -1),
            (7, -1),
        ],
    );

    // bu'tian: the divided ti/an put real keys at columns 3 and 5, so
    // cursor 5 stays at 5 while cursor 4 walks back over the apostrophe.
    let input = b"bu'tian";
    off(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 2),
            (3, 2),
            (4, 2),
            (5, 5),
            (6, 5),
            (7, 7),
        ],
    );
    left(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, -1),
            (4, 0),
            (5, 2),
            (6, 0),
            (7, 5),
        ],
    );
    right(
        input,
        &[
            (0, 2),
            (1, -2),
            (2, 7),
            (3, -1),
            (4, -2),
            (5, 7),
            (6, -2),
            (7, -1),
        ],
    );
}

#[test]
fn leading_apostrophe_runs_match_the_pin_tables() {
    // 'nihao: the DP propagates over the leading apostrophe without a
    // zero key at column 0 — OFF c=1 answers 1, RIGHT(0) is the graceful
    // false, and neither aborts.
    let input = b"'nihao";
    off(
        input,
        &[(0, 0), (1, 1), (2, 1), (3, 3), (4, 3), (5, 3), (6, 6)],
    );
    left(
        input,
        &[(0, 0), (1, 0), (2, 0), (3, 1), (4, 0), (5, 0), (6, 3)],
    );
    right(
        input,
        &[(0, -2), (1, 3), (2, -2), (3, 6), (4, -2), (5, -2), (6, -1)],
    );

    let input = b"'ni'hao";
    off(
        input,
        &[
            (0, 0),
            (1, 1),
            (2, 1),
            (3, 3),
            (4, 3),
            (5, 3),
            (6, 3),
            (7, 7),
        ],
    );
    left(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 1),
            (4, -1),
            (5, 0),
            (6, 0),
            (7, 3),
        ],
    );
    right(
        input,
        &[
            (0, -2),
            (1, 3),
            (2, -2),
            (3, 7),
            (4, -1),
            (5, -2),
            (6, -2),
            (7, -1),
        ],
    );

    // Two leading apostrophes: both columns empty, same propagation.
    let input = b"''nihao";
    off(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 2),
            (3, 2),
            (4, 4),
            (5, 4),
            (6, 4),
            (7, 7),
        ],
    );
    left(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 2),
            (5, 0),
            (6, 0),
            (7, 4),
        ],
    );
    right(
        input,
        &[
            (0, -2),
            (1, -2),
            (2, 4),
            (3, -2),
            (4, 7),
            (5, -2),
            (6, -2),
            (7, -1),
        ],
    );
}

#[test]
fn doubled_and_trailing_apostrophes_match_the_pin_tables() {
    // ni''hao: both apostrophes of an internal run are lone zero keys.
    let input = b"ni''hao";
    off(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 2),
            (3, 2),
            (4, 2),
            (5, 2),
            (6, 2),
            (7, 7),
        ],
    );
    left(
        input,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, -1),
            (4, -1),
            (5, 0),
            (6, 0),
            (7, 2),
        ],
    );
    right(
        input,
        &[
            (0, 2),
            (1, -2),
            (2, 7),
            (3, -1),
            (4, -1),
            (5, -2),
            (6, -2),
            (7, -1),
        ],
    );

    // nihao': the trailing apostrophe is a separator zero at 5 plus the
    // reserved-slot zero at 6; zero-start pulls cursor 6 back to 5.
    let input = b"nihao'";
    off(
        input,
        &[(0, 0), (1, 0), (2, 2), (3, 2), (4, 2), (5, 5), (6, 5)],
    );
    left(
        input,
        &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 2), (6, -1)],
    );
    right(
        input,
        &[(0, 2), (1, -2), (2, 5), (3, -2), (4, -2), (5, -1), (6, -1)],
    );

    // ni'h: the separator zero at 2, the incomplete h at 3, tail at 4.
    let input = b"ni'h";
    off(input, &[(0, 0), (1, 0), (2, 2), (3, 2), (4, 4)]);
    left(input, &[(0, 0), (1, 0), (2, 0), (3, -1), (4, 2)]);
    right(input, &[(0, 2), (1, -2), (2, 4), (3, -1), (4, -1)]);
}

#[test]
fn early_stopping_parses_zero_fill_the_tail() {
    // ni2hao: the parse stops at 2; columns 2..=6 are lone zero keys, so
    // every LEFT/RIGHT at offset >= 2 (except the graceful falses) hits
    // the abort shape.
    let input = b"ni2hao";
    off(
        input,
        &[(0, 0), (1, 0), (2, 2), (3, 2), (4, 2), (5, 2), (6, 2)],
    );
    left(
        input,
        &[(0, 0), (1, 0), (2, 0), (3, -1), (4, -1), (5, -1), (6, -1)],
    );
    right(
        input,
        &[(0, 2), (1, -2), (2, -1), (3, -1), (4, -1), (5, -1), (6, -1)],
    );

    // niX: garbage tail, same shape at one byte.
    let input = b"niX";
    off(input, &[(0, 0), (1, 0), (2, 2), (3, 2)]);
    left(input, &[(0, 0), (1, 0), (2, 0), (3, -1)]);
    right(input, &[(0, 2), (1, -2), (2, -1), (3, -1)]);

    // nih / nix: the incomplete final key sits at the parse end.
    let input = b"nih";
    off(input, &[(0, 0), (1, 0), (2, 2), (3, 3)]);
    left(input, &[(0, 0), (1, 0), (2, 0), (3, 2)]);
    right(input, &[(0, 2), (1, -2), (2, 3), (3, -1)]);

    // h2: one incomplete key, then the zero tail.
    let input = b"h2";
    off(input, &[(0, 0), (1, 1), (2, 1)]);
    left(input, &[(0, 0), (1, 0), (2, -1)]);
    right(input, &[(0, 1), (1, -1), (2, -1)]);
}

#[test]
fn garbage_inputs_zero_fill_from_position_zero() {
    let input = b"2";
    off(input, &[(0, 0), (1, 0)]);
    left(input, &[(0, 0), (1, -1)]);
    right(input, &[(0, -1), (1, -1)]);

    let input = b"2x";
    off(input, &[(0, 0), (1, 0), (2, 0)]);
    left(input, &[(0, 0), (1, -1), (2, -1)]);
    right(input, &[(0, -1), (1, -1), (2, -1)]);
}

#[test]
fn leading_garbage_parses_as_an_incomplete_key() {
    // xnihao: the leading x is an incomplete key at column 0 under the
    // parity word's PINYIN_INCOMPLETE.
    let input = b"xnihao";
    off(
        input,
        &[(0, 0), (1, 1), (2, 1), (3, 3), (4, 3), (5, 3), (6, 6)],
    );
    left(
        input,
        &[(0, 0), (1, 0), (2, 0), (3, 1), (4, 0), (5, 0), (6, 3)],
    );
    right(
        input,
        &[(0, 1), (1, 3), (2, -2), (3, 6), (4, -2), (5, -2), (6, -1)],
    );

    let input = b"x";
    off(input, &[(0, 0), (1, 1)]);
    left(input, &[(0, 0), (1, 0)]);
    right(input, &[(0, 1), (1, -1)]);
}

#[test]
fn apostrophe_only_inputs_follow_the_zero_fill() {
    // The pin parses apostrophe-only input to its full length and places
    // a lone zero key at every consumed position. Before the termination
    // law (#178) the graph consumed 0 for these, so the columns came from
    // the tail zero-fill instead and OFF answered the clamped 0 where the
    // pin aborts — a parse-derived divergence the old expectations
    // recorded. The law's apostrophe propagation makes `matrix_spans`
    // report the run-consumed length, so the columns are now the pin's
    // own and every law agrees with it: LEFT/RIGHT abort as before, and
    // OFF aborts for every cursor past 0 (cursor 0 clamps below the
    // zero-run walk and stays 0).
    for input in [b"'".as_slice(), b"''", b"'''"] {
        let len = input.len();
        assert_eq!(lookup_offset_for_cursor(input, PARITY, 0), Ok(0));
        for cursor in 1..=len {
            assert!(
                matches!(
                    lookup_offset_for_cursor(input, PARITY, cursor),
                    Err(EngineError::ZeroKeyOffsetCheck { .. })
                ),
                "{input:?} OFF c={cursor}: pin aborts"
            );
        }
        assert_eq!(left_word_offset(input, PARITY, 0), Ok(0));
        for offset in 1..=len {
            assert!(
                matches!(
                    left_word_offset(input, PARITY, offset),
                    Err(EngineError::ZeroKeyOffsetCheck { .. })
                ),
                "{input:?} LEFT off={offset}: pin aborts"
            );
        }
        for offset in 0..=len {
            assert!(
                matches!(
                    right_word_offset(input, PARITY, offset),
                    Err(EngineError::ZeroKeyOffsetCheck { .. })
                ),
                "{input:?} RIGHT off={offset}: pin aborts"
            );
        }
    }
}

#[test]
fn span_law_without_separators_steps_keys_only() {
    // The zhuyin/double degenerate shape: key spans only, no zero
    // columns, so nothing aborts and gaps answer the graceful false.
    let input = b"abcd";
    let spans = [(0usize, 2usize), (2, 4)];
    assert_eq!(lookup_offset_over_spans(input, 4, &spans, false, 3), Ok(2));
    assert_eq!(
        left_word_offset_over_spans(input, 4, &spans, false, 4),
        Ok(2)
    );
    assert_eq!(
        right_word_offset_over_spans(input, 4, &spans, false, 0),
        Ok(Some(2))
    );
    assert_eq!(
        right_word_offset_over_spans(input, 4, &spans, false, 1),
        Ok(None)
    );
}

#[test]
fn an_empty_buffer_is_an_empty_matrix() {
    // No parse ever ran: every column is empty, no zero keys. Offsets
    // past the (zero) one-past-end position are the range refusal — the
    // pin asserts on its empty matrix there (the F-E-14 shape).
    assert_eq!(lookup_offset_for_cursor(b"", PARITY, 3), Ok(0));
    assert_eq!(left_word_offset(b"", PARITY, 0), Ok(0));
    assert_eq!(
        left_word_offset(b"", PARITY, 1),
        Err(EngineError::LookupOffsetOutOfRange { offset: 1, len: 0 })
    );
    assert_eq!(right_word_offset(b"", PARITY, 0), Ok(None));
}

#[test]
fn offsets_past_one_past_end_are_refused() {
    let input = b"nihao";
    assert_eq!(
        left_word_offset(input, PARITY, 6),
        Err(EngineError::LookupOffsetOutOfRange { offset: 6, len: 5 })
    );
    assert_eq!(
        right_word_offset(input, PARITY, 9),
        Err(EngineError::LookupOffsetOutOfRange { offset: 9, len: 5 })
    );
}
