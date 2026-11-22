//! Data adapters for the standalone lookup, table, and training commands.

use oxpinyin::data::chunk_format::chunk_checksum;
use oxpinyin::data::phrase_library::{LibraryBytes, LibraryError, PhraseLibrary, RowFault};
use oxpinyin::data::{
    BigramTable, ChewingTable, DefaultDbm, DictError, PhraseLibraries, PhraseTable, RawChewingDbm,
    SystemTables, TableError, visit_matching_items,
};
use oxpinyin::parser::ChewingKey;
use oxpinyin::single_gram::{SingleGramRecord, get_total_freq, is_complete, retrieve_all};
use oxpinyin::store::{DefaultStore, RawReadStore, StoreError, SystemAttachment, WriteStore};
use std::path::Path;

fn open_tree(path: &Path) -> Result<ChewingTable, DictError> {
    let store = DefaultStore::open_read_only(path).map_err(TableError::from)?;
    Ok(ChewingTable::new(RawChewingDbm::new(store)))
}

fn open_phrase_table(path: &Path) -> Result<PhraseTable, DictError> {
    let store = DefaultStore::open_read_only(path).map_err(TableError::from)?;
    Ok(PhraseTable::new(RawChewingDbm::new(store)))
}

// ── the phrase DBM alone ─────────────────────────────────────────

/// The phrase DBM alone (`phrase_index.bin`), as
/// `FacadePhraseTable3::load` opens it for the segmentation utilities:
/// no pinyin DBM and no phrase libraries.
///
/// [`Self::search_ucs4`] answers `FacadePhraseTable3::search`'s presence
/// bit — `None` is "no key" (no `SEARCH_CONTINUED`), `Some` is "key
/// present" — and the stored token bytes as the pin iterates them
/// (`floor(len / 4)` tokens; a trailing partial word is not a token).
/// `SEARCH_OK` and the library filter stay with the caller: it owns the
/// loaded-library set.
pub struct PhraseIndexTable {
    phrase: PhraseTable,
}

impl PhraseIndexTable {
    /// Opens `phrase_index.bin` read-only.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the backend cannot open the file.
    pub fn open(phrase_index: &Path) -> Result<Self, DictError> {
        Ok(Self {
            phrase: open_phrase_table(phrase_index)?,
        })
    }

    /// `PhraseLargeTable3::search`: `None` when the key is absent,
    /// `Some(tokens)` when it is present, empty for a continuation marker.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read fails.
    pub fn search_ucs4(&self, words: &[u32]) -> Result<Option<Vec<u32>>, DictError> {
        if words.is_empty() {
            return Ok(None);
        }
        let key = oxpinyin::data::row_format::phrase_index::encode_ucs4_key_scalars(words);
        let Some(value) = self.phrase.present_key(&key)? else {
            return Ok(None);
        };
        Ok(Some(
            value
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| u32::from_le_bytes(*word))
                .collect(),
        ))
    }
}

// ── the pinyin DBM alone ─────────────────────────────────────────

/// The pinyin DBM alone (`pinyin_index.bin`): the `ChewingLargeTable2`
/// a caller needs when it has no phrase table. `eval_correction_rate`
/// opens `FacadeChewingTable2` and resolves every surviving token through
/// the loaded phrase libraries alone
/// (`utils/training/eval_correction_rate.cpp:143-163`).
///
/// [`Self::search`] answers the tokens whose stored pronunciation matches
/// the query, in stored order; [`Self::key_exists`] answers
/// `SEARCH_CONTINUED`.
pub struct PinyinIndexTable {
    pinyin: ChewingTable,
}

impl PinyinIndexTable {
    /// Opens `pinyin_index.bin` read-only.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the backend cannot open the file.
    pub fn open(pinyin_index: &Path) -> Result<Self, DictError> {
        Ok(Self {
            pinyin: open_tree(pinyin_index)?,
        })
    }

    /// `ChewingLargeTable2::search` without the caller's library filter:
    /// the matching records' tokens, empty when the index key is absent or
    /// no record matches.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read or a record decode fails.
    pub fn search(&self, keys: &[ChewingKey]) -> Result<Vec<u32>, DictError> {
        self.pinyin.search(keys)
    }

    /// `SEARCH_CONTINUED`: whether the query's index key exists.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read fails.
    pub fn key_exists(&self, keys: &[ChewingKey]) -> Result<bool, DictError> {
        self.pinyin.key_exists(keys)
    }
}

/// SEARCH_CONTINUED after decoding a physical-matrix path, including
/// the pin search's malformed-row checks.
///
/// # Errors
/// Returns native table errors.
pub fn system_key_exists(tables: &SystemTables, keys: &[ChewingKey]) -> Result<bool, DictError> {
    tables
        .pinyin()
        .search_continued(keys, &mut Vec::new())
        .map(|(continued, _)| continued)
}

/// Attach the one index read by a standalone pin lookup program.
/// The other table is transient and is never read by that program.
///
/// # Errors
/// Returns a native DBM construction error.
pub fn lookup_tables(path: &Path, phonetic: bool) -> Result<SystemTables, DictError> {
    let attached = DefaultDbm::attach(path, false)?;
    let empty = DefaultDbm::new(
        <DefaultStore as WriteStore>::create_in_memory()
            .map_err(|error| DictError::Table(error.into()))?,
    );
    let (pinyin, phrase) = if phonetic {
        (attached, empty)
    } else {
        (empty, attached)
    };
    Ok(SystemTables::new(
        ChewingTable::new(pinyin),
        PhraseTable::new(phrase),
        0,
    ))
}

// ChewingLargeTable2::search, chewing_large_table2.cpp:161–174; SEARCH_CONTINUED is
// independent of the filtered tokens.
/// Returns whether a row was present for a nonempty query and its matching tokens in
/// stored order. A present empty prefix marker or a row with no matching readings still
/// reports presence.
///
/// # Errors
///
/// Propagates a row-decoding error.
pub fn search_row(
    keys: &[ChewingKey],
    value: Option<&[u8]>,
) -> Result<(bool, Vec<u32>), DictError> {
    if keys.is_empty() {
        return Ok((false, Vec::new()));
    }
    let Some(value) = value else {
        return Ok((false, Vec::new()));
    };
    let mut tokens = Vec::new();
    visit_matching_items(keys, value, |token| tokens.push(token))?;
    Ok((true, tokens))
}

/// Opens a bigram DBM lazily (no scan). libpinyin's `bigram.db` is
/// a KC **`HashDB`** / Tkrzw **`HashDBM`**, so the open goes through
/// `RawReadStore::open_hash_read_only`.
///
/// # Errors
///
/// Returns [`DictError`] when the file cannot be opened read-only.
pub fn open_bigram(path: &Path) -> Result<BigramTable, DictError> {
    let store = DefaultStore::open_hash_read_only(path).map_err(TableError::from)?;
    Ok(BigramTable::new(RawChewingDbm::new(store)))
}
/// Attaches the plain bigram hash with one native open.
///
/// # Errors
/// Returns an error that occurs before the native open.
pub fn attach_bigram(path: &Path, read_only: bool) -> Result<Option<DefaultStore>, StoreError> {
    match SystemAttachment::attach_bigram(path, read_only)? {
        SystemAttachment::Open(store) => Ok(Some(store)),
        SystemAttachment::Unopened => Ok(None),
    }
}

/// Read and validate a utility chunk through `MemoryChunk::load`'s calls.
///
/// # Errors
/// Returns a read, checksum, or layout error.
pub fn load_heap(path: &Path) -> Result<PhraseLibrary, LibraryError> {
    let loaded = oxpinyin::input::chunk::load(path).map_err(|error| match error {
        oxpinyin::input::chunk::ChunkLoadError::Header => {
            LibraryError::Row(RowFault::ChunkHeaderRead)
        }
        oxpinyin::input::chunk::ChunkLoadError::Io(error) => LibraryError::Io(error),
        oxpinyin::input::chunk::ChunkLoadError::ShortHeader => {
            LibraryError::Format("chunk shorter than its header".into())
        }
    })?;
    let payload = loaded
        .finish(chunk_checksum)
        .ok_or_else(|| LibraryError::Format("chunk checksum mismatch".into()))?;
    library_from_payload(payload)
}

/// Constructs a library from an owned chunk payload. A missing first
/// separator or a wrong separator byte leaves a library with no accessible
/// items and the stored total, or zero if the total is unavailable.
///
/// # Errors
/// Returns an error for an out-of-bounds later separator, sections out of
/// order, or an offset array without a whole number of entries.
pub fn library_from_payload(payload: Vec<u8>) -> Result<PhraseLibrary, LibraryError> {
    let len = payload.len();
    PhraseLibrary::from_bytes(LibraryBytes::from(payload), 0..len, None)
}

/// Compares two equal-length key sequences by initial, then middle and final, then tone.
/// Only positions in `lhs` participate.
///
/// # Panics
///
/// Panics if a comparison indexes beyond `rhs`. Pass a right slice at least as long as
/// `lhs`.
#[must_use]
pub fn exact_compare2(lhs: &[ChewingKey], rhs: &[ChewingKey]) -> std::cmp::Ordering {
    let len = lhs.len();
    for i in 0..len {
        match lhs[i].initial.cmp(&rhs[i].initial) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    for i in 0..len {
        match lhs[i].middle.cmp(&rhs[i].middle) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
        match lhs[i].final_.cmp(&rhs[i].final_) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    for i in 0..len {
        match lhs[i].tone.cmp(&rhs[i].tone) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// Decodes a `SingleGram` blob into `(total_freq, records)`.
///
/// Returns `None` for a short total or a partial 8-byte record.
#[must_use]
pub fn decode_single_gram(bytes: &[u8]) -> Option<(u32, Vec<SingleGramRecord>)> {
    if !is_complete(bytes) {
        return None;
    }
    Some((get_total_freq(bytes)?, retrieve_all(bytes).collect()))
}

/// Loads and installs one heap-backed phrase library.
///
/// # Errors
/// Returns read, checksum, layout, and out-of-range library errors.
pub fn load_heap_library(
    libraries: &mut PhraseLibraries,
    nibble: u8,
    path: &Path,
) -> Result<bool, LibraryError> {
    let library = load_heap(path)?;
    if nibble >= 16 {
        return Err(LibraryError::Format("library nibble out of range".into()));
    }
    libraries.insert_mapping(nibble, path.to_path_buf(), library);
    Ok(true)
}

/// Encodes the phrase-index token row in stored order.
#[must_use]
pub fn encode_tokens(tokens: &[u32]) -> Vec<u8> {
    tokens
        .iter()
        .flat_map(|token| token.to_le_bytes())
        .collect()
}

#[cfg(test)]
mod tests;
