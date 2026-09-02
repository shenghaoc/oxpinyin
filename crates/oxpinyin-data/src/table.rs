//! Loader for ordered-store-backed lookup tables.
//!
//! Each table is a store database with a single `data` table mapping raw
//! `&[u8]` keys to raw `&[u8]` values.  These are committed under
//! `fixtures/w3/` (frozen; no longer regenerated in-tree) per
//! `docs/findings/data-layer-export.md`.
//!
//! # Backend selection
//!
//! One peer backend per binary, resolved by `oxpinyin-store`'s
//! compile-time selection (Kyoto Cabinet is the default; redb, LMDB and
//! tkrzw are the other peers, each selected with
//! `--no-default-features --features <peer>`).  The committed fixtures
//! carry one file per peer's extension so tests can exercise each
//! independently.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;

use oxpinyin_store::{DefaultStore, ReadStore, StoreError};

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
            other => Self::Store(other),
        }
    }
}

/// Visit every row of the default backend's `data` table without
/// retaining a copy.
///
/// Used by the typed dictionary and LM loaders so they can parse records
/// once into native keys instead of slurping `BTreeMap<Vec<u8>, Vec<u8>>`.
///
/// Opens the file read-only via [`DefaultStore`] and walks it.  For any
/// other backend use [`for_each_row_with_store`].
///
/// # Errors
///
/// Returns `E` converted from [`TableError`] on I/O or store failure, or
/// whatever `visit` returns.
pub fn for_each_row<E, F>(path: &Path, visit: F) -> Result<(), E>
where
    F: FnMut(&[u8], &[u8]) -> Result<(), E>,
    E: From<TableError>,
{
    for_each_row_with_store::<DefaultStore, E, F>(path, visit)
}

/// Like [`for_each_row`] but generic over the store's **read** tier.
///
/// Loading a system table opens the file read-only and walks it, so a
/// backend that offers nothing but [`ReadStore`] can serve this path.
/// Callers name the backend, e.g.
/// `for_each_row_with_store::<DefaultStore, _, _>(path, visit)`.
///
/// # Errors
///
/// Returns `E` converted from [`TableError`] on I/O or store failure, or
/// whatever `visit` returns.
pub fn for_each_row_with_store<S, E, F>(path: &Path, mut visit: F) -> Result<(), E>
where
    S: ReadStore,
    F: FnMut(&[u8], &[u8]) -> Result<(), E>,
    E: From<TableError>,
{
    let store = S::open_read_only(path)
        .map_err(TableError::from)
        .map_err(E::from)?;
    let mut visitor_error: Option<E> = None;
    let scan_result = store.for_each("data", &mut |key, value| match visit(key, value) {
        Ok(()) => Ok(()),
        Err(e) => {
            visitor_error = Some(e);
            Err(StoreError::Backend("visitor error".into()))
        }
    });
    if let Some(e) = visitor_error {
        return Err(e);
    }
    scan_result.map_err(TableError::from).map_err(E::from)
}

/// A read-only lookup table backed by a store database.
///
/// Keys and values are opaque byte slices.  Interpretation (e.g. as
/// `phrase_token_t[]` arrays or UTF-8 text) is the caller's responsibility.
///
/// On open the whole table is loaded into an in-memory map. The decoder
/// issues millions of lookups over a session (every keystroke × every
/// prefix × every path), and a per-call store read cannot keep up; the
/// portable tables are tens of megabytes, so the cache fits.
/// [`LookupTable::get`] and [`LookupTable::iter`] borrow those bytes; they
/// do not clone a row on every call.
///
/// Generic over the store's **read** tier: the backend is used once, by
/// [`GenericLookupTable::open`], and never again — the rows live in the
/// map from then on.  [`LookupTable`] is the default-backend alias every
/// consumer uses.
pub struct GenericLookupTable<S: ReadStore> {
    entries: BTreeMap<Vec<u8>, Vec<u8>>,
    /// The backend is only a loading detail, so it is not stored. `fn() -> S`
    /// keeps the table's auto traits independent of the backend's.
    _backend: PhantomData<fn() -> S>,
}

/// Lookup table over the compiled-in default backend ([`DefaultStore`]).
pub type LookupTable = GenericLookupTable<DefaultStore>;

impl<S: ReadStore> GenericLookupTable<S> {
    /// Open a store table file for reading.
    ///
    /// # Errors
    ///
    /// Returns [`TableError`] when the file cannot be opened or fails validation.
    pub fn open(path: &Path) -> Result<Self, TableError> {
        let mut entries = BTreeMap::new();
        for_each_row_with_store::<S, _, _>(path, |key, value| {
            entries.insert(key.to_vec(), value.to_vec());
            Ok::<(), TableError>(())
        })?;
        Ok(Self {
            entries,
            _backend: PhantomData,
        })
    }

    /// Look up a key in the table.
    ///
    /// Returns `None` if the key is not present. The bytes are borrowed from
    /// the in-memory map; they are not cloned.
    ///
    /// # Errors
    ///
    /// Returns [`TableError`] when the table cannot be read.
    pub fn get(&self, key: &[u8]) -> Result<Option<&[u8]>, TableError> {
        Ok(self.entries.get(key).map(Vec::as_slice))
    }

    /// Return the number of entries in the table.
    ///
    /// # Errors
    ///
    /// Returns [`TableError`] when the table cannot be read.
    pub fn len(&self) -> Result<u64, TableError> {
        Ok(self.entries.len() as u64)
    }

    /// Returns `true` if the table is empty.
    ///
    /// # Errors
    ///
    /// Returns [`TableError`] when the table cannot be read.
    pub fn is_empty(&self) -> Result<bool, TableError> {
        Ok(self.entries.is_empty())
    }

    /// Iterate over all (key, value) pairs, borrowed from the in-memory map.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &[u8])> + '_ {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_slice(), value.as_slice()))
    }
}

impl<S: ReadStore> fmt::Debug for GenericLookupTable<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LookupTable")
            .field("len", &self.entries.len())
            .finish()
    }
}

// ── tests ──────────────────────────────────────────────────────────
