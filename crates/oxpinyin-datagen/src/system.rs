//! System tables: the four default-loaded phrase libraries plus the bigram,
//! compiled natively from the canonical model20 text.
//!
//! * `pinyin_index` — pinyin string (UTF-8, syllables joined by `'`) →
//!   records `{token: u32 LE, freq: u32 LE}` sorted freq-desc then
//!   token-asc; `freq` is the `.table` count column verbatim (same-pinyin
//!   duplicate rows of one token sum, as `PhraseItem::add_pronunciation`
//!   does).
//! * `phrase_index` — token `u32 LE` → phrase UTF-8.
//! * `bigram` — previous token `u32 LE` → `total: u32 LE` plus
//!   `{next_token: u32 LE, count: u32 LE}` records in file order, from the
//!   `\2-gram` section of `interpolation2.text`, exactly as
//!   `import_interpolation` stores it (`total == Σ count`).
//!
//! Schemas are frozen in `docs/findings/data-layer-export.md` and
//! `docs/findings/data-formats.md`; this module produces the same bytes the
//! retired oracle-ABI export produced for the pinned model.

use std::collections::BTreeMap;
use std::path::Path;

use crate::table::read_table_file;
use crate::{DatagenError, Entries};

/// The four default-loaded system libraries: token top byte and `.table`
/// base name (table.conf `default …_DICTIONARY` lines).
pub const SYSTEM_LIBRARIES: &[(u8, &str)] = &[
    (1, "gb_char"),
    (2, "gbk_char"),
    (3, "opengram"),
    (4, "merged"),
];

/// Pinyin keys kept in the mini fixture subset.
///
/// Chosen to cover the dictionary unit tests: single syllables, two
/// multi-syllable phrases, and the `xian` / `xi'an` pair whose distinctness
/// is exactly what the apostrophe-separated key format exists to preserve.
/// This is the reproducible recipe for `fixtures/w3/`.
pub const MINI_KEYS: &[&str] = &[
    "a",
    "ni",
    "hao",
    "ni'hao",
    "ni'men",
    "zhong",
    "guo",
    "zhong'guo",
    "xian",
    "xi'an",
];

/// Which subset of the compiled tables to emit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Subset {
    /// Every key of every table.
    Full,
    /// The [`MINI_KEYS`] restriction used by the committed `fixtures/w3/`
    /// tables: index keys are restricted, phrase and bigram entries are
    /// restricted to the tokens the restricted index references.
    MiniFixture,
}

/// The three compiled system tables, each sorted by ascending key bytes.
#[derive(Debug)]
pub struct SystemTables {
    /// pinyin string → phrase records.
    pub pinyin_index: Entries,
    /// token → phrase text.
    pub phrase_index: Entries,
    /// previous token → successor records.
    pub bigram: Entries,
}

/// Counters reported by a compile, for humans and CI logs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemStats {
    /// Rows read per library, index-aligned with [`SYSTEM_LIBRARIES`].
    pub library_rows: [u64; 4],
    /// Distinct pinyin keys.
    pub index_keys: u64,
    /// Distinct tokens in `phrase_index`.
    pub phrases: u64,
    /// Bigram entries (distinct previous tokens).
    pub bigram_entries: u64,
    /// Total bigram successor records.
    pub bigram_records: u64,
    /// `<start>`-style special-token occurrences accepted without a
    /// phrase-index lookup.
    pub special_tokens: u64,
}

/// Compiles the system tables from the extracted model20 directory.
///
/// `model_dir` must hold the four system `.table` files and
/// `interpolation2.text` (the fetch cache from `tools/model/fetch-model.sh`
/// does).
///
/// # Errors
///
/// Fails on missing files, unparsable lines, token/word contradictions
/// between `interpolation2.text` and the tables, duplicate 2-gram pairs, or
/// u32 overflow of a bigram total — never silently drops data.
pub fn compile(
    model_dir: &Path,
    subset: Subset,
) -> Result<(SystemTables, SystemStats), DatagenError> {
    let mut stats = SystemStats::default();

    // ---- dictionary from the four system .table files -------------------
    // BTreeMap over the encoded keys: iteration is ascending key-byte order.
    let mut index: BTreeMap<Vec<u8>, Vec<(u32, u64)>> = BTreeMap::new();
    let mut phrases: BTreeMap<u32, String> = BTreeMap::new();

    for (slot, &(library, name)) in SYSTEM_LIBRARIES.iter().enumerate() {
        let path = model_dir.join(format!("{name}.table"));
        if !path.is_file() {
            return Err(DatagenError::MissingModel {
                dir: model_dir.to_path_buf(),
                file: SYSTEM_LIBRARIES[slot].1,
            });
        }
        let rows = read_table_file(&path)?;
        stats.library_rows[slot] = rows.len() as u64;

        // load_text switches PhraseItems on token change, so a token's rows
        // must be consecutive in the file; a re-grouped token would start a
        // second item upstream and is refused here instead.
        let mut seen: BTreeMap<u32, ()> = BTreeMap::new();
        let mut previous: Option<u32> = None;
        for row in &rows {
            if (row.token >> 24) as u8 != library {
                return Err(DatagenError::Consistency(format!(
                    "{name}.table token {:#010x} outside library {library}",
                    row.token
                )));
            }
            if previous.is_some() && previous != Some(row.token) && seen.contains_key(&row.token) {
                return Err(DatagenError::Consistency(format!(
                    "{name}.table token {:#010x} appears in two row groups",
                    row.token
                )));
            }
            seen.insert(row.token, ());
            previous = Some(row.token);

            match phrases.entry(row.token) {
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(row.phrase.clone());
                }
                std::collections::btree_map::Entry::Occupied(e) => {
                    if e.get() != &row.phrase {
                        return Err(DatagenError::Consistency(format!(
                            "token {:#010x} names both {} and {}",
                            row.token,
                            e.get(),
                            row.phrase
                        )));
                    }
                }
            }
            let records = index.entry(row.pinyin.as_bytes().to_vec()).or_default();
            match records.iter_mut().find(|(token, _)| *token == row.token) {
                Some((_, sum)) => *sum += row.count,
                None => records.push((row.token, row.count)),
            }
        }
    }

    // ---- bigram + 1-gram validation from interpolation2.text ------------
    // Grouped by previous token; records keep file order within a group.
    let mut bigram: BTreeMap<u32, (u64, Vec<(u32, u32)>)> = BTreeMap::new();
    let text_path = model_dir.join("interpolation2.text");
    if !text_path.is_file() {
        return Err(DatagenError::MissingModel {
            dir: model_dir.to_path_buf(),
            file: "interpolation2.text",
        });
    }
    let text = std::fs::read_to_string(&text_path)?;
    let mut section = Section::Header;
    for (number, line) in text.lines().enumerate() {
        let number = number + 1;
        match line {
            "\\data model interpolation" => {
                if section != Section::Header {
                    return Err(bad_line(&text_path, number, "repeated \\data header"));
                }
            }
            "\\1-gram" => section = Section::Unigram,
            "\\2-gram" => section = Section::Bigram,
            "\\end" => break,
            _ if line.starts_with("\\item ") => {
                let rest = line["\\item ".len()..].trim();
                let fields = rest.split_whitespace().collect::<Vec<_>>();
                match section {
                    Section::Unigram => {
                        let [token, word, keyword, count] = fields[..] else {
                            return Err(bad_line(&text_path, number, "malformed \\item"));
                        };
                        if keyword != "count" {
                            return Err(bad_line(&text_path, number, "expected `count` keyword"));
                        }
                        validate_pair(&phrases, token, word, &mut stats.special_tokens)
                            .map_err(|m| bad_line(&text_path, number, &m))?;
                        count
                            .parse::<i64>()
                            .map_err(|_| bad_line(&text_path, number, "bad count"))?;
                    }
                    Section::Bigram => {
                        let [token1, word1, token2, word2, keyword, count] = fields[..] else {
                            return Err(bad_line(&text_path, number, "malformed \\item"));
                        };
                        if keyword != "count" {
                            return Err(bad_line(&text_path, number, "expected `count` keyword"));
                        }
                        validate_pair(&phrases, token1, word1, &mut stats.special_tokens)
                            .map_err(|m| bad_line(&text_path, number, &m))?;
                        validate_pair(&phrases, token2, word2, &mut stats.special_tokens)
                            .map_err(|m| bad_line(&text_path, number, &m))?;
                        let token1 = token1
                            .parse::<u32>()
                            .map_err(|_| bad_line(&text_path, number, "bad token"))?;
                        let token2 = token2
                            .parse::<u32>()
                            .map_err(|_| bad_line(&text_path, number, "bad token"))?;
                        let count = count
                            .parse::<i64>()
                            .map_err(|_| bad_line(&text_path, number, "bad count"))?;
                        if !(0..=i64::from(u32::MAX)).contains(&count) {
                            return Err(bad_line(&text_path, number, "count out of u32 range"));
                        }
                        let count = count as u32;
                        let entry = bigram.entry(token1).or_default();
                        if entry.1.iter().any(|(next, _)| *next == token2) {
                            return Err(DatagenError::Consistency(format!(
                                "duplicate 2-gram pair {token1:#010x} → {token2:#010x}"
                            )));
                        }
                        entry.1.push((token2, count));
                        entry.0 += u64::from(count);
                    }
                    Section::Header => {
                        return Err(bad_line(
                            &text_path,
                            number,
                            "\\item before a \\N-gram header",
                        ));
                    }
                }
            }
            _ => {
                return Err(bad_line(&text_path, number, "unexpected line"));
            }
        }
    }

    stats.index_keys = index.len() as u64;
    stats.phrases = phrases.len() as u64;
    stats.bigram_entries = bigram.len() as u64;
    stats.bigram_records = bigram.values().map(|(_, r)| r.len() as u64).sum();

    // ---- optional mini restriction (a strict subset of the full tables) --
    let mut keep_tokens: Option<std::collections::BTreeSet<u32>> = None;
    if subset == Subset::MiniFixture {
        index.retain(|key, _| MINI_KEYS.iter().any(|mini| key == mini.as_bytes()));
        let kept: std::collections::BTreeSet<u32> = index
            .values()
            .flat_map(|records| records.iter().map(|(token, _)| *token))
            .collect();
        phrases.retain(|token, _| kept.contains(token));
        bigram.retain(|token, _| kept.contains(token));
        keep_tokens = Some(kept);
    }
    let _ = keep_tokens;

    // ---- serialise -------------------------------------------------------
    let pinyin_index = index
        .into_iter()
        .map(|(pinyin, mut records)| {
            records.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let mut value = Vec::with_capacity(records.len() * 8);
            for (token, freq) in records {
                value.extend_from_slice(&token.to_le_bytes());
                value.extend_from_slice(&u32::try_from(freq).unwrap_or(u32::MAX).to_le_bytes());
            }
            (pinyin, value)
        })
        .collect();
    // Insertion order is part of the byte-identity recipe with the frozen
    // tables: the retired writers inserted token-keyed tables in integer
    // token order (BTreeMap iteration) and the bigram in ascending key-byte
    // order (the old converter sorted lexicographically). redb's file layout
    // depends on insertion order, so keep both exactly.
    let phrase_index: Entries = phrases
        .into_iter()
        .map(|(token, text)| (token.to_le_bytes().to_vec(), text.into_bytes()))
        .collect();
    let mut bigram_entries: Entries = bigram
        .into_iter()
        .map(|(token, (total, records))| {
            if total > u64::from(u32::MAX) {
                return Err(DatagenError::Consistency(format!(
                    "bigram total for {token:#010x} overflows u32"
                )));
            }
            let mut value = Vec::with_capacity(4 + records.len() * 8);
            value.extend_from_slice(&(total as u32).to_le_bytes());
            for (next, count) in records {
                value.extend_from_slice(&next.to_le_bytes());
                value.extend_from_slice(&count.to_le_bytes());
            }
            Ok((token.to_le_bytes().to_vec(), value))
        })
        .collect::<Result<Entries, DatagenError>>()?;
    bigram_entries.sort_by(|a, b| a.0.cmp(&b.0));

    Ok((
        SystemTables {
            pinyin_index,
            phrase_index,
            bigram: bigram_entries,
        },
        stats,
    ))
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Section {
    Header,
    Unigram,
    Bigram,
}

/// Validates one `token word` pair from `interpolation2.text` against the
/// compiled phrase index, upstream `taglib_validate_token_with_string`
/// semantics: top-byte-zero tokens are the special table (`<start>`,
/// null) and pass without a lookup.
fn validate_pair(
    phrases: &BTreeMap<u32, String>,
    token: &str,
    word: &str,
    special_tokens: &mut u64,
) -> Result<(), String> {
    let Ok(token) = token.parse::<u32>() else {
        return Err(format!("bad token {token:?}"));
    };
    if token >> 24 == 0 {
        *special_tokens += 1;
        return Ok(());
    }
    match phrases.get(&token) {
        Some(phrase) if phrase == word => Ok(()),
        Some(phrase) => Err(format!(
            "token {token:#010x} is {phrase:?} in the tables but {word:?} in the model"
        )),
        None => Err(format!(
            "token {token:#010x} ({word:?}) is missing from the system tables"
        )),
    }
}

fn bad_line(path: &Path, line: usize, message: &str) -> DatagenError {
    DatagenError::Parse {
        path: path.to_path_buf(),
        line,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_model(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-datagen-sys-{}-{name}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &std::path::PathBuf, content: &str) {
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn same_token_two_readings_sum_in_the_index() {
        let dir = tmp_model("two-readings");
        write(
            &dir.join("gb_char.table"),
            "a\t吖\t16777218\t104\nya\t吖\t16777218\t793\n",
        );
        for name in ["gbk_char.table", "opengram.table", "merged.table"] {
            write(&dir.join(name), "");
        }
        write(
            &dir.join("interpolation2.text"),
            "\\data model interpolation\n\\end\n",
        );
        let (tables, stats) = compile(&dir, Subset::Full).unwrap();
        assert_eq!(stats.library_rows, [2, 0, 0, 0]);
        assert_eq!(tables.phrase_index.len(), 1);
        assert_eq!(tables.pinyin_index.len(), 2);
        let a = &tables.pinyin_index[0];
        assert_eq!(a.0, b"a");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writer_orders_match_the_frozen_recipe() {
        let dir = tmp_model("writer-order");
        // 0x02000000 (gbk) < 0x01000001 (gb) as LE bytes but not as
        // integers, so the two orders are genuinely different sequences.
        write(&dir.join("gb_char.table"), "a\t锕\t16777217\t7\n");
        write(&dir.join("gbk_char.table"), "e\t㤅\t33554432\t3\n");
        for name in ["opengram.table", "merged.table"] {
            write(&dir.join(name), "");
        }
        write(
            &dir.join("interpolation2.text"),
            "\\data model interpolation\n\\2-gram\n\
             \\item 33554432 㤅 16777217 锕 count 2\n\\item 16777217 锕 33554432 㤅 count 1\n\\end\n",
        );
        let (tables, _) = compile(&dir, Subset::Full).unwrap();
        // Token-keyed dictionary: integer token order.
        assert_eq!(tables.phrase_index[0].0, 16_777_217u32.to_le_bytes());
        assert_eq!(tables.phrase_index[1].0, 33_554_432u32.to_le_bytes());
        // Bigram: ascending key-byte order.
        assert_eq!(tables.bigram[0].0, 33_554_432u32.to_le_bytes());
        assert_eq!(tables.bigram[1].0, 16_777_217u32.to_le_bytes());
        // Bigram value layout: total then records, total == Σ count.
        assert_eq!(&tables.bigram[0].1[..4], &2u32.to_le_bytes());
        assert_eq!(tables.bigram[0].1.len(), 4 + 8);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bigram_groups_total_and_start_token() {
        let dir = tmp_model("bigram");
        write(&dir.join("gb_char.table"), "de\t的\t16778715\t275240\n");
        for name in ["gbk_char.table", "opengram.table", "merged.table"] {
            write(&dir.join(name), "");
        }
        write(
            &dir.join("interpolation2.text"),
            "\\data model interpolation\n\
             \\1-gram\n\\item 16778715 的 count 5\n\\item 1 <start> count 99\n\\end\n",
        );
        let (tables, stats) = compile(&dir, Subset::Full).unwrap();
        assert_eq!(stats.special_tokens, 1);
        assert_eq!(tables.pinyin_index.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn model_word_contradiction_is_an_error() {
        let dir = tmp_model("contradiction");
        write(&dir.join("gb_char.table"), "de\t的\t16778715\t275240\n");
        for name in ["gbk_char.table", "opengram.table", "merged.table"] {
            write(&dir.join(name), "");
        }
        write(
            &dir.join("interpolation2.text"),
            "\\data model interpolation\n\\1-gram\n\\item 16778715 地 count 5\n\\end\n",
        );
        let err = compile(&dir, Subset::Full).unwrap_err().to_string();
        assert!(err.contains("0x010005db"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn regrouped_token_is_refused() {
        let dir = tmp_model("regroup");
        write(
            &dir.join("gb_char.table"),
            "a\t吖\t16777218\t1\nb\t吧\t16777219\t2\nya\t吖\t16777218\t3\n",
        );
        for name in ["gbk_char.table", "opengram.table", "merged.table"] {
            write(&dir.join(name), "");
        }
        write(
            &dir.join("interpolation2.text"),
            "\\data model interpolation\n\\end\n",
        );
        let err = compile(&dir, Subset::Full).unwrap_err().to_string();
        assert!(err.contains("two row groups"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mini_subset_restricts_all_three_tables() {
        let dir = tmp_model("mini");
        write(
            &dir.join("gb_char.table"),
            "a\t吖\t16777218\t104\nya\t吖\t16777218\t793\nb\t吧\t16777219\t5\n",
        );
        for name in ["gbk_char.table", "opengram.table", "merged.table"] {
            write(&dir.join(name), "");
        }
        write(
            &dir.join("interpolation2.text"),
            "\\data model interpolation\n\\end\n",
        );
        let (full, _) = compile(&dir, Subset::Full).unwrap();
        let (mini, _) = compile(&dir, Subset::MiniFixture).unwrap();
        assert_eq!(full.pinyin_index.len(), 3);
        // "a" and "ni" keys: only "a" present; "b" and "ya" are dropped.
        assert_eq!(mini.pinyin_index.len(), 1);
        assert_eq!(mini.pinyin_index[0].0, b"a");
        // token 16777218 referenced by key "a"; 16777219 dropped.
        assert_eq!(mini.phrase_index.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
