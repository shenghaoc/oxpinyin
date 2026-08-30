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

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;

use oxpinyin_store::{DefaultStore, ReadStore, StoreError};

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

/// Like [`for_each_row`] but generic over the store's **read** tier:
/// loading a system table opens the file read-only and walks it, so a
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

impl<S: ReadStore> fmt::Debug for GenericLookupTable<S> {
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
            keys.into_iter()
                .map(super::LeByteKey::token)
                .collect::<Vec<_>>(),
            [0x0100, 0x00ff]
        );
        assert!(LeByteKey::new(0x0700) < LeByteKey::new(7));
        assert_eq!(LeByteKey::new(7), LeByteKey::new(7));
    }

    #[test]
    fn store_walk_is_le_byte_ordered_so_loaders_append_without_sorting() {
        use oxpinyin_store::{DefaultStore, WriteStore};

        // Tokens crossing the 256 boundary, where little-endian byte order
        // and integer order diverge. Inserted unsorted so the walk order is
        // the store's, not ours. Includes the USER_DICTIONARY nibble region
        // (0x0700_00xx) to cross 256 in a low byte under a high one.
        let tokens: [u32; 10] = [
            0x0000_0200,
            0x0000_00FF,
            0x0700_0100,
            0x0000_0100,
            0x0000_0001,
            0x0000_FFFF,
            0x0000_01FF,
            0x0700_0001,
            0x0001_0000,
            0x0700_00FF,
        ];
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-data-le-invariant-{}.redb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let store = DefaultStore::create(&path).unwrap();
        store
            .write(|txn| {
                for token in tokens {
                    txn.put("data", &token.to_le_bytes(), b"v")?;
                }
                Ok(())
            })
            .unwrap();
        drop(store);

        // Walk the committed table the way the typed loaders do: append each
        // row in store-walk order, wrapping the decoded token in LeByteKey.
        let mut walked: Vec<(LeByteKey, ())> = Vec::new();
        let mut raw_tokens: Vec<u32> = Vec::new();
        for_each_row::<TableError, _>(&path, |key, _value| {
            let token = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
            raw_tokens.push(token);
            walked.push((LeByteKey::new(token), ()));
            Ok(())
        })
        .unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(walked.len(), tokens.len(), "every row must be walked");

        // The load-without-sort invariant: the walk is ALREADY strictly
        // ascending under LeByteKey, so `ensure_sorted_unique` takes its O(n)
        // fast path and never re-sorts — the append is sound.
        assert!(
            walked.is_sorted_by(|a, b| a.0 < b.0),
            "store walk must already be LeByteKey-sorted",
        );
        let before = walked.clone();
        ensure_sorted_unique(&mut walked);
        assert_eq!(walked, before, "a well-formed store walk needs no repair");

        // Non-vacuity: the SAME walk is NOT ascending by raw integer token.
        // A loader that assumed integer order (the opposite convention) would
        // mis-binary-search on this set — the drift this invariant catches.
        assert!(
            !raw_tokens.is_sorted(),
            "the 256-boundary set must make byte order differ from integer order",
        );
    }
}
