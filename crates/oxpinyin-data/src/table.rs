//! Loader for ordered-store-backed lookup tables.
//!
//! Each table is a store database with a single `data` table mapping raw
//! `&[u8]` keys to raw `&[u8]` values.  These are committed under
//! `fixtures/w3/` (frozen; no longer regenerated in-tree).
//!
//! # Backend selection
//!
//! One peer backend per binary, resolved by `oxpinyin-store`'s
//! compile-time selection (tkrzw is the default since 2026-10-30;
//! Kyoto Cabinet and Berkeley DB are the other peers, each selected with
//! `--no-default-features --features <peer>`).  The committed fixtures
//! carry one file per peer's extension so tests can exercise each
//! independently.

use std::fmt;

use oxpinyin_store::StoreError;

/// Errors that can occur when opening or querying a table.
#[derive(Debug)]
pub enum TableError {
    /// The store file could not be opened (I/O error).
    Io(std::io::Error),
    /// The storage backend reported a non-I/O error.
    Store(StoreError),
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Store(e) => write!(f, "store error: {e}"),
        }
    }
}

impl std::error::Error for TableError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Store(e) => Some(e),
        }
    }
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
