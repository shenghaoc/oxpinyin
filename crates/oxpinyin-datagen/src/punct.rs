//! Punctuation table: `punct.table` text → token-keyed punctuation lists
//! in `PunctTableEntry::escape`'s layout (`punct_table.cpp:40-54`): a raw
//! UCS-4 stream, each punctuation's codepoints followed by a u32 zero
//! terminator (`docs/findings/bigram-punct-format-2026-09-01.md` §2).

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

/// Groups rows into token → first-seen punctuation lists. File order is
/// load-bearing (`PunctTableEntry` comments require decreasing frequency);
/// later duplicates are dropped, matching `append_punctuation`'s
/// `g_strv_contains` skip.
fn group_rows(rows: &[PunctRow]) -> BTreeMap<u32, Vec<String>> {
    let mut by_token: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for row in rows {
        let list = by_token.entry(row.token).or_default();
        if !list.iter().any(|existing| existing == &row.punct) {
            list.push(row.punct.clone());
        }
    }
    by_token
}

/// Serialises rows into `PunctTableEntry::escape`'s layout
/// (`punct_table.cpp:40-54`): token → raw UCS-4 stream, each punctuation's
/// codepoints followed by a u32 zero terminator, successive punctuations
/// concatenated.
#[must_use]
pub fn rows_to_entries(rows: &[PunctRow]) -> Entries {
    group_rows(rows)
        .into_iter()
        .map(|(token, puncts)| {
            let mut value = Vec::new();
            for punct in puncts {
                for ch in punct.chars() {
                    value.extend_from_slice(&u32::from(ch).to_le_bytes());
                }
                value.extend_from_slice(&0_u32.to_le_bytes());
            }
            (token.to_le_bytes().to_vec(), value)
        })
        .collect()
}

/// Reads `model_dir/punct.table` into rows.
///
/// # Errors
///
/// Fails when `punct.table` is missing or contains a malformed line.
pub fn read_rows(model_dir: &Path) -> Result<Vec<PunctRow>, DatagenError> {
    let table_path = model_dir.join("punct.table");
    if !table_path.is_file() {
        return Err(DatagenError::MissingModel {
            dir: model_dir.to_path_buf(),
            file: "punct.table",
        });
    }
    read_punct_file(&table_path)
}

/// Compiles `model_dir/punct.table` into the `punct.bin` rows.
///
/// # Errors
///
/// Fails when `punct.table` is missing or contains a malformed line.
pub fn compile(model_dir: &Path) -> Result<Entries, DatagenError> {
    Ok(rows_to_entries(&read_rows(model_dir)?))
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
    fn ucs4_encoding_matches_escape_layout() {
        let rows = [
            parse_punct_line("16778715 的 ， 275240").unwrap(),
            parse_punct_line("16778715 的 。 214463").unwrap(),
        ];
        let entries = rows_to_entries(&rows);
        // ， = U+FF0C, 。 = U+3002, each zero-terminated as u32.
        let mut want = Vec::new();
        want.extend_from_slice(&0xFF0C_u32.to_le_bytes());
        want.extend_from_slice(&0_u32.to_le_bytes());
        want.extend_from_slice(&0x3002_u32.to_le_bytes());
        want.extend_from_slice(&0_u32.to_le_bytes());
        assert_eq!(entries[0].1, want);
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
        // ， then 。 once each: the duplicate ， is `g_strv_contains`-skipped.
        assert_eq!(entries[0].1.len(), 16);
    }
}
