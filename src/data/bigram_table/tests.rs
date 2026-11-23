use super::*;

use std::sync::Mutex;

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
            Err(()) => Err(DictError::Table(crate::data::TableError::Store(
                crate::store::StoreError::ReadAfterCheck,
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
            Err(DictError::Table(crate::data::TableError::Store(
                crate::store::StoreError::ReadAfterCheck
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

impl crate::store::RawReadStore for Shared {
    fn open_read_only(_path: &Path) -> Result<Self, crate::store::StoreError> {
        unreachable!("the double is built in memory")
    }

    fn get_raw(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, crate::store::StoreError> {
        let token = u32::from_le_bytes(key.try_into().expect("a four-byte token key"));
        self.0.reads.lock().unwrap().push(token);
        value.extend_from_slice(b"stale");
        if self.0.backend.contains(&token) {
            return Err(crate::store::StoreError::Backend("injected".into()));
        }
        if self.0.after_check.contains(&token) {
            return Err(crate::store::StoreError::ReadAfterCheck);
        }
        value.clear();
        Ok(false)
    }

    fn contains_raw(&self, _key: &[u8]) -> Result<bool, crate::store::StoreError> {
        unreachable!("the bigram reader never checks")
    }

    fn scan_raw(
        &self,
        _lo: std::ops::Bound<&[u8]>,
        _hi: std::ops::Bound<&[u8]>,
        _visit: impl FnMut(&[u8], &[u8]) -> Result<crate::store::Flow, crate::store::StoreError>,
    ) -> Result<(), crate::store::StoreError> {
        unreachable!("the bigram reader never scans")
    }
}

fn failing(
    backend: &[u32],
    after_check: &[u32],
) -> (
    BigramTable<crate::data::chewing_table::RawChewingDbm<Shared>>,
    std::sync::Arc<FailingStore>,
) {
    let store = std::sync::Arc::new(FailingStore {
        backend: backend.to_vec(),
        after_check: after_check.to_vec(),
        reads: Mutex::new(Vec::new()),
    });
    let table = BigramTable::new(crate::data::chewing_table::RawChewingDbm::new(Shared(
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
        Err(DictError::Table(crate::data::TableError::Store(
            crate::store::StoreError::ReadAfterCheck
        )))
    ));
}
