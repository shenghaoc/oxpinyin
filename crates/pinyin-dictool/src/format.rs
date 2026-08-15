//! The pinned `pinyin-dictool` user-vocabulary text format.
//!
//! Grammar is frozen in `docs/findings/dictool-format.md`; this module is
//! the executable copy and its tests pin the same cases as the spec.

use std::collections::HashSet;
use std::fmt;

use pinyin_core::{Completeness, SyllableKey};

use crate::import::MAX_COUNT;

/// One parsed import record: phrase, `'`-joined pinyin, desired count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Phrase text, non-empty and at most 15 Unicode scalar values.
    pub phrase: String,
    /// `'`-joined complete untuned full-pinyin syllables, one per phrase
    /// character.
    pub pinyin: String,
    /// Desired absolute pronunciation count, `1..=2147483647`.
    pub count: u64,
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

/// Parse the complete file text.
///
/// Line endings: `\n`-delimited, one trailing `\r` removed. Blank lines
/// (empty or only ASCII space/tab) and comment lines (first non-space/tab
/// byte is `#`) are skipped. Data lines are exactly
/// `phrase<TAB>pinyin<TAB>count`; the phrase and pinyin fields may not be
/// empty or contain TAB/CR/LF/NUL, the phrase must be 1..=15 Unicode scalar
/// values, and the pinyin must be one complete untuned [`SyllableKey`] per
/// phrase character joined by `'`. Duplicate `(phrase, pinyin)` lines are
/// rejected so the desired-count file is deterministic.
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

        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 3 {
            return Err(ParseError::new(
                line_number,
                format!("expected 3 tab-separated fields, found {}", fields.len()),
            ));
        }
        let phrase = fields[0];
        let pinyin = fields[1];
        let count_text = fields[2];

        if phrase.is_empty() {
            return Err(ParseError::new(line_number, "phrase field is empty"));
        }
        if pinyin.is_empty() {
            return Err(ParseError::new(line_number, "pinyin field is empty"));
        }
        if count_text.is_empty() {
            return Err(ParseError::new(line_number, "count field is empty"));
        }
        if phrase.contains(['\t', '\r', '\n', '\0']) {
            return Err(ParseError::new(
                line_number,
                "phrase field contains a forbidden control byte",
            ));
        }
        if pinyin.contains(['\t', '\r', '\n', '\0']) {
            return Err(ParseError::new(
                line_number,
                "pinyin field contains a forbidden control byte",
            ));
        }

        let phrase_len = phrase.chars().count();
        if phrase_len == 0 || phrase_len >= 16 {
            return Err(ParseError::new(
                line_number,
                "phrase must be 1..=15 Unicode scalar values",
            ));
        }

        let syllables: Vec<&str> = pinyin.split('\'').collect();
        if syllables.iter().any(|syllable| syllable.is_empty()) {
            return Err(ParseError::new(
                line_number,
                "pinyin has an empty syllable between apostrophes",
            ));
        }
        if syllables.len() != phrase_len {
            return Err(ParseError::new(
                line_number,
                format!(
                    "pinyin has {} syllable(s) but the phrase has {phrase_len} character(s)",
                    syllables.len()
                ),
            ));
        }
        if let Some(bad) = syllables.iter().find(|syllable| {
            SyllableKey::from_text(syllable)
                .is_none_or(|key| key.completeness() != Completeness::Complete)
        }) {
            return Err(ParseError::new(
                line_number,
                format!("unknown or incomplete pinyin syllable {bad:?}"),
            ));
        }

        let count = count_text.parse::<u64>().map_err(|_| {
            ParseError::new(
                line_number,
                format!("count is not a decimal integer: {count_text:?}"),
            )
        })?;
        if count == 0 || count > MAX_COUNT {
            return Err(ParseError::new(
                line_number,
                format!("count must be 1..={MAX_COUNT}"),
            ));
        }

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
        });
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(phrase: &str, pinyin: &str, count: u64) -> Record {
        Record {
            phrase: phrase.to_owned(),
            pinyin: pinyin.to_owned(),
            count,
        }
    }

    #[test]
    fn parses_records_and_skips_comments_and_blank_lines() {
        let text = "# user vocabulary\n\n   \n你好\tni'hao\t3\r\n世界\tshi'jie\t7\n";
        assert_eq!(
            parse(text).unwrap(),
            vec![record("你好", "ni'hao", 3), record("世界", "shi'jie", 7)]
        );
    }

    #[test]
    fn indented_comments_and_crlf_are_supported() {
        let text = "  # indented comment\n词\tci\t1\r\n";
        assert_eq!(parse(text).unwrap(), vec![record("词", "ci", 1)]);
    }

    #[test]
    fn malformed_lines_carry_their_one_based_line_number() {
        let cases = [
            ("\n\n词\tci\tx\n", 3, "count is not a decimal integer"),
            ("词\tci\t0\n", 1, "count must be 1"),
            ("词\tci\t2147483648\n", 1, "count must be 1"),
            ("词\tci\n", 1, "expected 3 tab-separated fields"),
            ("词\tci\t1\tx\n", 1, "expected 3 tab-separated fields"),
            ("\tci\t1\n", 1, "phrase field is empty"),
            ("词\t\t1\n", 1, "pinyin field is empty"),
            ("词\t''\t1\n", 1, "empty syllable"),
            ("词\t'n\t1\n", 1, "empty syllable"),
            ("词\tci'x\t1\n", 1, "pinyin has 2 syllable"),
            ("词\tn\t1\n", 1, "unknown or incomplete"),
            ("你好\tni\t1\n", 1, "pinyin has 1 syllable"),
            ("你好\tni'hao\t1\n你好\tni'hao\t2\n", 2, "duplicate"),
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
    fn records_render_tsv_lines_that_parse_back() {
        let text = "你好\tni'hao\t3\n世界\tshi'jie\t7\n";
        let records = parse(text).unwrap();
        let rendered: String = records
            .iter()
            .map(|record| format!("{}\t{}\t{}\n", record.phrase, record.pinyin, record.count))
            .collect();
        assert_eq!(parse(&rendered).unwrap(), records);
    }
}
