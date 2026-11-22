//! Native system-file attachments and their open outcomes.

use std::path::Path;

use crate::store::{DefaultStore, StoreError};

/// The result of one system-file attach.
pub enum SystemAttachment {
    /// The native file opened successfully.
    Open(DefaultStore),
    /// The native open failed. No handle remains.
    Unopened,
}

impl SystemAttachment {
    /// Attaches one tree or hash system file read-only, without retrying it.
    ///
    /// # Errors
    /// Returns failures before the native open, such as a path containing NUL.
    pub fn open(path: &Path, hash: bool) -> Result<Self, StoreError> {
        Self::attach(path, hash, true)
    }

    /// Attaches a hash file with one native open, without a CREATE retry or a
    /// signature check (`ngram_bdb.cpp:110-124`, `ngram_kyotodb.cpp:110-120`,
    /// `ngram_tkrzwdb.cpp:86-99` at 074a2219).
    ///
    /// # Errors
    /// Returns failures before the native open, such as a path containing NUL.
    pub fn attach_bigram(path: &Path, read_only: bool) -> Result<Self, StoreError> {
        Self::attach(path, true, read_only)
    }

    /// Attaches a tree or hash file with one native open. Writable attachments
    /// create the file if it is missing.
    ///
    /// # Errors
    /// Returns failures before the native open. A failed native open returns
    /// [`Self::Unopened`].
    pub fn attach(path: &Path, hash: bool, read_only: bool) -> Result<Self, StoreError> {
        #[cfg(feature = "bdb")]
        let store = crate::store::bdb::open_system(path, hash, read_only)?;
        #[cfg(feature = "kyotocabinet")]
        let store = crate::store::kyotocabinet::open_system(path, hash, read_only)?;
        #[cfg(feature = "tkrzw")]
        let store = crate::store::tkrzw::open_system(path, hash, read_only)?;
        Ok(store.map_or(Self::Unopened, Self::Open))
    }
}

#[cfg(test)]
mod tests;
