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
