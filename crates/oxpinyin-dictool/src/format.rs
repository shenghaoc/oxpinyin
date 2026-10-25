//! The classic ibus-libpinyin user-dictionary interchange format.
//!
//! The grammar is frozen in `docs/findings/dictool-format.md`. This module
//! is the executable copy; the frontend reference implementation is
//! `LibPinyinBackEnd::importPinyinDictionary` (`PYLibPinyin.cc:230-277`).

use std::collections::HashSet;
use std::fmt;

use oxpinyin_core::graph::FewestKeys;

use crate::import::MAX_COUNT;

/// One parsed classic-format record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Phrase text, non-empty and at most 15 Unicode scalar values.
    pub phrase: String,
    /// Canonical `'`-joined pinyin (`ni'hao`), not the source spelling.
    ///
    /// `nihao` and `nihaoXYZ` parse as the same reading as `ni'hao`, so
    /// identity and the desired-count snapshot must use this form.
    pub pinyin: String,
    /// Desired absolute pronunciation count; `None` for a 2-field line,
    /// which floors at the ABI default count.
    pub count: Option<u64>,
    /// 1-based source line number, retained for ABI-failure reporting after
    /// comment/blank lines have been skipped.
    pub line: usize,
}

/// A malformed input line, reported as `line N: message`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    /// 1-based line number.
    pub line: usize,
    /// What made the line malformed.
    pub message: String,
}

impl ParseError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Split one line exactly like the frontend's `g_strsplit_set(line, " \t", 3)`
/// (`PYLibPinyin.cc:249`): at most three pieces, delimiters are space or TAB,
/// consecutive delimiters produce empty pieces, and the third piece keeps any
/// remaining separator bytes.
fn split_classic(line: &[u8]) -> Vec<&[u8]> {
    line.splitn(3, |byte| matches!(byte, b' ' | b'\t'))
        .collect()
}

/// Parse the complete file as byte records, ending each record at its first NUL.
///
/// Classic interchange: `phrase SP/TAB pinyin [SP/TAB count]`, LF or CRLF,
/// one record per line. Two-field lines mean "use the ABI default count".
/// Dictool superset extensions over the frontend: comment lines and blank
/// lines are skipped with a diagnostic-free parse, and every malformed line
/// (wrong field count, empty field, unparseable pinyin, bad/out-of-range
/// count, duplicate `(phrase, pinyin)`) is an error carrying its 1-based line
/// number.
pub fn parse(text: impl AsRef<[u8]>) -> Result<Vec<Record>, ParseError> {
    let mut records = Vec::new();
    let mut seen = HashSet::new();

    for (index, raw_line) in text.as_ref().split(|byte| *byte == b'\n').enumerate() {
        let line_number = index + 1;
        // Frontend PYLibPinyin.cc:245-254 sees only the prefix before NUL.
        let visible = raw_line.split(|byte| *byte == 0).next().unwrap_or_default();
        let visible = visible.strip_suffix(b"\r").unwrap_or(visible);
        let Some(first) = visible
            .iter()
            .position(|byte| !matches!(byte, b' ' | b'\t'))
        else {
            continue;
        };
        if visible[first] == b'#' {
            std::str::from_utf8(visible)
                .map_err(|_| ParseError::new(line_number, "record is not valid UTF-8"))?;
            continue;
        }

        let fields = split_classic(visible);
        if fields.len() != 2 && fields.len() != 3 {
            return Err(ParseError::new(
                line_number,
                format!(
                    "expected 2 or 3 space/tab-separated fields, found {}",
                    fields.len()
                ),
            ));
        }
        let phrase = std::str::from_utf8(fields[0])
            .map_err(|_| ParseError::new(line_number, "phrase is not valid UTF-8"))?;
        // FullPinyinParser2 takes a longest prefix from byte zero. No key can
        // cross an invalid UTF-8 byte: its spelling inventory is ASCII.
        // pinyin_parser2.cpp:164-213,217-333,346-379 at 074a2219.
        let pinyin = match std::str::from_utf8(fields[1]) {
            Ok(text) => text,
            Err(error) => {
                std::str::from_utf8(&fields[1][..error.valid_up_to()]).unwrap_or_default()
            }
        };
        let count_text = fields
            .get(2)
            .map(|bytes| {
                std::str::from_utf8(bytes)
                    .map_err(|_| ParseError::new(line_number, "count is not valid UTF-8"))
            })
            .transpose()?;

        if phrase.is_empty() {
            return Err(ParseError::new(line_number, "phrase field is empty"));
        }
        if pinyin.is_empty() {
            return Err(ParseError::new(line_number, "pinyin field is empty"));
        }
        if phrase.contains(['\r', '\n', '\0']) {
            return Err(ParseError::new(
                line_number,
                "phrase field contains a forbidden control byte",
            ));
        }
        if pinyin.contains('\0') {
            return Err(ParseError::new(
                line_number,
                "pinyin field contains a forbidden NUL byte",
            ));
        }

        let phrase_len = phrase.chars().count();
        if phrase_len == 0 || phrase_len >= 16 {
            return Err(ParseError::new(
                line_number,
                "phrase must be 1..=15 Unicode scalar values",
            ));
        }
        // The import ABI's parse selection: longest parsed prefix under
        // the frozen untuned full-pinyin inventory, fewest complete keys,
        // trailing unparsed bytes ignored (`pinyin_iterator_add_phrase`).
        let Some(parsed) = FewestKeys::parse(pinyin) else {
            return Err(ParseError::new(
                line_number,
                format!("pinyin does not parse: {pinyin:?}"),
            ));
        };
        let key_count = parsed.keys().len();
        if key_count != phrase_len {
            return Err(ParseError::new(
                line_number,
                format!(
                    "pinyin has {key_count} key(s) but the phrase has {phrase_len} character(s)"
                ),
            ));
        }
        let canonical = parsed.canonical();

        let count = match count_text {
            None => None,
            Some(count_text) => {
                if count_text.is_empty() {
                    return Err(ParseError::new(line_number, "count field is empty"));
                }
                // `count := digit+` (`dictool-format.md` §2): `str::parse`
                // would also take a leading `+`.
                let count = Some(count_text)
                    .filter(|text| text.bytes().all(|byte| byte.is_ascii_digit()))
                    .and_then(|text| text.parse::<u64>().ok())
                    .ok_or_else(|| {
                        ParseError::new(
                            line_number,
                            format!("count is not a decimal integer: {count_text:?}"),
                        )
                    })?;
                if count > MAX_COUNT {
                    return Err(ParseError::new(
                        line_number,
                        format!("count must be 0..={MAX_COUNT}"),
                    ));
                }
                Some(count)
            }
        };

        if !seen.insert((phrase.to_owned(), canonical.clone())) {
            return Err(ParseError::new(
                line_number,
                format!("duplicate (phrase, pinyin) pair: {phrase:?} {canonical:?}"),
            ));
        }

        records.push(Record {
            phrase: phrase.to_owned(),
            pinyin: canonical,
            count,
            line: line_number,
        });
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(phrase: &str, pinyin: &str, count: Option<u64>, line: usize) -> Record {
        Record {
            phrase: phrase.to_owned(),
            pinyin: pinyin.to_owned(),
            count,
            line,
        }
    }

    #[test]
    fn split_classic_matches_g_strsplit_set_max_three() {
        for (input, expected) in [
            ("a b 3", vec!["a", "b", "3"]),
            ("a  b 3", vec!["a", "", "b 3"]),
            ("a b 3 extra", vec!["a", "b", "3 extra"]),
            (" a b 3", vec!["", "a", "b 3"]),
            ("a b\t3", vec!["a", "b", "3"]),
            ("a b ", vec!["a", "b", ""]),
            ("a\tb", vec!["a", "b"]),
        ] {
            assert_eq!(
                split_classic(input.as_bytes()),
                expected.iter().map(|s| s.as_bytes()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn classic_separators_are_ascii_and_escapes_are_literal() {
        for separator in ['\u{a0}', '\u{2003}', '\u{2028}', '\u{2029}'] {
            assert!(parse(format!("词{separator}ci")).is_err());
        }
        for phrase in ["\\", "\u{200d}", "\u{378}", "\u{301}"] {
            // No quoting, escaping or Unicode-category filter in this format.
            assert_eq!(
                parse(format!("{phrase}\tci\t7")).unwrap(),
                vec![record(phrase, "ci", Some(7), 1)]
            );
        }
        assert_eq!(
            parse("词 ci\\ 7").unwrap(),
            vec![record("词", "ci", Some(7), 1)]
        );
        for line in ["词  ci", "词\t\tci", "词 ci ", "词 ci 7\t"] {
            assert!(parse(line).is_err(), "{line:?}");
        }
    }

    #[test]
    fn classic_count_validation_is_not_frontend_atoi() {
        assert_eq!(
            parse("词 ci 007").unwrap(),
            vec![record("词", "ci", Some(7), 1)]
        );
        // The frozen dictool validation contract (`count := digit+`) refuses
        // these count texts; the frontend's signed/prefix conversion has a
        // different domain. A leading `+` is not a digit.
        for count in [
            "+7",
            "+0",
            "+",
            "-0",
            "-1",
            "-2",
            "7suffix",
            "2147483648",
            "4294967303",
        ] {
            assert!(parse(format!("词 ci {count}")).is_err(), "{count}");
        }
    }

    #[test]
    fn a_plus_sign_count_is_refused_with_its_line_number() {
        let error = parse("词 ci 3\n词二 ci'ci +7\n").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("line 2"), "{message}");
        assert!(
            message.contains("count is not a decimal integer"),
            "{message}"
        );
    }

    #[test]
    fn parses_two_and_three_field_space_or_tab_records() {
        let text = "# classic export\n   \n你好 ni'hao 3\r\n世界\tshi'jie\t7\n词 ci\n";
        assert_eq!(
            parse(text).unwrap(),
            vec![
                record("你好", "ni'hao", Some(3), 3),
                record("世界", "shi'jie", Some(7), 4),
                record("词", "ci", None, 5),
            ]
        );
    }

    #[test]
    fn unseparated_pinyin_and_trailing_bytes_match_the_import_abi() {
        assert_eq!(
            parse("你好 nihao\n").unwrap(),
            vec![record("你好", "ni'hao", None, 1)]
        );
        assert_eq!(
            parse("你好 nihaoXYZ 4\n").unwrap(),
            vec![record("你好", "ni'hao", Some(4), 1)]
        );
        let error = parse("你好 nihao\n你好 nihaoXYZ 4\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("duplicate"), "{}", error.message);
    }

    #[test]
    fn malformed_lines_carry_their_one_based_line_number() {
        let cases = [
            ("\n\n词 ci x\n", 3, "count is not a decimal integer"),
            ("词 ci 2147483648\n", 1, "count must be 0..="),
            ("词 ci 3 extra\n", 1, "count is not a decimal integer"),
            ("词 ci\nextra\n", 2, "expected 2 or 3"),
            ("\tci 1\n", 1, "phrase field is empty"),
            ("词\t 1\n", 1, "pinyin field is empty"),
            ("词 n 1\n", 1, "pinyin does not parse"),
            ("你好 ni 1\n", 1, "1 key(s) but the phrase has 2"),
            ("你好 ni'hao 1\n你好 ni'hao 2\n", 2, "duplicate"),
            ("你好 nihao 1\n你好 ni'hao 2\n", 2, "duplicate"),
        ];
        for (text, line, needle) in cases {
            let error = parse(text).unwrap_err();
            assert_eq!(error.line, line, "text: {text:?}");
            assert!(
                error.message.contains(needle),
                "text {text:?}: {} does not contain {needle:?}",
                error.message
            );
        }
    }

    #[test]
    fn classic_records_render_space_separated_lines_that_parse_back() {
        let text = "你好 ni'hao 3\n世界 shi'jie 7\n词 ci\n";
        let records = parse(text).unwrap();
        let rendered: String = records
            .iter()
            .map(|record| {
                record.count.map_or_else(
                    || format!("{} {}\n", record.phrase, record.pinyin),
                    |count| format!("{} {} {}\n", record.phrase, record.pinyin, count),
                )
            })
            .collect();
        assert_eq!(parse(&rendered).unwrap(), records);
    }
    #[test]
    fn nul_terminates_classic_records_before_utf8_validation() {
        let ordinary = parse("词 ci 7\n你好 nihao\n").unwrap();
        let mut bytes = "词 ci 7\0".as_bytes().to_vec();
        bytes.extend_from_slice(b"\xff\n");
        bytes.extend_from_slice("你好 nihao\0hidden\n".as_bytes());
        assert_eq!(parse(bytes).unwrap(), ordinary);
        assert_eq!(parse(b"\0\xff\n").unwrap(), []);
        assert!(parse("词\0 ci 7").is_err());
        assert!(parse(b"\xff ci 7\0ignored").is_err());
    }
    #[test]
    fn invalid_pinyin_suffix_keeps_the_longest_ascii_prefix() {
        let mut input = "你好 nihao".as_bytes().to_vec();
        input.extend_from_slice(b"\xffgarbage 7\n");
        assert_eq!(parse(input).unwrap(), parse("你好 nihao 7\n").unwrap());
        let mut input = "你好 ni".as_bytes().to_vec();
        input.extend_from_slice(b"\xffhao 7\n");
        assert!(
            parse(input).is_err(),
            "invalid byte must not join separated keys"
        );
    }
}
