//! One sort body for the stable sorts by an integer key.
//!
//! A stable sort by key gives the same order as an unstable sort of
//! `(key, original index)` rows: the index makes every row unique, so no tie
//! is left to the algorithm. Every caller of this module sorts this one row
//! type, so these callers share one instance of the sort, not one per element
//! type and comparator. Sorts that call the standard library directly keep
//! their own instances.

/// A sort key and the position the element had before the sort.
pub(crate) type Row = (u128, usize);

/// The rows of `keys`, in the order of a stable sort by key.
pub(crate) fn sorted_rows(keys: impl Iterator<Item = u128>) -> Vec<Row> {
    let mut rows: Vec<Row> = keys.enumerate().map(|(index, key)| (key, index)).collect();
    rows.sort_unstable();
    rows
}

/// Moves each element of `items` to the place `rows` gives it: the element
/// at `rows[i].1` goes to position `i`. Each displaced element is followed
/// to where an earlier swap moved it (the standard library's
/// `sort_by_cached_key` scheme), so there is no second buffer. `rows` ends
/// holding, at `i`, the original position of the element now at `i`.
pub(crate) fn apply_order<T>(items: &mut [T], rows: &mut [Row]) {
    for position in 0..rows.len() {
        let mut index = rows[position].1;
        while index < position {
            index = rows[index].1;
        }
        rows[position].1 = index;
        items.swap(position, index);
    }
}

/// Stably sorts `items` by `key`: equal keys keep their order.
pub(crate) fn sort_by_key<T>(items: &mut [T], key: impl FnMut(&T) -> u128) {
    if items.len() < 2 {
        return;
    }
    let mut rows = sorted_rows(items.iter().map(key));
    apply_order(items, &mut rows);
}
