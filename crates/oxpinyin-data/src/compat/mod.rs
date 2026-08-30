//! The libpinyin drop-in compat loader: opens a real libpinyin data
//! directory (the files a distro's `libpinyin-data` installs) and converts
//! it, at load time, into the same in-memory model the native tables
//! produce — so the decode path above is byte-for-byte the code that runs
//! on oxpinyin's own data.
//!
//! What a libpinyin directory holds (Phase 1, read from the 2.11.91 pin
//! and verified against RHEL 10.2's installed 2.8.1 files; every format
//! here is identical between those versions):
//!
//! * `table.conf` — the only file whose absence fails `pinyin_init`;
//!   declares the DBM (`database format:KyotoCabinet` / `Tkrzw` / …), λ,
//!   and the default phrase libraries.
//! * Content tables (`gb_char.bin`, `merged.bin`, …) — [`MemoryChunk`]
//!   images (backend-independent) holding a `SubPhraseIndex`: per-token
//!   offsets into packed `PhraseItem`s (text, per-pronunciation
//!   `ChewingKey` sequences and frequencies, unigram frequency).
//! * `bigram.db` — the build DBM's hash database (`ngram_*.cpp`), keyed by
//!   the raw `u32` token, valued by a `SingleGram` chunk.
//! * `phrase_index.bin` / `pinyin_index.bin` — the build DBM's **tree**
//!   databases (despite the extension), libpinyin's lookup acceleration.
//!   The in-memory model derives every lookup structure from the content
//!   tables, so these serve detection only.
//!
//! [`MemoryChunk`]: crate::memory_chunk

mod chewing_table;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use oxpinyin_core::SyllableKey;

use crate::layout::{Dbm, dbm_of};
use crate::lm::BigramRow;
use crate::memory_chunk::{self, MemoryChunkError};

/// libpinyin's `PHRASE_MASK` (`novel_types.h`): the position half of a
/// token.
const PHRASE_MASK: u32 = 0x00FF_FFFF;

/// Why the compat loader rejected a directory.
#[derive(Debug)]
pub enum CompatError {
    /// `table.conf` was unreadable or unparsable.
    TableConf(PathBuf, String),
    /// A content table failed the `MemoryChunk` verification.
    Chunk(MemoryChunkError),
    /// A content table's `SubPhraseIndex` framing is invalid.
    SubPhrase(PathBuf, String),
    /// The declared/observed DBM cannot be read by this build.
    Backend(String),
    /// The bigram database could not be read.
    Bigram(PathBuf, String),
}

impl fmt::Display for CompatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TableConf(path, msg) => write!(f, "{}: {msg}", path.display()),
            Self::Chunk(e) => write!(f, "{e}"),
            Self::SubPhrase(path, msg) => write!(f, "{}: {msg}", path.display()),
            Self::Backend(msg) => f.write_str(msg),
            Self::Bigram(path, msg) => write!(f, "{}: {msg}", path.display()),
        }
    }
}

impl std::error::Error for CompatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Chunk(e) => Some(e),
            _ => None,
        }
    }
}

impl From<MemoryChunkError> for CompatError {
    fn from(e: MemoryChunkError) -> Self {
        Self::Chunk(e)
    }
}

// ── detection ───────────────────────────────────────────────────────

/// A detected libpinyin-compat data directory.
#[derive(Debug)]
pub struct CompatLayout {
    /// The DBM that wrote `bigram.db`, from its file magic.
    pub dbm: Dbm,
    /// The format `table.conf` declares (`database format:` line), for the
    /// cross-check message; `None` when the line is absent (old configs).
    pub declared: Option<String>,
    /// The default SYSTEM phrase libraries: `(library index, .bin file)`.
    pub default_tables: Vec<(u8, String)>,
}

/// libpinyin's default-table names → `PHRASE_INDEX` indices
/// (`table_info.h`: RESERVED 0, GB 1 (=TSI), GBK 2, OPENGRAM 3, MERGED 4,
/// ADDON 5, NETWORK 6, USER 7).
fn dictionary_index(name: &str) -> Option<u8> {
    Some(match name {
        "RESERVED" => 0,
        "GB_DICTIONARY" | "TSI_DICTIONARY" => 1,
        "GBK_DICTIONARY" => 2,
        "OPENGRAM_DICTIONARY" => 3,
        "MERGED_DICTIONARY" => 4,
        "ADDON_DICTIONARY" => 5,
        "NETWORK_DICTIONARY" => 6,
        "USER_DICTIONARY" => 7,
        _ => return None,
    })
}

impl CompatLayout {
    /// Classifies `dir`: `Some` when it is a libpinyin install (it has a
    /// parsable `table.conf` and a `bigram.db` whose magic names a DBM),
    /// `None` when it is anything else — an oxpinyin-native directory has
    /// neither file, so the native paths are untouched by construction.
    #[must_use]
    pub fn detect(dir: &Path) -> Option<Self> {
        let conf_path = dir.join("table.conf");
        let conf = std::fs::read_to_string(&conf_path).ok()?;
        let bigram = dir.join("bigram.db");
        let dbm = dbm_of(&bigram).ok().flatten()?;

        let mut declared = None;
        let mut default_tables = Vec::new();
        for line in conf.lines() {
            if let Some(value) = line.strip_prefix("database format:") {
                declared = Some(value.trim().to_owned());
            }
            // `default <NAME> <table> <bin> <dbin> <TYPE>`
            let mut parts = line.split_whitespace();
            if parts.next() != Some("default") {
                continue;
            }
            let (Some(name), _table, Some(bin), _dbin, Some(kind)) = (
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
            ) else {
                continue;
            };
            if kind != "SYSTEM_FILE" {
                continue;
            }
            let Some(index) = dictionary_index(name) else {
                continue;
            };
            default_tables.push((index, bin.to_owned()));
        }
        Some(Self {
            dbm,
            declared,
            default_tables,
        })
    }
}

// ── ChewingKey → pinyin syllable ────────────────────────────────────

/// The toneless `ChewingKey` bit pattern for a content row
/// (`chewing_key.h`, GCC little-endian bitfield allocation: initial bits
/// 0–4, middle 5–6, final 7–11, tone 12–14, padding 15).
const fn chewing_bits(initial: u8, middle: u8, fin: u8) -> u16 {
    (initial as u16) | ((middle as u16) << 5) | ((fin as u16) << 7)
}

/// Mask clearing the tone (and padding) bits of a stored `ChewingKey`.
const TONELESS_MASK: u16 = 0x0FFF;

/// Tone bits of a stored `ChewingKey`.
const fn tone_of(bits: u16) -> u16 {
    (bits >> 12) & 0x7
}

/// The `(toneless ChewingKey bits → canonical pinyin)` map, restricted to
/// spellings the frozen syllable inventory resolves. Sorted by bits;
/// first row wins for a triple with several spellings (lüe/lve …).
fn chewing_to_pinyin() -> Vec<(u16, &'static str)> {
    let mut map: Vec<(u16, &'static str)> = Vec::new();
    for &(initial, middle, fin, pinyin) in &chewing_table::CHEWING_CONTENT_ROWS {
        if SyllableKey::from_text(pinyin).is_none() {
            continue;
        }
        map.push((chewing_bits(initial, middle, fin), pinyin));
    }
    map.sort_by_key(|&(bits, _)| bits);
    map.dedup_by_key(|&mut (bits, _)| bits);
    map
}

// ── content tables (`SubPhraseIndex`) ───────────────────────────────

/// One parsed phrase item.
struct Item {
    token: u32,
    text: String,
    unigram: u32,
    /// Per pronunciation: the toneless spelling joined with `'`, and its
    /// frequency. Pronunciations whose keys resolve to no frozen syllable
    /// are dropped, like a mini table's unresolvable tokens.
    pronunciations: Vec<(String, u32)>,
}

fn read_u32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u16(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Parses one content table (already MemoryChunk-verified `data`) into
/// items, composing each token as `(library << 24) | position`.
fn parse_sub_phrase_index(
    path: &Path,
    library: u8,
    data: &[u8],
    key_map: &[(u16, &'static str)],
    items: &mut Vec<Item>,
) -> Result<u64, CompatError> {
    let framing = |msg: &str| CompatError::SubPhrase(path.to_path_buf(), msg.to_owned());
    let total_freq = read_u32(data, 0).ok_or_else(|| framing("truncated header"))?;
    let index_one = read_u32(data, 4).ok_or_else(|| framing("truncated header"))? as usize;
    let index_two = read_u32(data, 8).ok_or_else(|| framing("truncated header"))? as usize;
    let index_three = read_u32(data, 12).ok_or_else(|| framing("truncated header"))? as usize;
    // The pin's separators (`SubPhraseIndex::load`): a `#` after the
    // header and before each region boundary.
    let separator = |at: usize| data.get(at).copied() == Some(b'#');
    if !separator(16) || index_two < 1 || !separator(index_two - 1) {
        return Err(framing("missing '#' separator"));
    }
    if index_three < 1 || index_three > data.len() || !separator(index_three - 1) {
        return Err(framing("content bounds out of range"));
    }
    if index_one > index_two - 1 || index_two > index_three - 1 {
        return Err(framing("region offsets are not ascending"));
    }
    let offsets = &data[index_one..index_two - 1];
    let content = &data[index_two..index_three - 1];

    for (position, chunk) in offsets.chunks_exact(4).enumerate() {
        let offset = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as usize;
        if offset == 0 {
            continue;
        }
        let item_err = || {
            CompatError::SubPhrase(
                path.to_path_buf(),
                format!("truncated phrase item at content offset {offset}"),
            )
        };
        let len = *content.get(offset).ok_or_else(item_err)? as usize;
        let npron = *content.get(offset + 1).ok_or_else(item_err)? as usize;
        let unigram = read_u32(content, offset + 2).ok_or_else(item_err)?;
        let mut text = String::with_capacity(len * 3);
        for i in 0..len {
            let cp = read_u32(content, offset + 6 + i * 4).ok_or_else(item_err)?;
            text.push(char::from_u32(cp).ok_or_else(|| {
                CompatError::SubPhrase(
                    path.to_path_buf(),
                    format!("invalid code point {cp:#x} at content offset {offset}"),
                )
            })?);
        }
        let prons_at = offset + 6 + len * 4;
        let pron_size = len * 2 + 4;
        let mut pronunciations = Vec::with_capacity(npron);
        for p in 0..npron {
            let at = prons_at + p * pron_size;
            let mut spelling = String::new();
            let mut resolved = true;
            for k in 0..len {
                let bits = read_u16(content, at + k * 2).ok_or_else(item_err)?;
                let toneless = bits & TONELESS_MASK;
                let _tone = tone_of(bits); // aggregated below; kept for clarity
                match key_map
                    .binary_search_by_key(&toneless, |&(b, _)| b)
                    .ok()
                    .map(|i| key_map[i].1)
                {
                    Some(pinyin) => {
                        if k > 0 {
                            spelling.push('\'');
                        }
                        spelling.push_str(pinyin);
                    }
                    None => {
                        resolved = false;
                        break;
                    }
                }
            }
            let freq = read_u32(content, at + len * 2).ok_or_else(item_err)?;
            if resolved {
                pronunciations.push((spelling, freq));
            }
        }
        items.push(Item {
            token: (u32::from(library) << 24) | (position as u32 & PHRASE_MASK),
            text,
            unigram,
            pronunciations,
        });
    }
    Ok(u64::from(total_freq))
}

// ── bigram walkers ──────────────────────────────────────────────────

fn load_bigram_rows(dir: &Path, dbm: Dbm) -> Result<Vec<(u32, BigramRow)>, CompatError> {
    let path = dir.join("bigram.db");
    let unsupported = |what: &str| {
        CompatError::Backend(format!(
            "{}: written by {what}; rebuild oxpinyin with the matching backend \
             feature to read it",
            path.display()
        ))
    };
    match dbm {
        #[cfg(feature = "kyotocabinet")]
        Dbm::KyotoHash | Dbm::KyotoTree => {
            let db = oxpinyin_store::BigramDb::open(&path, true)
                .map_err(|e| CompatError::Bigram(path.clone(), e.to_string()))?;
            let mut rows = Vec::new();
            db.for_each(&mut |prev, gram| {
                rows.push((prev, gram_row(gram)));
                Ok(())
            })
            .map_err(|e| CompatError::Bigram(path.clone(), e.to_string()))?;
            Ok(rows)
        }
        #[cfg(feature = "tkrzw")]
        Dbm::Tkrzw => {
            let db = oxpinyin_store::TkrzwBigramDb::open_read_only(&path)
                .map_err(|e| CompatError::Bigram(path.clone(), e.to_string()))?;
            let mut rows = Vec::new();
            db.for_each(&mut |prev, gram| {
                rows.push((prev, gram_row(&gram)));
                Ok(())
            })
            .map_err(|e| CompatError::Bigram(path.clone(), e.to_string()))?;
            Ok(rows)
        }
        #[cfg(not(feature = "kyotocabinet"))]
        Dbm::KyotoHash | Dbm::KyotoTree => Err(unsupported("Kyoto Cabinet")),
        #[cfg(not(feature = "tkrzw"))]
        Dbm::Tkrzw => Err(unsupported("tkrzw")),
        Dbm::BerkeleyHash => Err(unsupported("Berkeley DB")),
    }
}

#[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
fn gram_row(gram: &oxpinyin_store::single_gram::SingleGram) -> BigramRow {
    BigramRow {
        total: gram.total(),
        records: gram.items().to_vec(),
    }
}

// ── the loaded model ────────────────────────────────────────────────

/// Everything the runtime needs to assemble the in-memory engine from a
/// libpinyin directory.
pub struct CompatModel {
    /// `(token, phrase text)` rows for the phrase index.
    pub phrase_rows: Vec<(u32, String)>,
    /// `(toneless pinyin spelling, {token, freq} records)` rows for the
    /// pinyin index, tone variants of one spelling aggregated.
    pub pinyin_rows: Vec<(String, Vec<(u32, u32)>)>,
    /// libpinyin's own unigram frequencies (`PhraseItem` headers).
    pub unigrams: BTreeMap<u32, u64>,
    /// Sum of the content tables' `total_freq` headers.
    pub unigram_total: u64,
    /// `(prev, successors)` rows from `bigram.db`, stored order.
    pub bigram_rows: Vec<(u32, BigramRow)>,
}

/// Loads a detected libpinyin directory into the in-memory model.
///
/// # Errors
///
/// [`CompatError`] on any unreadable or malformed input — every failure
/// names the offending file; nothing falls back silently.
pub fn load(dir: &Path, layout: &CompatLayout) -> Result<CompatModel, CompatError> {
    let key_map = chewing_to_pinyin();

    let mut items: Vec<Item> = Vec::new();
    let mut unigram_total: u64 = 0;
    for (library, bin) in &layout.default_tables {
        if *library == 0 {
            continue; // RESERVED
        }
        let path = dir.join(bin);
        let data = memory_chunk::load(&path)?;
        unigram_total = unigram_total.saturating_add(parse_sub_phrase_index(
            &path, *library, &data, &key_map, &mut items,
        )?);
    }

    let mut phrase_rows: Vec<(u32, String)> = Vec::with_capacity(items.len());
    let mut unigrams: BTreeMap<u32, u64> = BTreeMap::new();
    let mut spelling_rows: BTreeMap<String, BTreeMap<u32, u32>> = BTreeMap::new();
    for item in &items {
        phrase_rows.push((item.token, item.text.clone()));
        unigrams.insert(item.token, u64::from(item.unigram));
        for (spelling, freq) in &item.pronunciations {
            let per_token = spelling_rows.entry(spelling.clone()).or_default();
            let slot = per_token.entry(item.token).or_default();
            *slot = slot.saturating_add(*freq);
        }
    }
    let pinyin_rows: Vec<(String, Vec<(u32, u32)>)> = spelling_rows
        .into_iter()
        .map(|(spelling, tokens)| (spelling, tokens.into_iter().collect()))
        .collect();

    let bigram_rows = load_bigram_rows(dir, layout.dbm)?;

    Ok(CompatModel {
        phrase_rows,
        pinyin_rows,
        unigrams,
        unigram_total,
        bigram_rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chewing_map_resolves_the_frozen_inventory_spellings() {
        let map = chewing_to_pinyin();
        assert!(map.len() > 380, "map holds the syllable inventory");
        let find = |bits: u16| {
            map.binary_search_by_key(&bits, |&(b, _)| b)
                .ok()
                .map(|i| map[i].1)
        };
        // ba = B + A (chewing_enum.h: B=1, A=1).
        assert_eq!(find(chewing_bits(1, 0, 1)), Some("ba"));
        // zhong = ZH + ONG (ZH=23, PINYIN_ONG=14).
        assert_eq!(find(chewing_bits(23, 0, 14)), Some("zhong"));
        // ying = Y + ING (PINYIN_Y=21, PINYIN_ING=17).
        assert_eq!(find(chewing_bits(21, 0, 17)), Some("ying"));
        // er = bare final ER (11).
        assert_eq!(find(chewing_bits(0, 0, 11)), Some("er"));
    }

    #[test]
    fn a_synthesized_content_table_parses_field_for_field() {
        // One library with two items: 好 (hao, tone 3) and 好好.
        let key_map = chewing_to_pinyin();
        let hao = key_map
            .iter()
            .find(|&&(_, p)| p == "hao")
            .map(|&(bits, _)| bits | (3 << 12))
            .expect("hao is in the map");

        // The items region, offsets relative to its start. Offset 0 means
        // "no item", so a one-byte pad keeps real items off offset 0 —
        // upstream's region begins with live data but never at 0 for a
        // stored item either.
        let mut items_region: Vec<u8> = vec![0];
        let item = |region: &mut Vec<u8>, text: &str, prons: &[(&[u16], u32)]| -> u32 {
            let offset = region.len() as u32;
            let chars: Vec<char> = text.chars().collect();
            region.push(chars.len() as u8);
            region.push(prons.len() as u8);
            region.extend_from_slice(&7u32.to_le_bytes()); // unigram freq
            for c in &chars {
                region.extend_from_slice(&(*c as u32).to_le_bytes());
            }
            for (keys, freq) in prons {
                for k in *keys {
                    region.extend_from_slice(&k.to_le_bytes());
                }
                region.extend_from_slice(&freq.to_le_bytes());
            }
            offset
        };
        let o1 = item(&mut items_region, "好", &[(&[hao], 5)]);
        let o2 = item(&mut items_region, "好好", &[(&[hao, hao], 2)]);

        // Offsets region: positions 0 (absent), 1, 2.
        let mut offsets = Vec::new();
        offsets.extend_from_slice(&0u32.to_le_bytes());
        offsets.extend_from_slice(&o1.to_le_bytes());
        offsets.extend_from_slice(&o2.to_le_bytes());

        // header(16) '#' offsets '#' items '#'
        let index_one = 17u32;
        let index_two = index_one + offsets.len() as u32 + 1;
        let index_three = index_two + items_region.len() as u32 + 1;
        let mut data = Vec::new();
        data.extend_from_slice(&9u32.to_le_bytes()); // total_freq
        data.extend_from_slice(&index_one.to_le_bytes());
        data.extend_from_slice(&index_two.to_le_bytes());
        data.extend_from_slice(&index_three.to_le_bytes());
        data.push(b'#');
        data.extend_from_slice(&offsets);
        data.push(b'#');
        data.extend_from_slice(&items_region);
        data.push(b'#');

        let mut items = Vec::new();
        let total = parse_sub_phrase_index(Path::new("synth.bin"), 1, &data, &key_map, &mut items)
            .expect("parse");
        assert_eq!(total, 9);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].token, (1 << 24) | 1);
        assert_eq!(items[0].text, "好");
        assert_eq!(items[0].unigram, 7);
        assert_eq!(items[0].pronunciations, vec![("hao".to_owned(), 5)]);
        assert_eq!(items[1].token, (1 << 24) | 2);
        assert_eq!(items[1].text, "好好");
        assert_eq!(items[1].pronunciations, vec![("hao\'hao".to_owned(), 2)]);
    }

    #[test]
    fn detect_never_fires_on_a_native_oxpinyin_dir() {
        let dir =
            std::env::temp_dir().join(format!("oxpinyin-compat-native-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create");
        // A native dir: store tables and interpolation2.text, no
        // table.conf, no bigram.db.
        std::fs::write(dir.join(crate::default_store_file("pinyin_index")), b"x").expect("write");
        std::fs::write(dir.join("interpolation2.text"), b"\\data\n").expect("write");
        assert!(CompatLayout::detect(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
