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
//! unopenable file is an empty table (`PunctTable::attach`).

use std::path::Path;

use crate::vocab::PhraseText;

use crate::data::DictError;
use crate::data::chewing_table::DefaultDbm;
// The key and value layout this reader shares with `gen_binary_files`'s
// writer: one written copy (`crate::data::row_format`).
use crate::data::row_format::encode_token_key;
use crate::data::row_format::punct::decode_puncts;

/// The read-only punctuation table over a DBM handle.
///
/// Does not materialize the table at open time. Each lookup is a single
/// point read.
pub struct PunctTable {
    dbm: Option<DefaultDbm>,
}

impl PunctTable {
    pub(crate) const fn new(dbm: DefaultDbm) -> Self {
        Self { dbm: Some(dbm) }
    }

    /// An empty table: every lookup returns no punctuation.
    #[must_use]
    pub const fn empty() -> Self {
        Self { dbm: None }
    }

    /// Whether a table file is open behind this handle.
    #[cfg(test)]
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.dbm
            .as_ref()
            .is_some_and(|dbm| !dbm.attachment_failed())
    }

    /// Closes the table's native object once at facade fini
    /// (`pinyin.cpp:1215` at 074a2219).
    pub(crate) fn finish_attachment(&mut self) {
        if let Some(dbm) = self.dbm.as_mut() {
            dbm.finish_attachment();
        }
    }

    /// Whether the native object is closed; `true` for an empty table,
    /// which holds none.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn attachment_finished(&self) -> bool {
        self.dbm
            .as_ref()
            .is_none_or(DefaultDbm::attachment_finished)
    }

    // PunctTable::load_entry, punct_table_tkrzwdb.cpp:58–73.
    /// Returns punctuation phrases for `token` in stored order. An empty table, a missing
    /// token or an empty value yields an empty vector.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] if the read fails.
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
        Ok(Self::new(DefaultDbm::attach(
            path,
            crate::store::DbmKind::Tree,
        )?))
    }
}

#[cfg(test)]
mod tests;
