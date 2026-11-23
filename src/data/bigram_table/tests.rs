use super::*;

use std::sync::Mutex;

struct ScriptedDbm {
    answers: Mutex<Vec<Option<Vec<u8>>>>,
    reads: Mutex<Vec<Vec<u8>>>,
}

impl ScriptedDbm {
    fn new(mut answers: Vec<Option<Vec<u8>>>) -> Self {
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
        let stored = self.answers.lock().unwrap().pop().expect("a scripted read");
        if let Some(bytes) = stored {
            value.extend(bytes);
            Ok(true)
        } else {
            Ok(false)
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

// 074a2219 ngram_tkrzwdb.cpp:102-121 reads the attached DBM on each load.
#[test]
fn every_predecessor_is_read_on_each_load() {
    let token = 1;
    let first = stored(3, &[(0x0100_0002, 3)]);
    let second = stored(5, &[(0x0100_0002, 5)]);
    let dbm = ScriptedDbm::new(vec![Some(first.clone()), Some(second.clone()), None]);
    let table = BigramTable::new(&dbm);
    assert_eq!(table.load_raw_successors(token).unwrap(), Some(first));
    assert_eq!(table.load_raw_successors(token).unwrap(), Some(second));
    assert!(table.load_raw_successors(token).unwrap().is_none());
    assert_eq!(*dbm.reads.lock().unwrap(), vec![encode_token_key(token); 3]);
}

struct FailingStore {
    after_check: bool,
}

impl crate::store::RawReadStore for FailingStore {
    fn open_read_only(_path: &Path) -> Result<Self, crate::store::StoreError> {
        unreachable!("the double is built in memory")
    }

    fn get_raw(&self, _key: &[u8], value: &mut Vec<u8>) -> Result<bool, crate::store::StoreError> {
        value.extend_from_slice(b"stale");
        Err(if self.after_check {
            crate::store::StoreError::ReadAfterCheck
        } else {
            crate::store::StoreError::Backend("injected".into())
        })
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

// 074a2219 ngram_tkrzwdb.cpp:111-114 and ngram_kyotodb.cpp:131-139 distinguish a miss from a checked body failure.
#[test]
fn native_read_failures_follow_the_check_result() {
    let token = 0x0100_0010;
    for after_check in [false, true] {
        let dbm = crate::data::chewing_table::RawChewingDbm::new(FailingStore { after_check });
        let mut value = b"left over".to_vec();
        let result = dbm.get_bigram(&encode_token_key(token), &mut value);
        if after_check {
            assert!(matches!(
                result,
                Err(DictError::Table(crate::data::TableError::Store(
                    crate::store::StoreError::ReadAfterCheck
                )))
            ));
        } else {
            assert!(!result.unwrap());
            assert!(value.is_empty());
        }
    }
}
