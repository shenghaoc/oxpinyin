//! One stable sort body for the stable sorts by an integer key.
//!
//! `slice::sort_by_key` is stable: elements with equal keys keep their
//! original order, which is the contract every caller here needs.

/// Stably sorts `items` by `key`: equal keys keep their order.
pub(crate) fn sort_by_key<T>(items: &mut [T], mut key: impl FnMut(&T) -> u128) {
    items.sort_by_key(|item| key(item));
}
