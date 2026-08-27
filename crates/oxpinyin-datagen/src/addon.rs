//! Addon tables: the twelve topic `.table` files, each compiled into its
//! own pinyin/phrase index pair.
//!
//! System and addon libraries live in **separate** index files upstream
//! (`phrase_index.bin` vs `addon_phrase_index.bin`, two
//! `FacadePhraseIndex` instances), which is why the addon library numbers
//! restart at 4 and their token ranges intentionally overlap the merged
//! library's. Tokens and counts come from the `.table` columns, so they
//! match `FacadePhraseIndex::load_text` exactly — this is the same
//! compilation the retired `oxpinyin-migrate export-addon` performed
//! (no oracle involved), restored verbatim from that code.

use std::collections::BTreeMap;
use std::path::Path;

use crate::table::{TableRow, read_table_file};
use crate::{DatagenError, Entries};

/// Addon libraries named in `table.conf` (`docs/findings/data-formats.md`
/// §3.2): library index and `.table` base name.
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

/// Pinyin keys kept in the mini fixture subset (art.table).
const MINI_ART_KEYS: &[&str] = &["er'huang", "bo'cai", "ban'she"];

/// Which addon libraries to compile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Subset {
    /// Every addon library.
    Full,
    /// Only art, restricted to [`MINI_ART_KEYS`] — the reproducible recipe
    /// for `fixtures/w3/addon_4_*.redb`.
    MiniFixture,
}

/// One addon library's compiled index pair: `(library index, pinyin index,
/// phrase index)`.
#[derive(Debug)]
pub struct AddonTables {
    /// Library index from [`ADDON_LIBRARIES`].
    pub index: u8,
    /// pinyin string → phrase records (`{token u32 LE, freq u32 LE}`).
    pub pinyin_index: Entries,
    /// token → phrase text.
    pub phrase_index: Entries,
}

/// Compiles the selected addon `.table` files from `model_dir`.
///
/// `Subset::Full` compiles all configured addon libraries. `Subset::MiniFixture`
/// compiles only the fixture library and retains its configured pinyin keys.
///
/// # Errors
///
/// Returns an error if an addon table is missing, cannot be read, or contains
/// a token outside its library's range.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// let tables = compile(Path::new("models"), Subset::MiniFixture)?;
/// # Ok::<(), DatagenError>(())
/// ```
pub fn compile(model_dir: &Path, subset: Subset) -> Result<Vec<AddonTables>, DatagenError> {
    let libraries: &[(u8, &str)] = match subset {
        Subset::Full => ADDON_LIBRARIES,
        Subset::MiniFixture => &ADDON_LIBRARIES[..1],
    };
    let mut out = Vec::with_capacity(libraries.len());
    for &(index, name) in libraries {
        let path = model_dir.join(format!("{name}.table"));
        if !path.is_file() {
            return Err(DatagenError::MissingModel {
                dir: model_dir.to_path_buf(),
                file: "addon table",
            });
        }
        let rows = read_table_file(&path)?;
        for row in &rows {
            if (row.token >> 24) as u8 != index {
                return Err(DatagenError::Consistency(format!(
                    "{} row {row:#?} outside library {index}",
                    path.display()
                )));
            }
        }
        let mut rows = rows;
        if subset == Subset::MiniFixture {
            rows.retain(|row| MINI_ART_KEYS.contains(&row.pinyin.as_str()));
        }
        let (pinyin_index, phrase_index) = rows_to_index_entries(&rows);
        out.push(AddonTables {
            index,
            pinyin_index,
            phrase_index,
        });
    }
    Ok(out)
}

/// Converts table rows into pinyin and phrase index entries.
///
/// Duplicate token readings are aggregated by pinyin and token. Token records
/// are ordered by descending frequency and then ascending token, while the
/// first phrase associated with each token is retained. Frequencies exceeding
/// `u32::MAX` are saturated.
///
/// # Examples
///
/// ```
/// let rows = vec![TableRow {
///     pinyin: "ni".to_owned(),
///     token: 1,
///     count: 2,
///     phrase: "你".to_owned(),
/// }];
///
/// let (pinyin_index, phrase_index) = rows_to_index_entries(&rows);
///
/// assert_eq!(pinyin_index.len(), 1);
/// assert_eq!(phrase_index.len(), 1);
/// ```
#[must_use]
pub fn rows_to_index_entries(rows: &[TableRow]) -> (Entries, Entries) {
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
        .into_iter()
        .map(|(pinyin, records)| {
            let mut ordered: Vec<(u32, u64)> = records.iter().map(|(t, f)| (*t, *f)).collect();
            ordered.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let mut value = Vec::with_capacity(ordered.len() * 8);
            for (token, freq) in ordered {
                value.extend_from_slice(&token.to_le_bytes());
                value.extend_from_slice(&u32::try_from(freq).unwrap_or(u32::MAX).to_le_bytes());
            }
            (pinyin.into_bytes(), value)
        })
        .collect();
    let phrase_entries = phrases
        .into_iter()
        .map(|(token, text)| (token.to_le_bytes().to_vec(), text.into_bytes()))
        .collect();
    (index_entries, phrase_entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_token_two_readings_sum_in_the_index() {
        let rows = [
            crate::table::parse_table_line("tiao'de 调的 67108885 60").unwrap(),
            crate::table::parse_table_line("diao'de 调的 67108885 39").unwrap(),
        ];
        let (index, phrases) = rows_to_index_entries(&rows);
        assert_eq!(phrases.len(), 1);
        assert_eq!(index.len(), 2);
    }
}
