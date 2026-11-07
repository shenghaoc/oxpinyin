//! Helpers shared by the backends' raw walks (Kyoto Cabinet, tkrzw,
//! Berkeley DB) and by the file-backed ones that hand a path to a C
//! library (tkrzw). One definition each: the range-bound test and the
//! path check used to live once per backend, byte-identical, and a fix
//! to one copy could miss the others.
//!
//! Each item is compiled only for the backends that use it, so a
//! single-backend build (the exactly-one-backend invariant) carries no
//! dead helper.

#[cfg(any(feature = "kyotocabinet", feature = "tkrzw", feature = "bdb"))]
use std::ops::Bound;
#[cfg(feature = "tkrzw")]
use std::path::Path;

#[cfg(feature = "tkrzw")]
use crate::StoreError;

/// One owned `(key, value)` row.
#[cfg(feature = "bdb")]
pub(crate) type Row = (Vec<u8>, Vec<u8>);

/// Whether `key` falls inside the `[lo, hi]` range — the one reading of
/// `Bound` every backend's `range` shares. An empty-slice bound needs no
/// special case here: `key >= []` holds for every key, and `key <= []`
/// for none but the empty key, which is what the shared read suite pins
/// (`empty_bounds_never_match_or_error`).
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw", feature = "bdb"))]
pub fn in_bounds(key: &[u8], lo: Bound<&[u8]>, hi: Bound<&[u8]>) -> bool {
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
#[cfg(feature = "tkrzw")]
pub fn validate_path(path: &Path) -> Result<(), StoreError> {
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
