//! libpinyin's `punct.bin` — the predicted-punctuation table, read
//! directly.
//!
//! libpinyin stores the punctuation table as a **`TreeDB`** (KC) /
//! **`TreeDBM`** (Tkrzw) — same container class as the pinyin and phrase
//! indexes. The key is a `phrase_token_t` (4 bytes LE); the value is a
//! raw UCS-4 stream (`PunctTableEntry::escape`, `punct_table.cpp:40-54`):
//! each punctuation is its UCS-4 codepoints followed by a u32 zero
//! terminator, with successive punctuations concatenated — e.g. `，` then
//! `、` stores `[0xFF0C, 0][0x3001, 0]` as little-endian u32s.
//!
//! Every lookup is one point read; nothing is walked at open. Upstream's
//! `pinyin_init` ignores a failed `PunctTable::attach`, so a missing or
//! unopenable file is an empty table ([`PunctTable::open_optional`]).

use std::path::Path;

use oxpinyin_core::PhraseText;
use oxpinyin_store::{DefaultStore, RawReadStore};

use crate::chewing_table::{ChewingDbm, DefaultDbm, RawChewingDbm};
use crate::dict::DictError;
// The key and value layout this reader shares with `gen_binary_files`'s
// writer: one written copy (`crate::row_format`).
use crate::row_format::encode_token_key;
use crate::row_format::punct::decode_puncts;
#[cfg(test)]
use crate::row_format::punct::encode_puncts;
use crate::table::TableError;

/// The read-only punctuation table over a DBM handle.
///
/// Does not materialize the table at open time. Each lookup is a single
/// point read.
pub struct PunctTable<D = DefaultDbm> {
    dbm: Option<D>,
}

impl<D: ChewingDbm> PunctTable<D> {
    pub(crate) const fn new(dbm: D) -> Self {
        Self { dbm: Some(dbm) }
    }

    /// An empty table: every lookup returns no punctuation.
    #[must_use]
    pub const fn empty() -> Self {
        Self { dbm: None }
    }

    /// Whether a table file is open behind this handle.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.dbm
            .as_ref()
            .is_some_and(|dbm| !dbm.attachment_failed())
    }

    /// Resets the retained failed object once at facade fini.
    pub fn finish_failed_attachment(&self) {
        if let Some(dbm) = self.dbm.as_ref() {
            dbm.finish_failed_attachment();
        }
    }

    /// Whether this table holds the retained native object of a failed
    /// attach, as the pin's `PunctTable` does after `attach` returns false
    /// (`punct_table_tkrzwdb.cpp:70-72` at 074a2219). An empty table holds
    /// none.
    #[must_use]
    pub fn retains_failed_attachment(&self) -> bool {
        self.dbm.as_ref().is_some_and(|dbm| dbm.attachment_failed())
    }

    /// Whether the retained failed object has been finalized.
    #[must_use]
    pub fn failed_attachment_finished(&self) -> bool {
        self.dbm
            .as_ref()
            .is_some_and(|dbm| dbm.failed_attachment_finished())
    }

    /// Punctuations stored for `token`, in table order, as stored words
    /// (`decode_puncts`).
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the read fails.
    pub fn punctuations(&self, token: u32) -> Result<Vec<PhraseText>, DictError> {
        let Some(dbm) = self.dbm.as_ref() else {
            return Ok(Vec::new());
        };
        let key = encode_token_key(token);
        let mut value = Vec::new();
        // Upstream's load_entry conflates absent and empty (0 == value.size());
        // remove_punctuation can leave a stored chunk at size 0. A miss
        // leaves the buffer empty, so one test covers both.
        dbm.get(&key, &mut value)?;
        if value.is_empty() {
            return Ok(Vec::new());
        }
        Ok(decode_puncts(&value))
    }
}

impl PunctTable {
    /// Attaches the runtime punctuation file, retaining a failed native
    /// object (`PunctTable::attach`, `punct_table_tkrzwdb.cpp:58-73` and its
    /// BDB and KC peers at 074a2219).
    ///
    /// # Errors
    /// Returns failures before native attach.
    pub fn attach(path: &Path) -> Result<Self, DictError> {
        Ok(Self::new(DefaultDbm::attach(path, false)?))
    }
    /// Opens a punct DBM lazily (no scan). `punct.bin` is a KC
    /// **`TreeDB`** / Tkrzw **`TreeDBM`** — the plain read-only open.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the file cannot be opened read-only.
    pub fn open(path: &Path) -> Result<Self, DictError> {
        let store = DefaultStore::open_read_only(path).map_err(TableError::from)?;
        Ok(Self::new(RawChewingDbm::new(store)))
    }

    /// Opens `path` when it is a readable table; otherwise empty —
    /// upstream `pinyin_init` ignores a failed `PunctTable::attach`.
    #[must_use]
    pub fn open_optional(path: &Path) -> Self {
        if !path.is_file() {
            return Self::empty();
        }
        Self::open(path).unwrap_or_else(|_| Self::empty())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_missing_file_leaves_the_retained_failed_object_not_an_empty_table() {
        // `PunctTable::attach` allocates the TreeDBM and returns the failed
        // Open's answer with the object still owned (`punct_table_tkrzwdb.cpp:
        // 70-72`, `punct_table_kyotodb.cpp:37-47`, BDB `db_create` then
        // `open`); the facade ignores the answer (`pinyin.cpp:440`) and
        // later calls that object. An empty table would call nothing.
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-data-punct-missing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let table = super::PunctTable::attach(&dir.join("punct.bin")).unwrap();
        assert!(table.retains_failed_attachment());
        assert!(!table.is_open());
        assert!(!table.failed_attachment_finished());
        table.finish_failed_attachment();
        assert!(table.failed_attachment_finished());
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    use crate::memory_dbm::MemoryDbm;

    #[test]
    fn an_empty_table_answers_nothing() {
        let table = PunctTable::<MemoryDbm>::empty();
        assert!(!table.is_open());
        assert!(table.punctuations(0x0100_0295).unwrap().is_empty());
        assert!(!PunctTable::open_optional(std::path::Path::new("/no/such/punct.bin")).is_open());
    }

    #[test]
    fn lookup_finds_punctuation() {
        let dbm = MemoryDbm::new();
        let token: u32 = 0x0100_0295;
        let value = encode_puncts(&["，", "。"]);
        dbm.put(token.to_le_bytes().to_vec(), value);

        let table = PunctTable::new(dbm);
        let result = table.punctuations(token).unwrap();
        assert_eq!(result, ["，", "。"]);
    }

    #[test]
    fn lookup_miss_returns_empty() {
        let dbm = MemoryDbm::new();
        let table = PunctTable::new(dbm);
        let result = table.punctuations(0x0100_0295).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn partial_word_has_no_punctuation() {
        let dbm = MemoryDbm::new();
        dbm.put(0x0100_0295_u32.to_le_bytes().to_vec(), vec![0xFF; 3]);
        let table = PunctTable::new(dbm);
        assert!(table.punctuations(0x0100_0295).unwrap().is_empty());
    }
}
