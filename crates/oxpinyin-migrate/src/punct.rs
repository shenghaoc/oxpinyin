//! Option A punctuation export: `punct.table` text → public-ABI redb.
//!
//! Same pattern as [`crate::addon`]: read the model-archive text table and
//! write a string-valued redb that `oxpinyin-data` already knows how to
//! open. The raw `punct.bin` convert is not consumable — HashDBM iteration
//! sees one 6-byte key, while upstream `PunctTable::attach` opens the same
//! file as TreeDBM and reads 272 token keys
//! (`docs/findings/prediction-punct.md`).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::write_redb;

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
pub fn read_punct_file(path: &Path) -> Result<Vec<PunctRow>, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(path)?;
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(row) = parse_punct_line(trimmed) else {
            return Err(format!("malformed punct.table line {}: {trimmed}", index + 1).into());
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
pub fn rows_to_entries(rows: &[PunctRow]) -> Vec<(Vec<u8>, Vec<u8>)> {
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

/// Exports `table_dir/punct.table` as `out_dir/punct.redb`.
pub fn run(table_dir: &Path, out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(out_dir)?;
    let table_path = table_dir.join("punct.table");
    if !table_path.is_file() {
        return Err(format!("missing punctuation table {}", table_path.display()).into());
    }
    let rows = read_punct_file(&table_path)?;
    let entries = rows_to_entries(&rows);
    eprintln!(
        "  punct: {} rows / {} tokens from {}",
        rows.len(),
        entries.len(),
        table_path.display()
    );
    write_redb(&out_dir.join("punct.redb"), &entries)?;
    eprintln!("punct export complete → {}", out_dir.display());
    Ok(())
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

    #[test]
    fn read_punct_file_skips_blank_and_comment_and_rejects_malformed() {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-punct-parse-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.table");
        std::fs::write(&good, "# comment\n\n16778715 的 ， 275240\n").unwrap();
        assert_eq!(read_punct_file(&good).unwrap().len(), 1);

        let bad = dir.join("punct.table");
        std::fs::write(&bad, "# comment\nnot-a-row\n").unwrap();
        let err = read_punct_file(&bad).unwrap_err().to_string();
        assert!(err.contains("line 2"), "{err}");
        assert!(err.contains("not-a-row"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
