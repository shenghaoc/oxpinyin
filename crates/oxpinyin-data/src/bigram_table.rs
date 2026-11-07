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
//! See `docs/findings/libpinyin-system-data-formats-2026-09-01.md` §1.1
//! and `ngram_tkrzwdb.cpp`.

use std::path::Path;
use std::sync::OnceLock;

use oxpinyin_core::SharedGram;
use oxpinyin_store::{DefaultStore, RawReadStore};

use crate::chewing_table::{ChewingDbm, DefaultDbm, RawChewingDbm};
use crate::dict::DictError;
use crate::lm::BigramRow;
// The key and value layout this reader shares with `import_interpolation`'s
// writer: one written copy (`crate::row_format`).
#[cfg(test)]
use crate::row_format::bigram::encode_value as encode_bigram_value;
use crate::row_format::bigram::parse_value as parse_bigram_value;
use crate::row_format::encode_token_key;
use crate::table::TableError;

// ── BigramTable ──────────────────────────────────────────────────

/// `sentence_start` (`novel_types.h:122` at the pin `074a2219`): the
/// predecessor every guess starts from (`pinyin.cpp:1376-1377`).
const SENTENCE_START: u32 = 1;

/// A lazy, read-only view over a bigram DBM.
///
/// Does not materialize the entire database at open time. Each
/// `load_successors` call is a single DBM `get_raw`, except for
/// `sentence_start`, whose value is read once ([`Self::load_gram`]).
pub struct BigramTable<D = DefaultDbm> {
    dbm: D,
    /// `sentence_start`'s stored value, taken by the first read that finds
    /// and validates it and kept until the table drops. The file is
    /// attached read-only for the table's lifetime (`pinyin.cpp:394-396`,
    /// `:1208`; [`Self::attach`], [`Self::open`]) and nothing here writes
    /// it. The slot is this attachment's alone ([`Self::load_gram`]).
    sentence_start: OnceLock<SharedGram>,
}

/// One validated system gram value.
pub enum SystemGram {
    /// The caller's own copy of the stored value.
    Owned(Vec<u8>),
    /// The table's one copy of `sentence_start`'s stored value.
    Shared(SharedGram),
}

impl SystemGram {
    /// The stored value's bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Owned(value) => value,
            Self::Shared(value) => value.as_bytes(),
        }
    }
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
        Self {
            dbm,
            sentence_start: OnceLock::new(),
        }
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
        self.load_gram(prev_token)?
            .map(|value| parse_bigram_value(value.as_bytes()))
            .transpose()
    }

    /// Loads one encoded successor value: `sentence_start`'s from the
    /// table's one copy, any other predecessor's from the DBM.
    ///
    /// The first read that finds and validates `sentence_start`'s value
    /// keeps it; a miss or an error keeps nothing and the next call reads
    /// again. The value is the largest in the file and every guess reads
    /// it (`phonetic_lookup.h:588-596` over the initial step,
    /// `pinyin.cpp:2210-2213` at offset zero).
    ///
    /// Maintainer ruling, 2026-11-06, "sentence-start system bigram
    /// caching" (`docs/findings/compatibility-policy.md`): for one
    /// attachment of the system `bigram.db`, the successfully read and
    /// validated `sentence_start` value may be kept and reused without
    /// repeating the native DBM read. A failure that only an omitted later
    /// read would have met is therefore not observed on a hit. Attachment
    /// failures, misses, read failures and validation failures before a
    /// value is kept behave as before. External in-place modification of
    /// the attached file is outside the supported contract for the kept
    /// value. The slot belongs to this table, which owns the attached
    /// handle: a fresh attachment builds a fresh table and inherits
    /// nothing. No other system file and no user data is covered.
    ///
    /// The pin reads the file through the one handle `Bigram::attach`
    /// opened, on every backend: `m_db->get` (`ngram_bdb.cpp:110-125`,
    /// `:140`), `m_db->check` then `m_db->get` (`ngram_kyotodb.cpp:110-120`,
    /// `:131`, `:138`) and `m_db->Get` (`ngram_tkrzwdb.cpp:86-99`, `:111`)
    /// at `074a2219`. It attaches once per context (`pinyin.cpp:394-396`,
    /// `zhuyin.cpp:336-338`) and closes the handle only when the context is
    /// freed (`pinyin.cpp:1208`, `zhuyin.cpp:747`). A package upgrade that
    /// replaces the file by rename is therefore invisible to the pin too:
    /// it keeps reading the file it opened.
    ///
    /// # Errors
    /// Returns the same read and schema errors as
    /// [`Self::load_raw_successors`].
    pub fn load_gram(&self, prev_token: u32) -> Result<Option<SystemGram>, DictError> {
        if prev_token != SENTENCE_START {
            return Ok(self.load_raw_successors(prev_token)?.map(SystemGram::Owned));
        }
        if let Some(shared) = self.sentence_start.get() {
            return Ok(Some(SystemGram::Shared(shared.clone())));
        }
        let Some(value) = self.load_raw_successors(prev_token)? else {
            return Ok(None);
        };
        let shared = self.sentence_start.get_or_init(|| SharedGram::new(value));
        Ok(Some(SystemGram::Shared(shared.clone())))
    }

    /// Reads one encoded successor value from the DBM without building a
    /// decoded record array. N-best retains these bytes for only its
    /// current step.
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

    use std::collections::BTreeMap;
    use std::sync::Mutex;

    struct MemoryDbm {
        data: Mutex<BTreeMap<Vec<u8>, Vec<u8>>>,
    }

    impl MemoryDbm {
        fn new() -> Self {
            Self {
                data: Mutex::new(BTreeMap::new()),
            }
        }

        fn put(&self, key: Vec<u8>, value: Vec<u8>) {
            self.data.lock().unwrap().insert(key, value);
        }
    }

    impl ChewingDbm for MemoryDbm {
        fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
            value.clear();
            let data = self.data.lock().unwrap();
            let stored = data.get(key);
            if let Some(stored) = stored {
                value.extend_from_slice(stored);
            }
            Ok(stored.is_some())
        }

        fn contains(&self, key: &[u8]) -> Result<bool, DictError> {
            Ok(self.data.lock().unwrap().contains_key(key))
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

    #[test]
    fn load_successors_finds_row() {
        let dbm = MemoryDbm::new();
        let row = BigramRow {
            total: 50,
            records: vec![(0x0100_0099, 50)],
        };
        dbm.put(
            0x0100_0010_u32.to_le_bytes().to_vec(),
            encode_bigram_value(&row),
        );
        let table = BigramTable::new(dbm);
        let result = table.load_successors(0x0100_0010).unwrap().unwrap();
        assert_eq!(result.total, 50);
        assert_eq!(result.records.len(), 1);
    }

    #[test]
    fn load_successors_miss_returns_none() {
        let dbm = MemoryDbm::new();
        let table = BigramTable::new(dbm);
        assert!(table.load_successors(0x0100_0010).unwrap().is_none());
    }

    #[test]
    fn transition_finds_count() {
        let dbm = MemoryDbm::new();
        let row = BigramRow {
            total: 100,
            records: vec![(0x0100_0099, 60), (0x0100_00A0, 40)],
        };
        dbm.put(
            0x0100_0010_u32.to_le_bytes().to_vec(),
            encode_bigram_value(&row),
        );
        let table = BigramTable::new(dbm);
        let (count, total) = table.transition(0x0100_0010, 0x0100_0099).unwrap().unwrap();
        assert_eq!(count, 60);
        assert_eq!(total, 100);
    }

    #[test]
    fn transition_absent_next_returns_zero_count() {
        let dbm = MemoryDbm::new();
        let row = BigramRow {
            total: 100,
            records: vec![(0x0100_0099, 100)],
        };
        dbm.put(
            0x0100_0010_u32.to_le_bytes().to_vec(),
            encode_bigram_value(&row),
        );
        let table = BigramTable::new(dbm);
        let (count, total) = table.transition(0x0100_0010, 0xFFFF_FFFF).unwrap().unwrap();
        assert_eq!(count, 0);
        assert_eq!(total, 100);
    }

    #[test]
    fn malformed_value_does_not_panic() {
        let dbm = MemoryDbm::new();
        dbm.put(0x0100_0010_u32.to_le_bytes().to_vec(), vec![0xFF; 5]);
        let table = BigramTable::new(dbm);
        assert!(table.load_successors(0x0100_0010).is_err());
        assert!(table.load_raw_successors(0x0100_0010).is_err());
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
        encode_bigram_value(&BigramRow {
            total,
            records: records.to_vec(),
        })
    }

    #[test]
    fn sentence_start_is_read_once_and_returns_the_stored_bytes() {
        let value = stored(9, &[(0x0100_0001, 4), (0x0100_0007, 5)]);
        let dbm = ScriptedDbm::new(vec![Ok(Some(value.clone()))]);
        let table = BigramTable::new(&dbm);
        for _ in 0..3 {
            let gram = table.load_gram(SENTENCE_START).unwrap().unwrap();
            assert!(matches!(gram, SystemGram::Shared(_)));
            assert_eq!(gram.as_bytes(), value);
        }
        assert_eq!(
            table
                .load_successors(SENTENCE_START)
                .unwrap()
                .unwrap()
                .total,
            9
        );
        assert_eq!(
            *dbm.reads.lock().unwrap(),
            [encode_token_key(SENTENCE_START)]
        );
    }

    #[test]
    fn other_predecessors_are_read_every_time() {
        let value = stored(3, &[(0x0100_0002, 3)]);
        let dbm = ScriptedDbm::new(vec![Ok(Some(value.clone())), Ok(Some(value.clone()))]);
        let table = BigramTable::new(&dbm);
        for _ in 0..2 {
            let gram = table.load_gram(0x0100_0010).unwrap().unwrap();
            assert!(matches!(gram, SystemGram::Owned(_)));
            assert_eq!(gram.as_bytes(), value);
        }
        assert_eq!(dbm.reads.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_miss_or_an_error_for_sentence_start_is_not_kept() {
        let value = stored(1, &[(0x0100_0003, 1)]);
        let dbm = ScriptedDbm::new(vec![
            Ok(None),
            Err(()),
            Ok(Some(Vec::new())),
            Ok(Some(vec![0xFF; 5])),
            Ok(Some(value.clone())),
        ]);
        let table = BigramTable::new(&dbm);
        assert!(table.load_gram(SENTENCE_START).unwrap().is_none());
        assert!(matches!(
            table.load_gram(SENTENCE_START),
            Err(DictError::Table(crate::TableError::Store(
                oxpinyin_store::StoreError::ReadAfterCheck
            )))
        ));
        // A present empty value, then one off the item grid.
        assert!(matches!(
            table.load_gram(SENTENCE_START),
            Err(DictError::Parse(_))
        ));
        assert!(matches!(
            table.load_gram(SENTENCE_START),
            Err(DictError::Parse(_))
        ));
        assert_eq!(
            table.load_gram(SENTENCE_START).unwrap().unwrap().as_bytes(),
            value
        );
        assert_eq!(
            table.load_gram(SENTENCE_START).unwrap().unwrap().as_bytes(),
            value
        );
        assert_eq!(dbm.reads.lock().unwrap().len(), 5);
    }

    #[test]
    fn checked_read_failure_is_not_a_miss() {
        // Kyoto Cabinet's `check`/`get` schedule: a record that the size
        // read finds and the body read then fails is the pin's assert, so
        // it must surface as the checked-read error, never as absence.
        struct CheckedDbm;
        impl ChewingDbm for CheckedDbm {
            fn get(&self, _key: &[u8], _value: &mut Vec<u8>) -> Result<bool, DictError> {
                Ok(false)
            }

            fn get_bigram(&self, _key: &[u8], _value: &mut Vec<u8>) -> Result<bool, DictError> {
                Err(DictError::Table(crate::TableError::Store(
                    oxpinyin_store::StoreError::ReadAfterCheck,
                )))
            }

            fn contains(&self, _key: &[u8]) -> Result<bool, DictError> {
                Ok(false)
            }

            fn walk(
                &self,
                _lo: &[u8],
                _hi: Option<&[u8]>,
                _visit: impl FnMut(&[u8], &[u8]) -> Result<bool, DictError>,
            ) -> Result<(), DictError> {
                Ok(())
            }
        }
        let table = BigramTable::new(CheckedDbm);
        assert!(matches!(
            table.load_raw_successors(0x0100_0010),
            Err(DictError::Table(crate::TableError::Store(
                oxpinyin_store::StoreError::ReadAfterCheck
            )))
        ));
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

    impl oxpinyin_store::ReadStore for Shared {
        fn open_read_only(_path: &Path) -> Result<Self, oxpinyin_store::StoreError> {
            unreachable!("the double is built in memory")
        }

        fn get(
            &self,
            _table: &str,
            key: &[u8],
            value: &mut Vec<u8>,
        ) -> Result<bool, oxpinyin_store::StoreError> {
            oxpinyin_store::RawReadStore::get_raw(self, key, value)
        }

        fn scan(
            &self,
            _table: &str,
            _lo: std::ops::Bound<&[u8]>,
            _hi: std::ops::Bound<&[u8]>,
            _visit: impl FnMut(&[u8], &[u8]) -> Result<oxpinyin_store::Flow, oxpinyin_store::StoreError>,
        ) -> Result<(), oxpinyin_store::StoreError> {
            unreachable!("the bigram reader never scans")
        }

        fn is_empty(&self, _table: &str) -> Result<bool, oxpinyin_store::StoreError> {
            unreachable!("the bigram reader never asks")
        }
    }

    impl oxpinyin_store::RawReadStore for Shared {
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
        assert!(table.load_gram(0x0100_0010).unwrap().is_none());
    }

    /// The miss is not kept: the pin reads the handle on every load.
    #[test]
    fn a_failed_sentence_start_read_is_read_again() {
        let (table, store) = failing(&[SENTENCE_START], &[]);
        for _ in 0..3 {
            assert!(table.load_gram(SENTENCE_START).unwrap().is_none());
        }
        assert_eq!(
            *store.reads.lock().unwrap(),
            [SENTENCE_START, SENTENCE_START, SENTENCE_START]
        );
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
