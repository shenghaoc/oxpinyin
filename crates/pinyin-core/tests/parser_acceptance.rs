//! Frozen F-A acceptance and arbitrary-byte parser properties.

use pinyin_core::{
    Completeness, FULL_PINYIN_SYLLABLES, FullPinyinParser, InputParser, MAX_PARSE_RESULTS,
    ParseError, ParseResult, ParsedSyllable,
};
use proptest::prelude::*;

const F_A: &str = include_str!("../../../fixtures/foundation/f-a.txt");

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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_bytes_are_total_and_deterministic(
        input in prop::collection::vec(any::<u8>(), 0..=4_096),
    ) {
        let parser = FullPinyinParser;
        let first = parser.parse(&input);
        let second = parser.parse(&input);
        prop_assert_eq!(&first, &second);

        match first {
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
        prop_assert!(gap.is_empty() || gap == b"'");
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
