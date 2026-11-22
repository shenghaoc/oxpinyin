//! Reads bigram successors on demand from the hash database. Each lookup uses the
//! previous token's four little-endian bytes as the key. Its value contains the stored
//! total and successor-token/count pairs.

// Bigram::load, ngram_tkrzwdb.cpp:86–122. See docs/findings/data-formats.md §6; Kyoto
// Cabinet uses HashDB and tkrzw uses HashDBM.

use std::path::Path;

use crate::data::DictError;
use crate::data::chewing_table::{ChewingDbm, DefaultDbm};
// The key layout (`crate::data::row_format`) and the value layout
// (`crate::single_gram`) have one owner each.
use crate::data::row_format::encode_token_key;
#[cfg(test)]
use crate::single_gram::encode_single_gram;

// ── BigramTable ──────────────────────────────────────────────────

/// A lazy, read-only view over a bigram DBM.
///
/// Does not materialize the entire database at open time. Each
/// `load_raw_successors` call reads the attached DBM, including sentence start.
pub struct BigramTable<D = DefaultDbm> {
    dbm: D,
}

impl BigramTable {
    pub(crate) fn attach(path: &Path) -> Result<Self, DictError> {
        Ok(Self::new(DefaultDbm::attach(path, true)?))
    }
}

impl<D: ChewingDbm> BigramTable<D> {
    /// Wraps an open native DBM.
    pub const fn new(dbm: D) -> Self {
        Self { dbm }
    }

    pub(crate) fn finish_attachment(&mut self) {
        self.dbm.finish_attachment();
    }

    #[cfg(test)]
    pub(crate) fn attachment_finished(&self) -> bool {
        self.dbm.attachment_finished()
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
        if !crate::single_gram::is_complete(&value) {
            return Err(DictError::Parse);
        }
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests;
