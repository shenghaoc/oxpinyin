//! Errors from opening or querying a lookup table through the selected storage backend.

use crate::store::StoreError;

/// Errors that can occur when opening or querying a table.
#[derive(Debug, thiserror::Error)]
pub enum TableError {
    /// The store file could not be opened (I/O error).
    #[error("I/O error: {0}")]
    Io(#[source] std::io::Error),
    /// The storage backend reported a non-I/O error.
    #[error("store error: {0}")]
    Store(#[source] StoreError),
}

impl From<StoreError> for TableError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Io(io) => Self::Io(io),
            other @ (StoreError::Backend(_)
            | StoreError::ReadOnly
            | StoreError::InvalidInput(_)
            | StoreError::ReadAfterCheck
            | StoreError::ShortArrayHeader) => Self::Store(other),
        }
    }
}

// ── tests ──────────────────────────────────────────────────────────
