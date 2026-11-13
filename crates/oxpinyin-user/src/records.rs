//! The native ordered container shared by both session index tables.

use std::ops::{Bound, ControlFlow};
use std::path::Path;

use oxpinyin_store::{DefaultStore, RawReadStore, RecordRewrite, StoreError, WriteStore};

pub(crate) struct NativeTable(DefaultStore);

impl std::fmt::Debug for NativeTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTable").finish_non_exhaustive()
    }
}

impl Default for NativeTable {
    fn default() -> Self {
        Self(DefaultStore::create_in_memory().unwrap_or_else(|_| std::process::abort()))
    }
}

impl NativeTable {
    pub(crate) fn insert(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.0.set_raw(key, value)
    }

    pub(crate) fn record(&self, key: &[u8]) -> Option<Vec<u8>> {
        let mut value = Vec::new();
        // Kyoto checks size first and asserts on a failed or short get
        // after that check. The other backends treat a failed get as a miss.
        let result = self.0.get_bigram_raw(key, &mut value);
        match result {
            Ok(true) => Some(value),
            Err(StoreError::ReadAfterCheck) => std::process::abort(),
            Ok(false) | Err(_) => None,
        }
    }

    pub(crate) fn contains_key(&self, key: &[u8]) -> bool {
        self.0.contains_raw(key).unwrap_or(false)
    }

    pub(crate) fn for_each_record<E>(
        &self,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut result = Ok(());
        self.0
            .scan_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
                result = visit(key, value);
                Ok(if result.is_ok() {
                    ControlFlow::Continue(())
                } else {
                    ControlFlow::Break(())
                })
            })
            .unwrap_or_else(|_| std::process::abort());
        result
    }

    pub(crate) fn rewrite(&self, rewrite: &mut RecordRewrite<'_>) -> Result<(), StoreError> {
        self.0.rewrite_raw(rewrite)
    }

    pub(crate) fn save(&self, path: &Path) -> Result<(), StoreError> {
        self.0.save_user_index(path)
    }

    pub(crate) fn keys_starting_with(&self, prefix: &[u8]) -> Vec<Vec<u8>> {
        let mut keys = Vec::new();
        self.0
            .scan_raw(Bound::Included(prefix), Bound::Unbounded, |key, _| {
                if !key.starts_with(prefix) {
                    return Ok(ControlFlow::Break(()));
                }
                keys.push(key.to_vec());
                Ok(ControlFlow::Continue(()))
            })
            .unwrap_or_else(|_| std::process::abort());
        keys
    }

    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<Vec<u8>> {
        self.keys_starting_with(&[])
    }
}
