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

use oxpinyin_store::{DefaultStore, RawReadStore};

use crate::chewing_table::{ChewingDbm, DefaultDbm, RawChewingDbm};
use crate::dict::DictError;
use crate::lm::BigramRow;
// The key layout (`crate::row_format`) and the value layout
// (`crate::single_gram`) have one owner each.
use crate::row_format::bigram::parse_value as parse_bigram_value;
use crate::row_format::encode_token_key;
#[cfg(test)]
use crate::single_gram::encode_single_gram;
use crate::table::TableError;

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

    pub(crate) fn attachment_failed(&self) -> bool {
        self.dbm.attachment_failed()
    }

    pub(crate) fn finish_failed_attachment(&self) {
        self.dbm.finish_failed_attachment();
    }

    pub(crate) fn failed_attachment_finished(&self) -> bool {
        self.dbm.failed_attachment_finished()
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
        self.load_raw_successors(prev_token)?
            .map(|value| parse_bigram_value(&value))
            .transpose()
    }

    /// Reads one encoded successor value from the DBM without building a
    /// decoded record array. N-best retains these bytes for only its
    /// current step. Every load reads the attached handle, including
    /// sentence start: `ngram_bdb.cpp:140`, `ngram_kyotodb.cpp:131,138-139`
    /// and `ngram_tkrzwdb.cpp:111` at `074a2219`.
    ///
    /// # Errors
    /// Returns the same read and schema errors as [`Self::load_successors`],
    /// including a present empty value.
    pub fn load_raw_successors(&self, prev_token: u32) -> Result<Option<Vec<u8>>, DictError> {
        let key = encode_token_key(prev_token);
        let mut value = Vec::new();
        if !self.dbm.get_bigram(&key, &mut value)? {
            return Ok(None);
        }
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

    /// Returns `(count, total)` for the `prev → next` transition.
    ///
    /// # Errors
    ///
    /// Propagates [`DictError`] from [`Self::load_successors`].
    pub fn transition(&self, prev: u32, next: u32) -> Result<Option<(u32, u32)>, DictError> {
        let Some(row) = self.load_successors(prev)? else {
            return Ok(None);
        };
        let count = row
            .records
            .iter()
            .find(|(next_token, _)| *next_token == next)
            .map_or(0, |(_, count)| *count);
        Ok(Some((count, row.total)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use crate::memory_dbm::MemoryDbm;

    #[test]
    fn load_successors_miss_returns_none() {
        let dbm = MemoryDbm::new();
        let table = BigramTable::new(dbm);
        assert!(table.load_successors(0x0100_0010).unwrap().is_none());
    }

    #[test]
    fn transition_finds_count() {
        let dbm = MemoryDbm::new();
        dbm.put(
            0x0100_0010_u32.to_le_bytes().to_vec(),
            encode_single_gram(100, &[(0x0100_0099, 60), (0x0100_00A0, 40)]),
        );
        let table = BigramTable::new(dbm);
        let (count, total) = table.transition(0x0100_0010, 0x0100_0099).unwrap().unwrap();
        assert_eq!(count, 60);
        assert_eq!(total, 100);
    }

    #[test]
    fn transition_absent_next_returns_zero_count() {
        let dbm = MemoryDbm::new();
        dbm.put(
            0x0100_0010_u32.to_le_bytes().to_vec(),
            encode_single_gram(100, &[(0x0100_0099, 100)]),
        );
        let table = BigramTable::new(dbm);
        let (count, total) = table.transition(0x0100_0010, 0xFFFF_FFFF).unwrap().unwrap();
        assert_eq!(count, 0);
        assert_eq!(total, 100);
    }

    /// A DBM that answers each read from a script and counts the reads.
    struct ScriptedDbm {
        answers: Mutex<Vec<Result<Option<Vec<u8>>, ()>>>,
        reads: Mutex<Vec<Vec<u8>>>,
    }

    impl ScriptedDbm {
        fn new(mut answers: Vec<Result<Option<Vec<u8>>, ()>>) -> Self {
            answers.reverse();
            Self {
                answers: Mutex::new(answers),
                reads: Mutex::new(Vec::new()),
            }
        }
    }

    impl ChewingDbm for &ScriptedDbm {
        fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
            self.reads.lock().unwrap().push(key.to_vec());
            value.clear();
            match self.answers.lock().unwrap().pop().expect("a scripted read") {
                Ok(Some(stored)) => {
                    value.extend_from_slice(&stored);
                    Ok(true)
                }
                Ok(None) => Ok(false),
                Err(()) => Err(DictError::Table(crate::TableError::Store(
                    oxpinyin_store::StoreError::ReadAfterCheck,
                ))),
            }
        }

        fn contains(&self, _key: &[u8]) -> Result<bool, DictError> {
            unreachable!("the bigram reader never checks")
        }

        fn walk(
            &self,
            _lo: &[u8],
            _hi: Option<&[u8]>,
            _visit: impl FnMut(&[u8], &[u8]) -> Result<bool, DictError>,
        ) -> Result<(), DictError> {
            unreachable!("the bigram reader never walks")
        }
    }

    fn stored(total: u32, records: &[(u32, u32)]) -> Vec<u8> {
        encode_single_gram(total, records)
    }

    /// Every `Bigram::load` calls the attached DBM, including key 1
    /// (`ngram_bdb.cpp:127-145`, `ngram_kyotodb.cpp:124-142`,
    /// `ngram_tkrzwdb.cpp:102-121` at `074a2219`).
    #[test]
    fn every_predecessor_is_read_on_each_load() {
        for token in [1, 0x0100_0010] {
            let first = stored(3, &[(0x0100_0002, 3)]);
            let second = stored(5, &[(0x0100_0002, 5)]);
            let dbm = ScriptedDbm::new(vec![
                Ok(Some(first.clone())),
                Ok(Some(second.clone())),
                Ok(None),
                Err(()),
            ]);
            let table = BigramTable::new(&dbm);
            assert_eq!(table.load_raw_successors(token).unwrap(), Some(first));
            assert_eq!(table.load_raw_successors(token).unwrap(), Some(second));
            assert!(table.load_raw_successors(token).unwrap().is_none());
            assert!(matches!(
                table.load_raw_successors(token),
                Err(DictError::Table(crate::TableError::Store(
                    oxpinyin_store::StoreError::ReadAfterCheck
                )))
            ));
            assert_eq!(*dbm.reads.lock().unwrap(), vec![encode_token_key(token); 4]);
        }
    }

    /// A system store whose raw reads fail for chosen predecessors, with
    /// the failure the pin's `Bigram::load` meets: an ordinary backend
    /// failure, or Kyoto Cabinet's body read after a good size check. It
    /// logs every read.
    struct FailingStore {
        backend: Vec<u32>,
        after_check: Vec<u32>,
        reads: Mutex<Vec<u32>>,
    }

    struct Shared(std::sync::Arc<FailingStore>);

    impl oxpinyin_store::RawReadStore for Shared {
        fn open_read_only(_path: &Path) -> Result<Self, oxpinyin_store::StoreError> {
            unreachable!("the double is built in memory")
        }

        fn get_raw(
            &self,
            key: &[u8],
            value: &mut Vec<u8>,
        ) -> Result<bool, oxpinyin_store::StoreError> {
            let token = u32::from_le_bytes(key.try_into().expect("a four-byte token key"));
            self.0.reads.lock().unwrap().push(token);
            value.extend_from_slice(b"stale");
            if self.0.backend.contains(&token) {
                return Err(oxpinyin_store::StoreError::Backend("injected".into()));
            }
            if self.0.after_check.contains(&token) {
                return Err(oxpinyin_store::StoreError::ReadAfterCheck);
            }
            value.clear();
            Ok(false)
        }

        fn contains_raw(&self, _key: &[u8]) -> Result<bool, oxpinyin_store::StoreError> {
            unreachable!("the bigram reader never checks")
        }

        fn scan_raw(
            &self,
            _lo: std::ops::Bound<&[u8]>,
            _hi: std::ops::Bound<&[u8]>,
            _visit: impl FnMut(&[u8], &[u8]) -> Result<oxpinyin_store::Flow, oxpinyin_store::StoreError>,
        ) -> Result<(), oxpinyin_store::StoreError> {
            unreachable!("the bigram reader never scans")
        }
    }

    fn failing(
        backend: &[u32],
        after_check: &[u32],
    ) -> (
        BigramTable<crate::chewing_table::RawChewingDbm<Shared>>,
        std::sync::Arc<FailingStore>,
    ) {
        let store = std::sync::Arc::new(FailingStore {
            backend: backend.to_vec(),
            after_check: after_check.to_vec(),
            reads: Mutex::new(Vec::new()),
        });
        let table = BigramTable::new(crate::chewing_table::RawChewingDbm::new(Shared(
            store.clone(),
        )));
        (table, store)
    }

    /// `Bigram::load` answers a failed read with false and a NULL gram
    /// (`ngram_bdb.cpp:140-142`, `ngram_tkrzwdb.cpp:111-114` at 074a2219).
    #[test]
    fn a_failed_read_is_a_miss_with_the_value_cleared() {
        let (table, _store) = failing(&[0x0100_0010], &[]);
        let dbm = &table.dbm;
        let mut value = b"left over".to_vec();
        assert!(!ChewingDbm::get_bigram(dbm, &encode_token_key(0x0100_0010), &mut value).unwrap());
        assert!(value.is_empty());
        assert!(table.load_raw_successors(0x0100_0010).unwrap().is_none());
    }

    /// Kyoto Cabinet's body read after a good size check is the pin's
    /// `check_result` assert (`ngram_kyotodb.cpp:138`): not a miss.
    #[test]
    fn a_read_after_check_failure_stays_an_error() {
        let (table, _store) = failing(&[], &[0x0100_0010]);
        assert!(matches!(
            table.load_raw_successors(0x0100_0010),
            Err(DictError::Table(crate::TableError::Store(
                oxpinyin_store::StoreError::ReadAfterCheck
            )))
        ));
    }
}
