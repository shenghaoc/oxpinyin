//! Punctuation table: `punct.table` text → token-keyed punctuation lists.
//!
//! The raw `punct.bin` convert is not consumable — `HashDBM` iteration sees
//! one 6-byte key, while upstream `PunctTable::attach` opens the same file
//! as `TreeDBM` and reads 272 token keys
//! (`docs/findings/prediction-punct.md`). This compilation is the same
//! one the retired `oxpinyin-migrate export-punct` performed (no oracle
//! involved), restored verbatim from that code.

use std::collections::BTreeMap;
use std::path::Path;

use crate::{DatagenError, Entries};

/// One `punct.table` row: token, phrase, punctuation, frequency.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PunctRow {
    /// `phrase_token_t` the punctuation is predicted after.
    pub token: u32,
    /// Phrase text the token names (ignored at lookup time).
    pub phrase: String,
    /// Punctuation string, already in table-file order (decreasing frequency).
    pub punct: String,
    /// Count column; used only to document source order.
    pub count: u64,
}

/// Parses one `fscanf("%u %s %s %ld")` `punct.table` line.
#[must_use]
pub fn parse_punct_line(line: &str) -> Option<PunctRow> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut parts = line.split_whitespace();
    let token = parts.next()?.parse().ok()?;
    let phrase = parts.next()?.to_owned();
    let punct = parts.next()?.to_owned();
    let count = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(PunctRow {
        token,
        phrase,
        punct,
        count,
    })
}

/// Reads every data row from a `punct.table` file.
///
/// Blank lines and `#` comments are skipped. Any other unparsable line is
/// an error, identified by 1-based line number and content.
///
/// # Errors
///
/// Returns [`DatagenError::Io`] on read failure or [`DatagenError::Parse`]
/// on a malformed line.
pub fn read_punct_file(path: &Path) -> Result<Vec<PunctRow>, DatagenError> {
    let text = std::fs::read_to_string(path)?;
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(row) = parse_punct_line(trimmed) else {
            return Err(DatagenError::Parse {
                path: path.to_path_buf(),
                line: index + 1,
                message: format!("malformed punct.table line: {trimmed}"),
            });
        };
        rows.push(row);
    }
    Ok(rows)
}

/// Serialises rows into token → NUL-terminated UTF-8 punctuation lists.
///
/// File order is load-bearing (`PunctTableEntry` comments require decreasing
/// frequency). First-seen punctuation for a token wins; later duplicates are
/// dropped, matching `append_punctuation`'s `g_strv_contains` skip.
#[must_use]
pub fn rows_to_entries(rows: &[PunctRow]) -> Entries {
    let mut by_token: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for row in rows {
        let list = by_token.entry(row.token).or_default();
        if !list.iter().any(|existing| existing == &row.punct) {
            list.push(row.punct.clone());
        }
    }
    by_token
        .into_iter()
        .map(|(token, puncts)| {
            let mut value = Vec::new();
            for punct in puncts {
                value.extend_from_slice(punct.as_bytes());
                value.push(0);
            }
            (token.to_le_bytes().to_vec(), value)
        })
        .collect()
}

/// Compiles `model_dir/punct.table` into the punctuation table.
///
/// # Errors
///
/// Fails when `punct.table` is missing or contains a malformed line.
pub fn compile(model_dir: &Path) -> Result<Entries, DatagenError> {
    let table_path = model_dir.join("punct.table");
    if !table_path.is_file() {
        return Err(DatagenError::MissingModel {
            dir: model_dir.to_path_buf(),
            file: "punct.table",
        });
    }
    let rows = read_punct_file(&table_path)?;
    Ok(rows_to_entries(&rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_line_is_four_whitespace_fields() {
        let row = parse_punct_line("16778715 的 ， 275240").unwrap();
        assert_eq!(row.token, 16_778_715);
        assert_eq!(row.phrase, "的");
        assert_eq!(row.punct, "，");
        assert_eq!(row.count, 275_240);
    }

    #[test]
    fn token_keeps_first_seen_punct_order() {
        let rows = [
            parse_punct_line("16778715 的 ， 275240").unwrap(),
            parse_punct_line("16778715 的 。 214463").unwrap(),
            parse_punct_line("16778715 的 ， 1").unwrap(),
        ];
        let entries = rows_to_entries(&rows);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, 16_778_715u32.to_le_bytes());
        assert_eq!(entries[0].1, b"\xef\xbc\x8c\x00\xe3\x80\x82\x00");
    }
}
