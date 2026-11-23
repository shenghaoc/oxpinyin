use crate::OptionBits;
use crate::graph::SegmentGraph;

use super::*;
use crate::engine::error::EngineError;
use crate::engine::session::build_scan_matrix;
use crate::engine::session::matrix::ScanMatrix;

/// The parity word: `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
/// USE_RESPLIT_TABLE` plus the harness's `0x2` bit.
const PARITY: OptionBits = OptionBits::from_bits_retain(0x18a);

/// The full-pinyin scan matrix's keys of `input` with their spans, plus
/// the graph's consumed length, as a parse under `options` fills them.
///
/// Keys appear in the matrix's column order, so the first key of a column
/// is the pin's `matrix.get_item(column, 0, ...)`.
fn matrix_keys(input: &[u8], options: OptionBits) -> Result<(Vec<MatrixKey>, usize), EngineError> {
    let graph = SegmentGraph::build_with_options(input, options).ok_or(EngineError::Failure)?;
    let parsed = graph.consumed();
    let matrix = ScanMatrix::from_columns(build_scan_matrix(&graph, options, true));
    Ok((MatrixKeys::new(matrix.keys()).iter().collect(), parsed))
}

/// The setup the cursor laws' plain entry points did: the full-pinyin scan
/// matrix of `input` as (syllable start, raw end) spans, then `law` over
/// them with separators on.
fn over_matrix<T>(
    input: &[u8],
    options: OptionBits,
    law: impl FnOnce(&[u8], usize, &[(usize, usize)], bool) -> Result<T, EngineError>,
) -> Result<T, EngineError> {
    let (keys, parsed) = matrix_keys(input, options)?;
    let spans: Vec<(usize, usize)> = keys
        .iter()
        .map(|key| (key.syllable_start(), key.end()))
        .collect();
    law(input, parsed, &spans, true)
}

// Basis: pinyin.cpp:2163–2180,2987–3095 at 074a2219 and the Rust Columns::zero_end projection contract.
#[test]
fn nihaoshijie_matches_the_pin_tables() {
    let cases: &[(&[u8], usize, [i64; 3])] = &[
        (b"nihaoshijie", 0, [0, 0, 2]),
        (b"nihaoshijie", 3, [2, 0, -2]),
        (b"nihaoshijie", 5, [5, 2, 8]),
        (b"nihaoshijie", 8, [8, 5, 11]),
        (b"nihaoshijie", 10, [10, 8, 11]),
        (b"nihaoshijie", 11, [11, 10, -1]),
        (b"ni'hao", 2, [2, 0, 6]),
        (b"ni'hao", 3, [2, -1, -1]),
        (b"'ni'hao", 0, [0, 0, -2]),
        (b"'ni'hao", 1, [1, 0, 3]),
        (b"''nihao", 1, [0, 0, -2]),
        (b"''nihao", 2, [2, 0, 4]),
        (b"ni''hao", 2, [2, 0, 7]),
        (b"ni''hao", 4, [2, -1, -1]),
        (b"nihao'", 6, [5, -1, -1]),
        (b"ni2hao", 2, [2, 0, -1]),
        (b"ni2hao", 3, [2, -1, -1]),
        (b"2", 0, [0, 0, -1]),
        (b"2", 1, [0, -1, -1]),
        (b"''", 0, [0, 0, -1]),
        (b"''", 1, [-1, -1, -1]),
    ];
    let expected = |answer| match answer {
        -2 => Ok(None),
        -1 => Err(EngineError::ZeroKeyOffsetCheck),
        position => Ok(Some(usize::try_from(position).unwrap())),
    };
    for &(input, offset, [lookup, left, right]) in cases {
        let (keys, parsed) = matrix_keys(input, PARITY).unwrap();
        let spans: Vec<_> = keys
            .iter()
            .map(|key| (key.syllable_start(), key.end()))
            .collect();
        let input_label = String::from_utf8_lossy(input);
        assert_eq!(
            lookup_offset_over_spans(input, parsed, &spans[..], true, offset).map(Some),
            expected(lookup),
            "{input_label} lookup at {offset}"
        );
        assert_eq!(
            left_word_offset_over_spans(input, parsed, &spans[..], true, offset).map(Some),
            expected(left),
            "{input_label} left at {offset}"
        );
        assert_eq!(
            right_word_offset_over_spans(input, parsed, &spans[..], true, offset),
            expected(right),
            "{input_label} right at {offset}"
        );
    }
}

// Basis: Rust KeySpans contract with physical separator projection disabled.
#[test]
fn span_law_without_separators_steps_keys_only() {
    let input = b"abcd";
    let spans = [(0usize, 2usize), (2, 4)];
    assert_eq!(
        lookup_offset_over_spans(input, 4, &spans[..], false, 3),
        Ok(2)
    );
    assert_eq!(
        left_word_offset_over_spans(input, 4, &spans[..], false, 4),
        Ok(2)
    );
    assert_eq!(
        right_word_offset_over_spans(input, 4, &spans[..], false, 0),
        Ok(Some(2))
    );
    assert_eq!(
        right_word_offset_over_spans(input, 4, &spans[..], false, 1),
        Ok(None)
    );
}

// Basis: Rust Columns::zero_end and cursor range contracts for an empty input.
#[test]
fn an_empty_buffer_is_an_empty_matrix() {
    assert_eq!(
        over_matrix(b"", PARITY, |i, p, s, sep| lookup_offset_over_spans(
            i, p, s, sep, 3
        )),
        Ok(0)
    );
    assert_eq!(
        over_matrix(b"", PARITY, |i, p, s, sep| left_word_offset_over_spans(
            i, p, s, sep, 0
        )),
        Ok(0)
    );
    assert_eq!(
        over_matrix(b"", PARITY, |i, p, s, sep| left_word_offset_over_spans(
            i, p, s, sep, 1
        )),
        Err(EngineError::LookupOffsetOutOfRange { offset: 1, len: 0 })
    );
    assert_eq!(
        over_matrix(b"", PARITY, |i, p, s, sep| right_word_offset_over_spans(
            i, p, s, sep, 0
        )),
        Ok(None)
    );
}

// Basis: Rust cursor range contract at the raw-buffer bound.
#[test]
fn offsets_past_one_past_end_are_refused() {
    let input = b"nihao";
    assert_eq!(
        over_matrix(input, PARITY, |i, p, s, sep| left_word_offset_over_spans(
            i, p, s, sep, 6
        )),
        Err(EngineError::LookupOffsetOutOfRange { offset: 6, len: 5 })
    );
    assert_eq!(
        over_matrix(input, PARITY, |i, p, s, sep| right_word_offset_over_spans(
            i, p, s, sep, 9
        )),
        Err(EngineError::LookupOffsetOutOfRange { offset: 9, len: 5 })
    );
}
