//! Addon tables: the twelve topic `.table` files.
//!
//! System and addon libraries live in **separate** index files upstream
//! (`phrase_index.bin` vs `addon_phrase_index.bin`, two
//! `FacadePhraseIndex` instances), which is why the addon library numbers
//! restart at 4 and their token ranges intentionally overlap the merged
//! library's. Tokens and counts come from the `.table` columns, so they
//! match `FacadePhraseIndex::load_text` exactly.
//!
//! Two serializers mirror the system split:
//!
//! * **native** ([`compile`]) — the frozen oxpinyin per-library index pair
//!   for the redb/LMDB producers and the `.redb` fixtures.
//! * **libpinyin** ([`compile_libpinyin`]) — upstream's addon run of
//!   `gen_binary_files` + `gen_unigram`: one merged `addon_pinyin_index` /
//!   `addon_phrase_index` DBM pair over all twelve tables, one chunk file
//!   per library (`art.bin` … `technology.bin`), and `gen_unigram`'s +1 on
//!   every addon token (addon tokens carry no `\1-gram` counts in the
//!   pinned model — verified: every `\1-gram` token is nibble 1-3).

use std::collections::BTreeMap;
use std::path::Path;

use crate::chunks::ChunkItem;
use crate::libpinyin::ParsedRow;
use crate::system::{LibraryModel, read_libraries};
use crate::table::{TableRow, read_table_file};
use crate::{DatagenError, Entries, chunks, libpinyin};

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
/// phrase index)` in the frozen native schema.
#[derive(Debug)]
pub struct AddonTables {
    /// Library index from [`ADDON_LIBRARIES`].
    pub index: u8,
    /// pinyin string → phrase records (`{token u32 LE, freq u32 LE}`).
    pub pinyin_index: Entries,
    /// token → phrase text.
    pub phrase_index: Entries,
}

/// The libpinyin-schema output of the addon compile: upstream's second
/// `generate_binary_files` run (`ADDON_SYSTEM_PINYIN_INDEX` /
/// `ADDON_SYSTEM_PHRASE_INDEX`) plus `gen_unigram`.
#[derive(Debug)]
pub struct AddonLibpinyin {
    /// Per-library chunk files: `(file name, bytes)` — `art.bin` …
    /// `technology.bin`.
    pub chunks: Vec<(String, Vec<u8>)>,
    /// `addon_pinyin_index.bin` rows — one merged DBM over all twelve
    /// tables (both keyspaces, prefix markers).
    pub pinyin_index: Entries,
    /// `addon_phrase_index.bin` rows — one merged DBM over all twelve
    /// tables (UCS-4 keys, prefix markers).
    pub phrase_index: Entries,
}

/// Compiles every addon `.table` in `model_dir` in the frozen native
/// schema (one index pair per library).
///
/// # Errors
///
/// Fails on a missing addon table or a row whose token falls outside the
/// library's range.
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

/// Compiles the addon tables in the libpinyin byte schemas: one merged
/// `addon_pinyin_index` / `addon_phrase_index` DBM pair and one chunk file
/// per library, all with `gen_unigram`'s +1 applied (no `\1-gram` counts
/// exist for addon tokens in the pinned model).
///
/// # Errors
///
/// Same read/validation failures as [`compile`], plus chunk serialization
/// failures.
pub fn compile_libpinyin(model_dir: &Path, subset: Subset) -> Result<AddonLibpinyin, DatagenError> {
    let libraries: &[(u8, &str)] = match subset {
        Subset::Full => ADDON_LIBRARIES,
        Subset::MiniFixture => &ADDON_LIBRARIES[..1],
    };
    // The native raw index is not needed here, but the reader requires the
    // accumulator; addon rows never leak into the system tables upstream
    // and never will here either.
    let mut raw_index = BTreeMap::new();
    let mut libs = read_libraries(model_dir, libraries, &mut raw_index)?.0;

    if subset == Subset::MiniFixture {
        // Restrict to the mini recipe: keep only tokens whose phrase
        // spellings match MINI_ART_KEYS. The semantic model no longer
        // carries the raw spelling strings, so re-read art.table for the
        // restriction set (it was just read by read_libraries anyway).
        let path = model_dir.join("art.table");
        let rows = read_table_file(&path)?;
        let keep: std::collections::BTreeSet<u32> = rows
            .iter()
            .filter(|row| MINI_ART_KEYS.contains(&row.pinyin.as_str()))
            .map(|row| row.token)
            .collect();
        for lib in &mut libs {
            lib.items.retain(|token, _| keep.contains(token));
            lib.parsed_rows.retain(|row| keep.contains(&row.token));
        }
    }

    build_libpinyin(&libs)
}

/// The shared addon serialization: chunks + merged index DBM streams.
fn build_libpinyin(libs: &[LibraryModel]) -> Result<AddonLibpinyin, DatagenError> {
    let mut chunk_files = Vec::with_capacity(libs.len());
    for lib in libs {
        let items: Vec<(u32, ChunkItem)> = lib
            .items
            .iter()
            .map(|(token, item)| {
                // gen_unigram adds 1 to every addon token; the pinned
                // model carries no \1-gram counts for addon nibbles.
                (
                    token & chunks::PHRASE_MASK,
                    ChunkItem {
                        phrase: item.ucs4.clone(),
                        unigram: 1,
                        prons: item
                            .prons
                            .iter()
                            .map(|(keys, freq)| {
                                (
                                    keys.iter().map(|k| k.to_packed()).collect(),
                                    u32::try_from(*freq).unwrap_or(u32::MAX),
                                )
                            })
                            .collect(),
                    },
                )
            })
            .collect();
        let bytes = chunks::build_chunk(&items)?;
        chunk_files.push((format!("{}.bin", lib.name), bytes));
    }

    let parsed_rows: Vec<ParsedRow> = libs
        .iter()
        .flat_map(|lib| lib.parsed_rows.iter().cloned())
        .collect();
    let pinyin_index = libpinyin::pinyin_index_entries(&parsed_rows);
    let phrase_rows: Vec<(Vec<u32>, u32)> = libs
        .iter()
        .flat_map(|lib| {
            lib.items
                .iter()
                .map(|(token, item)| (item.ucs4.clone(), *token))
        })
        .collect();
    let phrase_index = libpinyin::phrase_index_entries(&phrase_rows);

    Ok(AddonLibpinyin {
        chunks: chunk_files,
        pinyin_index,
        phrase_index,
    })
}

/// Serialises rows into the native index pair, writer order preserved.
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

    #[test]
    fn libpinyin_addon_chunks_carry_unigram_one() {
        let dir = std::env::temp_dir().join(format!("oxpinyin-addon-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("art.table"), "er'huang\t二簧\t67108865\t100\n").unwrap();
        for &(_, name) in &ADDON_LIBRARIES[1..] {
            std::fs::write(dir.join(format!("{name}.table")), "").unwrap();
        }
        let out = compile_libpinyin(&dir, Subset::Full).unwrap();
        assert_eq!(out.chunks.len(), 12);
        assert_eq!(out.chunks[0].0, "art.bin");
        let path = dir.join("art.bin");
        std::fs::write(&path, &out.chunks[0].1).unwrap();
        let lib = oxpinyin_data::phrase_library::PhraseLibrary::open(&path).unwrap();
        assert_eq!(lib.total_freq(), 1);
        assert_eq!(lib.item(67_108_865).unwrap().unigram(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
