//! Frozen F-A acceptance, structured path-set cases, and arbitrary-byte properties.

use oxpinyin_core::{
    Completeness, FULL_PINYIN_SYLLABLE_COUNT, FULL_PINYIN_SYLLABLES, FullPinyinParser, InputParser,
    MAX_PARSE_RESULTS, ParseError, ParseResult, ParsedSyllable,
};
use proptest::prelude::*;

const F_A: &str = include_str!("../../../fixtures/foundation/f-a.txt");

// ---------------------------------------------------------------------------
// Structured acceptance cases (path-set SPEC + R3)
// ---------------------------------------------------------------------------

#[test]
fn empty_input_returns_one_empty_segmentation() {
    assert_parse(b"", &[result(vec![], b"")]);
}

#[test]
fn single_valid_syllables_parse() {
    for syllable in ["ma", "zhuang", "nv"] {
        let paths = FullPinyinParser
            .parse(syllable.as_bytes())
            .expect("single table syllables stay below the path limit");
        assert!(
            !paths.is_empty(),
            "syllable {syllable:?} must produce at least one path"
        );
        assert_eq!(
            paths[0],
            result(vec![complete(syllable, 0, syllable.len())], b""),
            "greedy first path for {syllable:?}"
        );
        for path in &paths {
            assert!(path.remainder.is_empty(), "syllable {syllable:?}");
            assert!(
                path.syllables
                    .iter()
                    .all(|segment| segment.completeness == Completeness::Complete),
                "complete table syllable {syllable:?} must not emit partials"
            );
        }
    }
}

#[test]
fn ambiguous_xian_returns_both_complete_segmentations() {
    assert_parse(
        b"xian",
        &[
            result(vec![complete("xian", 0, 4)], b""),
            result(vec![complete("xi", 0, 2), complete("an", 2, 4)], b""),
        ],
    );
}

#[test]
fn apostrophe_is_hard_boundary_for_xi_an() {
    assert_parse(
        b"xi'an",
        &[result(
            vec![complete("xi", 0, 2), complete("an", 3, 5)],
            b"",
        )],
    );
}

#[test]
fn trailing_junk_after_valid_prefix_nihx() {
    // Maximal complete+partial prefix is `ni` + `h`; unconsumable `x` is remainder.
    assert_parse(
        b"nihx",
        &[result(vec![complete("ni", 0, 2), partial("h", 2, 3)], b"x")],
    );
}

#[test]
fn pure_non_pinyin_input() {
    // Leading lowercase letter that is only a partial prefix of some table syllable.
    assert_parse(b"xyz", &[result(vec![partial("x", 0, 1)], b"yz")]);
    // Digits are unsupported and start the untouched remainder.
    assert_parse(b"123", &[result(vec![], b"123")]);
    // Bytes above 0x7F are unsupported and start the untouched remainder.
    assert_parse(&[0xff], &[result(vec![], &[0xff])]);
    assert_parse(&[0x80, 0xc0, 0xff], &[result(vec![], &[0x80, 0xc0, 0xff])]);
}

#[test]
fn maximum_length_valid_syllable_zhuang() {
    let paths = FullPinyinParser
        .parse(b"zhuang")
        .expect("zhuang stays below the path limit");
    assert_eq!(
        paths[0],
        result(vec![complete("zhuang", 0, 6)], b""),
        "max-length syllable must be the greedy first complete path"
    );
    assert!(
        paths.iter().all(|path| {
            path.remainder.is_empty()
                && path
                    .syllables
                    .iter()
                    .all(|segment| segment.completeness == Completeness::Complete)
        }),
        "zhuang must emit only complete segmentations"
    );
}

#[test]
fn every_table_syllable_round_trips_as_complete_identity() {
    assert_eq!(FULL_PINYIN_SYLLABLES.len(), FULL_PINYIN_SYLLABLE_COUNT);
    assert_eq!(
        FULL_PINYIN_SYLLABLE_COUNT, 405,
        "frozen inventory is exactly 405 complete syllables"
    );

    let parser = FullPinyinParser;
    for syllable in FULL_PINYIN_SYLLABLES {
        let paths = parser
            .parse(syllable.as_bytes())
            .unwrap_or_else(|error| panic!("syllable {syllable:?} failed: {error}"));

        // Round-trip: the greedy first path is exactly one Complete segment
        // covering the whole table entry (identity segmentation).
        assert_eq!(
            paths[0],
            result(vec![complete(syllable, 0, syllable.len())], b""),
            "identity complete path for {syllable:?}"
        );

        // Additional alternatives, if any, are complete segmentations only.
        for path in &paths {
            assert!(path.remainder.is_empty(), "syllable {syllable:?}");
            assert!(
                !path.syllables.is_empty(),
                "non-empty syllable {syllable:?} must not produce empty segments"
            );
            assert!(
                path.syllables
                    .iter()
                    .all(|segment| segment.completeness == Completeness::Complete),
                "table syllable {syllable:?} must not emit partials"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Frozen F-A: oracle-selected path is present in the portable path set
// ---------------------------------------------------------------------------

#[test]
fn frozen_f_a_selected_paths_are_portable_paths() {
    let parser = FullPinyinParser;
    let mut records = 0;

    for line in F_A.lines() {
        records += 1;
        assert_eq!(field(line, "schema"), "pinyin-capture-v1");
        assert_eq!(field(line, "family"), "F-A");
        assert!(!field(line, "pin_ref").is_empty());

        let case = field(line, "case");
        let input = unescape(field(line, "input"));
        let expected = ParseResult {
            syllables: parse_segments(field(line, "segments")),
            remainder: unescape(field(line, "remainder")),
        };
        let paths = parser
            .parse(&input)
            .unwrap_or_else(|error| panic!("F-A case {case} failed to parse: {error}"));

        assert!(
            paths.contains(&expected),
            "F-A case {case} selected path missing\nexpected: {expected:?}\nactual: {paths:?}"
        );
    }

    assert_eq!(records, 15, "the frozen F-A family changed record count");
}

// ---------------------------------------------------------------------------
// Property tests: totality and determinism over arbitrary bytes
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Totality: every byte sequence returns Ok or a defined ParseError.
    /// The call must not panic and must not hit unreachable control flow.
    #[test]
    fn arbitrary_bytes_never_panic(
        input in prop::collection::vec(any::<u8>(), 0..=4_096),
    ) {
        let parser = FullPinyinParser;
        match parser.parse(&input) {
            Ok(paths) => {
                prop_assert!(!paths.is_empty());
                prop_assert!(paths.len() <= MAX_PARSE_RESULTS);
                for path in &paths {
                    assert_path_invariants(&input, path)?;
                }
            }
            Err(error) => {
                prop_assert_eq!(
                    error,
                    ParseError::TooManyAlternatives {
                        limit: MAX_PARSE_RESULTS,
                    }
                );
            }
        }
    }

    /// Determinism: identical input always yields identical Result.
    #[test]
    fn arbitrary_bytes_are_deterministic(
        input in prop::collection::vec(any::<u8>(), 0..=4_096),
    ) {
        let parser = FullPinyinParser;
        let first = parser.parse(&input);
        let second = parser.parse(&input);
        prop_assert_eq!(first, second);
    }

    /// Over-limit Cartesian products surface the sole defined error variant.
    #[test]
    fn over_limit_ambiguity_is_total_and_deterministic(group_count in 13_usize..=32) {
        let input = std::iter::repeat_n("xian", group_count)
            .collect::<Vec<_>>()
            .join("'")
            .into_bytes();
        let parser = FullPinyinParser;
        let first = parser.parse(&input);
        let second = parser.parse(&input);

        prop_assert_eq!(&first, &second);
        prop_assert_eq!(
            first,
            Err(ParseError::TooManyAlternatives {
                limit: MAX_PARSE_RESULTS,
            })
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn assert_path_invariants(
    input: &[u8],
    path: &ParseResult,
) -> Result<(), proptest::test_runner::TestCaseError> {
    prop_assert!(path.remainder.len() <= input.len());
    let remainder_start = input.len() - path.remainder.len();
    prop_assert_eq!(&input[remainder_start..], path.remainder.as_slice());

    let mut previous_end = 0;
    let mut saw_partial = false;
    for segment in &path.syllables {
        prop_assert!(segment.start >= previous_end);
        prop_assert!(segment.start < segment.end);
        prop_assert!(segment.end <= remainder_start);
        prop_assert_eq!(
            &input[segment.start..segment.end],
            segment.syllable.as_bytes()
        );

        let gap = &input[previous_end..segment.start];
        prop_assert!(
            gap.is_empty() || (!gap.is_empty() && gap.iter().all(|&b| b == b'\'')),
            "gap between segments must be empty or apostrophes, got {gap:?}"
        );
        prop_assert!(!saw_partial, "partial segment must be last");

        match segment.completeness {
            Completeness::Complete => {
                prop_assert!(FULL_PINYIN_SYLLABLES.contains(&segment.syllable.as_str()));
            }
            Completeness::Partial => {
                saw_partial = true;
                prop_assert!(!FULL_PINYIN_SYLLABLES.contains(&segment.syllable.as_str()));
                let is_partial = FULL_PINYIN_SYLLABLES.iter().any(|syllable| {
                    segment.syllable.len() < syllable.len()
                        && syllable.as_bytes().starts_with(segment.syllable.as_bytes())
                });
                prop_assert!(is_partial);
            }
        }

        previous_end = segment.end;
    }

    prop_assert_eq!(previous_end, remainder_start);
    Ok(())
}

fn assert_parse(input: &[u8], expected: &[ParseResult]) {
    let actual = FullPinyinParser
        .parse(input)
        .expect("structured acceptance cases stay below the path limit");
    assert_eq!(actual, expected, "input: {input:?}");
}

fn complete(syllable: &str, start: usize, end: usize) -> ParsedSyllable {
    ParsedSyllable {
        syllable: syllable.to_owned(),
        start,
        end,
        completeness: Completeness::Complete,
    }
}

fn partial(syllable: &str, start: usize, end: usize) -> ParsedSyllable {
    ParsedSyllable {
        syllable: syllable.to_owned(),
        start,
        end,
        completeness: Completeness::Partial,
    }
}

fn result(syllables: Vec<ParsedSyllable>, remainder: &[u8]) -> ParseResult {
    ParseResult {
        syllables,
        remainder: remainder.to_vec(),
    }
}

fn field<'a>(line: &'a str, name: &str) -> &'a str {
    line.split('\t')
        .find_map(|entry| {
            let (key, value) = entry.split_once('=')?;
            (key == name).then_some(value)
        })
        .unwrap_or_else(|| panic!("fixture record is missing {name:?}: {line}"))
}

fn parse_segments(value: &str) -> Vec<ParsedSyllable> {
    if value == "-" {
        return Vec::new();
    }

    value
        .split(',')
        .map(|value| {
            let (syllable, range_and_state) = value
                .split_once('@')
                .unwrap_or_else(|| panic!("invalid fixture segment: {value}"));
            let (range, state) = range_and_state
                .rsplit_once(':')
                .unwrap_or_else(|| panic!("invalid fixture segment state: {value}"));
            let (start, end) = range
                .split_once(':')
                .unwrap_or_else(|| panic!("invalid fixture segment range: {value}"));
            let completeness = match state {
                "complete" => Completeness::Complete,
                "partial" => Completeness::Partial,
                other => panic!("invalid fixture segment completeness {other:?}"),
            };

            ParsedSyllable {
                syllable: syllable.to_owned(),
                start: start
                    .parse()
                    .unwrap_or_else(|error| panic!("invalid segment start {start:?}: {error}")),
                end: end
                    .parse()
                    .unwrap_or_else(|error| panic!("invalid segment end {end:?}: {error}")),
                completeness,
            }
        })
        .collect()
}

fn unescape(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut cursor = 0;

    while cursor < bytes.len() {
        if bytes[cursor] != b'\\' {
            output.push(bytes[cursor]);
            cursor += 1;
            continue;
        }

        let escaped = *bytes
            .get(cursor + 1)
            .unwrap_or_else(|| panic!("trailing escape in fixture value {value:?}"));
        match escaped {
            b'\\' => output.push(b'\\'),
            b't' => output.push(b'\t'),
            b'r' => output.push(b'\r'),
            b'n' => output.push(b'\n'),
            b'x' => {
                let high = *bytes
                    .get(cursor + 2)
                    .unwrap_or_else(|| panic!("short hex escape in fixture value {value:?}"));
                let low = *bytes
                    .get(cursor + 3)
                    .unwrap_or_else(|| panic!("short hex escape in fixture value {value:?}"));
                output.push((hex(high) << 4) | hex(low));
                cursor += 2;
            }
            other => panic!("unknown fixture escape \\{}", char::from(other)),
        }
        cursor += 2;
    }

    output
}

fn hex(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        b'A'..=b'F' => value - b'A' + 10,
        _ => panic!("invalid fixture hex digit {:?}", char::from(value)),
    }
}
