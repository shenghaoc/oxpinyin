//! Addon tables: the twelve topic `.table` files.
//!
//! System and addon libraries live in **separate** index files upstream
//! (`phrase_index.bin` vs `addon_phrase_index.bin`, two
//! `FacadePhraseIndex` instances), which is why the addon library numbers
//! restart at 4 and their token ranges intentionally overlap the merged
//! library's. Tokens and counts come from the `.table` columns, so they
//! match `FacadePhraseIndex::load_text` exactly.
//!
//! [`compile`] is upstream's addon run of `gen_binary_files` +
//! `gen_unigram`: one merged `addon_pinyin_index` / `addon_phrase_index`
//! DBM pair over all twelve tables, one chunk file per library (`art.bin`
//! … `technology.bin`), and `gen_unigram`'s +1 on every addon token
//! (addon tokens carry no `\1-gram` counts in the pinned model —
//! verified: every `\1-gram` token is nibble 1-3).

use std::collections::BTreeMap;
use std::path::Path;

use crate::chunks::ChunkItem;
use crate::libpinyin::ParsedRow;
use crate::system::{LibraryModel, read_libraries};
use crate::table::read_table_file;
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

/// The output of the addon compile: upstream's second
/// `generate_binary_files` run (`ADDON_SYSTEM_PINYIN_INDEX` /
/// `ADDON_SYSTEM_PHRASE_INDEX`) plus `gen_unigram`.
#[derive(Debug)]
pub struct AddonOutput {
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

/// Compiles the addon tables: one merged `addon_pinyin_index` /
/// `addon_phrase_index` DBM pair and one chunk file per library, all with
/// `gen_unigram`'s +1 applied (no `\1-gram` counts exist for addon tokens
/// in the pinned model).
///
/// # Errors
///
/// Fails on a missing addon table, a row whose token falls outside the
/// library's range, or a chunk serialization failure.
pub fn compile(model_dir: &Path, subset: Subset) -> Result<AddonOutput, DatagenError> {
    let libraries: &[(u8, &str)] = match subset {
        Subset::Full => ADDON_LIBRARIES,
        Subset::MiniFixture => &ADDON_LIBRARIES[..1],
    };
    // The spelling selector is unused here (the mini recipe below reads
    // the table again); addon rows never leak into the system tables
    // upstream and never will here either.
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

    build(&libs)
}

/// The shared addon serialization: chunks + merged index DBM streams.
fn build(libs: &[LibraryModel]) -> Result<AddonOutput, DatagenError> {
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

    Ok(AddonOutput {
        chunks: chunk_files,
        pinyin_index,
        phrase_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addon_chunks_carry_unigram_one() {
        let dir = std::env::temp_dir().join(format!("oxpinyin-addon-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("art.table"), "er'huang\t二簧\t67108865\t100\n").unwrap();
        for &(_, name) in &ADDON_LIBRARIES[1..] {
            std::fs::write(dir.join(format!("{name}.table")), "").unwrap();
        }
        let out = compile(&dir, Subset::Full).unwrap();
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
