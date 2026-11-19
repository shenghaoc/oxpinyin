//! Lazy reader for libpinyin's `bigram.db` — direct DBM consumption.
//!
//! libpinyin stores the bigram as a **`HashDB`** (KC) / **`HashDBM`** (Tkrzw),
//! while the other DBM files use TreeDB/TreeDBM. The store's
//! `RawReadStore::open_hash_read_only` selects the correct container class.
//!
//! - **Key:** previous `phrase_token_t` as 4 bytes LE
//! - **Value:** `total:u32` + `{next_token:u32, count:u32}[]`
//!
//! The value schema is byte-identical to what `BigramLanguageModel::open`
//! already parses. The difference is access pattern: `BigramLanguageModel`
//! eagerly slurps every row at init, while this reader does lazy per-key
//! `get_raw` lookups.
//!
//! See `docs/findings/data-formats.md` §6
//! and `ngram_tkrzwdb.cpp`.

use std::path::Path;

use crate::store::{DefaultStore, RawReadStore};

use crate::data::chewing_table::{ChewingDbm, DefaultDbm, RawChewingDbm};
use crate::data::dict::DictError;
use crate::data::lm::BigramRow;
// The key layout (`crate::data::row_format`) and the value layout
// (`crate::data::single_gram`) have one owner each.
use crate::data::row_format::bigram::parse_value as parse_bigram_value;
use crate::data::row_format::encode_token_key;
#[cfg(test)]
use crate::data::single_gram::encode_single_gram;
use crate::data::table::TableError;

// ── BigramTable ──────────────────────────────────────────────────

/// A lazy, read-only view over a bigram DBM.
///
/// Does not materialize the entire database at open time. Each
/// `load_successors` call reads the attached DBM, including sentence start.
pub struct BigramTable<D = DefaultDbm> {
    dbm: D,
}

impl BigramTable {
    pub(crate) fn attach(path: &Path) -> Result<Self, DictError> {
        Ok(Self::new(DefaultDbm::attach(path, true)?))
    }

    /// Opens a bigram DBM lazily (no scan). libpinyin's `bigram.db` is
    /// a KC **`HashDB`** / Tkrzw **`HashDBM`**, so the open goes through
    /// `RawReadStore::open_hash_read_only`.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the file cannot be opened read-only.
    pub fn open(path: &Path) -> Result<Self, DictError> {
        let store = DefaultStore::open_hash_read_only(path).map_err(TableError::from)?;
        Ok(Self::new(RawChewingDbm::new(store)))
    }
}

impl<D: ChewingDbm> BigramTable<D> {
    pub(crate) const fn new(dbm: D) -> Self {
        Self { dbm }
    }

    pub(crate) fn finish_attachment(&mut self) {
        self.dbm.finish_attachment();
    }

    #[cfg(test)]
    pub(crate) fn attachment_finished(&self) -> bool {
        self.dbm.attachment_finished()
    }

    /// Loads successor records for `prev_token`.
    ///
    /// Returns `None` when the token has no bigram entry.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read fails, or `Parse` when a
    /// stored value is malformed (an empty value is corruption, reported
    /// rather than masked as a clean miss).
    pub fn load_successors(&self, prev_token: u32) -> Result<Option<BigramRow>, DictError> {
        self.load_checked_successors(prev_token)?
            .map(|value| parse_bigram_value(&value))
            .transpose()
    }

    /// Reads one encoded successor value from the DBM as stored, without
    /// building a decoded record array. N-best retains these bytes for only
    /// its current step. Every load reads the attached handle, including
    /// sentence start: `ngram_bdb.cpp:140`, `ngram_kyotodb.cpp:131,138-139`
    /// and `ngram_tkrzwdb.cpp:111` at `074a2219`. The pin's `load` keeps any
    /// stored value, so a value of any length comes back unchanged; the
    /// reads over it decide where the pin asserts or reads past it.
    ///
    /// # Errors
    /// Returns [`DictError`] for a read failure the pin asserts on: a body
    /// read that fails after a good size check. Every other failed read is a
    /// miss.
    pub fn load_raw_successors(&self, prev_token: u32) -> Result<Option<Vec<u8>>, DictError> {
        let key = encode_token_key(prev_token);
        let mut value = Vec::new();
        if !self.dbm.get_bigram(&key, &mut value)? {
            return Ok(None);
        }
        Ok(Some(value))
    }

    /// [`Self::load_raw_successors`] for the callers that decode the value
    /// whole: a value that is not a total and whole items is refused.
    ///
    /// # Errors
    /// Returns the read errors of [`Self::load_raw_successors`], or `Parse`
    /// when the stored value is malformed (an empty value is corruption,
    /// reported rather than masked as a clean miss).
    pub(crate) fn load_checked_successors(
        &self,
        prev_token: u32,
    ) -> Result<Option<Vec<u8>>, DictError> {
        let Some(value) = self.load_raw_successors(prev_token)? else {
            return Ok(None);
        };
        if value.is_empty() {
            // A present but empty value is malformed: a stored bigram
            // always carries at least its header. Report the corruption
            // rather than masking it as a clean miss.
            return Err(DictError::Parse(format!(
                "bigram entry for token {prev_token:#010x} has an empty value"
            )));
        }
        if value.len() < 4 || !(value.len() - 4).is_multiple_of(8) {
            return Err(DictError::Parse(format!(
                "bigram value length {} is not 4 + 8n",
                value.len()
            )));
        }
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests;
