//! Helpers shared by the backends that frame every table into one
//! keyspace (Kyoto Cabinet, tkrzw, Berkeley DB) and by the file-backed
//! ones that hand a path to a C library (tkrzw, LMDB, Berkeley DB). One
//! definition each: the framing scheme, the range-bound test and the
//! path check used to live once per backend, byte-identical, and a fix
//! to one copy could miss the others.
//!
//! Each item is compiled only for the backends that use it, so a
//! single-backend build (the exactly-one-backend invariant) carries no
//! dead helper.

#[cfg(any(feature = "kyotocabinet", feature = "tkrzw", feature = "bdb"))]
use std::ops::Bound;
#[cfg(any(feature = "tkrzw", feature = "lmdb"))]
use std::path::Path;

#[cfg(any(feature = "tkrzw", feature = "lmdb"))]
use crate::StoreError;
#[cfg(feature = "tkrzw")]
use crate::validate_table_name;

/// The byte between a table name and its keys: `table || 0x00 || key`.
/// `validate_table_name` rejects names containing NUL, which is exactly
/// what makes the framing prefix-free — no table's prefix is a prefix of
/// another's, so every table is a contiguous run of the ordered keyspace
/// whose internal order is the caller's key order.
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw", feature = "bdb"))]
pub(crate) const SEPARATOR: u8 = 0;

/// `table || 0x00 || key`. The caller has validated `table`.
#[cfg(any(feature = "kyotocabinet", feature = "bdb"))]
pub(crate) fn frame(table: &str, key: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(table.len() + 1 + key.len());
    framed.extend_from_slice(table.as_bytes());
    framed.push(SEPARATOR);
    framed.extend_from_slice(key);
    framed
}

/// `table || 0x00` — the prefix every one of `table`'s rows carries.
/// The caller has validated `table`.
#[cfg(any(feature = "kyotocabinet", feature = "bdb"))]
pub(crate) fn prefix(table: &str) -> Vec<u8> {
    frame(table, &[])
}

/// `table || 0x00`, validating the table name first (the empty name and
/// a name containing NUL are refused, which is what keeps the framing
/// prefix-free).
///
/// # Errors
///
/// [`StoreError::InvalidInput`] for an invalid table name.
#[cfg(feature = "tkrzw")]
pub(crate) fn table_prefix(table: &str) -> Result<Vec<u8>, StoreError> {
    validate_table_name(table)?;
    let mut prefix = Vec::with_capacity(table.len() + 1);
    prefix.extend_from_slice(table.as_bytes());
    prefix.push(SEPARATOR);
    Ok(prefix)
}

/// `prefix || key` for a prefix from [`table_prefix`].
#[cfg(feature = "tkrzw")]
pub(crate) fn framed(prefix: &[u8], key: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(prefix.len() + key.len());
    framed.extend_from_slice(prefix);
    framed.extend_from_slice(key);
    framed
}

/// The caller's key inside a framed one, or `None` if the framed key
/// belongs to another table.
#[cfg(any(feature = "kyotocabinet", feature = "bdb"))]
pub(crate) fn unframe<'a>(prefix: &[u8], framed: &'a [u8]) -> Option<&'a [u8]> {
    framed.strip_prefix(prefix)
}

/// Whether `key` falls inside the `[lo, hi]` range — the one reading of
/// `Bound` every backend's `range` shares. An empty-slice bound needs no
/// special case here: `key >= []` holds for every key, and `key <= []`
/// for none but the empty key, which is what the shared read suite pins
/// (`empty_bounds_never_match_or_error`). LMDB normalises the same
/// bounds for heed's range API instead of testing keys one by one.
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw", feature = "bdb"))]
pub(crate) fn in_bounds(key: &[u8], lo: Bound<&[u8]>, hi: Bound<&[u8]>) -> bool {
    let above_lo = match lo {
        Bound::Unbounded => true,
        Bound::Included(bound) => key >= bound,
        Bound::Excluded(bound) => key > bound,
    };
    let below_hi = match hi {
        Bound::Unbounded => true,
        Bound::Included(bound) => key <= bound,
        Bound::Excluded(bound) => key < bound,
    };
    above_lo && below_hi
}

/// Refuses a path a C library would truncate at the first NUL.
///
/// # Errors
///
/// [`StoreError::InvalidInput`] when the path contains a NUL byte.
#[cfg(any(feature = "tkrzw", feature = "lmdb"))]
pub(crate) fn validate_path(path: &Path) -> Result<(), StoreError> {
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(StoreError::InvalidInput("path contains NUL"));
    }
    Ok(())
}

#[cfg(all(
    test,
    any(feature = "kyotocabinet", feature = "tkrzw", feature = "bdb")
))]
mod tests {
    use super::*;

    #[test]
    fn bounds_read_the_way_the_shared_suite_expects() {
        let key: &[u8] = b"m";
        assert!(in_bounds(key, Bound::Unbounded, Bound::Unbounded));
        assert!(in_bounds(key, Bound::Included(b"m"), Bound::Included(b"m")));
        assert!(!in_bounds(key, Bound::Excluded(b"m"), Bound::Unbounded));
        assert!(!in_bounds(key, Bound::Unbounded, Bound::Excluded(b"m")));
        // The empty-slice bounds: every key is above an empty lower bound,
        // no non-empty key is below an empty upper bound.
        assert!(in_bounds(key, Bound::Included(&[]), Bound::Unbounded));
        assert!(in_bounds(key, Bound::Excluded(&[]), Bound::Unbounded));
        assert!(!in_bounds(key, Bound::Unbounded, Bound::Included(&[])));
        assert!(!in_bounds(key, Bound::Unbounded, Bound::Excluded(&[])));
    }
}
