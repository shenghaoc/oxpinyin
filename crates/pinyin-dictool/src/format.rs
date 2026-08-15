//! The classic ibus-libpinyin user-dictionary interchange format.
//!
//! The grammar is frozen in `docs/findings/dictool-format.md`. This module
//! is the executable copy; the frontend reference implementation is
//! `LibPinyinBackEnd::importPinyinDictionary` (`PYLibPinyin.cc:230-277`).

use std::collections::HashSet;
use std::fmt;

use pinyin_capi::import_pinyin_key_count;

use crate::import::MAX_COUNT;

/// One parsed classic-format record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Phrase text, non-empty and at most 15 Unicode scalar values.
    pub phrase: String,
    /// Pinyin text accepted by the import ABI (space-free by construction:
    /// space/tab is the field separator).
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
fn split_classic(line: &str) -> Vec<&str> {
    let Some(first) = line.find([' ', '\t']) else {
        return vec![line];
    };
    let mut rest = &line[first + 1..];
    let Some(second) = rest.find([' ', '\t']) else {
        return vec![&line[..first], rest];
    };
    let third = &rest[second + 1..];
    rest = &rest[..second];
    vec![&line[..first], rest, third]
}

/// Parse the complete file text.
///
/// Classic interchange: `phrase SP/TAB pinyin [SP/TAB count]`, LF or CRLF,
/// one record per line. Two-field lines mean "use the ABI default count".
/// Dictool superset extensions over the frontend: comment lines and blank
/// lines are skipped with a diagnostic-free parse, and every malformed line
/// (wrong field count, empty field, unparseable pinyin, bad/out-of-range
/// count, duplicate `(phrase, pinyin)`) is an error carrying its 1-based line
/// number.
pub fn parse(text: &str) -> Result<Vec<Record>, ParseError> {
    let mut records = Vec::new();
    let mut seen = HashSet::new();

    for (index, raw_line) in text.split('\n').enumerate() {
        let line_number = index + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed = line.trim_matches(|c: char| c == ' ' || c == '\t');
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let fields = split_classic(line);
        if fields.len() != 2 && fields.len() != 3 {
            return Err(ParseError::new(
                line_number,
                format!(
                    "expected 2 or 3 space/tab-separated fields, found {}",
                    fields.len()
                ),
            ));
        }
        let phrase = fields[0];
        let pinyin = fields[1];
        let count_text = fields.get(2).copied();

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
        let Some(key_count) = import_pinyin_key_count(pinyin) else {
            return Err(ParseError::new(
                line_number,
                format!("pinyin does not parse: {pinyin:?}"),
            ));
        };
        if key_count != phrase_len {
            return Err(ParseError::new(
                line_number,
                format!(
                    "pinyin has {key_count} key(s) but the phrase has {phrase_len} character(s)"
                ),
            ));
        }

        let count = match count_text {
            None => None,
            Some(count_text) => {
                if count_text.is_empty() {
                    return Err(ParseError::new(line_number, "count field is empty"));
                }
                let count = count_text.parse::<u64>().map_err(|_| {
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

        if !seen.insert((phrase.to_owned(), pinyin.to_owned())) {
            return Err(ParseError::new(
                line_number,
                format!("duplicate (phrase, pinyin) pair: {phrase:?} {pinyin:?}"),
            ));
        }

        records.push(Record {
            phrase: phrase.to_owned(),
            pinyin: pinyin.to_owned(),
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
        assert_eq!(split_classic("a b 3"), ["a", "b", "3"]);
        assert_eq!(split_classic("a  b 3"), ["a", "", "b 3"]);
        assert_eq!(split_classic("a b 3 extra"), ["a", "b", "3 extra"]);
        assert_eq!(split_classic(" a b 3"), ["", "a", "b 3"]);
        assert_eq!(split_classic("a b\t3"), ["a", "b", "3"]);
        assert_eq!(split_classic("a b "), ["a", "b", ""]);
        assert_eq!(split_classic("a\tb"), ["a", "b"]);
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
        let text = "你好 nihao\n你好 nihaoXYZ 4\n";
        assert_eq!(
            parse(text).unwrap(),
            vec![
                record("你好", "nihao", None, 1),
                record("你好", "nihaoXYZ", Some(4), 2),
            ]
        );
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
            .map(|record| match record.count {
                Some(count) => format!("{} {} {}\n", record.phrase, record.pinyin, count),
                None => format!("{} {}\n", record.phrase, record.pinyin),
            })
            .collect();
        assert_eq!(parse(&rendered).unwrap(), records);
    }
}
