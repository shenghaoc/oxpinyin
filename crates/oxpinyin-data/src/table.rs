//! Loader for ordered-store-backed lookup tables.
//!
//! Each table is a store database with a single `data` table mapping raw
//! `&[u8]` keys to raw `&[u8]` values.  These are committed under
//! `fixtures/w3/` (frozen; no longer regenerated in-tree) per
//! `docs/findings/data-layer-export.md`.
//!
//! # Portability
//!
//! The default backend is redb, a pure-Rust embedded database.  The
//! committed tables can be read on any platform redb supports.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use oxpinyin_store::{DefaultStore, OrderedStore, StoreError};

/// A `phrase_token_t` ordered the way redb stores it: 4 bytes little-endian,
/// so ascending **byte** order — which is not ascending integer order.
///
/// redb walks its B-tree in ascending key-byte order, so a table keyed by
/// `token.to_le_bytes()` yields rows already sorted under this order. The
/// typed loaders append such rows into sorted vectors wrapped in this key,
/// and binary-search with the same wrapping, so the walk order is loadable
/// without a sort and lookups stay O(log n).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LeByteKey(u32);

impl LeByteKey {
    /// Wraps a token for lookup.
    pub(crate) const fn new(token: u32) -> Self {
        Self(token)
    }

    /// The wrapped token.
    pub(crate) const fn token(self) -> u32 {
        self.0
    }
}

impl Ord for LeByteKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.swap_bytes().cmp(&other.0.swap_bytes())
    }
}

impl PartialOrd for LeByteKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

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

/// Visit every row of a store's `data` table without retaining a copy.
///
/// Used by the typed dictionary and LM loaders so they can parse records
/// once into native keys instead of slurping `BTreeMap<Vec<u8>, Vec<u8>>`.
///
/// # Errors
///
/// Returns `E` converted from [`TableError`] on I/O or store failure, or
/// whatever `visit` returns.
pub fn for_each_row<E, F>(path: &Path, mut visit: F) -> Result<(), E>
where
    F: FnMut(&[u8], &[u8]) -> Result<(), E>,
    E: From<TableError>,
{
    let store = DefaultStore::open_read_only(path)
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

/// Restores the ascending unique-key order a sorted-vector map is searched
/// under: sort, then keep the last row per key — the value
/// `BTreeMap::insert` would have left. The sort must be **stable**: that
/// is what keeps equal keys in visitation order, so the last row of each
/// run is the last-visited one, as insert's overwrite left it. redb's
/// B-tree walk is ascending and its table keys are unique, so on every
/// well-formed table this is a single O(n) order check and the repair
/// never runs.
pub(crate) fn ensure_sorted_unique<K: Ord, V>(rows: &mut Vec<(K, V)>) {
    if rows.is_sorted_by(|a, b| a.0 < b.0) {
        return;
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut kept = 0;
    for read in 0..rows.len() {
        if kept > 0 && rows[kept - 1].0 == rows[read].0 {
            rows.swap(kept - 1, read);
        } else {
            rows.swap(kept, read);
            kept += 1;
        }
    }
    rows.truncate(kept);
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
pub struct LookupTable {
    entries: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl LookupTable {
    /// Open a store table file for reading.
    pub fn open(path: &Path) -> Result<Self, TableError> {
        let mut entries = BTreeMap::new();
        for_each_row(path, |key, value| {
            entries.insert(key.to_vec(), value.to_vec());
            Ok::<(), TableError>(())
        })?;
        Ok(Self { entries })
    }

    /// Look up a key in the table.
    ///
    /// Returns `None` if the key is not present. The bytes are borrowed from
    /// the in-memory map; they are not cloned.
    pub fn get(&self, key: &[u8]) -> Result<Option<&[u8]>, TableError> {
        Ok(self.entries.get(key).map(Vec::as_slice))
    }

    /// Return the number of entries in the table.
    pub fn len(&self) -> Result<u64, TableError> {
        Ok(self.entries.len() as u64)
    }

    /// Returns `true` if the table is empty.
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

impl fmt::Debug for LookupTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LookupTable")
            .field("len", &self.entries.len())
            .finish()
    }
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        PathBuf::from(manifest)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join("w3")
    }

    #[test]
    fn open_mini_index_fixture() {
        let table = LookupTable::open(&fixtures_dir().join("pinyin_index.redb")).unwrap();
        // The --mini export keeps the ten allowlisted pinyin keys.
        assert_eq!(table.len().unwrap(), 10);
        assert!(!table.is_empty().unwrap());
    }

    #[test]
    fn keys_are_pinyin_strings() {
        let table = LookupTable::open(&fixtures_dir().join("pinyin_index.redb")).unwrap();
        let val = table.get(b"ni'hao").unwrap();
        assert!(val.is_some(), "ni'hao is in the mini allowlist");
        // Records are 8-byte {token, freq} pairs.
        assert_eq!(val.unwrap().len() % 8, 0);
    }

    #[test]
    fn missing_key_returns_none() {
        let table = LookupTable::open(&fixtures_dir().join("pinyin_index.redb")).unwrap();
        let val = table.get(b"nonexistent").unwrap();
        assert!(val.is_none());
    }

    #[test]
    fn ensure_sorted_unique_repairs_order_and_keeps_last_row() {
        let mut rows: Vec<(Box<str>, u8)> = vec![
            (Box::from("zhong'guo"), 1),
            (Box::from("ni"), 2),
            (Box::from("hao"), 3),
            (Box::from("ni"), 4),
        ];
        super::ensure_sorted_unique(&mut rows);
        // BTreeMap::insert semantics: one entry per key, last row's value.
        assert_eq!(
            rows,
            vec![
                (Box::from("hao"), 3),
                (Box::from("ni"), 4),
                (Box::from("zhong'guo"), 1),
            ]
        );

        // Already strictly ascending: untouched.
        let mut sorted: Vec<(Box<str>, u8)> = vec![
            (Box::from("a"), 1),
            (Box::from("b"), 2),
            (Box::from("c"), 3),
        ];
        super::ensure_sorted_unique(&mut sorted);
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[1].1, 2);
    }

    #[test]
    fn le_byte_key_orders_tokens_as_stored_bytes() {
        // redb orders the 4-byte LE keys bytewise: 0x0100 (bytes 00 01 …)
        // sorts before 0x00ff (bytes ff 00 …) even though 0x00ff < 0x0100
        // as integers. The newtype must reproduce the walk order, and stay
        // a total order (equality iff tokens are equal).
        use super::LeByteKey;
        let mut keys = vec![LeByteKey::new(0x00ff), LeByteKey::new(0x0100)];
        keys.sort();
        assert_eq!(
            keys.into_iter().map(|key| key.token()).collect::<Vec<_>>(),
            [0x0100, 0x00ff]
        );
        assert!(LeByteKey::new(0x0700) < LeByteKey::new(7));
        assert_eq!(LeByteKey::new(7), LeByteKey::new(7));
    }

    #[test]
    fn iter_all_fixture_files() {
        let dir = fixtures_dir();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("redb") {
                continue;
            }
            let table = LookupTable::open(&path).unwrap_or_else(|e| {
                panic!("failed to open {}: {e}", path.display());
            });
            let count = table.len().unwrap();
            assert!(count > 0, "{} should have records", path.display());

            // Verify iteration matches count.
            let entries = table.iter().count();
            assert_eq!(entries as u64, count);
        }
    }
}
