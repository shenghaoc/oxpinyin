//! `.table` text parsing — one row format shared by every library.
//!
//! The files in `model20.text.tar.gz` are line-oriented with four
//! whitespace-separated fields, read upstream by
//! `fscanf("%255s %255s %u %ld")`; blank lines and short lines are skipped
//! there, and skipped here. Tokens are the library's `phrase_token_t` and
//! are taken verbatim from the column — `FacadePhraseIndex::load_text`
//! never renumbers them (`PHRASE_INDEX_LIBRARY_INDEX(token) ==
//! phrase_index` is asserted upstream).

use std::fs;
use std::path::Path;

use crate::DatagenError;

/// One `.table` row: pinyin, phrase, token, count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRow {
    /// `'`-joined pinyin spelling, already canonical.
    pub pinyin: String,
    /// Phrase text.
    pub phrase: String,
    /// `phrase_token_t` as stored in the table.
    pub token: u32,
    /// Count column.
    pub count: u64,
}

/// Parses a valid `.table` line into a [`TableRow`].
///
/// Blank, comment, malformed, negative-count, and incorrectly fielded lines are skipped.
///
/// # Examples
///
/// ```
/// let row = parse_table_line("ni hao 42 10").unwrap();
/// assert_eq!(row.pinyin, "ni");
/// assert_eq!(row.phrase, "hao");
/// assert_eq!(row.token, 42);
/// assert_eq!(row.count, 10);
/// ```
///
/// # Returns
///
/// `Some(TableRow)` for a valid four-field line, or `None` otherwise.
#[must_use]
pub fn parse_table_line(line: &str) -> Option<TableRow> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut parts = line.split_whitespace();
    let pinyin = parts.next()?;
    let phrase = parts.next()?;
    let token = parts.next()?.parse().ok()?;
    let count = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    if count < 0 {
        return None;
    }
    Some(TableRow {
        pinyin: pinyin.to_owned(),
        phrase: phrase.to_owned(),
        token,
        count: count as u64,
    })
}

/// Reads all valid rows from a `.table` file.
///
/// Blank, comment, malformed, and negative-count lines are skipped. Lines with
/// more than four fields produce a parse error.
///
/// # Errors
///
/// Returns [`DatagenError::Io`] when the file cannot be read, or
/// [`DatagenError::Parse`] when a non-comment line contains more than four
/// fields.
///
/// # Examples
///
/// ```
/// let path = std::env::temp_dir().join(format!("example-{}.table", std::process::id()));
/// std::fs::write(&path, "ni3 你 1 2\n").unwrap();
///
/// let rows = read_table_file(&path).unwrap();
/// assert_eq!(rows.len(), 1);
/// assert_eq!(rows[0].phrase, "你");
///
/// std::fs::remove_file(path).unwrap();
/// ```
pub fn read_table_file(path: &Path) -> Result<Vec<TableRow>, DatagenError> {
    let text = fs::read_to_string(path)?;
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Upstream's fscanf reads four fields and would re-tokenize a
        // fifth as the next row's pinyin; a strict compiler refuses the
        // line instead (no pinned row carries extra fields). The check
        // applies to candidate rows only — comments are skipped above.
        if trimmed.split_whitespace().count() > 4 {
            return Err(DatagenError::Parse {
                path: path.to_path_buf(),
                line: index + 1,
                message: format!("more than four fields: {line}"),
            });
        }
        if let Some(row) = parse_table_line(line) {
            rows.push(row);
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_line_is_four_whitespace_fields() {
        let row = parse_table_line("er'huang\t二簧\t67108865\t100").unwrap();
        assert_eq!(row.pinyin, "er'huang");
        assert_eq!(row.phrase, "二簧");
        assert_eq!(row.token, 67_108_865);
        assert_eq!(row.count, 100);
        assert_eq!((row.token >> 24) as u8, 4);
    }

    #[test]
    fn blank_comment_short_and_negative_lines_are_skipped() {
        assert!(parse_table_line("").is_none());
        assert!(parse_table_line("   ").is_none());
        assert!(parse_table_line("# comment").is_none());
        assert!(parse_table_line("a 锕").is_none());
        assert!(parse_table_line("a 锕 16777217").is_none());
        assert!(parse_table_line("a 锕 16777217 -3").is_none());
        assert!(parse_table_line("a 锕 16777217 7 trailing").is_none());
        assert!(parse_table_line("a 锕 not-a-token 7").is_none());
    }
}
