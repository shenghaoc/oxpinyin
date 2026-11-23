use std::ops::Bound::{Excluded, Included, Unbounded};

use super::in_bounds;

// RawReadStore compares each lower and upper bound in unsigned byte order.
#[test]
fn bounds_read_the_way_the_shared_suite_expects() {
    let m = b"m".as_slice();
    for (key, lo, hi, inside) in [
        (m, Unbounded, Unbounded, true),
        (b"l", Included(m), Unbounded, false),
        (m, Included(m), Included(m), true),
        (b"n", Unbounded, Included(m), false),
        (m, Excluded(m), Unbounded, false),
        (b"n", Excluded(m), Unbounded, true),
        (b"l", Unbounded, Excluded(m), true),
        (m, Unbounded, Excluded(m), false),
    ] {
        assert_eq!(in_bounds(key, lo, hi), inside, "{key:?} {lo:?} {hi:?}");
    }
}
