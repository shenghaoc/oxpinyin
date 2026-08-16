//! Option A addon export: `.table` text → public-ABI redb pair.
//!
//! Each addon library becomes one string-keyed pinyin index and one
//! token→text phrase index, the same schema `SystemDictionary` already
//! reads (`docs/findings/data-layer-export.md`). Tokens and counts come
//! from the `.table` columns, so they match `FacadePhraseIndex::load_text`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::write_redb;

/// Addon libraries named in `table.conf` (`docs/findings/data-formats.md` §3.2).
pub const ADDON_LIBRARIES: &[(u8, &str)] = &[
    (4, "art"),
    (5, "culture"),
    (6, "economy"),
    (7, "geology"),
    (8, "history"),
    (9, "life"),
    (10, "nature"),
    (11, "people"),
    (12, "science"),
    (13, "society"),
    (14, "sport"),
    (15, "technology"),
];

/// Pinyin keys kept in the `--mini` fixture subset (art.table).
const MINI_ART_KEYS: &[&str] = &["er'huang", "bo'cai", "ban'she"];

/// One `.table` row: pinyin, phrase, token, count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRow {
    /// `'`-joined pinyin spelling.
    pub pinyin: String,
    /// Phrase text.
    pub phrase: String,
    /// `phrase_token_t` as stored in the table.
    pub token: u32,
    /// Count column.
    pub count: u64,
}

/// Parses one `fscanf("%s %s %u %ld")` `.table` line.
#[must_use]
pub fn parse_table_line(line: &str) -> Option<TableRow> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut parts = line.split_whitespace();
    let pinyin = parts.next()?.to_owned();
    let phrase = parts.next()?.to_owned();
    let token = parts.next()?.parse().ok()?;
    let count = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(TableRow {
        pinyin,
        phrase,
        token,
        count,
    })
}

/// Reads every well-formed row from a `.table` file.
pub fn read_table_file(path: &Path) -> Result<Vec<TableRow>, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(path)?;
    Ok(text.lines().filter_map(parse_table_line).collect())
}

/// One redb `(key, value)` pair.
type RedbEntry = (Vec<u8>, Vec<u8>);

/// Serialises rows into the public-ABI pinyin/phrase index pair.
pub fn rows_to_index_entries(rows: &[TableRow]) -> (Vec<RedbEntry>, Vec<RedbEntry>) {
    let mut index: BTreeMap<String, BTreeMap<u32, u64>> = BTreeMap::new();
    let mut phrases: BTreeMap<u32, String> = BTreeMap::new();
    for row in rows {
        phrases
            .entry(row.token)
            .or_insert_with(|| row.phrase.clone());
        *index
            .entry(row.pinyin.clone())
            .or_default()
            .entry(row.token)
            .or_default() += row.count;
    }

    let index_entries = index
        .iter()
        .map(|(pinyin, records)| {
            let mut ordered: Vec<(u32, u64)> = records.iter().map(|(t, f)| (*t, *f)).collect();
            ordered.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let mut value = Vec::with_capacity(ordered.len() * 8);
            for (token, freq) in ordered {
                value.extend_from_slice(&token.to_le_bytes());
                value.extend_from_slice(&u32::try_from(freq).unwrap_or(u32::MAX).to_le_bytes());
            }
            (pinyin.as_bytes().to_vec(), value)
        })
        .collect();
    let phrase_entries = phrases
        .iter()
        .map(|(token, text)| (token.to_le_bytes().to_vec(), text.as_bytes().to_vec()))
        .collect();
    (index_entries, phrase_entries)
}

/// Exports every addon `.table` in `table_dir` into `out_dir`.
pub fn run(table_dir: &Path, out_dir: &Path, mini: bool) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(out_dir)?;
    let libraries = if mini {
        &ADDON_LIBRARIES[..1]
    } else {
        ADDON_LIBRARIES
    };
    for &(index, name) in libraries {
        let table_path = table_dir.join(format!("{name}.table"));
        if !table_path.is_file() {
            return Err(format!("missing addon table {}", table_path.display()).into());
        }
        let mut rows = read_table_file(&table_path)?;
        let nibble = (index as u32) << 24;
        rows.retain(|row| (row.token & 0x0F00_0000) == nibble);
        if mini {
            rows.retain(|row| MINI_ART_KEYS.contains(&row.pinyin.as_str()));
        }
        eprintln!(
            "  addon {index} {name}: {} rows from {}",
            rows.len(),
            table_path.display()
        );
        let (index_entries, phrase_entries) = rows_to_index_entries(&rows);
        write_redb(
            &out_dir.join(format!("addon_{index}_pinyin_index.redb")),
            &index_entries,
        )?;
        write_redb(
            &out_dir.join(format!("addon_{index}_phrase_index.redb")),
            &phrase_entries,
        )?;
    }
    eprintln!("addon export complete → {}", out_dir.display());
    Ok(())
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
    fn same_token_two_readings_sum_in_the_index() {
        let rows = [
            parse_table_line("tiao'de 调的 67108885 60").unwrap(),
            parse_table_line("diao'de 调的 67108885 39").unwrap(),
        ];
        let (index, phrases) = rows_to_index_entries(&rows);
        assert_eq!(phrases.len(), 1);
        assert_eq!(index.len(), 2);
    }
}
