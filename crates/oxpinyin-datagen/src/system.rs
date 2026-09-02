//! System tables: the four default-loaded phrase libraries plus the bigram,
//! compiled natively from the canonical model20 text.
//!
//! The read pass ([`read_semantic`]) parses the `.table` files and
//! `interpolation2.text` into a [`SemanticModel`] — phrases with their
//! pronunciations and unigram counts, parsed pinyin rows (tones kept), and
//! the bigram groups. [`compile`] serialises it into libpinyin's own
//! formats — the byte-level output of upstream's `gen_binary_files` +
//! `import_interpolation` + `gen_unigram` chain: per-library chunk files,
//! the two index DBMs' row streams, and the `bigram.db` row stream. Every
//! backend writes the same rows; on Kyoto Cabinet and tkrzw they are the
//! files a libpinyin install of that DBM ships.
//!
//! Formats: `docs/findings/pinyin-dbm-format-2026-09-01.md`,
//! `phrase-dbm-format-2026-09-01.md`, `bigram-punct-format-2026-09-01.md`,
//! `datagen-compat-2026-09-01.md`.

use std::collections::BTreeMap;
use std::path::Path;

use oxpinyin_core::ChewingKey;

use crate::chunks::ChunkItem;
use crate::libpinyin::ParsedRow;
use crate::table::read_table_file;
use crate::{DatagenError, Entries, chunks, libpinyin};

/// Pinyin spelling (UTF-8 bytes, as written in the `.table`) → the tokens
/// carrying it — the selector [`Subset::MiniFixture`] restricts by.
type SpellingIndex = BTreeMap<Vec<u8>, Vec<u32>>;

/// Previous token → `(total, successor records)` in token order.
type BigramGroups = BTreeMap<u32, (u64, Vec<(u32, u32)>)>;

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

/// The output of one system compile — the byte-level product of
/// upstream's `gen_binary_files` + `import_interpolation` + `gen_unigram`
/// chain (`docs/findings/datagen-compat-2026-09-01.md`).
#[derive(Debug)]
pub struct SystemOutput {
    /// Per-library chunk files: `(file name, bytes)` — `gb_char.bin`,
    /// `gbk_char.bin`, `opengram.bin`, `merged.bin`.
    pub chunks: Vec<(String, Vec<u8>)>,
    /// `pinyin_index.bin` rows (both keyspaces, prefix markers included).
    pub pinyin_index: Entries,
    /// `phrase_index.bin` rows (UCS-4 keys, token-ascending values,
    /// prefix markers included).
    pub phrase_index: Entries,
    /// `bigram.db` rows: token LE → total + token-ascending records.
    pub bigram: Entries,
}

/// Counters reported by a compile, for humans and CI logs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemStats {
    /// Rows read per library, index-aligned with [`SYSTEM_LIBRARIES`].
    pub library_rows: [u64; 4],
    /// Distinct pinyin spellings across the `.table` rows.
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

/// One phrase's semantic record — the shared input of both serializers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhraseModel {
    /// The phrase text.
    pub text: String,
    /// The phrase's UCS-4 code points.
    pub ucs4: Vec<u32>,
    /// Pronunciations in `.table` row order: parsed keys (with their
    /// tones) and the summed count — `PhraseItem::add_pronunciation` sums
    /// duplicate exact key sequences.
    pub prons: Vec<(Vec<ChewingKey>, u64)>,
}

/// One library's parsed semantic content.
#[derive(Debug)]
pub struct LibraryModel {
    /// Token top byte (`PHRASE_INDEX_LIBRARY_INDEX`).
    pub nibble: u8,
    /// `.table` base name (`gb_char` etc.).
    pub name: &'static str,
    /// token → phrase, ascending token (`.table` file order).
    pub items: BTreeMap<u32, PhraseModel>,
    /// Every parseable row's keys, in file order — the chewing index's
    /// input (rows upstream skips are already excluded).
    pub parsed_rows: Vec<ParsedRow>,
}

/// The full semantic content of a model20 read.
#[derive(Debug)]
pub struct SemanticModel {
    /// The four system libraries, in [`SYSTEM_LIBRARIES`] order.
    pub libraries: Vec<LibraryModel>,
    /// Spelling → tokens, the mini-subset selector.
    pub raw_index: SpellingIndex,
    /// `\1-gram` counts per token.
    pub unigrams: BTreeMap<u32, u64>,
    /// `\2-gram` groups: token1 → `(total, records)`.
    pub bigram: BigramGroups,
    /// Read counters.
    pub stats: SystemStats,
}

/// Parses one `.table` row's pinyin spelling into syllable keys, upstream
/// `PinyinDirectParser2::parse` with `USE_TONE`: split on `'` and space;
/// a trailing `1`-`5` character is the tone digit; the remaining spelling
/// resolves through the content table. `None` when any syllable fails
/// (`parse_one_key` false → `parse` returns short, the row is skipped).
fn parse_pinyin_keys(pinyin: &str) -> Option<Vec<ChewingKey>> {
    let mut keys = Vec::new();
    for syllable in pinyin.split(['\'', ' ']) {
        if syllable.is_empty() {
            return None;
        }
        let mut text = syllable;
        let mut tone = 0_u8;
        if let Some(last @ b'1'..=b'5') = syllable.as_bytes().last() {
            tone = last - b'0';
            text = &syllable[..syllable.len() - 1];
        }
        let key = ChewingKey::from_pinyin(text)?.with_tone(tone);
        keys.push(key);
    }
    Some(keys)
}

/// Reads a list of `.table` files into per-library semantic models:
/// phrase records, parsed pinyin rows, and the spelling → tokens selector.
/// Shared by the system compile ([`SYSTEM_LIBRARIES`]) and the addon
/// compile ([`crate::addon`]).
///
/// # Errors
///
/// Fails on missing `.table` files, tokens outside their library, a token
/// re-grouped across the file, or a token naming two different phrases.
pub fn read_libraries(
    model_dir: &Path,
    libraries: &[(u8, &'static str)],
    raw_index: &mut SpellingIndex,
) -> Result<(Vec<LibraryModel>, Vec<u64>), DatagenError> {
    let mut out = Vec::with_capacity(libraries.len());
    let mut row_counts = Vec::with_capacity(libraries.len());
    for &(library, name) in libraries {
        let path = model_dir.join(format!("{name}.table"));
        if !path.is_file() {
            return Err(DatagenError::MissingModel {
                dir: model_dir.to_path_buf(),
                file: name,
            });
        }
        let rows = read_table_file(&path)?;
        row_counts.push(rows.len() as u64);

        // load_text switches PhraseItems on token change, so a token's rows
        // must be consecutive in the file; a re-grouped token would start a
        // second item upstream and is refused here instead.
        let mut seen: BTreeMap<u32, ()> = BTreeMap::new();
        let mut previous: Option<u32> = None;
        let mut lib_items: BTreeMap<u32, PhraseModel> = BTreeMap::new();
        let mut parsed_rows: Vec<ParsedRow> = Vec::new();

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

            let is_new = !lib_items.contains_key(&row.token);
            let item = lib_items.entry(row.token).or_insert_with(|| PhraseModel {
                ucs4: row.phrase.chars().map(u32::from).collect(),
                text: row.phrase.clone(),
                prons: Vec::new(),
            });
            if !is_new && item.text != row.phrase {
                return Err(DatagenError::Consistency(format!(
                    "token {:#010x} names both {} and {}",
                    row.token, item.text, row.phrase
                )));
            }

            // The spelling selector: every row, keyed by the raw pinyin
            // string as written (no parse gate).
            let tokens = raw_index.entry(row.pinyin.as_bytes().to_vec()).or_default();
            if !tokens.contains(&row.token) {
                tokens.push(row.token);
            }

            // Libpinyin paths gate on the parse, upstream's
            // `len != keys->len` skip in load_text / add_pronunciation.
            let Some(keys) = parse_pinyin_keys(&row.pinyin) else {
                continue;
            };
            if keys.len() != row.phrase.chars().count() {
                continue;
            }
            parsed_rows.push(ParsedRow {
                token: row.token,
                keys: keys.clone(),
            });
            match item.prons.iter_mut().find(|(k, _)| *k == keys) {
                Some((_, sum)) => *sum += row.count,
                None => item.prons.push((keys, row.count)),
            }
        }
        out.push(LibraryModel {
            nibble: library,
            name,
            items: lib_items,
            parsed_rows,
        });
    }
    Ok((out, row_counts))
}

/// The shared table-read half of [`read_semantic`]: the four system
/// libraries' semantic content plus the spelling selector.
///
/// # Errors
///
/// Propagates [`read_libraries`] failures.
fn read_tables(model_dir: &Path, model: &mut SemanticModel) -> Result<(), DatagenError> {
    let (libraries, row_counts) =
        read_libraries(model_dir, SYSTEM_LIBRARIES, &mut model.raw_index)?;
    model.stats.library_rows.copy_from_slice(&row_counts[..4]);
    model.libraries = libraries;
    Ok(())
}

/// Parses `interpolation2.text` into the `\1-gram` counts and the
/// `\2-gram` groups, validating every `\item` pair against the compiled
/// phrases.
///
/// # Errors
///
/// Fails on a missing `interpolation2.text`, malformed or unexpected lines,
/// token/word contradictions, duplicate 2-gram pairs, or counts outside
/// `u32` range.
fn read_interpolation(model_dir: &Path, model: &mut SemanticModel) -> Result<(), DatagenError> {
    let phrases: BTreeMap<u32, &str> = model
        .libraries
        .iter()
        .flat_map(|lib| {
            lib.items
                .iter()
                .map(|(token, item)| (*token, item.text.as_str()))
        })
        .collect();
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
                        validate_pair(&phrases, token, word, &mut model.stats.special_tokens)
                            .map_err(|m| bad_line(&text_path, number, &m))?;
                        let token = token
                            .parse::<u32>()
                            .map_err(|_| bad_line(&text_path, number, "bad token"))?;
                        let count = parse_count(count, &text_path, number)?;
                        let entry = model.unigrams.entry(token).or_insert(0);
                        *entry += u64::from(count);
                    }
                    Section::Bigram => {
                        let [token1, word1, token2, word2, keyword, count] = fields[..] else {
                            return Err(bad_line(&text_path, number, "malformed \\item"));
                        };
                        if keyword != "count" {
                            return Err(bad_line(&text_path, number, "expected `count` keyword"));
                        }
                        validate_pair(&phrases, token1, word1, &mut model.stats.special_tokens)
                            .map_err(|m| bad_line(&text_path, number, &m))?;
                        validate_pair(&phrases, token2, word2, &mut model.stats.special_tokens)
                            .map_err(|m| bad_line(&text_path, number, &m))?;
                        let token1 = token1
                            .parse::<u32>()
                            .map_err(|_| bad_line(&text_path, number, "bad token"))?;
                        let token2 = token2
                            .parse::<u32>()
                            .map_err(|_| bad_line(&text_path, number, "bad token"))?;
                        let count = parse_count(count, &text_path, number)?;
                        let entry = model.bigram.entry(token1).or_default();
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
    Ok(())
}

fn parse_count(count: &str, path: &Path, number: usize) -> Result<u32, DatagenError> {
    let count = count
        .parse::<i64>()
        .map_err(|_| bad_line(path, number, "bad count"))?;
    u32::try_from(count).map_err(|_| bad_line(path, number, "count out of u32 range"))
}

/// Runs the shared model20 read pass.
///
/// # Errors
///
/// Propagates every read and validation failure; never drops data silently.
pub fn read_semantic(model_dir: &Path) -> Result<SemanticModel, DatagenError> {
    let mut model = SemanticModel {
        libraries: Vec::new(),
        raw_index: SpellingIndex::new(),
        unigrams: BTreeMap::new(),
        bigram: BTreeMap::new(),
        stats: SystemStats::default(),
    };
    read_tables(model_dir, &mut model)?;
    read_interpolation(model_dir, &mut model)?;
    model.stats.index_keys = model.raw_index.len() as u64;
    model.stats.phrases = model.libraries.iter().map(|l| l.items.len() as u64).sum();
    model.stats.bigram_entries = model.bigram.len() as u64;
    model.stats.bigram_records = model.bigram.values().map(|(_, r)| r.len() as u64).sum();
    Ok(model)
}

/// The mini restriction: keep the tokens the [`MINI_KEYS`] spellings
/// reference, and nothing else.
fn mini_keep_tokens(index: &SpellingIndex) -> std::collections::BTreeSet<u32> {
    index
        .iter()
        .filter(|(key, _)| {
            MINI_KEYS
                .iter()
                .any(|mini| key.as_slice() == mini.as_bytes())
        })
        .flat_map(|(_, tokens)| tokens.iter().copied())
        .collect()
}

/// Compiles the system tables from the extracted model20 directory into
/// libpinyin's own formats.
///
/// Reproduces upstream's three-tool chain exactly:
/// `gen_binary_files` (parse + `compact()` + `SubPhraseIndex::store` for
/// the chunk files; `ChewingLargeTable2` / `PhraseLargeTable3` for the
/// index DBMs), `import_interpolation` (the `\1-gram` counts into the
/// phrase items, the `\2-gram` into `bigram.db`), and `gen_unigram` (+1
/// unigram for every library token).
///
/// # Errors
///
/// Fails on missing files, unparsable lines, token/word contradictions
/// between `interpolation2.text` and the tables, duplicate 2-gram pairs,
/// u32 overflow of a bigram total, or a chunk serialization failure —
/// never silently drops data.
pub fn compile(
    model_dir: &Path,
    subset: Subset,
) -> Result<(SystemOutput, SystemStats), DatagenError> {
    let mut model = read_semantic(model_dir)?;
    let stats = model.stats.clone();
    if subset == Subset::MiniFixture {
        let kept = mini_keep_tokens(&model.raw_index);
        for lib in &mut model.libraries {
            lib.items.retain(|token, _| kept.contains(token));
            lib.parsed_rows.retain(|row| kept.contains(&row.token));
        }
        model.unigrams.retain(|token, _| kept.contains(token));
        model.bigram.retain(|token, _| kept.contains(token));
    }

    // ---- chunk files (per-library SubPhraseIndex::store) --------------
    let mut chunk_files = Vec::with_capacity(model.libraries.len());
    for lib in &model.libraries {
        // compact() order: ascending token == ascending slot.
        let items: Vec<(u32, ChunkItem)> = lib
            .items
            .iter()
            .map(|(token, item)| {
                let unigram = model
                    .unigrams
                    .get(token)
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(1); // gen_unigram's +1
                let unigram = u32::try_from(unigram).unwrap_or(u32::MAX);
                (
                    token & chunks::PHRASE_MASK,
                    ChunkItem {
                        phrase: item.ucs4.clone(),
                        unigram,
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

    // ---- the two index DBM row streams ---------------------------------
    let parsed_rows: Vec<ParsedRow> = model
        .libraries
        .iter()
        .flat_map(|lib| lib.parsed_rows.iter().cloned())
        .collect();
    let pinyin_index = libpinyin::pinyin_index_entries(&parsed_rows);
    let phrase_rows: Vec<(Vec<u32>, u32)> = model
        .libraries
        .iter()
        .flat_map(|lib| {
            lib.items
                .iter()
                .map(|(token, item)| (item.ucs4.clone(), *token))
        })
        .collect();
    let phrase_index = libpinyin::phrase_index_entries(&phrase_rows);

    // ---- bigram.db row stream -------------------------------------------
    let mut bigram_entries: Entries = Vec::with_capacity(model.bigram.len());
    for (token, (total, mut records)) in model.bigram {
        let total = u32::try_from(total).map_err(|_| {
            DatagenError::Consistency(format!("bigram total for {token:#010x} overflows u32"))
        })?;
        // SingleGram::insert_freq keeps records token-ascending.
        records.sort_unstable_by_key(|(next, _)| *next);
        let mut value = Vec::with_capacity(4 + records.len() * 8);
        value.extend_from_slice(&total.to_le_bytes());
        for (next, count) in records {
            value.extend_from_slice(&next.to_le_bytes());
            value.extend_from_slice(&count.to_le_bytes());
        }
        bigram_entries.push((token.to_le_bytes().to_vec(), value));
    }
    bigram_entries.sort_by(|a, b| a.0.cmp(&b.0));

    Ok((
        SystemOutput {
            chunks: chunk_files,
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
    phrases: &BTreeMap<u32, &str>,
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
        Some(phrase) if *phrase == word => Ok(()),
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
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &std::path::PathBuf, content: &str) {
        std::fs::write(path, content).unwrap();
    }

    fn write_empty_libraries(dir: &std::path::Path, except: &str) {
        for (_, name) in SYSTEM_LIBRARIES {
            if *name != except {
                write(&dir.join(format!("{name}.table")), "");
            }
        }
    }

    #[test]
    fn parse_pinyin_keys_handles_tones_and_apostrophes() {
        let keys = parse_pinyin_keys("ni'hao3").unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].tone, 0);
        assert_eq!(keys[1].tone, 3);
        assert!(parse_pinyin_keys("zzz").is_none());
        assert!(parse_pinyin_keys("ni''hao").is_none());
    }

    #[test]
    fn compile_matches_upstream_arithmetic() {
        let dir = tmp_model("libpinyin");
        write(
            &dir.join("gb_char.table"),
            "a\t吖\t16777218\t104\nya\t吖\t16777218\t793\nb\t吧\t16777219\t5\n",
        );
        write_empty_libraries(&dir, "gb_char");
        write(
            &dir.join("interpolation2.text"),
            "\\data model interpolation\n\\1-gram\n\\item 16777218 吖 count 9\n\\end\n",
        );
        let (out, stats) = compile(&dir, Subset::Full).unwrap();
        assert_eq!(stats.library_rows, [3, 0, 0, 0]);
        assert_eq!(stats.index_keys, 3);
        assert_eq!(stats.phrases, 2);
        assert_eq!(out.chunks.len(), 4);
        assert_eq!(out.chunks[0].0, "gb_char.bin");
        // Chunk unigram = \1-gram + 1 = 10; the other token has 0 + 1.
        let lib = oxpinyin_data::phrase_library::PhraseLibrary::open(&{
            let path = dir.join("gb_char.bin");
            std::fs::write(&path, &out.chunks[0].1).unwrap();
            path
        })
        .unwrap();
        assert_eq!(lib.total_freq(), 11);
        let item = lib.item(16_777_218).expect("item");
        assert_eq!(item.unigram(), 10);
        assert_eq!(item.n_pronunciations(), 2);
        // Pronunciation frequencies are the table counts verbatim.
        let freqs: Vec<u32> = item.pronunciations().map(|p| p.freq).collect();
        assert_eq!(freqs, vec![104, 793]);
        // Both keyspaces + prefix markers: "a", "b", "ya" are all
        // one-syllable → 3 real keys + no proper prefixes = 3 + 3 = 6.
        assert_eq!(out.pinyin_index.len(), 6);
        // Phrase index: two phrases, each a single character → no markers.
        assert_eq!(out.phrase_index.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bigram_groups_total_and_start_token() {
        let dir = tmp_model("bigram");
        write(
            &dir.join("gb_char.table"),
            "de\t的\t16778715\t275240\nzhong'guo\t中国\t16778716\t5\n",
        );
        write_empty_libraries(&dir, "gb_char");
        write(
            &dir.join("interpolation2.text"),
            "\\data model interpolation\n\
             \\1-gram\n\\item 16778715 的 count 5\n\\item 1 <start> count 99\n\
             \\2-gram\n\\item 16778716 中国 16778715 的 count 2\n\\item 1 <start> 16778715 的 count 3\n\\end\n",
        );
        let (out, stats) = compile(&dir, Subset::Full).unwrap();
        assert_eq!(stats.special_tokens, 2);
        assert_eq!(stats.bigram_entries, 2);
        // Bigram: ascending key-byte order; each value = total + records.
        assert_eq!(out.bigram[0].0, 1_u32.to_le_bytes());
        assert_eq!(&out.bigram[0].1[..4], &3_u32.to_le_bytes());
        assert_eq!(out.bigram[1].0, 16_778_716_u32.to_le_bytes());
        assert_eq!(out.bigram[1].1.len(), 4 + 8);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn model_word_contradiction_is_an_error() {
        let dir = tmp_model("contradiction");
        write(&dir.join("gb_char.table"), "de\t的\t16778715\t275240\n");
        write_empty_libraries(&dir, "gb_char");
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
        write_empty_libraries(&dir, "gb_char");
        write(
            &dir.join("interpolation2.text"),
            "\\data model interpolation\n\\end\n",
        );
        let err = compile(&dir, Subset::Full).unwrap_err().to_string();
        assert!(err.contains("two row groups"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mini_subset_keeps_the_allowlisted_spellings_only() {
        let dir = tmp_model("mini");
        write(
            &dir.join("gb_char.table"),
            "a\t吖\t16777218\t104\nya\t吖\t16777218\t793\nb\t吧\t16777219\t5\n",
        );
        write_empty_libraries(&dir, "gb_char");
        write(
            &dir.join("interpolation2.text"),
            "\\data model interpolation\n\\end\n",
        );
        let (full, _) = compile(&dir, Subset::Full).unwrap();
        let (mini, _) = compile(&dir, Subset::MiniFixture).unwrap();
        assert_eq!(full.phrase_index.len(), 2);
        // "a" is allowlisted and names 吖 (both its readings survive);
        // 吧 is only reachable through "b", which is not.
        assert_eq!(mini.phrase_index.len(), 1);
        assert_eq!(mini.pinyin_index.len(), 4, "a and ya in both keyspaces");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
