//! The in-memory [`ChewingDbm`] test double the punct and bigram table
//! unit tests share: a `BTreeMap` of raw rows behind the same trait the
//! production [`crate::chewing_table::DefaultDbm`] implements, whose
//! `walk` is unreachable because neither reader scans. Test-only:
//! `lib.rs` declares this module under `#[cfg(test)]`.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::chewing_table::ChewingDbm;
use crate::dict::DictError;

/// Raw rows behind `get` and `contains`; a test over a reader that
/// starts walking fails here.
pub(crate) struct MemoryDbm {
    data: Mutex<BTreeMap<Vec<u8>, Vec<u8>>>,
}

impl MemoryDbm {
    pub(crate) fn new() -> Self {
        Self {
            data: Mutex::new(BTreeMap::new()),
        }
    }

    pub(crate) fn put(&self, key: Vec<u8>, value: Vec<u8>) {
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
        unreachable!("this reader never walks")
    }
}
